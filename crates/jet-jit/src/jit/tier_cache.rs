//! Disk-backed warm reuse of compiled tier-1 Cranelift modules (#741).
//!
//! A hit reloads machine code via `define_function_bytes` and skips Jet
//! load/parse/check/TIR lowering and Cranelift IR generation. Keying and
//! WatchService invalidation live in `Source/RunCache.rs`.
//!
//! The artifact carries the entry's error rail and the tier roster as well as
//! its code: a warm run has no TIR program left to ask whether the entry is
//! fallible, nor which functions the planner put on the native tier.

use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::ir::types::{self, Type as ClifType};
use cranelift_codegen::ir::{
    AbiParam, ExternalName, Function, Signature, UserExternalName, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::Context;
use cranelift_codegen::{FinalizedMachReloc, FinalizedRelocTarget};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::{
    JitBackend::RunOutcome,
    MIR::{MirArtifactId, MirFunctionId},
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use super::resident::{fresh_runtime, publish_runtime_decisions, resident_invoke};
use super::runtime_host::{new_jit_module, EntryErrorType, ResidentModule};
use super::tiers::{record_trace, Tier, TierRow};
use super::{RESIDENT_MODULE, RESIDENT_RUNTIME};

/// Current on-disk format: native code and tier roster with checked MIR ids,
/// the checked entry symbol, App serving policy, and self-contained error rails.
/// The reader accepts only this format.
const FORMAT: u32 = 11;

thread_local! {
    static CAPTURE: RefCell<CaptureState> = const { RefCell::new(CaptureState::Idle) };
}

/// The capture of the compile in progress on this thread.
///
/// `Refused` keeps the first reason lowering gave for code that cannot be
/// replayed, so a refusal names the rule it broke instead of a generic
/// "cannot be replayed".
enum CaptureState {
    Idle,
    Active(Capture),
    Refused(&'static str),
}

/// One in-progress capture: the functions defined so far, and the `FuncId` each
/// one held, in the same order.
///
/// The ids are capture-time bookkeeping for `capture_replay_refusal`, never
/// artifact content, so they stay out of `CapturedFn` and are read by zipping the
/// two — no positional lookup into either.
#[derive(Default)]
struct Capture {
    func_ids: Vec<u32>,
    fns: Vec<CapturedFn>,
    /// Lowering baked a checked runtime type id into the code. The id resolves
    /// only against the checked type registry, which a warm run artifact does
    /// not restore and a dev-build image does.
    reads_type_registry: bool,
}

#[derive(Clone)]
pub(crate) struct CapturedFn {
    export_name: String,
    sig: EncodedSig,
    alignment: u64,
    bytes: Vec<u8>,
    relocs: Vec<StoredReloc>,
}

#[derive(Clone)]
struct EncodedSig {
    call_conv: u8,
    params: Vec<u16>,
    returns: Vec<u16>,
}

#[derive(Clone)]
struct StoredReloc {
    offset: u32,
    kind: u8,
    addend: i64,
    target: StoredTarget,
}

#[derive(Clone)]
enum StoredTarget {
    User { namespace: u32, index: u32 },
    FuncOffset(u32),
}

/// The entry's error rail, as the artifact carries it.
///
/// The rail is projected once from the checked MIR failure type. Warm artifacts
/// can reproduce default Err/packed IOError reporting and !Never entries without a
/// schema registry. A descriptor-dependent entry needs the checked type
/// registry, so only a dev-build image, which restores it, carries one; the
/// warm run cache keeps such entries on native cold code.
struct EntryRail {
    entry_symbol: String,
    returns_result: bool,
    returns_app: bool,
    serves_app: bool,
    error_type: Option<EntryErrorType>,
    default_error_type: Option<u64>,
}

/// Read the rail the cold run already decided, off the live resident module.
///
/// `None` means there is no resident module to read, and then there is no
/// artifact either: an artifact that cannot say whether the entry is fallible is
/// worse than a cold recompile.
fn capture_rail(restores_runtime_tables: bool) -> Option<EntryRail> {
    RESIDENT_MODULE.with(|slot| {
        let resident = slot.borrow();
        let resident = resident.as_ref()?;
        if !restores_runtime_tables
            && matches!(resident.main_error_type, Some(EntryErrorType::Descriptor(_)))
        {
            return None;
        }
        let default_error_type = RESIDENT_RUNTIME.with(|slot| {
            slot.borrow().as_ref().and_then(|runtime| runtime.default_error_type)
        });
        if resident.main_error_type == Some(EntryErrorType::Default)
            && default_error_type.is_none()
        {
            return None;
        }
        Some(EntryRail {
            entry_symbol: resident.module.declarations().get_function_decl(resident.main_id).name.clone()?,
            returns_result: resident.main_returns_result,
            returns_app: resident.main_returns_app,
            serves_app: resident.main_serves_app,
            error_type: resident.main_error_type,
            default_error_type,
        })
    })
}

pub(crate) fn begin_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = CaptureState::Active(Capture::default()));
}

/// Discard the capture: the compile failed, or its result is not published.
pub(crate) fn abort_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = CaptureState::Idle);
}

