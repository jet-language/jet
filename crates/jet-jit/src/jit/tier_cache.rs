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
use cranelift_codegen::{FinalizedMachReloc, FinalizedRelocTarget};
use cranelift_codegen::Context;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::{JitBackend::RunOutcome, AST::Type};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use super::resident::{fresh_runtime, resident_invoke, resident_teardown};
use super::runtime_host::{new_jit_module, ResidentModule};
use super::tiers::{record_trace, Tier, TierRow};
use super::{RESIDENT_MODULE, RESIDENT_RUNTIME};

/// On-disk artifact version.
///
/// FORMAT 5 added the tier roster: the function names the cold run reported as
/// tier-1 native. Without it a warm hit printed NOTHING under `--trace-tiers`,
/// so a native replay was indistinguishable from a run that reached no tier at
/// all — the exact silence the tier lens exists to prevent.
///
/// FORMAT 4 added the entry error rail ([`EntryRail`]). A FORMAT 3 artifact is
/// REFUSED rather than read: it cannot say whether the entry is fallible, so
/// replaying one renders the error and still exits 0 from a program that failed.
const FORMAT: u32 = 5;

thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

/// One in-progress capture: the functions defined so far, and the `FuncId` each
/// one held, in the same order.
///
/// The ids are capture-time bookkeeping for `capture_is_replayable`, never
/// artifact content, so they stay out of `CapturedFn` and are read by zipping the
/// two — no positional lookup into either.
#[derive(Default)]
struct Capture {
    func_ids: Vec<u32>,
    fns: Vec<CapturedFn>,
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
/// The rail is decided once, in `resident::ensure_resident_module`, from the TIR
/// program, and lives on `ResidentModule`. A warm run never sees the program, so
/// the artifact carries the same decided values; no engine re-derives them.
///
/// The error type travels as a NAME. Every consumer of `main_error_type` only
/// ever tests `Type::Named` — `resident_invoke` renders a packed enum by name —
/// so a name is the whole payload. An error type that is not `Named` leaves the
/// name empty, and both booleans that need a name (`returns_default_err`,
/// `error_is_packed`) are already false in exactly that case, so the reload
/// takes the same branch the cold run took.
struct EntryRail {
    returns_result: bool,
    returns_app: bool,
    returns_default_err: bool,
    error_is_packed: bool,
    error_name: Option<String>,
}

/// Read the rail the cold run already decided, off the live resident module.
///
/// `None` means there is no resident module to read, and then there is no
/// artifact either: an artifact that cannot say whether the entry is fallible is
/// worse than a cold recompile.
fn capture_rail() -> Option<EntryRail> {
    RESIDENT_MODULE.with(|slot| {
        slot.borrow().as_ref().map(|resident| EntryRail {
            returns_result: resident.main_returns_result,
            returns_app: resident.main_returns_app,
            returns_default_err: resident.main_returns_default_err,
            error_is_packed: resident.main_error_is_packed,
            error_name: match &resident.main_error_type {
                Some(Type::Named(name)) => Some(name.clone()),
                _ => None,
            },
        })
    })
}

pub(crate) fn begin_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = Some(Capture::default()));
}

pub(crate) fn abort_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn take_capture() -> Option<Vec<CapturedFn>> {
    let capture = CAPTURE.with(|slot| slot.borrow_mut().take())?;
    if capture.fns.is_empty()
        || !capture
            .fns
            .iter()
            .any(|f| f.export_name == "__jet_jit_main")
    {
        return None;
    }
    if !capture_is_replayable(&capture) {
        return None;
    }
    Some(capture.fns)
}

