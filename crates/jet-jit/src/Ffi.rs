//! Resident JIT FFI: load the prepared bridge cdylib and call `*_cabi` trampolines.
//! Same bridge AOT links — no parallel/fake native path.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::AST::{ProgramBundle, Type};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::sync::Mutex;
use crate::Marshal::{clone_string, alloc_string};

#[cfg(unix)]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

#[cfg(unix)]
const RTLD_NOW: c_int = 2;

#[derive(Clone, Copy)]
enum ParamAbi {
    Int,
    Float,
    Bool,
    String,
}

#[derive(Clone, Copy)]
enum RetAbi {
    Unit,
    Int,
    Float,
    Bool,
    String,
}

struct FfiEntry {
    params: Vec<ParamAbi>,
    ret: RetAbi,
    /// Function pointer into the loaded cdylib.
    ptr: *const (),
}

// Safety: entries are only used from the JIT host thread with the library kept alive.
unsafe impl Send for FfiEntry {}
unsafe impl Sync for FfiEntry {}

struct FfiState {
    handle: *mut c_void,
    free_fn: Option<unsafe extern "C" fn(*mut u8, usize)>,
    by_wrapper: HashMap<String, FfiEntry>,
}

unsafe impl Send for FfiState {}

static FFI_STATE: Mutex<Option<FfiState>> = Mutex::new(None);

fn param_abi(ty: &Type) -> Option<ParamAbi> {
    match ty {
        Type::Int => Some(ParamAbi::Int),
        Type::Float | Type::Float32 => Some(ParamAbi::Float),
        Type::Bool => Some(ParamAbi::Bool),
        Type::String => Some(ParamAbi::String),
        _ => None,
    }
}

fn ret_abi(ty: Option<&Type>) -> Option<RetAbi> {
    match ty {
        None => Some(RetAbi::Unit),
        Some(Type::Int) => Some(RetAbi::Int),
        Some(Type::Float) | Some(Type::Float32) => Some(RetAbi::Float),
        Some(Type::Bool) => Some(RetAbi::Bool),
        Some(Type::String) => Some(RetAbi::String),
        _ => None,
    }
}

fn trap(msg: &str) {
    Concurrency::with_runtime_mut(|rt| rt.set_trap(msg));
}

/// Prepare the AOT FFI bridge and bind its C-ABI trampolines for resident JIT.
pub(crate) fn bind_bundle_ffi(bundle: &ProgramBundle) -> Result<(), String> {
    clear_ffi();
    let entries = jet_pkg_model::FFI::collect_externs(bundle);
    if entries.is_empty() {
        return Ok(());
    }
    let link = jet_pkg_model::FFI::prepare(bundle).map_err(|diags| {
        diags
            .into_iter()
            .map(|d| format!("{}: {}", d.code, d.what))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let Some(link) = link else {
        return Err("jit ffi: prepare returned no link for extern entries".into());
    };
    load_cdylib(&link.cdylib_path, &entries)
}

pub(crate) fn clear_ffi() {
    let mut slot = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(state) = slot.take() {
        if !state.handle.is_null() {
            unsafe {
                dlclose(state.handle);
            }
        }
    }
}

pub(crate) fn has_bound_ffi() -> bool {
    FFI_STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

fn load_cdylib(
    path: &Path,
    entries: &[jet_pkg_model::FFI::ExternEntry],
) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (path, entries);
        return Err("jit ffi: cdylib load unsupported on this host".into());
    }
    #[cfg(unix)]
    {
        let c_path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "jit ffi: bad cdylib path".to_string())?;
        let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            let err = unsafe { CStr::from_ptr(dlerror()) }
                .to_string_lossy()
                .into_owned();
            return Err(format!("jit ffi: dlopen {}: {err}", path.display()));
        }
        let free_name = CString::new("jet_ffi_cabi_free").unwrap();
        let free_ptr = unsafe { dlsym(handle, free_name.as_ptr()) };
        let free_fn = if free_ptr.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(*mut u8, usize),
                >(free_ptr)
            })
        };
        let mut by_wrapper = HashMap::new();
        for entry in entries {
            let Some(params) = entry
                .params
                .iter()
                .map(|(_, ty)| param_abi(ty))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(ret) = ret_abi(entry.return_type.as_ref()) else {
                continue;
            };
            let cabi = format!("{}_cabi", entry.wrapper_name);
            let c_name = CString::new(cabi.as_str()).map_err(|_| "jit ffi: bad symbol".to_string())?;
            let ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
            if ptr.is_null() {
                unsafe {
                    dlclose(handle);
                }
                return Err(format!(
                    "jit ffi: missing symbol `{cabi}` in {}",
                    path.display()
                ));
            }
            by_wrapper.insert(
                entry.wrapper_name.clone(),
                FfiEntry {
                    params,
                    ret,
                    ptr: ptr as *const (),
                },
            );
        }
        *FFI_STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(FfiState {
            handle,
            free_fn,
            by_wrapper,
        });
        Ok(())
    }
}