/// Lowering baked state into the code that no image restores. The first
/// reason wins; without an active capture this is a no-op.
pub(crate) fn refuse_capture(reason: &'static str) {
    CAPTURE.with(|slot| {
        let mut state = slot.borrow_mut();
        if matches!(*state, CaptureState::Active(_)) {
            *state = CaptureState::Refused(reason);
        }
    });
}

/// Lowering baked a checked runtime type id into the code.
pub(crate) fn note_type_registry_read() {
    CAPTURE.with(|slot| {
        if let CaptureState::Active(capture) = &mut *slot.borrow_mut() {
            capture.reads_type_registry = true;
        }
    });
}

/// Take the finished capture, or the reason its code cannot be replayed.
/// `restores_runtime_tables` says whether the replay restores the checked type
/// registry and closure targets (a dev-build image does, a warm run artifact
/// does not).
fn take_capture(restores_runtime_tables: bool) -> Result<Vec<CapturedFn>, &'static str> {
    let state = CAPTURE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), CaptureState::Idle));
    let capture = match state {
        CaptureState::Active(capture) => capture,
        CaptureState::Refused(reason) => return Err(reason),
        CaptureState::Idle => return Err("no compiled code was captured for the image"),
    };
    if capture.reads_type_registry && !restores_runtime_tables {
        return Err("the compiled code reads the checked type registry");
    }
    let entry_id = RESIDENT_MODULE
        .with(|slot| slot.borrow().as_ref().map(|resident| resident.main_id.as_u32()))
        .ok_or("no compiled module holds the entry")?;
    if !capture.func_ids.contains(&entry_id) {
        return Err("the captured code has no entry function");
    }
    // Definition order is not declaration order: closure and generator bodies
    // are defined before the functions that declared them. A reload declares
    // the captured functions in list order, so list them in id order.
    let mut rows = capture.func_ids.into_iter().zip(capture.fns).collect::<Vec<_>>();
    rows.sort_unstable_by_key(|(func_id, _)| *func_id);
    match capture_replay_refusal(&rows) {
        Some(reason) => Err(reason),
        None => Ok(rows.into_iter().map(|(_, f)| f).collect()),
    }
}

