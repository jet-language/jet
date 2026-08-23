//! Resident JIT FFI: load the prepared bridge cdylib and call `*_cabi` trampolines.
//! Same bridge AOT links — no parallel/fake native path.

use super::Concurrency;
use crate::Marshal::{alloc_string, clone_string};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::AST::{AccessConvention, CtFloat, CtValue, ProgramBundle, Type};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::sync::Mutex;

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

/// The bridge's panic reporter, as a plain Rust `fn`.
///
/// D-JITUNWIND1 (#1995 / #1997): the `extern "C"` frame the bridge actually
/// calls is the shim `host_seam::guarded` generates below, never this body. That
/// is the whole point of the rule — rustc gives an `extern "C"` *body* an
/// abort-on-unwind shim, so had this stayed `extern "C" fn` a panic raised here
/// would die as `thread caused non-unwinding panic` at its own edge, before any
/// guarded seam below it could catch.
///
/// This seam is worth naming because the bridge calls it *precisely* when
/// something already went wrong: a foreign function failed inside a `*_cabi`
/// trampoline that generated code called, so a Cranelift frame is on the stack
/// below every line of this body — the lossy decode's allocation, the
/// `ACTIVE_RUNTIME` borrow, and `set_trap`'s report formatting alike.
fn ffi_reporter(message: *const u8, len: usize) {
    let message = if message.is_null() {
        "a foreign function panicked".into()
    } else {
        // JET_VETTED_UNSAFE_BEGIN: ffi_reporter
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(message, len) }).into_owned()
        // JET_VETTED_UNSAFE_END: ffi_reporter
    };
    Concurrency::with_runtime_mut(|rt| rt.set_trap(&message));
}

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

enum BindError {
    Diagnostics(Vec<Diagnostic>),
    Message(String),
}

fn param_abi(ty: &Type) -> Option<ParamAbi> {
    match ty {
        Type::Int | Type::InlineRange { .. } => Some(ParamAbi::Int),
        Type::Float | Type::Float32 => Some(ParamAbi::Float),
        Type::Bool => Some(ParamAbi::Bool),
        Type::String => Some(ParamAbi::String),
        Type::Apply { name, .. } if name == jet_foundation::Syntax::TYPE_CHECKED_TEXT => {
            Some(ParamAbi::String)
        }
        _ => None,
    }
}

fn ret_abi(ty: Option<&Type>) -> Option<RetAbi> {
    match ty {
        None => Some(RetAbi::Unit),
        Some(Type::Int) | Some(Type::InlineRange { .. }) => Some(RetAbi::Int),
        Some(Type::Float) | Some(Type::Float32) => Some(RetAbi::Float),
        Some(Type::Bool) => Some(RetAbi::Bool),
        Some(Type::String) => Some(RetAbi::String),
        Some(Type::Apply { name, .. }) if name == jet_foundation::Syntax::TYPE_CHECKED_TEXT => {
            Some(RetAbi::String)
        }
        _ => None,
    }
}

fn trap(msg: &str) {
    Concurrency::with_runtime_mut(|rt| rt.set_trap(msg));
}

/// Prepare the AOT FFI bridge and bind its C-ABI trampolines for resident JIT.
pub(crate) fn bind_bundle_ffi(bundle: &ProgramBundle) -> Result<(), String> {
    bind_bundle_ffi_impl(bundle).map_err(|error| match error {
        BindError::Diagnostics(diags) => diags
            .into_iter()
            .map(|d| format!("{}: {}", d.code, d.what))
            .collect::<Vec<_>>()
            .join("; "),
        BindError::Message(reason) => reason,
    })
}

/// Bind the bridge for tier 0 while preserving capability diagnostics from
/// bridge preparation (for example E3201 when a native library is absent).
pub(crate) fn bind_bundle_ffi_for_interpreter(
    bundle: &ProgramBundle,
) -> Result<(), Vec<Diagnostic>> {
    bind_bundle_ffi_impl(bundle).map_err(|error| match error {
        BindError::Diagnostics(diags) => diags,
        BindError::Message(reason) => vec![Diagnostic::error(
            "E0956",
            format!("interpreter FFI bridge unavailable: {reason}"),
            "the interpreter could not load the bridge used by the native FFI path".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )],
    })
}