/// Can `run_cached_module` reproduce the `FuncId` numbering this capture was
/// compiled against?
///
/// A reload declares the host functions first (`new_jit_module`, deterministic
/// and identical every time), then the captured functions in list order. So the
/// numbering is reproduced only when
///
/// * the captured ids are consecutive from the first one, and
/// * every relocation names a function (namespace 0) that is either a host
///   (`index < first`) or one of the captured ones (`index < first + len`).
///
/// Any program that DECLARES a function the capture skips fails one of those,
/// and that is not hypothetical: `lower_generator_body` and
/// `lower_generator_wrapper` call `define_function` with no `note_defined`, so a
/// generator leaves a hole in the captured id range and the consumer's call to
/// the generator is stored as an index the reload never declares. Replaying it
/// indexed one past the reloaded table and panicked inside
/// `Module::get_function_decl` — an ICE with exit 101 on the SECOND `jet run` of
/// any program containing a generator, while every cold run stayed correct.
///
/// Refusing the artifact keeps such a program on the cold path: correct, only
/// slower. Per I2 the guard belongs here, where an inconsistent artifact would
/// otherwise be written, and never as a clamp at replay — a clamp would turn a
/// wrong relocation into a wrong call.
fn capture_is_replayable(capture: &Capture) -> bool {
    let Some(&first) = capture.func_ids.first() else {
        return false;
    };
    let first = u64::from(first);
    let limit = first + capture.fns.len() as u64;
    let mut expected = first;
    for (&func_id, f) in capture.func_ids.iter().zip(&capture.fns) {
        if u64::from(func_id) != expected {
            return false;
        }
        expected += 1;
        let replayable = f.relocs.iter().all(|reloc| match &reloc.target {
            // Namespace 1 is a data object, and a reload declares no data
            // objects at all, so a data relocation is equally unreplayable.
            StoredTarget::User { namespace, index } => {
                *namespace == 0 && u64::from(*index) < limit
            }
            StoredTarget::FuncOffset(_) => true,
        });
        if !replayable {
            return false;
        }
    }
    true
}