/// Can `run_cached_module` reproduce the `FuncId` numbering this capture was
/// compiled against? `rows` are the captured functions in id order. `None`
/// when it can, else the rule the capture breaks.
///
/// A reload declares the host functions first (`new_jit_module`, deterministic
/// and identical every time), then the captured functions in list order. So the
/// numbering is reproduced only when
///
/// * the captured ids are consecutive from the first one, and
/// * every relocation names a function (namespace 0) that is either a host
///   (`index < first`) or one of the captured ones (`index < first + len`).
///
/// A declared function without a captured definition leaves a gap that cannot
/// reproduce that table. Reject such captures rather than replay a relocation
/// against a different function id.
///
/// Refusing the artifact keeps such a program on the cold path: correct, only
/// slower. Per I2 the guard belongs here, where an inconsistent artifact would
/// otherwise be written, and never as a clamp at replay — a clamp would turn a
/// wrong relocation into a wrong call.
fn capture_replay_refusal(rows: &[(u32, CapturedFn)]) -> Option<&'static str> {
    let Some(&(first, _)) = rows.first() else {
        return Some("the captured code has no functions");
    };
    let first = u64::from(first);
    let limit = first + rows.len() as u64;
    let mut expected = first;
    for (func_id, f) in rows {
        if u64::from(*func_id) != expected {
            return Some("a declared function has no captured definition");
        }
        expected += 1;
        for reloc in &f.relocs {
            match &reloc.target {
                // Namespace 1 is a data object, and a reload declares no data
                // objects at all, so a data relocation is equally unreplayable.
                StoredTarget::User { namespace, .. } if *namespace != 0 => {
                    return Some("the compiled code references a data object");
                }
                StoredTarget::User { index, .. } if u64::from(*index) >= limit => {
                    return Some("the compiled code calls a function defined outside the image");
                }
                StoredTarget::User { .. } | StoredTarget::FuncOffset(_) => {}
            }
        }
    }
    None
}

thread_local! {
    static LAST_ARTIFACT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Move a successful capture into the process-local "last artifact" slot.
///
/// `native_fns` is the tier roster this run is publishing with its code: the
/// checked MIR id and name of every function, since the warm run has no MIR
/// program to recover ids from. A capture is only published from
/// `resident_run_fresh`, which runs with an empty deopt list, so every listed
/// function is tier-1 native with no reason — exactly the rows the cold run
/// printed under `--trace-tiers`.
pub(crate) fn publish_capture(native_fns: &[(MirFunctionId, &str)], artifact: MirArtifactId) {
    let bytes = encode_capture(native_fns, artifact, false).ok();
    LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = bytes);
}

/// Encode the finished capture as a warm module, or say why the compiled
/// program cannot be replayed from one. The warm run cache and the Cranelift
/// dev-build image (#3953) share this one admission rule; only the dev-build
/// image restores the checked type registry and closure targets
/// (`restores_runtime_tables`).
pub(super) fn encode_capture(
    native_fns: &[(MirFunctionId, &str)],
    artifact: MirArtifactId,
    restores_runtime_tables: bool,
) -> Result<Vec<u8>, &'static str> {
    // FFI entries point into a process-local cdylib. A disk artifact cannot
    // recreate that bridge on a warm run, so force the bundle through the cold
    // entry that binds its FFI table before execution.
    if crate::Ffi::has_bound_ffi() {
        abort_capture();
        return Err("the program binds a native FFI bridge");
    }
    // Cell schema/projection/layout handles are iconst-baked at compile time.
    // A cache hit rebuilds a fresh CellState and would leave those handles dangling.
    let cell_handles = RESIDENT_RUNTIME.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|rt| rt.cells.has_compile_handles())
    });
    if cell_handles {
        abort_capture();
        return Err("the compiled code holds Cell schema handles");
    }
    // Closure targets are registered by finalized address after the compile.
    let closure_targets = RESIDENT_RUNTIME.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|rt| !rt.jit_closure_targets.is_empty())
    });
    if closure_targets && !restores_runtime_tables {
        abort_capture();
        return Err("the compiled code registers closure targets");
    }
    let fns = take_capture(restores_runtime_tables)?;
    let strings = RESIDENT_RUNTIME.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|rt| {
                if rt.compile_strings.is_empty() {
                    rt.heap.string_slots()
                } else {
                    rt.compile_strings.clone()
                }
            })
            .unwrap_or_default()
    });
    // No rail, no artifact. A stored module that has forgotten whether its entry
    // is fallible replays as a success on the next run.
    let rail = capture_rail(restores_runtime_tables)
        .ok_or("the entry's error type needs the checked type registry")?;
    Ok(encode_module(artifact, &rail, native_fns, &fns, &strings))
}