fn bind_bundle_ffi_impl(bundle: &ProgramBundle) -> Result<(), BindError> {
    clear_ffi();
    let entries = jet_pkg_model::FFI::collect_externs(bundle);
    if entries.is_empty() {
        return Ok(());
    }
    let link = jet_pkg_model::FFI::prepare(bundle).map_err(BindError::Diagnostics)?;
    let Some(link) = link else {
        return Err(BindError::Message(
            "jit ffi: prepare returned no link for extern entries".into(),
        ));
    };
    load_cdylib(&link.cdylib_path, &entries).map_err(BindError::Message)
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

fn load_cdylib(path: &Path, entries: &[jet_pkg_model::FFI::ExternEntry]) -> Result<(), String> {
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
        let setter_name = CString::new("jet_ffi_set_reporter").unwrap();
        let setter_ptr = unsafe { dlsym(handle, setter_name.as_ptr()) };
        if setter_ptr.is_null() {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_set_reporter` in {}",
                path.display()
            ));
        }
        // The reporter the bridge stores is the generated no-unwind shim, whose
        // C signature is `ffi_reporter`'s own (D-JITUNWIND1). The setter is
        // typed to take that address rather than a `fn` item so the shim is the
        // only thing this crate can pass: a raw `ffi_reporter as *const u8` here
        // would be the unguarded boundary
        // `tests/jit_no_unwind_boundary.rs` scans for.
        let set_reporter = unsafe {
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*const u8)>(setter_ptr)
        };
        unsafe { set_reporter(crate::host_seam::guarded(ffi_reporter)) };
        let free_name = CString::new("jet_ffi_cabi_free").unwrap();
        let free_ptr = unsafe { dlsym(handle, free_name.as_ptr()) };
        let free_fn = if free_ptr.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut u8, usize)>(free_ptr)
            })
        };
        if free_fn.is_none()
            && entries.iter().any(|entry| {
                matches!(entry.return_type.as_ref(), Some(Type::String))
                    && entry.params.iter().all(|(convention, ty)| {
                        *convention == AccessConvention::Read
                            && matches!(
                                ty,
                                Type::Int
                                    | Type::InlineRange { .. }
                                    | Type::Float
                                    | Type::Float32
                                    | Type::Bool
                                    | Type::String
                            )
                    })
            })
        {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_cabi_free` in {}",
                path.display()
            ));
        }
        let mut by_wrapper = HashMap::new();
        for entry in entries {
            // D-FFI-CAP1: capability calls are native-boundary operations. The
            // resident JIT can marshal only value-shaped `*_cabi` entries; it
            // must not fabricate a by-value adapter for `&` or `^`.
            if entry
                .params
                .iter()
                .any(|(convention, _)| *convention != AccessConvention::Read)
            {
                continue;
            }
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
            let c_name =
                CString::new(cabi.as_str()).map_err(|_| "jit ffi: bad symbol".to_string())?;
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

fn ffi_diag(wrapper: &str, detail: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("extern call `{wrapper}` {}", detail.into()),
        "the runtime FFI adapter could not marshal this call through the prepared bridge"
            .to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn release_cabi_buffer(
    free_fn: Option<unsafe extern "C" fn(*mut u8, usize)>,
    ptr: *mut u8,
    len: usize,
) {
    if !ptr.is_null() {
        if let Some(free) = free_fn {
            unsafe { free(ptr, len) };
        }
    }
}

/// Call one prepared bridge entry from the canonical TIR evaluator.
///
/// This is the interpreter-side adapter for the same `*_cabi` symbols that
/// Cranelift calls. It does not call a raw foreign symbol or reproduce the C
/// wrapper's conversion/policy logic; the generated bridge remains the one
/// semantic boundary shared with AOT.
pub(crate) fn call_ctvalue(
    wrapper: &str,
    args: &[CtValue],
    ret_ty: Option<&Type>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = state.as_ref() else {
        return Err(ffi_diag(wrapper, "has no prepared bridge", span));
    };
    let Some(entry) = state.by_wrapper.get(wrapper) else {
        return Err(ffi_diag(
            wrapper,
            "is not bound in the prepared bridge",
            span,
        ));
    };
    if args.len() != entry.params.len() {
        return Err(ffi_diag(
            wrapper,
            format!("argc {} != {}", args.len(), entry.params.len()),
            span,
        ));
    }

    let strings: Vec<Option<String>> = entry
        .params
        .iter()
        .enumerate()
        .map(|(index, abi)| match abi {
            ParamAbi::String => match args.get(index) {
                Some(CtValue::Str(value)) => Ok(Some(value.clone())),
                _ => Err(ffi_diag(
                    wrapper,
                    format!("argument {index} is not a String"),
                    span,
                )),
            },
            _ => Ok(None),
        })
        .collect::<Result<_, _>>()?;
    let int_arg = |index: usize| match args.get(index) {
        Some(CtValue::Int(value)) => Ok(*value),
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not an Int"),
            span,
        )),
    };
    let float_arg = |index: usize| match args.get(index) {
        Some(CtValue::Float(value)) => Ok(value.as_f64()),
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not a Float"),
            span,
        )),
    };
    let float_result = |value: f64| {
        if matches!(ret_ty, Some(Type::Float32)) {
            CtValue::Float(CtFloat::f32(value as f32))
        } else {
            CtValue::Float(CtFloat::f64(value))
        }
    };

    match (entry.params.as_slice(), entry.ret) {
        ([ParamAbi::String], RetAbi::Int) => {
            type FnStrInt = unsafe extern "C" fn(*const u8, usize) -> i64;
            let f: FnStrInt = unsafe { std::mem::transmute(entry.ptr) };
            let value = strings[0].as_deref().expect("String ABI argument");
            Ok(CtValue::Int(unsafe { f(value.as_ptr(), value.len()) }))
        }
        ([ParamAbi::String], RetAbi::String) => {
            type FnStr = unsafe extern "C" fn(*const u8, usize, *mut *mut u8, *mut usize) -> i32;
            let f: FnStr = unsafe { std::mem::transmute(entry.ptr) };
            let value = strings[0].as_deref().expect("String ABI argument");
            let mut out_ptr = std::ptr::null_mut();
            let mut out_len = 0;
            let rc = unsafe { f(value.as_ptr(), value.len(), &mut out_ptr, &mut out_len) };
            if rc != 0 {
                release_cabi_buffer(state.free_fn, out_ptr, out_len);
                return Err(ffi_diag(wrapper, format!("returned {rc}"), span));
            }
            if out_ptr.is_null() && out_len != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if out_ptr.is_null() {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(out_ptr, out_len) }.to_vec()
            };
            release_cabi_buffer(state.free_fn, out_ptr, out_len);
            String::from_utf8(bytes)
                .map(CtValue::Str)
                .map_err(|_| ffi_diag(wrapper, "returned invalid UTF-8", span))
        }
        ([ParamAbi::Int, ParamAbi::String], RetAbi::Int) => {
            type FnIntStrInt = unsafe extern "C" fn(i64, *const u8, usize) -> i64;
            let f: FnIntStrInt = unsafe { std::mem::transmute(entry.ptr) };
            let code = strings[1].as_deref().expect("String ABI argument");
            Ok(CtValue::Int(unsafe {
                f(int_arg(0)?, code.as_ptr(), code.len())
            }))
        }
        ([ParamAbi::Int, ParamAbi::String], RetAbi::Float) => {
            type FnIntStrFloat = unsafe extern "C" fn(i64, *const u8, usize) -> f64;
            let f: FnIntStrFloat = unsafe { std::mem::transmute(entry.ptr) };
            let code = strings[1].as_deref().expect("String ABI argument");
            Ok(float_result(unsafe {
                f(int_arg(0)?, code.as_ptr(), code.len())
            }))
        }
        ([ParamAbi::Int, ParamAbi::String], RetAbi::String) => {
            type FnIntStr =
                unsafe extern "C" fn(i64, *const u8, usize, *mut *mut u8, *mut usize) -> i32;
            let f: FnIntStr = unsafe { std::mem::transmute(entry.ptr) };
            let code = strings[1].as_deref().expect("String ABI argument");
            let mut out_ptr = std::ptr::null_mut();
            let mut out_len = 0;
            let rc = unsafe {
                f(
                    int_arg(0)?,
                    code.as_ptr(),
                    code.len(),
                    &mut out_ptr,
                    &mut out_len,
                )
            };
            if rc != 0 {
                release_cabi_buffer(state.free_fn, out_ptr, out_len);
                return Err(ffi_diag(wrapper, format!("returned {rc}"), span));
            }
            if out_ptr.is_null() && out_len != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if out_ptr.is_null() {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(out_ptr, out_len) }.to_vec()
            };
            release_cabi_buffer(state.free_fn, out_ptr, out_len);
            String::from_utf8(bytes)
                .map(CtValue::Str)
                .map_err(|_| ffi_diag(wrapper, "returned invalid UTF-8", span))
        }
        ([ParamAbi::Int, ParamAbi::String, ParamAbi::Int], RetAbi::String) => {
            type FnIntStrInt = unsafe extern "C" fn(
                i64,
                *const u8,
                usize,
                i64,
                *mut *mut u8,
                *mut usize,
            ) -> i32;
            let f: FnIntStrInt = unsafe { std::mem::transmute(entry.ptr) };
            let code = strings[1].as_deref().expect("String ABI argument");
            let mut out_ptr = std::ptr::null_mut();
            let mut out_len = 0;
            let rc = unsafe {
                f(
                    int_arg(0)?,
                    code.as_ptr(),
                    code.len(),
                    int_arg(2)?,
                    &mut out_ptr,
                    &mut out_len,
                )
            };
            if rc != 0 {
                release_cabi_buffer(state.free_fn, out_ptr, out_len);
                return Err(ffi_diag(wrapper, format!("returned {rc}"), span));
            }
            if out_ptr.is_null() && out_len != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if out_ptr.is_null() {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(out_ptr, out_len) }.to_vec()
            };
            release_cabi_buffer(state.free_fn, out_ptr, out_len);
            String::from_utf8(bytes)
                .map(CtValue::Str)
                .map_err(|_| ffi_diag(wrapper, "returned invalid UTF-8", span))
        }
        ([ParamAbi::Int], RetAbi::Int) => {
            type FnInt = unsafe extern "C" fn(i64) -> i64;
            let f: FnInt = unsafe { std::mem::transmute(entry.ptr) };
            Ok(CtValue::Int(unsafe { f(int_arg(0)?) }))
        }
        ([ParamAbi::Int, ParamAbi::Int], RetAbi::Int) => {
            type FnIntInt = unsafe extern "C" fn(i64, i64) -> i64;
            let f: FnIntInt = unsafe { std::mem::transmute(entry.ptr) };
            Ok(CtValue::Int(unsafe { f(int_arg(0)?, int_arg(1)?) }))
        }
        ([ParamAbi::Int], RetAbi::Unit) => {
            type FnInt = unsafe extern "C" fn(i64);
            let f: FnInt = unsafe { std::mem::transmute(entry.ptr) };
            unsafe { f(int_arg(0)?) };
            Ok(CtValue::Unit)
        }
        ([ParamAbi::Bool], RetAbi::Bool) => {
            type FnBool = unsafe extern "C" fn(i8) -> i8;
            let f: FnBool = unsafe { std::mem::transmute(entry.ptr) };
            let value = match args.first() {
                Some(CtValue::Bool(value)) => *value,
                _ => {
                    return Err(ffi_diag(wrapper, "argument 0 is not a Bool", span));
                }
            };
            Ok(CtValue::Bool(unsafe { f(if value { 1 } else { 0 }) } != 0))
        }
        ([ParamAbi::Float], RetAbi::Float) => {
            type FnFloat = unsafe extern "C" fn(f64) -> f64;
            let f: FnFloat = unsafe { std::mem::transmute(entry.ptr) };
            Ok(float_result(unsafe { f(float_arg(0)?) }))
        }
        (
            [ParamAbi::Float, ParamAbi::Float, ParamAbi::Float, ParamAbi::Float, ParamAbi::Float, ParamAbi::Float],
            RetAbi::Float,
        ) => {
            type FnSixFloat = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64;
            let f: FnSixFloat = unsafe { std::mem::transmute(entry.ptr) };
            Ok(float_result(unsafe {
                f(
                    float_arg(0)?,
                    float_arg(1)?,
                    float_arg(2)?,
                    float_arg(3)?,
                    float_arg(4)?,
                    float_arg(5)?,
                )
            }))
        }
        ([], RetAbi::Unit) => {
            type FnUnit = unsafe extern "C" fn();
            let f: FnUnit = unsafe { std::mem::transmute(entry.ptr) };
            unsafe { f() };
            Ok(CtValue::Unit)
        }
        ([], RetAbi::Int) => {
            type FnUnitInt = unsafe extern "C" fn() -> i64;
            let f: FnUnitInt = unsafe { std::mem::transmute(entry.ptr) };
            Ok(CtValue::Int(unsafe { f() }))
        }
        ([], RetAbi::Bool) => {
            type FnUnitBool = unsafe extern "C" fn() -> i8;
            let f: FnUnitBool = unsafe { std::mem::transmute(entry.ptr) };
            Ok(CtValue::Bool(unsafe { f() } != 0))
        }
        _ => Err(ffi_diag(wrapper, "has an unsupported signature", span)),
    }
}

