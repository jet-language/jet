//! Disk-backed warm reuse of compiled tier-1 Cranelift modules (#741).
//!
//! A hit reloads machine code via `define_function_bytes` and skips Jet
//! load/parse/check/TIR lowering and Cranelift IR generation. Keying and
//! WatchService invalidation live in `Source/RunCache.rs`.

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
use jet_foundation::JitBackend::RunOutcome;
use std::cell::RefCell;
use std::collections::HashMap;

use super::resident::{fresh_runtime, resident_invoke, resident_teardown};
use super::runtime_host::{new_jit_module, ResidentModule};
use super::{RESIDENT_MODULE, RESIDENT_RUNTIME};

const FORMAT: u32 = 3;

thread_local! {
    static CAPTURE: RefCell<Option<Vec<CapturedFn>>> = const { RefCell::new(None) };
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

pub(crate) fn begin_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
}

pub(crate) fn abort_capture() {
    CAPTURE.with(|slot| *slot.borrow_mut() = None);
}

pub(crate) fn take_capture() -> Option<Vec<CapturedFn>> {
    let fns = CAPTURE.with(|slot| slot.borrow_mut().take())?;
    if fns.is_empty() || !fns.iter().any(|f| f.export_name == "__jet_jit_main") {
        return None;
    }
    Some(fns)
}

thread_local! {
    static LAST_ARTIFACT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Move a successful capture into the process-local "last artifact" slot.
pub(crate) fn publish_capture() {
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
    if let Some(fns) = fns {
        let bytes = encode_module(&fns, &strings);
        LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = Some(bytes));
    } else {
        LAST_ARTIFACT.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Take the artifact produced by the most recent successful native compile, if any.
pub fn take_last_tier_artifact() -> Option<Vec<u8>> {
    LAST_ARTIFACT.with(|slot| slot.borrow_mut().take())
}

pub(crate) fn note_defined(export_name: &str, ctx: &Context) {
    CAPTURE.with(|slot| {
        let mut guard = slot.borrow_mut();
        let Some(buf) = guard.as_mut() else {
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
        buf.push(CapturedFn {
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

fn encode_module(fns: &[CapturedFn], strings: &[(usize, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&FORMAT.to_le_bytes());
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

fn write_u16_slice(out: &mut Vec<u8>, v: &[u16]) {
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
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

fn decode_module(data: &[u8]) -> Option<(Vec<(usize, String)>, Vec<CapturedFn>)> {
    let mut i = 0usize;
    if read_u32(data, &mut i)? != FORMAT {
        return None;
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
    Some((strings, fns))
}

/// Load a previously captured tier-1 module and invoke `__jet_jit_main`.
pub fn run_cached_module(artifact: &[u8]) -> Result<RunOutcome, String> {
    if !super::api_debug::cranelift_host_supported() {
        return Err("cranelift host unsupported".into());
    }
    let (strings, fns) =
        decode_module(artifact).ok_or_else(|| "tier-cache: corrupt artifact".to_string())?;
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
            main_returns_result: false,
            main_returns_web_app: false,
            main_returns_default_err: false,
        });
    });
    resident_invoke()
}