/// Take the artifact produced by the most recent successful native compile, if any.
pub fn take_last_tier_artifact() -> Option<Vec<u8>> {
    LAST_ARTIFACT.with(|slot| slot.borrow_mut().take())
}

/// Move the artifact a compiler worker published back onto its caller's
/// thread, so the run cache still sees what the run just compiled.
pub(crate) fn publish_last_tier_artifact(artifact: Option<Vec<u8>>) {
    LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = artifact);
}

pub(crate) fn note_defined(export_name: &str, func_id: FuncId, ctx: &Context) {
    CAPTURE.with(|slot| {
        let mut guard = slot.borrow_mut();
        let CaptureState::Active(capture) = &mut *guard else {
            return;
        };
        let Some(cc) = ctx.compiled_code() else {
            return;
        };
        let bytes = cc.code_buffer().to_vec();
        let alignment = cc.buffer.alignment as u64;
        let mut relocs = Vec::new();
        for reloc in cc.buffer.relocs() {
            let Some(stored) = store_reloc(reloc, &ctx.func) else {
                *guard = CaptureState::Refused("the compiled code uses a relocation the image cannot store");
                return;
            };
            relocs.push(stored);
        }
        let Some(sig) = try_encode_sig(&ctx.func.signature) else {
            *guard = CaptureState::Refused("a compiled function signature uses a type the image cannot store");
            return;
        };
        capture.func_ids.push(func_id.as_u32());
        capture.fns.push(CapturedFn {
            export_name: export_name.to_string(),
            sig,
            alignment,
            bytes,
            relocs,
        });
    });
}

fn store_reloc(reloc: &FinalizedMachReloc, func: &Function) -> Option<StoredReloc> {
    let target = match &reloc.target {
        FinalizedRelocTarget::ExternalName(ExternalName::User(reff)) => {
            let name = func.params.user_named_funcs().get(*reff)?;
            StoredTarget::User {
                namespace: name.namespace,
                index: name.index,
            }
        }
        FinalizedRelocTarget::Func(offset) => StoredTarget::FuncOffset(*offset),
        _ => return None,
    };
    Some(StoredReloc {
        offset: reloc.offset,
        kind: reloc_to_u8(reloc.kind)?,
        addend: reloc.addend,
        target,
    })
}

fn reloc_to_u8(kind: Reloc) -> Option<u8> {
    Some(match kind {
        Reloc::Abs4 => 1,
        Reloc::Abs8 => 2,
        Reloc::X86PCRel4 => 3,
        Reloc::X86CallPCRel4 => 4,
        Reloc::X86CallPLTRel4 => 5,
        Reloc::X86GOTPCRel4 => 6,
        Reloc::Arm64Call => 7,
        _ => return None,
    })
}

fn reloc_from_u8(tag: u8) -> Option<Reloc> {
    Some(match tag {
        1 => Reloc::Abs4,
        2 => Reloc::Abs8,
        3 => Reloc::X86PCRel4,
        4 => Reloc::X86CallPCRel4,
        5 => Reloc::X86CallPLTRel4,
        6 => Reloc::X86GOTPCRel4,
        7 => Reloc::Arm64Call,
        _ => return None,
    })
}

fn clif_ty_tag(ty: ClifType) -> Option<u16> {
    if ty == types::I8 {
        Some(1)
    } else if ty == types::I32 {
        Some(2)
    } else if ty == types::I64 {
        Some(3)
    } else if ty == types::F64 {
        Some(4)
    } else if ty == types::F32 {
        Some(5)
    } else {
        None
    }
}

fn clif_ty_from_tag(tag: u16) -> Option<ClifType> {
    Some(match tag {
        1 => types::I8,
        2 => types::I32,
        3 => types::I64,
        4 => types::F64,
        5 => types::F32,
        _ => return None,
    })
}