/// `wrapper` / `args` are heap string / int-list handles. Returns Jet ABI value
/// (string handle, i64, f64-bits, bool as i64).
extern "C" fn jet_jit_extern_call(wrapper: i64, args: i64) -> i64 {
    let name = clone_string(wrapper);
    let argv = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(args).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(args, i).unwrap_or(0));
        }
        out
    });
    let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = state.as_ref() else {
        trap("jit ffi: no bridge bound");
        return 0;
    };
    let Some(entry) = state.by_wrapper.get(&name) else {
        trap(&format!("jit ffi: unbound wrapper `{name}`"));
        return 0;
    };
    if argv.len() != entry.params.len() {
        trap(&format!(
            "jit ffi: `{name}` argc {} != {}",
            argv.len(),
            entry.params.len()
        ));
        return 0;
    }
    // Materialize owned string args so pointers stay live for the call.
    let mut owned_strings: Vec<String> = Vec::new();
    for (i, abi) in entry.params.iter().enumerate() {
        if matches!(abi, ParamAbi::String) {
            owned_strings.push(clone_string(argv[i]));
        }
    }
    // Specialized fast paths for the shapes examples use.
    match (entry.params.as_slice(), entry.ret) {
        ([ParamAbi::String], RetAbi::String) => {
            let s = &owned_strings[0];
            type FnStr = unsafe extern "C" fn(
                *const u8,
                usize,
                *mut *mut u8,
                *mut usize,
            ) -> i32;
            let f: FnStr = unsafe { std::mem::transmute(entry.ptr) };
            let mut out_ptr: *mut u8 = std::ptr::null_mut();
            let mut out_len: usize = 0;
            let rc = unsafe { f(s.as_ptr(), s.len(), &mut out_ptr, &mut out_len) };
            if rc != 0 {
                trap(&format!("jit ffi: `{name}` returned {rc}"));
                return 0;
            }
            let bytes = if out_ptr.is_null() {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(out_ptr, out_len) }.to_vec()
            };
            if let Some(free) = state.free_fn {
                if !out_ptr.is_null() {
                    unsafe { free(out_ptr, out_len) };
                }
            }
            return alloc_string(String::from_utf8_lossy(&bytes).into_owned());
        }
        ([ParamAbi::Int, ParamAbi::Int], RetAbi::Int) => {
            type FnII = unsafe extern "C" fn(i64, i64) -> i64;
            let f: FnII = unsafe { std::mem::transmute(entry.ptr) };
            return unsafe { f(argv[0], argv[1]) };
        }
        ([ParamAbi::Int], RetAbi::Unit) => {
            type FnI = unsafe extern "C" fn(i64);
            let f: FnI = unsafe { std::mem::transmute(entry.ptr) };
            unsafe { f(argv[0]) };
            return 0;
        }
        ([ParamAbi::Float], RetAbi::Float) => {
            type FnF = unsafe extern "C" fn(f64) -> f64;
            let f: FnF = unsafe { std::mem::transmute(entry.ptr) };
            let value = unsafe { f(f64::from_bits(argv[0] as u64)) };
            return value.to_bits() as i64;
        }
        (
            [
                ParamAbi::Float,
                ParamAbi::Float,
                ParamAbi::Float,
                ParamAbi::Float,
                ParamAbi::Float,
                ParamAbi::Float,
            ],
            RetAbi::Float,
        ) => {
            type Fn6F = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64;
            let f: Fn6F = unsafe { std::mem::transmute(entry.ptr) };
            let value = unsafe {
                f(
                    f64::from_bits(argv[0] as u64),
                    f64::from_bits(argv[1] as u64),
                    f64::from_bits(argv[2] as u64),
                    f64::from_bits(argv[3] as u64),
                    f64::from_bits(argv[4] as u64),
                    f64::from_bits(argv[5] as u64),
                )
            };
            return value.to_bits() as i64;
        }
        _ => {}
    }

    // Generic scalar path (no strings).
    if entry.params.iter().any(|p| matches!(p, ParamAbi::String))
        || matches!(entry.ret, RetAbi::String)
    {
        trap(&format!("jit ffi: unsupported signature for `{name}`"));
        return 0;
    }
    match (entry.params.as_slice(), entry.ret) {
        ([], RetAbi::Unit) => {
            type Fn0 = unsafe extern "C" fn();
            let f: Fn0 = unsafe { std::mem::transmute(entry.ptr) };
            unsafe { f() };
            0
        }
        ([], RetAbi::Int) => {
            type Fn0 = unsafe extern "C" fn() -> i64;
            let f: Fn0 = unsafe { std::mem::transmute(entry.ptr) };
            unsafe { f() }
        }
        ([ParamAbi::Int], RetAbi::Int) => {
            type Fn1 = unsafe extern "C" fn(i64) -> i64;
            let f: Fn1 = unsafe { std::mem::transmute(entry.ptr) };
            unsafe { f(argv[0]) }
        }
        _ => {
            trap(&format!("jit ffi: unsupported signature for `{name}`"));
            0
        }
    }
}

host_fns! {
    struct FfiHostFns;
    register: register_ffi_host_symbols;
    declare: declare_ffi_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig = Signature::new(cc);
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
    }
    call: "jet_jit_extern_call" => jet_jit_extern_call: sig;
}