/// `wrapper` / `args` are heap string / int-list handles. Returns Jet ABI value
/// (string handle, i64, f64-bits, bool as i64).
fn jet_jit_extern_call(wrapper: i64, args: i64) -> i64 {
    let name = clone_string(wrapper);
    let argv = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(args).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(args, i).unwrap_or(0));
        }
        out
    });
    let params = {
        let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(state) = state.as_ref() else {
            trap("jit ffi: no bridge bound");
            return 0;
        };
        let Some(entry) = state.by_wrapper.get(&name) else {
            trap(&format!("jit ffi: unbound wrapper `{name}`"));
            return 0;
        };
        entry.params.clone()
    };
    if argv.len() != params.len() {
        trap(&format!(
            "jit ffi: `{name}` argc {} != {}",
            argv.len(),
            params.len()
        ));
        return 0;
    }
    let ct_args = params
        .iter()
        .zip(argv)
        .map(|(abi, value)| match abi {
            ParamAbi::Int => CtValue::Int(value),
            ParamAbi::Float => CtValue::Float(CtFloat::f64(f64::from_bits(value as u64))),
            ParamAbi::Bool => CtValue::Bool(value != 0),
            ParamAbi::String => CtValue::Str(clone_string(value)),
        })
        .collect::<Vec<_>>();
    let value = match call_ctvalue(&name, &ct_args, None, Span::new(0, 0)) {
        Ok(value) => value,
        Err(error) => {
            trap(&error.what);
            return 0;
        }
    };
    match value {
        CtValue::Unit => 0,
        CtValue::Int(value) => value,
        CtValue::Float(value) => value.as_f64().to_bits() as i64,
        CtValue::Bool(value) => i64::from(value),
        CtValue::Str(value) => alloc_string(value),
        _ => {
            trap(&format!("jit ffi: `{name}` returned an unsupported value"));
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

#[cfg(test)]
mod tests {
    #[test]
    fn reporter_is_installed_after_rtld_now_load() {
        let source = include_str!("Ffi.rs");
        let load = source.find("dlopen(c_path.as_ptr(), RTLD_NOW)").unwrap();
        let reporter = source
            .find("CString::new(\"jet_ffi_set_reporter\")")
            .unwrap();
        // The installed reporter is the generated no-unwind shim, so this also
        // pins that the bridge never receives a bare `extern "C"` body (#1995).
        let install = source
            .find("set_reporter(crate::host_seam::guarded(ffi_reporter))")
            .unwrap();
        assert!(load < reporter && reporter < install);
    }
}