fn try_encode_sig(sig: &Signature) -> Option<EncodedSig> {
    let call_conv = match sig.call_conv {
        CallConv::SystemV => 0,
        CallConv::WindowsFastcall => 1,
        CallConv::AppleAarch64 => 2,
        _ => 3,
    };
    let mut params = Vec::new();
    for p in &sig.params {
        params.push(clif_ty_tag(p.value_type)?);
    }
    let mut returns = Vec::new();
    for p in &sig.returns {
        returns.push(clif_ty_tag(p.value_type)?);
    }
    Some(EncodedSig {
        call_conv,
        params,
        returns,
    })
}

fn decode_sig(module: &JITModule, enc: &EncodedSig) -> Option<Signature> {
    let cc = match enc.call_conv {
        0 => CallConv::SystemV,
        1 => CallConv::WindowsFastcall,
        2 => CallConv::AppleAarch64,
        _ => module.target_config().default_call_conv,
    };
    let mut sig = Signature::new(cc);
    for &tag in &enc.params {
        sig.params.push(AbiParam::new(clif_ty_from_tag(tag)?));
    }
    for &tag in &enc.returns {
        sig.returns.push(AbiParam::new(clif_ty_from_tag(tag)?));
    }
    Some(sig)
}

fn encode_module(
    artifact: MirArtifactId,
    rail: &EntryRail,
    native_fns: &[(MirFunctionId, &str)],
    fns: &[CapturedFn],
    strings: &[(usize, String)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&artifact.0.to_le_bytes());
    write_rail(&mut out, rail);
    out.extend_from_slice(&(native_fns.len() as u32).to_le_bytes());
    for (id, name) in native_fns {
        out.extend_from_slice(&id.0.to_le_bytes());
        write_str(&mut out, name);
    }
    out.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    for (idx, text) in strings {
        out.extend_from_slice(&(*idx as u32).to_le_bytes());
        write_str(&mut out, text);
    }
    out.extend_from_slice(&(fns.len() as u32).to_le_bytes());
    for f in fns {
        write_str(&mut out, &f.export_name);
        out.push(f.sig.call_conv);
        write_u16_slice(&mut out, &f.sig.params);
        write_u16_slice(&mut out, &f.sig.returns);
        out.extend_from_slice(&f.alignment.to_le_bytes());
        out.extend_from_slice(&(f.bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&f.bytes);
        out.extend_from_slice(&(f.relocs.len() as u32).to_le_bytes());
        for r in &f.relocs {
            out.extend_from_slice(&r.offset.to_le_bytes());
            out.push(r.kind);
            out.extend_from_slice(&r.addend.to_le_bytes());
            match r.target {
                StoredTarget::User { namespace, index } => {
                    out.push(1);
                    out.extend_from_slice(&namespace.to_le_bytes());
                    out.extend_from_slice(&index.to_le_bytes());
                }
                StoredTarget::FuncOffset(v) => {
                    out.push(4);
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    out
}

pub(super) fn write_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn write_rail(out: &mut Vec<u8>, rail: &EntryRail) {
    write_str(out, &rail.entry_symbol);
    out.push(u8::from(rail.returns_result));
    out.push(u8::from(rail.returns_app));
    out.push(u8::from(rail.serves_app));
    out.push(match rail.error_type {
        None => 0,
        Some(EntryErrorType::Default) => 1,
        Some(EntryErrorType::Io) => 2,
        Some(EntryErrorType::Uninhabited) => 3,
        Some(EntryErrorType::FieldErrors) => 4,
        // Only a dev-build image, which restores the type registry, carries it.
        Some(EntryErrorType::Descriptor(_)) => 5,
    });
    if let Some(EntryErrorType::Descriptor(identity)) = rail.error_type {
        out.extend_from_slice(&identity.to_le_bytes());
    }
    out.push(u8::from(rail.default_error_type.is_some()));
    if let Some(identity) = rail.default_error_type {
        out.extend_from_slice(&identity.to_le_bytes());
    }
}

fn write_u16_slice(out: &mut Vec<u8>, v: &[u16]) {
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

pub(super) fn read_bool(data: &[u8], i: &mut usize) -> Option<bool> {
    let byte = *data.get(*i)?;
    *i += 1;
    match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn read_rail(data: &[u8], i: &mut usize) -> Option<EntryRail> {
    let entry_symbol = read_str(data, i)?;
    let returns_result = read_bool(data, i)?;
    let returns_app = read_bool(data, i)?;
    let serves_app = read_bool(data, i)?;
    let tag = *data.get(*i)?;
    *i += 1;
    let error_type = match tag {
        0 => None,
        1 => Some(EntryErrorType::Default),
        2 => Some(EntryErrorType::Io),
        3 => Some(EntryErrorType::Uninhabited),
        4 => Some(EntryErrorType::FieldErrors),
        5 => Some(EntryErrorType::Descriptor(read_u64(data, i)?)),
        _ => return None,
    };
    let default_error_type = if read_bool(data, i)? {
        Some(read_u64(data, i)?)
    } else {
        None
    };
    if error_type == Some(EntryErrorType::Default) && default_error_type.is_none() {
        return None;
    }
    if returns_result != error_type.is_some() {
        return None;
    }
    Some(EntryRail {
        entry_symbol,
        returns_result,
        returns_app,
        serves_app,
        error_type,
        default_error_type,
    })
}

pub(super) fn read_u32(data: &[u8], i: &mut usize) -> Option<u32> {
    let slice = data.get(*i..*i + 4)?;
    *i += 4;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

pub(super) fn read_u64(data: &[u8], i: &mut usize) -> Option<u64> {
    let slice = data.get(*i..*i + 8)?;
    *i += 8;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

pub(super) fn read_i64(data: &[u8], i: &mut usize) -> Option<i64> {
    let slice = data.get(*i..*i + 8)?;
    *i += 8;
    Some(i64::from_le_bytes(slice.try_into().ok()?))
}

pub(super) fn read_str(data: &[u8], i: &mut usize) -> Option<String> {
    let len = read_u32(data, i)? as usize;
    let slice = data.get(*i..*i + len)?;
    *i += len;
    String::from_utf8(slice.to_vec()).ok()
}

fn read_u16_slice(data: &[u8], i: &mut usize) -> Option<Vec<u16>> {
    let n = read_u32(data, i)? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let slice = data.get(*i..*i + 2)?;
        *i += 2;
        out.push(u16::from_le_bytes(slice.try_into().ok()?));
    }
    Some(out)
}

/// One decoded artifact: everything a warm run needs that a TIR program would
/// otherwise have answered.
pub(super) struct WarmModule {
    rail: EntryRail,
    /// Tier roster: the functions the cold run reported as tier-1 native.
    native_fns: Vec<(MirFunctionId, String)>,
    strings: Vec<(usize, String)>,
    fns: Vec<CapturedFn>,
}

/// Decode an artifact, refusing every FORMAT but the current one.
///
/// The version gate is load-bearing, not hygiene. A FORMAT 3 artifact carries no
/// entry error rail, so reading one as if it had a rail forgets that the entry is
/// fallible: the error still renders and the process exits 0. Refusing returns
pub(super) fn decode_module(data: &[u8], artifact: MirArtifactId) -> Result<WarmModule, String> {
    let mut i = 0usize;
    match read_u32(data, &mut i) {
        Some(FORMAT) => {}
        Some(found) => {
            return Err(format!(
                "tier-cache: artifact FORMAT {found}, this compiler reads {FORMAT}"
            ))
        }
        None => return Err("tier-cache: artifact has no format word".to_string()),
    }
    let found_artifact = read_u64(data, &mut i)
        .ok_or_else(|| "tier-cache: artifact has no MIR artifact id".to_string())?;
    if found_artifact != artifact.0 {
        return Err(format!(
            "tier-cache: artifact id {found_artifact} does not match requested {:?}",
            artifact
        ));
    }
    decode_body(data, i).ok_or_else(|| "tier-cache: corrupt artifact".to_string())
}

fn decode_body(data: &[u8], start: usize) -> Option<WarmModule> {
    let mut i = start;
    let rail = read_rail(data, &mut i)?;
    let native_n = read_u32(data, &mut i)? as usize;
    let mut native_fns = Vec::with_capacity(native_n);
    for _ in 0..native_n {
        let id = MirFunctionId(read_u64(data, &mut i)?);
        native_fns.push((id, read_str(data, &mut i)?));
    }
    let str_n = read_u32(data, &mut i)? as usize;
    let mut strings = Vec::with_capacity(str_n);
    for _ in 0..str_n {
        let idx = read_u32(data, &mut i)? as usize;
        let text = read_str(data, &mut i)?;
        strings.push((idx, text));
    }
    let n = read_u32(data, &mut i)? as usize;
    let mut fns = Vec::with_capacity(n);
    for _ in 0..n {
        let export_name = read_str(data, &mut i)?;
        let call_conv = *data.get(i)?;
        i += 1;
        let params = read_u16_slice(data, &mut i)?;
        let returns = read_u16_slice(data, &mut i)?;
        let alignment = read_u64(data, &mut i)?;
        let byte_len = read_u32(data, &mut i)? as usize;
        let bytes = data.get(i..i + byte_len)?.to_vec();
        i += byte_len;
        let reloc_n = read_u32(data, &mut i)? as usize;
        let mut relocs = Vec::with_capacity(reloc_n);
        for _ in 0..reloc_n {
            let offset = read_u32(data, &mut i)?;
            let kind = *data.get(i)?;
            i += 1;
            let addend = read_i64(data, &mut i)?;
            let tag = *data.get(i)?;
            i += 1;
            let target = match tag {
                1 => {
                    let namespace = read_u32(data, &mut i)?;
                    let index = read_u32(data, &mut i)?;
                    StoredTarget::User { namespace, index }
                }
                4 => StoredTarget::FuncOffset(read_u32(data, &mut i)?),
                _ => return None,
            };
            relocs.push(StoredReloc {
                offset,
                kind,
                addend,
                target,
            });
        }
        fns.push(CapturedFn {
            export_name,
            sig: EncodedSig {
                call_conv,
                params,
                returns,
            },
            alignment,
            bytes,
            relocs,
        });
    }
    Some(WarmModule {
        rail,
        native_fns,
        strings,
        fns,
    })
}

/// Read the stable artifact identity embedded in a warm module header.
///
/// The caller uses this only before frontend lowering, when the cached module
/// is the source of truth for the artifact being replayed. The full decoder
/// still validates the same identity before loading code.
pub fn cached_artifact_id(data: &[u8]) -> Option<MirArtifactId> {
    let mut index = 0usize;
    if read_u32(data, &mut index) != Some(FORMAT) {
        return None;
    }
    let value = read_u64(data, &mut index)?;
    (value != 0).then_some(MirArtifactId(value))
}

/// Load a previously captured tier-1 module and invoke its checked entry.
///
/// A hit is a tier-1 native execution like any other, so it reports the same
/// rows to `--trace-tiers`. Skipping the compile must not skip the lens: an
/// unreported run reads as a program that reached no tier at all, which is how
pub fn run_cached_module(
    bytes: &[u8],
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, String> {
    let _loaded_mod_scope = crate::Mod::LoadScope;
    if !super::api_debug::cranelift_host_supported() {
        return Err("cranelift host unsupported".into());
    }
    let reload = Instant::now();
    let warm = decode_module(bytes, artifact)?;
    jet_rt::__gc::initialize_trace().map_err(|e| e.to_string())?;
    let native_fns = install_warm_module(
        warm,
        fresh_runtime(release_devtools_policy.clone()),
        |_, _, _| Ok(()),
    )?;
    // The reload is this run's whole tier cost — there is no plan and no
    // compile — so it is what the rows time, and the reason says so rather than
    // letting a 0.05ms row read as a suspiciously fast Cranelift compile.
    let reload_ms = reload.elapsed().as_secs_f64() * 1000.0;
    let outcome = resident_invoke();
    if outcome.is_ok() {
        let observed_rows = native_fns
            .into_iter()
            .map(|(function, function_name)| TierRow {
                function,
                function_name,
                tier: Tier::Native,
                reason: "warm tier-1 module".to_string(),
                millis: reload_ms,
            })
            .collect::<Vec<_>>();
        record_trace(observed_rows.clone());
    }
    outcome
}

/// Install a decoded warm module as the resident module and `runtime` as the
/// resident runtime, ready for `resident_invoke`. The caller initializes GC
/// tracing before creating `runtime`. `restore` runs after the code
/// is finalized (read+execute, never writable) and receives each captured
/// function's id by export name, for state that needs finalized addresses.
/// Returns the module's tier roster.
pub(super) fn install_warm_module(
    warm: WarmModule,
    mut runtime: super::runtime_host::JitRuntime,
    restore: impl FnOnce(
        &JITModule,
        &HashMap<String, FuncId>,
        &mut super::runtime_host::JitRuntime,
    ) -> Result<(), String>,
) -> Result<Vec<(MirFunctionId, String)>, String> {
    let WarmModule {
        rail,
        native_fns,
        strings,
        fns,
    } = warm;
    runtime.heap.install_string_slots(&strings);
    runtime.compile_strings = strings;
    runtime.default_error_type = rail.default_error_type;

    let (mut module, host) = new_jit_module()?;
    let mut ids: HashMap<String, FuncId> = HashMap::new();
    for f in &fns {
        let sig = decode_sig(&module, &f.sig).ok_or("tier-cache: bad signature")?;
        let id = module
            .declare_function(&f.export_name, Linkage::Export, &sig)
            .map_err(|e| e.to_string())?;
        ids.insert(f.export_name.clone(), id);
    }

    for f in &fns {
        let id = ids[&f.export_name];
        let sig = decode_sig(&module, &f.sig).ok_or("tier-cache: bad signature")?;
        let mut func = Function::with_name_signature(UserFuncName::default(), sig);
        let mut name_map = HashMap::new();
        let mut mach_relocs = Vec::new();
        for r in &f.relocs {
            let kind = reloc_from_u8(r.kind).ok_or("tier-cache: bad reloc kind")?;
            let target = match &r.target {
                StoredTarget::User { namespace, index } => {
                    let key = (*namespace, *index);
                    let reff = *name_map.entry(key).or_insert_with(|| {
                        func.declare_imported_user_function(UserExternalName {
                            namespace: *namespace,
                            index: *index,
                        })
                    });
                    FinalizedRelocTarget::ExternalName(ExternalName::user(reff))
                }
                StoredTarget::FuncOffset(off) => FinalizedRelocTarget::Func(*off),
            };
            mach_relocs.push(FinalizedMachReloc {
                offset: r.offset,
                kind,
                addend: r.addend,
                target,
            });
        }
        module
            .define_function_bytes(id, &func, f.alignment, &f.bytes, &mach_relocs)
            .map_err(|e| e.to_string())?;
    }

    module.finalize_definitions().map_err(|e| e.to_string())?;
    let main_id = *ids
        .get(&rail.entry_symbol)
        .ok_or("tier-cache: missing checked entry symbol")?;
    restore(&module, &ids, &mut runtime)?;
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
            program: None,
            artifact: None,
            execution: None,
            typed_entry_id: None,
            // The rail the cold run decided, read back rather than re-derived:
            // a warm run has no TIR program to ask.
            main_returns_result: rail.returns_result,
            main_serves_app: rail.serves_app,
            main_returns_app: rail.returns_app,
            main_error_type: rail.error_type,
        });
    });
    Ok(native_fns)
}