thread_local! {
    static LAST_ARTIFACT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Move a successful capture into the process-local "last artifact" slot.
///
/// `native_fns` is the tier roster this run is publishing with its code. A
/// capture is only published from `resident_run_fresh`, which runs with an
/// empty deopt list, so every listed function is tier-1 native with no reason —
/// exactly the rows the cold run printed under `--trace-tiers`.
pub(crate) fn publish_capture(native_fns: &[&str]) {
    // FFI entries point into a process-local cdylib. A disk artifact cannot
    // recreate that bridge on a warm run, so force the bundle through the cold
    // entry that binds its FFI table before execution.
    if crate::Ffi::has_bound_ffi() {
        abort_capture();
        LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = None);
        return;
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
        LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = None);
        return;
    }
    let fns = take_capture();
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
    if let (Some(fns), Some(rail)) = (fns, capture_rail()) {
        let bytes = encode_module(&rail, native_fns, &fns, &strings);
        LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = Some(bytes));
    } else {
        LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = None);
    }
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
        let Some(capture) = guard.as_mut() else {
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
                *guard = None;
                return;
            };
            relocs.push(stored);
        }
        let Some(sig) = try_encode_sig(&ctx.func.signature) else {
            *guard = None;
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
    rail: &EntryRail,
    native_fns: &[&str],
    fns: &[CapturedFn],
    strings: &[(usize, String)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&FORMAT.to_le_bytes());
    write_rail(&mut out, rail);
    out.extend_from_slice(&(native_fns.len() as u32).to_le_bytes());
    for name in native_fns {
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

fn write_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn write_rail(out: &mut Vec<u8>, rail: &EntryRail) {
    out.push(u8::from(rail.returns_result));
    out.push(u8::from(rail.returns_app));
    out.push(u8::from(rail.returns_default_err));
    out.push(u8::from(rail.error_is_packed));
    match &rail.error_name {
        Some(name) => {
            out.push(1);
            write_str(out, name);
        }
        None => out.push(0),
    }
}

fn write_u16_slice(out: &mut Vec<u8>, v: &[u16]) {
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

fn read_bool(data: &[u8], i: &mut usize) -> Option<bool> {
    let byte = *data.get(*i)?;
    *i += 1;
    match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn read_rail(data: &[u8], i: &mut usize) -> Option<EntryRail> {
    let returns_result = read_bool(data, i)?;
    let returns_app = read_bool(data, i)?;
    let returns_default_err = read_bool(data, i)?;
    let error_is_packed = read_bool(data, i)?;
    let error_name = if read_bool(data, i)? {
        Some(read_str(data, i)?)
    } else {
        None
    };
    Some(EntryRail {
        returns_result,
        returns_app,
        returns_default_err,
        error_is_packed,
        error_name,
    })
}

fn read_u32(data: &[u8], i: &mut usize) -> Option<u32> {
    let slice = data.get(*i..*i + 4)?;
    *i += 4;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn read_u64(data: &[u8], i: &mut usize) -> Option<u64> {
    let slice = data.get(*i..*i + 8)?;
    *i += 8;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

fn read_i64(data: &[u8], i: &mut usize) -> Option<i64> {
    let slice = data.get(*i..*i + 8)?;
    *i += 8;
    Some(i64::from_le_bytes(slice.try_into().ok()?))
}

fn read_str(data: &[u8], i: &mut usize) -> Option<String> {
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
struct WarmModule {
    rail: EntryRail,
    /// Tier roster: the functions the cold run reported as tier-1 native.
    native_fns: Vec<String>,
    strings: Vec<(usize, String)>,
    fns: Vec<CapturedFn>,
}

/// Decode an artifact, refusing every FORMAT but the current one.
///
/// The version gate is load-bearing, not hygiene. A FORMAT 3 artifact carries no
/// entry error rail, so reading one as if it had a rail forgets that the entry is
/// fallible: the error still renders and the process exits 0. Refusing returns
/// `Err`, and `RunCache::try_warm_run` then drops the entry directory and
/// recompiles cold — correct, only slower.
fn decode_module(data: &[u8]) -> Result<WarmModule, String> {
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
    decode_body(data, i).ok_or_else(|| "tier-cache: corrupt artifact".to_string())
}

fn decode_body(data: &[u8], start: usize) -> Option<WarmModule> {
    let mut i = start;
    let rail = read_rail(data, &mut i)?;
    let native_n = read_u32(data, &mut i)? as usize;
    let mut native_fns = Vec::with_capacity(native_n);
    for _ in 0..native_n {
        native_fns.push(read_str(data, &mut i)?);
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

/// Load a previously captured tier-1 module and invoke `__jet_jit_main`.
///
/// A hit is a tier-1 native execution like any other, so it reports the same
/// rows to `--trace-tiers`. Skipping the compile must not skip the lens: an
/// unreported run reads as a program that reached no tier at all, which is how
/// a silent deopt would look too.
pub fn run_cached_module(artifact: &[u8]) -> Result<RunOutcome, String> {
    if !super::api_debug::cranelift_host_supported() {
        return Err("cranelift host unsupported".into());
    }
    let reload = Instant::now();
    let WarmModule {
        rail,
        native_fns,
        strings,
        fns,
    } = decode_module(artifact)?;
    jet_rt::__gc::initialize_trace().map_err(|e| e.to_string())?;
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| {
        let mut rt = fresh_runtime();
        rt.heap.install_string_slots(&strings);
        rt.compile_strings = strings.clone();
        *slot.borrow_mut() = Some(rt);
    });

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
        .get("__jet_jit_main")
        .ok_or("tier-cache: missing __jet_jit_main")?;
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
            // The rail the cold run decided, read back rather than re-derived:
            // a warm run has no TIR program to ask.
            main_returns_result: rail.returns_result,
            main_returns_app: rail.returns_app,
            main_returns_default_err: rail.returns_default_err,
            main_error_type: rail.error_name.map(Type::Named),
            main_error_is_packed: rail.error_is_packed,
        });
    });
    // The reload is this run's whole tier cost — there is no plan and no
    // compile — so it is what the rows time, and the reason says so rather than
    // letting a 0.05ms row read as a suspiciously fast Cranelift compile.
    let reload_ms = reload.elapsed().as_secs_f64() * 1000.0;
    let outcome = resident_invoke();
    if outcome.is_ok() {
        record_trace(
            native_fns
                .into_iter()
                .map(|function| TierRow {
                    function,
                    tier: Tier::Native,
                    reason: "warm tier-1 module".to_string(),
                    millis: reload_ms,
                })
                .collect(),
        );
    }
    outcome
}
