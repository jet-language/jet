//! Resident JIT FFI: load the prepared bridge cdylib and call `*_cabi` trampolines.
//! Same bridge AOT links — no parallel/fake native path.

use super::Concurrency;
use crate::Marshal::clone_string;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{
    MirAbi, MirAccess, MirArtifactId, MirArtifactTarget, MirForeign, MirForeignAbi,
    MirForeignLanguage, MirHandleId, MirHandleToken, MirProgram, MirRuntimeValue, MirScalarKind,
    MirStructLayout, MirType, MirTypeDefKind, MirTypeId, MirTypeKind,
};
use jet_rt::JetVal;
use std::alloc::{alloc, alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::{
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

#[cfg(unix)]
const RTLD_NOW: c_int = 2;

thread_local! {
    static BRIDGE_CDYLIB: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Install the prepared bridge cdylib for the next resident bind.
/// Interpreter sets this instead of mutating optimized MIR link artifacts.
pub fn set_bridge_cdylib(path: Option<PathBuf>) {
    BRIDGE_CDYLIB.with(|slot| *slot.borrow_mut() = path);
}

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
    Concurrency::with_runtime_mut(|rt| rt.set_ffi_runtime_stop(&message));
}

/// Record a host fault on the active runtime. Every bridge/atomic failure
/// reports through the runtime's fault path so the resident run stops the same
/// way AOT's `jet_ffi_*` trampolines stop: by fault, not by silent zero.
fn trap(msg: &str) {
    Concurrency::with_runtime_mut(|rt| rt.set_host_fault(msg));
}
#[repr(C)]
#[derive(Clone, Copy)]
struct FfiSlot {
    value: u64,
    ptr: *mut u8,
    len: usize,
}

/// Heap storage kept alive for the duration of one descriptor-driven call.
/// The allocation uses the checked record alignment rather than relying on a
/// `Vec<u64>`'s incidental alignment.  This matters for `CAligned` records
/// whose native callee may issue aligned loads directly.
struct FfiStorage {
    ptr: NonNull<u8>,
    layout: Layout,
    len: usize,
}

impl FfiStorage {
    fn allocate(len: usize, align: usize) -> Self {
        let layout = Layout::from_size_align(len.max(1), align.max(1))
            .expect("checked C/CAligned layout has a valid allocation alignment");
        // SAFETY: `layout` has a non-zero size and was constructed from the
        // checked MIR size/alignment pair.
        let ptr = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| handle_alloc_error(layout));
        Self { ptr, layout, len }
    }

    fn from_bytes(bytes: &[u8], align: usize) -> Self {
        let storage = Self::allocate(bytes.len(), align);
        if !bytes.is_empty() {
            // SAFETY: `storage` owns an allocation at least `bytes.len()`
            // bytes long; the copy is byte-wise and bounded by both slices.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), storage.ptr.as_ptr(), bytes.len());
            }
        }
        storage
    }

    fn zeroed(len: usize, align: usize) -> Self {
        Self::allocate(len, align)
    }

    fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the byte view is bounded by the logical byte length.
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len) }
    }

    fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the byte view is bounded by the logical byte length.
        unsafe { std::slice::from_raw_parts_mut(self.as_ptr(), self.len) }
    }
}

impl Drop for FfiStorage {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `layout` came from the matching allocation above
        // and this value owns that allocation exclusively.
        unsafe { dealloc(self.ptr.as_ptr(), self.layout) };
    }
}
type UniformFfi = unsafe extern "C" fn(*const FfiSlot, usize, *mut FfiSlot) -> i32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamAbi {
    Int,
    Float,
    Bool,
    String,
    Handle,
    List,
    /// Managed callback start rows are loaded through their typed callback
    /// helper, never through a uniform `*_cabi` call.
    Callback,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RetAbi {
    Unit,
    Int,
    Float,
    Bool,
    String,
    Handle,
    List,
}

#[derive(Clone)]
struct FfiRecordField {
    name: String,
    ty: MirType,
    offset: usize,
}

#[derive(Clone)]
struct FfiRecordDesc {
    id: MirTypeId,
    name: String,
    size: usize,
    align: usize,
    fields: Vec<FfiRecordField>,
}

type JitNativeCallback = unsafe extern "C" fn(*mut c_void, i64);
type JitCallbackStart =
    unsafe extern "C" fn(Option<JitNativeCallback>, *mut c_void) -> *mut c_void;

#[derive(Clone)]
struct FfiEntrySpec {
    wrapper_name: String,
    params: Vec<ParamAbi>,
    param_types: Vec<MirType>,
    param_access: Vec<MirAccess>,
    ret: RetAbi,
    ret_type: Option<MirType>,
    handle: Option<MirHandleId>,
    close_handle: Option<MirHandleId>,
}

#[derive(Clone)]
struct FfiEntry {
    params: Vec<ParamAbi>,
    param_types: Vec<MirType>,
    param_access: Vec<MirAccess>,
    ret: RetAbi,
    ret_type: Option<MirType>,
    handle: Option<MirHandleId>,
    close_handle: Option<MirHandleId>,
    /// Function pointer into the loaded cdylib.
    ptr: *const (),
    /// Managed native-start rows use a typed callback helper instead of a
    /// uniform trampoline because the callback pointer is part of the ABI.
    callback_start: Option<JitCallbackStart>,
}

// Safety: entries are only used from the JIT host thread with the library kept alive.
unsafe impl Send for FfiEntry {}
unsafe impl Sync for FfiEntry {}
/// Owner token for the separate provider handle used by Foundation's reader
/// slot.  The wrapper handle below may be replaced independently.
struct FfiProviderLibrary {
    handle: *mut c_void,
}

unsafe impl Send for FfiProviderLibrary {}
unsafe impl Sync for FfiProviderLibrary {}

impl Drop for FfiProviderLibrary {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: Foundation drops ProviderLease-owned registration before
            // this owner Arc is released.
            unsafe {
                dlclose(self.handle);
            }
        }
    }
}


struct FfiState {
    handle: *mut c_void,
    clear_panic_hook_fn: unsafe extern "C" fn(),
    free_fn: Option<unsafe extern "C" fn(*mut u8, usize)>,
    take_failure_fn: unsafe extern "C" fn() -> i8,
    by_wrapper: HashMap<String, FfiEntry>,
    close_by_handle: HashMap<MirHandleId, String>,
    records: HashMap<MirTypeId, FfiRecordDesc>,
    /// Foundation owns the registration slot; this lease keeps its provider
    /// callback and dynamic owner live until FFI teardown.
    arrow_provider_lease: Option<jet_foundation::ArrowFileReader::ProviderLease>,
}

unsafe impl Send for FfiState {}

#[derive(Default)]
struct FfiHandleRegistry {
    next_id: i64,
    by_id: HashMap<i64, MirHandleToken>,
}

impl FfiHandleRegistry {
    fn insert(&mut self, handle: MirHandleId, raw: i64) -> i64 {
        loop {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if self.next_id != 0 && !self.by_id.contains_key(&self.next_id) {
                self.by_id
                    .insert(self.next_id, MirHandleToken::new(handle, raw));
                return self.next_id;
            }
        }
    }

    fn raw(&self, id: i64) -> Option<i64> {
        self.by_id.get(&id).and_then(MirHandleToken::raw)
    }

    fn take(&mut self, id: i64) -> Option<(MirHandleId, i64)> {
        self.by_id.remove(&id)?.take_raw()
    }

    fn clear(&mut self) {
        self.by_id.clear();
    }
}

static FFI_STATE: Mutex<Option<FfiState>> = Mutex::new(None);
static FFI_HANDLES: std::sync::LazyLock<Mutex<FfiHandleRegistry>> =
    std::sync::LazyLock::new(|| {
        Mutex::new(FfiHandleRegistry {
            next_id: 0,
            by_id: HashMap::new(),
        })
    });
const JIT_CALLBACK_ACTIVE: u8 = 0;
const JIT_CALLBACK_STOPPING: u8 = 1;
const JIT_CALLBACK_STOPPED: u8 = 2;
const JIT_CALLBACK_QUARANTINED: u8 = 3;

struct JitCallbackState {
    callback: crate::runtime_host::JitCallableSlot,
    native_stop_wrapper: String,
    native_token: AtomicUsize,
    start_complete: AtomicBool,
    phase: AtomicU8,
    in_flight: AtomicUsize,
    stop_started: AtomicBool,
    owner_consumed: AtomicBool,
    callback_failure: Mutex<Option<String>>,
    wait: Mutex<()>,
    drained: Condvar,
}

unsafe impl Send for JitCallbackState {}
unsafe impl Sync for JitCallbackState {}

impl JitCallbackState {
    fn request_stop(&self) {
        let _ = self.phase.compare_exchange(
            JIT_CALLBACK_ACTIVE,
            JIT_CALLBACK_STOPPING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn quarantine(&self, message: impl Into<String>) {
        if self.phase.load(Ordering::Acquire) != JIT_CALLBACK_STOPPED {
            self.phase.store(JIT_CALLBACK_QUARANTINED, Ordering::Release);
        }
        let mut failure = self
            .callback_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failure.is_none() {
            *failure = Some(message.into());
        }
        self.drained.notify_all();
    }

    fn admission(self: &Arc<Self>) -> Option<JitCallbackGuard> {
        if self.phase.load(Ordering::Acquire) != JIT_CALLBACK_ACTIVE {
            return None;
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.phase.load(Ordering::Acquire) != JIT_CALLBACK_ACTIVE {
            self.release_admission();
            return None;
        }
        Some(JitCallbackGuard {
            state: Arc::clone(self),
        })
    }

    fn release_admission(&self) {
        if self.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.drained.notify_all();
        }
    }

    fn stop_and_drain(&self) -> Result<(), String> {
        self.request_stop();
        if self.stop_started.swap(true, Ordering::AcqRel) {
            let mut guard = self.wait.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            while self.phase.load(Ordering::Acquire) == JIT_CALLBACK_STOPPING
                || self.in_flight.load(Ordering::Acquire) != 0
            {
                guard = self
                    .drained
                    .wait(guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if self.phase.load(Ordering::Acquire) == JIT_CALLBACK_STOPPED {
                return self
                    .callback_failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
                    .map_or(Ok(()), Err);
            }
            return Err(self
                .callback_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .unwrap_or_else(|| "callback is quarantined".to_string()));
        }
        let mut guard = self.wait.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while !self.start_complete.load(Ordering::Acquire) {
            guard = self
                .drained
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        drop(guard);
        let token = self.native_token.load(Ordering::Acquire);
        if token == 0 {
            let error = self
                .callback_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .unwrap_or_else(|| "managed callback start returned a null native token".to_string());
            self.quarantine(error.clone());
            return Err(error);
        }
        let native_result = call_runtime_inner(
            &self.native_stop_wrapper,
            &[MirRuntimeValue::Int(token as i64)],
            false,
            Span::new(0, 0),
            MirForeignTarget::Cranelift,
        )
        .map_err(|error| error.what.clone())
        .and_then(|value| match value {
            MirRuntimeValue::Int(0) => Ok(()),
            MirRuntimeValue::Int(status) => Err(format!(
                "native callback shutdown acknowledged with status {status}"
            )),
            _ => Err("native callback shutdown returned a non-integer status".to_string()),
        });
        if let Err(error) = native_result {
            self.quarantine(error.clone());
            return Err(error);
        }
        let mut guard = self.wait.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while self.in_flight.load(Ordering::Acquire) != 0 {
            guard = self
                .drained
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        self.phase.store(JIT_CALLBACK_STOPPED, Ordering::Release);
        self.drained.notify_all();
        let callback_failure = self
            .callback_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        callback_failure.map_or(Ok(()), Err)
    }
}

struct JitCallbackGuard {
    state: Arc<JitCallbackState>,
}

impl Drop for JitCallbackGuard {
    fn drop(&mut self) {
        self.state.release_admission();
    }
}

thread_local! {
    static JIT_CALLBACK_CONTEXT: std::cell::Cell<*const JitCallbackState> =
        const { std::cell::Cell::new(std::ptr::null()) };
}

static JIT_CALLBACKS: std::sync::LazyLock<Mutex<HashMap<usize, Arc<JitCallbackState>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

unsafe fn callback_state_from_raw(raw: *mut c_void) -> Option<Arc<JitCallbackState>> {
    if raw.is_null() {
        return None;
    }
    // SAFETY: registration stores one raw Arc owner for the complete native
    // lifetime; callback admission takes a temporary strong reference.
    Arc::increment_strong_count(raw.cast::<JitCallbackState>());
    Some(Arc::from_raw(raw.cast::<JitCallbackState>()))
}

unsafe extern "C" fn jet_jit_ffi_callback_trampoline(ctx: *mut c_void, value: i64) {
    let Some(state) = (unsafe { callback_state_from_raw(ctx) }) else {
        return;
    };
    let Some(_admission) = state.admission() else {
        return;
    };
    let result = Concurrency::try_with_http_jet_runtime(|| {
        JIT_CALLBACK_CONTEXT.with(|slot| {
            let previous = slot.replace(Arc::as_ptr(&state));
            // Managed callback rows are checked as `fn(FfiCallbackEvent<T>)`
            // with no return. The resident closure ABI is therefore either
            // `(payload)` or `(environment, payload)`.
            unsafe {
                if state.callback.has_env {
                    let callback: unsafe extern "C" fn(i64, i64) =
                        std::mem::transmute(state.callback.fn_ptr as usize);
                    callback(state.callback.env, value);
                } else {
                    let callback: unsafe extern "C" fn(i64) =
                        std::mem::transmute(state.callback.fn_ptr as usize);
                    callback(value);
                }
            }
            slot.set(previous);
            Concurrency::task_trap_pending()
                || Concurrency::with_runtime_mut(|rt| rt.trap_pending())
        })
    });
    match result {
        None => state.quarantine("managed callback has no attached resident runtime"),
        Some(true) => state.quarantine("managed callback failed in resident runtime"),
        Some(false) => {}
    }
}

fn jet_jit_ffi_callback_event_stop() -> i64 {
    let state = JIT_CALLBACK_CONTEXT.with(|slot| {
        let raw = slot.get();
        if raw.is_null() {
            return None;
        }
        // SAFETY: the callback admission guard and registry owner keep this
        // state live until the current trampoline returns. Take an owned Arc
        // before handing the stop worker to another thread.
        unsafe {
            Arc::increment_strong_count(raw);
            Some(Arc::from_raw(raw))
        }
    });
    let Some(state) = state else {
        trap("managed callback event.stop called outside a callback");
        return 0;
    };
    state.request_stop();
    let stop_state = Arc::clone(&state);
    let task = Concurrency::spawn_ffi_task(move || {
        if let Err(error) = stop_state.stop_and_drain() {
            if stop_state.phase.load(Ordering::Acquire) != JIT_CALLBACK_STOPPED {
                stop_state.quarantine(error);
            }
        }
        0
    });
    Concurrency::detach_ffi_task(task);
    0
}

fn remove_jit_callback(raw: usize, owner_consumed: bool) {
    JIT_CALLBACKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&raw);
    if !owner_consumed {
        // SAFETY: the raw Arc owner is consumed exactly once on teardown.
        unsafe {
            drop(Arc::from_raw(raw as *const JitCallbackState));
        }
    }
}
fn shutdown_jit_callbacks() -> bool {
    let callbacks = JIT_CALLBACKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .map(|(raw, state)| (*raw, Arc::clone(state)))
        .collect::<Vec<_>>();
    let mut drained = true;
    for (raw, state) in callbacks {
        let result = state.stop_and_drain();
        if result.is_ok() || state.phase.load(Ordering::Acquire) == JIT_CALLBACK_STOPPED {
            let owner_consumed = state.owner_consumed.load(Ordering::Acquire);
            remove_jit_callback(raw, owner_consumed);
        } else {
            drained = false;
        }
    }
    drained
}



fn register_ffi_handle(handle: MirHandleId, raw: i64) -> i64 {
    FFI_HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(handle, raw)
}

fn ffi_handle_raw(id: i64) -> Option<i64> {
    FFI_HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .raw(id)
}

fn take_ffi_handle(id: i64) -> Option<(MirHandleId, i64)> {
    FFI_HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take(id)
}
mod atomic_prelude {
    // Exact `Int` and fixed-width `I64` have the same i64 ABI but distinct
    // marker types. The exact marker uses Foundation's owned immutable nodes,
    // never a resident-heap lock or a raw wrapping operation.
    include!("../../jet-codegen/src/Prelude/Core/Atomic.rs");
}

/// Canonical `JetAtomic<T>` carrier retained by the active JIT runtime.
/// Handles are stable pointers to these boxed values; the runtime owns every
/// box until the invocation lifetime ends, so method calls never touch a
/// registry or take a lock.
pub(crate) enum FfiAtomicCell {
    Bool(atomic_prelude::JetAtomic<bool>),
    I32(atomic_prelude::JetAtomic<i32>),
    U32(atomic_prelude::JetAtomic<u32>),
    I64(atomic_prelude::JetAtomic<i64>),
    U64(atomic_prelude::JetAtomic<u64>),
    Int(atomic_prelude::JetAtomic<atomic_prelude::JetAtomicInt>),
}

fn try_box_atomic_cell(
    cell: FfiAtomicCell,
) -> Result<Box<FfiAtomicCell>, jet_foundation::Outcome::AllocError> {
    let layout = Layout::new::<FfiAtomicCell>();
    let raw = unsafe { alloc(layout).cast::<FfiAtomicCell>() };
    let Some(raw) = NonNull::new(raw) else {
        return Err(jet_foundation::Outcome::jet_alloc_error(
            layout.size(),
            "jit Atomic",
        ));
    };
    // SAFETY: `raw` is a fresh allocation with the exact cell layout.
    unsafe {
        ptr::write(raw.as_ptr(), cell);
        Ok(Box::from_raw(raw.as_ptr()))
    }
}

impl FfiAtomicCell {
    /// JIT scalar arguments are borrowed raw words. Exact `Int` carries an
    /// owning Foundation node behind that word, so give each atomic cell
    /// operation its own owner before `JetAtomic` consumes a wire.
    fn clone_int(value: i64) -> i64 {
        // SAFETY: checked JIT `Int` arguments are borrowed exact words.
        unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(value) }.into_raw()
    }
    fn new(value: i64, kind_tag: i64) -> Option<Self> {
        match kind_tag {
            0 => Some(Self::Bool(atomic_prelude::JetAtomic::new(value != 0))),
            1 => Some(Self::I32(atomic_prelude::JetAtomic::new(value as i32))),
            2 => Some(Self::U32(atomic_prelude::JetAtomic::new(value as u32))),
            3 => Some(Self::I64(atomic_prelude::JetAtomic::new(value))),
            4 => Some(Self::U64(atomic_prelude::JetAtomic::new(value as u64))),
            5 => Some(Self::Int(atomic_prelude::JetAtomic::new(value))),
            _ => None,
        }
    }

    fn load(&self) -> i64 {
        match self {
            Self::Bool(value) => i64::from(value.load()),
            Self::I32(value) => i64::from(value.load()),
            Self::U32(value) => value.load() as i64,
            Self::I64(value) => value.load(),
            Self::U64(value) => value.load() as i64,
            Self::Int(value) => value.load(),
        }
    }


    fn store(&self, value: i64) {
        match self {
            Self::Bool(cell) => cell.store(value != 0),
            Self::I32(cell) => cell.store(value as i32),
            Self::U32(cell) => cell.store(value as u32),
            Self::I64(cell) => cell.store(value),
            Self::U64(cell) => cell.store(value as u64),
            Self::Int(cell) => cell.store(Self::clone_int(value)),
        }
    }
    fn add(&self, delta: i64) -> Option<i64> {
        match self {
            Self::Bool(_) => None,
            Self::I32(cell) => Some(i64::from(cell.add(delta as i32))),
            Self::U32(cell) => Some(cell.add(delta as u32) as i64),
            Self::I64(cell) => Some(cell.add(delta)),
            Self::U64(cell) => Some(cell.add(delta as u64) as i64),
            Self::Int(cell) => Some(cell.add(Self::clone_int(delta))),
        }
    }
    fn compare_exchange(&self, expected: i64, replacement: i64) -> bool {
        match self {
            Self::Bool(cell) => cell.compare_exchange(expected != 0, replacement != 0),
            Self::I32(cell) => cell.compare_exchange(expected as i32, replacement as i32),
            Self::U32(cell) => cell.compare_exchange(expected as u32, replacement as u32),
            Self::I64(cell) => cell.compare_exchange(expected, replacement),
            Self::U64(cell) => cell.compare_exchange(expected as u64, replacement as u64),
            Self::Int(cell) => cell.compare_exchange(Self::clone_int(expected), Self::clone_int(replacement)),
        }
    }
    fn publish(&self, value: i64) {
        match self {
            Self::Bool(cell) => cell.publish(value != 0),
            Self::I32(cell) => cell.publish(value as i32),
            Self::U32(cell) => cell.publish(value as u32),
            Self::I64(cell) => cell.publish(value),
            Self::U64(cell) => cell.publish(value as u64),
            Self::Int(cell) => cell.publish(Self::clone_int(value)),
        }
    }

    fn observe(&self) -> i64 {
        match self {
            Self::Bool(value) => i64::from(value.observe()),
            Self::I32(value) => i64::from(value.observe()),
            Self::U32(value) => value.observe() as i64,
            Self::I64(value) => value.observe(),
            Self::U64(value) => value.observe() as i64,
            Self::Int(value) => value.observe(),
        }
    }

}

impl FfiAtomicCell {
    fn try_add(&self, delta: i64) -> Result<i64, jet_foundation::Outcome::AllocError> {
        match self {
            Self::Int(cell) => cell.try_add(Self::clone_int(delta)),
            _ => Err(jet_foundation::Outcome::jet_alloc_error(
                0,
                "Atomic.try_add",
            )),
        }
    }
}

fn with_atomic_cell<R>(
    handle: i64,
    operation: impl FnOnce(&FfiAtomicCell) -> R,
) -> Option<R> {
    let pointer = NonNull::new(handle as *mut FfiAtomicCell)?;
    // SAFETY: only `jet_atomic_new` creates these opaque handles, and the
    // active JIT runtime owns each boxed cell for the full invocation lifetime.
    Some(unsafe { operation(pointer.as_ref()) })
}

fn atomic_value_or_fault(handle: i64, operation: impl FnOnce(&FfiAtomicCell) -> i64) -> i64 {
    with_atomic_cell(handle, operation).unwrap_or_else(|| {
        trap("jit Atomic handle is invalid");
        0
    })
}

fn jet_atomic_new(value: i64, kind_tag: i64) -> i64 {
    let Some(cell) = FfiAtomicCell::new(value, kind_tag) else {
        trap("jit Atomic constructor received an unsupported scalar kind");
        return 0;
    };
    let handle = Concurrency::with_runtime_mut(|rt| {
        let cell = Box::new(cell);
        let handle = (&*cell as *const FfiAtomicCell) as i64;
        rt.atomics.push(cell);
        handle
    });
    if handle == 0 {
        trap("jit Atomic constructor has no active runtime");
    }
    handle
}

fn jet_atomic_try_new(value: i64, kind_tag: i64) -> i64 {
    let Some(cell) = FfiAtomicCell::new(value, kind_tag) else {
        trap("jit Atomic constructor received an unsupported scalar kind");
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| {
        if rt.results.try_reserve(1).is_err() {
            rt.set_host_fault("jit Atomic.try_new could not allocate its Result carrier");
            return 0;
        }
        if rt.atomics.try_reserve(1).is_err() {
            let error = jet_foundation::Outcome::jet_alloc_error(
                Layout::new::<Box<FfiAtomicCell>>().size(),
                "jit Atomic",
            );
            return crate::Collections::alloc_error_result(rt, error);
        }
        let cell = match try_box_atomic_cell(cell) {
            Ok(cell) => cell,
            Err(error) => return crate::Collections::alloc_error_result(rt, error),
        };
        let handle = (&*cell as *const FfiAtomicCell) as i64;
        rt.atomics.push(cell);
        crate::runtime_host::alloc_jit_result(rt, true, handle as u64)
    })
}

fn jet_atomic_try_add(handle: i64, delta: i64) -> i64 {
    let result_slot_ready = Concurrency::with_runtime_mut(|rt| rt.results.try_reserve(1).is_ok());
    if !result_slot_ready {
        // The scalar argument is borrowed at this erased ABI boundary; the
        // cell's exact lane clones it before any publication.
        trap("jit Atomic.try_add could not allocate its Result carrier");
        return 0;
    }
    let Some(result) = with_atomic_cell(handle, |cell| cell.try_add(delta)) else {
        trap("jit Atomic handle is invalid");
        return 0;
    };
    match result {
        Ok(previous) => Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, previous as u64)
        }),
        Err(error) => Concurrency::with_runtime_mut(|rt| {
            crate::Collections::alloc_error_result(rt, error)
        }),
    }
}

fn jet_atomic_load(handle: i64) -> i64 {
    atomic_value_or_fault(handle, FfiAtomicCell::load)
}

fn jet_atomic_store(handle: i64, value: i64) -> i64 {
    let Some(()) = with_atomic_cell(handle, |cell| cell.store(value)) else {
        trap("jit Atomic handle is invalid");
        return 0;
    };
    0
}

fn jet_atomic_add(handle: i64, delta: i64) -> i64 {
    let Some(result) = with_atomic_cell(handle, |cell| cell.add(delta)) else {
        trap("jit Atomic handle is invalid");
        return 0;
    };
    result.unwrap_or_else(|| {
        trap("jit Atomic add received a non-addable scalar kind");
        0
    })
}

fn jet_atomic_compare_exchange(handle: i64, expected: i64, replacement: i64) -> i64 {
    i64::from(atomic_value_or_fault(handle, |cell| {
        i64::from(cell.compare_exchange(expected, replacement))
    }) != 0)
}

fn jet_atomic_publish(handle: i64, value: i64) -> i64 {
    let Some(()) = with_atomic_cell(handle, |cell| cell.publish(value)) else {
        trap("jit Atomic handle is invalid");
        return 0;
    };
    0
}

fn jet_atomic_observe(handle: i64) -> i64 {
    atomic_value_or_fault(handle, FfiAtomicCell::observe)
}

/// Why the native bridge could not be bound.
pub(crate) enum BindError {
    Message(String),
}

fn target_applicable(foreign: &MirForeign, target: MirArtifactTarget) -> bool {
    match target {
        MirArtifactTarget::Cranelift => foreign.target_applicability.cranelift,
        MirArtifactTarget::Interpreter => foreign.target_applicability.interpreter,
        MirArtifactTarget::RustAot => foreign.target_applicability.rust_aot,
        MirArtifactTarget::Web => foreign.target_applicability.web,
    }
}
pub(crate) fn bridge_wrapper_name(foreign: &MirForeign) -> String {
    match foreign.foreign_language {
        // C, C++, and assembly bodies are compiled into the prepared bridge
        // cdylib and exported under the `jet_ffi_` trampoline name; only Rust
        // items link by their own symbol.
        MirForeignLanguage::C | MirForeignLanguage::Cpp | MirForeignLanguage::Assembly => {
            format!("jet_ffi_{}", foreign.name)
        }
        MirForeignLanguage::Rust => foreign.symbol.clone(),
    }
}

fn bridge_path(
    program: &MirProgram,
    artifact: &jet_foundation::MIR::MirArtifactPlan,
) -> Option<std::path::PathBuf> {
    if let Some(path) = BRIDGE_CDYLIB.with(|slot| slot.borrow().clone()) {
        return Some(path);
    }
    artifact.links.iter().find_map(|link_id| {
        program
            .links
            .iter()
            .find(|link| link.id == *link_id)?
            .artifacts
            .iter()
            .find(|artifact| {
                matches!(
                    artifact.kind,
                    jet_foundation::MIR::MirLinkArtifactKind::DynamicLibrary
                )
            })
            .map(|artifact| std::path::PathBuf::from(&artifact.path))
    })
}

fn mir_foreign_handle_type(
    program: &MirProgram,
    foreign: &MirForeign,
    ty: &MirType,
) -> bool {
    let Some(handle_id) = foreign.handle else {
        return false;
    };
    program.handles.iter().any(|handle| {
        handle.id == handle_id
            && (handle.ty.same_checked_type(ty)
                || matches!(
                    (&handle.ty.kind(), &ty.kind()),
                    (MirTypeKind::Apply { name: left, .. }, MirTypeKind::Apply { name: right, .. })
                        if left.id == right.id
                ))
    })
}

fn mir_bridge_specs(
    program: &MirProgram,
    artifact: &jet_foundation::MIR::MirArtifactPlan,
) -> Result<Vec<FfiEntrySpec>, BindError> {
    let mut specs = Vec::new();
    for foreign in &program.foreign {
        let in_module = artifact.modules.contains(&foreign.module_id);
        let in_link = foreign
            .link
            .is_some_and(|link_id| artifact.links.contains(&link_id));
        if !(in_module || in_link) || !target_applicable(foreign, artifact.target) {
            continue;
        }
        if matches!(
            foreign.callback_transport.as_deref(),
            Some("managed" | "managed-close" | "emit-task")
        ) {
            continue;
        }
        let params = foreign
            .params
            .iter()
            .map(|parameter| {
                mir_param_abi(
                    &parameter.ty,
                    mir_foreign_handle_type(program, foreign, &parameter.ty),
                )
                .ok_or_else(|| {
                    BindError::Message(format!(
                        "jit ffi: foreign `{}` parameter `{}` has no bridge carrier",
                        foreign.name, parameter.name
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let param_types = foreign
            .params
            .iter()
            .map(|parameter| parameter.ty.clone())
            .collect::<Vec<_>>();
        let param_access = foreign
            .params
            .iter()
            .map(|parameter| parameter.access)
            .collect::<Vec<_>>();
        let ret_type = foreign.return_type.clone();
        let ret = mir_ret_abi(
            foreign.return_type.as_ref(),
            foreign
                .return_type
                .as_ref()
                .is_some_and(|ty| mir_foreign_handle_type(program, foreign, ty)),
        )
        .ok_or_else(|| {
            BindError::Message(format!(
                "jit ffi: foreign `{}` return type has no bridge carrier",
                foreign.name
            ))
        })?;
        if foreign.path.is_empty() || foreign.symbol.is_empty() {
            return Err(BindError::Message(format!(
                "jit ffi: foreign `{}` has no checked bridge identity",
                foreign.name
            )));
        }
        let close_handle = program
            .handles
            .iter()
            .find_map(|lifecycle| (lifecycle.close_foreign == Some(foreign.id)).then_some(lifecycle.id));
        specs.push(FfiEntrySpec {
            wrapper_name: bridge_wrapper_name(foreign),
            params,
            param_types,
            param_access,
            ret,
            ret_type,
            handle: foreign.handle,
            close_handle,
        });
    }
    specs.sort_by(|left, right| left.wrapper_name.cmp(&right.wrapper_name));
    specs.dedup_by(|left, right| {
        left.wrapper_name == right.wrapper_name
            && left.params == right.params
            && left
                .param_types
                .iter()
                .zip(&right.param_types)
                .all(|(left, right)| left.same_checked_type(right))
            && left
                .param_access
                .iter()
                .eq(right.param_access.iter())
            && left.ret == right.ret
            && left
                .ret_type
                .as_ref()
                .zip(right.ret_type.as_ref())
                .is_none_or(|(left, right)| left.same_checked_type(right))
            && left.handle == right.handle
            && left.close_handle == right.close_handle
    });
    Ok(specs)
}
fn align_up(value: usize, align: usize) -> Option<usize> {
    let align = align.max(1);
    let remainder = value % align;
    value.checked_add((align - remainder) % align)
}

fn record_descriptors(program: &MirProgram) -> Result<HashMap<MirTypeId, FfiRecordDesc>, BindError> {
    fn declared_alignment(
        layout: Option<MirStructLayout>,
        fact: Option<&jet_foundation::Layout::LayoutAlignmentFact>,
    ) -> Option<usize> {
        match layout {
            Some(MirStructLayout::C) => Some(1),
            Some(MirStructLayout::CAligned { .. }) => usize::try_from(fact?.effective_alignment)
                .ok()
                .filter(|alignment| {
                    alignment.is_power_of_two() && *alignment <= isize::MAX as usize
                }),
            None | Some(MirStructLayout::Columnar) => None,
        }
    }

    fn layout_for_type(
        ty: &MirType,
        defs: &[jet_foundation::MIR::MirTypeDef],
        memo: &mut HashMap<MirTypeId, (usize, usize)>,
        active: &mut HashSet<MirTypeId>,
    ) -> Option<(usize, usize)> {
        match ty.kind() {
            MirTypeKind::Apply { name: nominal, .. } => {
                let id = ty.identity.or(Some(nominal.id))?;
                if let Some(layout) = memo.get(&id).copied() {
                    return Some(layout);
                }
                if !active.insert(id) {
                    return None;
                }
                let definition = defs.iter().find(|definition| definition.id == id)?;
                let result = match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. }
                        if declared_alignment(
                            definition.layout,
                            definition.layout_alignment.as_ref(),
                        )
                        .is_some() =>
                    {
                        let mut offset = 0usize;
                        let mut align = declared_alignment(
                            definition.layout,
                            definition.layout_alignment.as_ref(),
                        )?;
                        for field in fields {
                            let (size, field_align) =
                                layout_for_type(&field.ty, defs, memo, active)?;
                            offset = align_up(offset, field_align)?;
                            offset = offset.checked_add(size)?;
                            align = align.max(field_align);
                        }
                        Some((align_up(offset, align)?, align))
                    }
                    MirTypeDefKind::Distinct { base, .. } => {
                        layout_for_type(base, defs, memo, active)
                    }
                    MirTypeDefKind::Alias { target } => {
                        layout_for_type(target, defs, memo, active)
                    }
                    _ => None,
                };
                active.remove(&id);
                if let Some(result) = result {
                    memo.insert(id, result);
                }
                result
            }
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => {
                layout_for_type(base, defs, memo, active)
            }
            _ => {
                let size = match ty.layout.size {
                    jet_foundation::MIR::MirSize::Static(size) => usize::try_from(size).ok()?,
                    jet_foundation::MIR::MirSize::Dynamic => return None,
                };
                let align = match ty.layout.align {
                    jet_foundation::MIR::MirSize::Static(align) => usize::try_from(align).ok()?,
                    jet_foundation::MIR::MirSize::Dynamic => return None,
                };
                Some((size, align.max(1)))
            }
        }
    }

    let mut descriptors = HashMap::new();
    let mut memo = HashMap::new();
    for definition in &program.types {
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            continue;
        };
        let Some(explicit_align) =
            declared_alignment(definition.layout, definition.layout_alignment.as_ref())
        else {
            continue;
        };
        let mut active = HashSet::new();
        let mut offset = 0usize;
        let mut align = explicit_align;
        let mut descriptor_fields = Vec::with_capacity(fields.len());
        let mut valid = true;
        for field in fields {
            let Some((size, field_align)) =
                layout_for_type(&field.ty, &program.types, &mut memo, &mut active)
            else {
                valid = false;
                break;
            };
            let Some(field_offset) = align_up(offset, field_align) else {
                valid = false;
                break;
            };
            descriptor_fields.push(FfiRecordField {
                name: field.name.clone(),
                ty: field.ty.clone(),
                offset: field_offset,
            });
            let Some(next) = field_offset.checked_add(size) else {
                valid = false;
                break;
            };
            offset = next;
            align = align.max(field_align);
        }
        if !valid {
            continue;
        }
        let Some(size) = align_up(offset, align) else {
            continue;
        };
        memo.insert(definition.id, (size, align));
        descriptors.insert(
            definition.id,
            FfiRecordDesc {
                id: definition.id,
                name: definition.name.clone(),
                size,
                align,
                fields: descriptor_fields,
            },
        );
    }
    Ok(descriptors)
}


pub(crate) fn bind_mir_ffi(
    program: &MirProgram,
    artifact_id: MirArtifactId,
) -> Result<(), BindError> {
    clear_ffi();
    let artifact = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| BindError::Message(format!("jit ffi: artifact {artifact_id:?} is missing")))?;
    if !matches!(
        artifact.target,
        MirArtifactTarget::Cranelift | MirArtifactTarget::Interpreter
    ) {
        return Err(BindError::Message(format!(
            "jit ffi: artifact `{}` is not an executable JIT target",
            artifact.name
        )));
    }
    let entries = mir_bridge_specs(program, artifact)?;
    let needs_data_provider = program
        .facts
        .runtime_parts
        .contains(&jet_foundation::MIR::MirRuntimePartId::Data);
    let path = bridge_path(program, artifact);
    // `core.db` marks Data for AOT Arrow/sqlite preludes, but JIT sqlite is
    // in-process rusqlite. A Data-only program with no foreign entries must not
    // ICE for a missing cdylib. Load the Arrow provider only when a bridge exists.
    if entries.is_empty() {
        let Some(path) = path else {
            return Ok(());
        };
        if !needs_data_provider {
            return Ok(());
        }
        let records = record_descriptors(program)?;
        return load_cdylib(&path, &entries, records, true).map_err(BindError::Message);
    }
    let path = path.ok_or_else(|| {
        BindError::Message(format!(
            "jit ffi: artifact `{}` needs a bridge for foreign calls, but has no dynamic link",
            artifact.name
        ))
    })?;
    let records = record_descriptors(program)?;
    load_cdylib(&path, &entries, records, needs_data_provider).map_err(BindError::Message)
}

pub(crate) fn clear_ffi() {
    if !shutdown_jit_callbacks() {
        // A failed native acknowledgement quarantines the loaded module. Keep
        // the wrapper/library and callback state live; teardown must not retry
        // or unload a native side that may still retain its trampoline.
        return;
    }
    let mut slot = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut state) = slot.take() {
        let token_ids = FFI_HANDLES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .by_id
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for token_id in token_ids {
            let _ = close_ffi_token(&state, token_id, Span::new(0, 0));
        }
        FFI_HANDLES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let provider_lease = state.arrow_provider_lease.take();
        // Release Foundation's callback registration before dropping the
        // wrapper reference. Escaped Arrow batches retain the erased provider
        // owner independently, so their release callbacks remain valid.
        drop(provider_lease);
        if !state.handle.is_null() {
            unsafe {
                (state.clear_panic_hook_fn)();
                dlclose(state.handle);
            }
        }
    } else {
        FFI_HANDLES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

pub(crate) fn has_bound_ffi() -> bool {
    FFI_STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())

        .is_some()
}
#[cfg(unix)]
fn load_arrow_provider(
    path: &Path,
) -> Result<jet_foundation::ArrowFileReader::ProviderLease, String> {
    let c_path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "jit ffi: bad provider bridge path".to_string())?;
    let provider_handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW) };
    if provider_handle.is_null() {
        let error = unsafe { dlerror() };
        let detail = if error.is_null() {
            "unknown loader error".to_string()
        } else {
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(format!(
            "jit ffi: dlopen provider {}: {detail}",
            path.display()
        ));
    }
    let provider_name = CString::new("jet_data_read").expect("static symbol has no NUL");
    let provider_ptr = unsafe { dlsym(provider_handle, provider_name.as_ptr()) };
    if provider_ptr.is_null() {
        unsafe {
            dlclose(provider_handle);
        }
        return Err(format!(
            "jit ffi: missing symbol `jet_data_read` in {}",
            path.display()
        ));
    }
    // SAFETY: Parquet.rs exports exactly Foundation's Provider ABI.
    let provider: jet_foundation::ArrowFileReader::Provider =
        unsafe { std::mem::transmute(provider_ptr) };
    let owner = Arc::new(FfiProviderLibrary {
        handle: provider_handle,
    });
    let owner_erased: Arc<dyn std::any::Any + Send + Sync> = owner.clone();
    match jet_foundation::ArrowFileReader::register_owned(provider, owner_erased) {
        Ok(lease) => Ok(lease),
        Err(error) => {
            drop(owner);
            Err(format!(
                "jit ffi: data provider registration failed for {}: {error}",
                path.display()
            ))
        }
    }
}

fn load_cdylib(
    path: &Path,
    entries: &[FfiEntrySpec],
    records: HashMap<MirTypeId, FfiRecordDesc>,
    needs_data_provider: bool,
) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (path, entries, records, needs_data_provider);
        return Err("jit ffi: cdylib load unsupported on this host".into());
    }
    #[cfg(unix)]
    {
        let arrow_provider_lease = if needs_data_provider {
            Some(load_arrow_provider(path)?)
        } else {
            None
        };
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
        let clear_hook_name = CString::new("jet_ffi_clear_panic_hook").unwrap();
        let clear_hook_ptr = unsafe { dlsym(handle, clear_hook_name.as_ptr()) };
        if clear_hook_ptr.is_null() {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_clear_panic_hook` in {}",
                path.display()
            ));
        }
        let clear_panic_hook_fn =
            unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn()>(clear_hook_ptr) };
        let failure_name = CString::new("jet_ffi_take_failure").unwrap();
        let failure_ptr = unsafe { dlsym(handle, failure_name.as_ptr()) };
        if failure_ptr.is_null() {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_take_failure` in {}",
                path.display()
            ));
        }
        let take_failure_fn = unsafe {
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> i8>(failure_ptr)
        };
        let free_name = CString::new("jet_ffi_cabi_free").unwrap();
        let free_ptr = unsafe { dlsym(handle, free_name.as_ptr()) };
        let free_fn = if free_ptr.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut u8, usize)>(free_ptr)
            })
        };
        if free_fn.is_none() && entries.iter().any(|entry| entry.ret == RetAbi::String) {
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
            let is_callback_start = entry.params.contains(&ParamAbi::Callback);
            let symbol = if is_callback_start {
                format!("{}_callback_start", entry.wrapper_name)
            } else {
                format!("{}_cabi", entry.wrapper_name)
            };
            let c_name =
                CString::new(symbol.as_str()).map_err(|_| "jit ffi: bad symbol".to_string())?;
            let raw_ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
            if raw_ptr.is_null() {
                unsafe {
                    dlclose(handle);
                }
                return Err(format!(
                    "jit ffi: missing symbol `{symbol}` in {}",
                    path.display()
                ));
            }
            let callback_start = is_callback_start.then(|| unsafe {
                std::mem::transmute::<*mut c_void, JitCallbackStart>(raw_ptr)
            });
            by_wrapper.insert(
                entry.wrapper_name.clone(),
                FfiEntry {
                    params: entry.params.clone(),
                    param_types: entry.param_types.clone(),
                    param_access: entry.param_access.clone(),
                    ret: entry.ret,
                    ret_type: entry.ret_type.clone(),
                    handle: entry.handle,
                    close_handle: entry.close_handle,
                    ptr: raw_ptr as *const (),
                    callback_start,
                },
            );
        }
        let close_by_handle = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .close_handle
                    .map(|handle| (handle, entry.wrapper_name.clone()))
            })
            .collect::<HashMap<_, _>>();
        *FFI_STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(FfiState {
            handle,
            clear_panic_hook_fn,
            free_fn,
            take_failure_fn,
            by_wrapper,
            close_by_handle,
            records,
            arrow_provider_lease,
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

const FFI_RUNTIME_PANIC: &str = "panic: a foreign function panicked";

fn ffi_runtime_diag(span: Span) -> Diagnostic {
    Diagnostic::from_row("E3014", &[("msg", FFI_RUNTIME_PANIC)], Some(span))
}

fn ffi_bridge_host_fault(wrapper: &str) -> Diagnostic {
    Diagnostic::runtime_host_fault(
        String::new(),
        format!("foreign FFI bridge `{wrapper}` panicked"),
    )
}

fn call_cabi<R>(
    state: &FfiState,
    wrapper: &str,
    span: Span,
    call: impl FnOnce() -> R,
) -> Result<R, Diagnostic> {
    jet_codegen::scheduler::jet_scheduler_world_reject_uncontrolled("foreign");
    let _ = unsafe { (state.take_failure_fn)() };
    let result = jet_codegen::scheduler::jet_scheduler_catch_foreign_boundary(call);
    let failure = unsafe { (state.take_failure_fn)() };
    match (failure, result) {
        (1, _) => Err(ffi_runtime_diag(span)),
        (0, Ok(value)) => Ok(value),
        _ => Err(ffi_bridge_host_fault(wrapper)),
    }
}
fn call_uniform(
    state: &FfiState,
    wrapper: &str,
    entry: &FfiEntry,
    slots: &[FfiSlot],
    out: &mut FfiSlot,
    span: Span,
) -> Result<(), Diagnostic> {
    let function: UniformFfi = unsafe { std::mem::transmute(entry.ptr) };
    let rc = call_cabi(state, wrapper, span, || unsafe {
        function(slots.as_ptr(), slots.len(), out as *mut FfiSlot)
    })?;
    if rc == 0 {
        Ok(())
    } else {
        Err(ffi_diag(wrapper, format!("returned {rc}"), span))
    }
}

fn close_ffi_token(
    state: &FfiState,
    token_id: i64,
    span: Span,
) -> Result<(), Diagnostic> {
    let Some((handle, raw)) = take_ffi_handle(token_id) else {
        return Ok(());
    };
    let Some(wrapper) = state.close_by_handle.get(&handle) else {
        return Err(ffi_diag(
            &format!("{handle:?}"),
            "has no registered close function",
            span,
        ));
    };
    let Some(entry) = state.by_wrapper.get(wrapper) else {
        return Err(ffi_diag(
            wrapper,
            "has no loaded close function",
            span,
        ));
    };
    if entry.params.as_slice() != [ParamAbi::Handle] {
        return Err(ffi_diag(
            wrapper,
            "has an unsupported close signature",
            span,
        ));
    }
    if !matches!(entry.ret, RetAbi::Unit | RetAbi::Int) {
        return Err(ffi_diag(
            wrapper,
            "has a close function with a non-status return",
            span,
        ));
    }
    let slots = [FfiSlot {
        value: raw as u64,
        ptr: std::ptr::null_mut(),
        len: 0,
    }];
    let mut out = FfiSlot {
        value: 0,
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    call_uniform(state, wrapper, entry, &slots, &mut out, span)
}

fn ffi_int_range_diag(span: Span) -> Diagnostic {
    let code = crate::runtime_host::contract_kernel::JET_C_INT_RANGE_CODE;
    let row = jet_foundation::Registry::diagnostic(code)
        .expect("C Int range diagnostic must be registered");
    Diagnostic::error(
        code,
        crate::runtime_host::contract_kernel::jet_c_int_range_message().to_string(),
        row.why.to_string(),
        row.fix.to_string(),
        Some(span),
    )
}

fn ffi_int_range_runtime_stop() {
    let report = crate::runtime_host::contract_kernel::jet_c_int_range_report();
    Concurrency::with_runtime_mut(|rt| rt.set_rendered_runtime_stop(report, 1));
}

fn runtime_int_result(value: i64) -> MirRuntimeValue {
    const SMALL_MIN: i64 = -(1i64 << 62);
    const SMALL_MAX: i64 = (1i64 << 62) - 1;
    if (SMALL_MIN..=SMALL_MAX).contains(&value) {
        MirRuntimeValue::Int(value)
    } else {
        MirRuntimeValue::BigInt(value.to_string())
    }
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

fn runtime_string_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Moved) => Err(ffi_diag(
            wrapper,
            format!("argument {index} was moved"),
            span,
        )),
        Some(MirRuntimeValue::String(value)) => Ok(value.clone()),
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not a String"),
            span,
        )),
    }
}

fn runtime_int_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Moved) => Err(ffi_diag(
            wrapper,
            format!("argument {index} was moved"),
            span,
        )),
        Some(MirRuntimeValue::Int(value)) => Ok(*value),
        Some(MirRuntimeValue::BigInt(value)) => {
            value.parse::<i64>().map_err(|_| ffi_int_range_diag(span))
        }
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not an Int"),
            span,
        )),
    }
}

fn runtime_handle_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
    target: MirForeignTarget,
) -> Result<i64, Diagnostic> {
    let value = match args.get(index) {
        Some(MirRuntimeValue::Moved) => {
            return Err(ffi_diag(
                wrapper,
                format!("argument {index} was moved"),
                span,
            ));
        }
        Some(MirRuntimeValue::Int(value)) => *value,
        Some(MirRuntimeValue::BigInt(value)) => {
            value.parse::<i64>().map_err(|_| ffi_int_range_diag(span))?
        }
        _ => {
            return Err(ffi_diag(
                wrapper,
                format!("argument {index} is not an opaque handle"),
                span,
            ));
        }
    };
    if matches!(target, MirForeignTarget::Interpreter) {
        return Ok(value);
    }
    ffi_handle_raw(value).ok_or_else(|| {
        ffi_diag(
            wrapper,
            format!("argument {index} is not a live opaque handle token"),
            span,
        )
    })
}

fn runtime_handle_result(
    handle: Option<MirHandleId>,
    raw: i64,
    target: MirForeignTarget,
    wrapper: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let Some(handle) = handle else {
        return Err(ffi_diag(
            wrapper,
            "returned a handle without a checked handle identity",
            span,
        ));
    };
    let value = if matches!(target, MirForeignTarget::Cranelift) {
        register_ffi_handle(handle, raw)
    } else {
        raw
    };
    Ok(MirRuntimeValue::Int(value))
}

fn retire_handle_args(
    args: &[MirRuntimeValue],
    params: &[ParamAbi],
    close_handle: Option<MirHandleId>,
    target: MirForeignTarget,
) {
    if close_handle.is_none() || !matches!(target, MirForeignTarget::Cranelift) {
        return;
    }
    for (arg, abi) in args.iter().zip(params) {
        if *abi == ParamAbi::Handle {
            if let MirRuntimeValue::Int(token_id) = arg {
                let _ = take_ffi_handle(*token_id);
            }
        }
    }
}

fn runtime_float_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
) -> Result<f64, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Moved) => Err(ffi_diag(
            wrapper,
            format!("argument {index} was moved"),
            span,
        )),
        Some(MirRuntimeValue::Float { value, .. }) => Ok(*value),
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not a Float"),
            span,
        )),
    }
}

fn runtime_bool_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
) -> Result<bool, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Moved) => Err(ffi_diag(
            wrapper,
            format!("argument {index} was moved"),
            span,
        )),
        Some(MirRuntimeValue::Bool(value)) => Ok(*value),
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not a Bool"),
            span,
        )),
    }
}

fn runtime_float_result(value: f64, ret_f32: bool) -> MirRuntimeValue {
    MirRuntimeValue::Float {
        value: if ret_f32 { value as f32 as f64 } else { value },
        f32: ret_f32,
    }
}

fn call_cabi_string(
    state: &FfiState,
    wrapper: &str,
    span: Span,
    call: impl FnOnce(*mut *mut u8, *mut usize) -> i32,
) -> Result<MirRuntimeValue, Diagnostic> {
    let mut out_ptr = std::ptr::null_mut();
    let mut out_len = 0;
    let rc = match call_cabi(state, wrapper, span, || call(&mut out_ptr, &mut out_len)) {
        Ok(rc) => rc,
        Err(error) => {
            release_cabi_buffer(state.free_fn, out_ptr, out_len);
            return Err(error);
        }
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
        .map(MirRuntimeValue::String)
        .map_err(|_| ffi_diag(wrapper, "returned invalid UTF-8", span))
}

fn type_size_align(
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Option<(usize, usize)> {
    match ty.kind() {
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty.identity.or(Some(nominal.id))?;
            records.get(&id).map(|record| (record.size, record.align))
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => type_size_align(base, records),
        _ => {
            let size = match ty.layout.size {
                jet_foundation::MIR::MirSize::Static(size) => usize::try_from(size).ok()?,
                jet_foundation::MIR::MirSize::Dynamic => return None,
            };
            let align = match ty.layout.align {
                jet_foundation::MIR::MirSize::Static(align) => usize::try_from(align).ok()?,
                jet_foundation::MIR::MirSize::Dynamic => return None,
            };
            Some((size, align.max(1)))
        }
    }
}

fn value_int(
    value: &MirRuntimeValue,
    wrapper: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(*value),
        MirRuntimeValue::BigInt(value) => value
            .parse::<i64>()
            .map_err(|_| ffi_int_range_diag(span)),
        MirRuntimeValue::Moved => Err(ffi_diag(wrapper, "received a moved value", span)),
        _ => Err(ffi_diag(wrapper, "received a non-integer value", span)),
    }
}

fn encode_bytes(
    value: &MirRuntimeValue,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    wrapper: &str,
    span: Span,
) -> Result<Vec<u8>, Diagnostic> {
    let (size, _) = type_size_align(ty, records).ok_or_else(|| {
        ffi_diag(
            wrapper,
            format!("type `{}` has no static C layout", ty.canonical_key()),
            span,
        )
    })?;
    let mut bytes = vec![0; size];
    encode_bytes_at(value, ty, records, &mut bytes, wrapper, span)?;
    Ok(bytes)
}

fn encode_bytes_at(
    value: &MirRuntimeValue,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    bytes: &mut [u8],
    wrapper: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let write_uint = |bytes: &mut [u8], value: u64| {
        let raw = value.to_ne_bytes();
        bytes.copy_from_slice(&raw[..bytes.len()]);
    };
    match ty.kind() {
        MirTypeKind::Int => write_uint(bytes, value_int(value, wrapper, span)? as u64),
        MirTypeKind::IntN { .. } => write_uint(bytes, value_int(value, wrapper, span)? as u64),
        MirTypeKind::Float => {
            let MirRuntimeValue::Float { value, .. } = value else {
                return Err(ffi_diag(wrapper, "received a non-Float value", span));
            };
            bytes.copy_from_slice(&value.to_ne_bytes()[..bytes.len()]);
        }
        MirTypeKind::Float32 => {
            let MirRuntimeValue::Float { value, .. } = value else {
                return Err(ffi_diag(wrapper, "received a non-Float value", span));
            };
            bytes.copy_from_slice(&(*value as f32).to_ne_bytes()[..bytes.len()]);
        }
        MirTypeKind::Bool => {
            let MirRuntimeValue::Bool(value) = value else {
                return Err(ffi_diag(wrapper, "received a non-Bool value", span));
            };
            bytes[0] = u8::from(*value);
        }
        MirTypeKind::Char => {
            let MirRuntimeValue::Char(value) = value else {
                return Err(ffi_diag(wrapper, "received a non-Char value", span));
            };
            write_uint(bytes, *value as u32 as u64);
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => {
            encode_bytes_at(value, base, records, bytes, wrapper, span)?
        }
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty.identity.or(Some(nominal.id)).ok_or_else(|| {
                ffi_diag(wrapper, "record has no checked type identity", span)
            })?;
            let descriptor = records.get(&id).ok_or_else(|| {
                ffi_diag(
                    wrapper,
                    format!("record `{}` has no checked C layout", nominal.name),
                    span,
                )
            })?;
            let MirRuntimeValue::Struct { fields, .. } = value else {
                return Err(ffi_diag(wrapper, "received a non-record value", span));
            };
            for (index, field) in descriptor.fields.iter().enumerate() {
                let field_value = fields
                    .iter()
                    .find(|(name, _)| name == &field.name)
                    .or_else(|| fields.get(index))
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        ffi_diag(
                            wrapper,
                            format!("record is missing field `{}`", field.name),
                            span,
                        )
                    })?;
                let field_bytes =
                    encode_bytes(field_value, &field.ty, records, wrapper, span)?;
                let end = field.offset.checked_add(field_bytes.len()).ok_or_else(|| {
                    ffi_diag(wrapper, "record field offset overflowed", span)
                })?;
                let destination = bytes.get_mut(field.offset..end).ok_or_else(|| {
                    ffi_diag(wrapper, "record field exceeds its checked C layout", span)
                })?;
                destination.copy_from_slice(&field_bytes);
            }
        }
        _ => {
            return Err(ffi_diag(
                wrapper,
                format!("type `{}` cannot cross the C bridge", ty.canonical_key()),
                span,
            ))
        }
    }
    Ok(())
}

fn decode_bytes(
    bytes: &[u8],
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    wrapper: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let (size, _) = type_size_align(ty, records).ok_or_else(|| {
        ffi_diag(
            wrapper,
            format!("type `{}` has no static C layout", ty.canonical_key()),
            span,
        )
    })?;
    if bytes.len() < size {
        return Err(ffi_diag(wrapper, "native data is shorter than its C layout", span));
    }
    let bytes = &bytes[..size];
    let read_uint = |bytes: &[u8]| {
        let mut raw = [0; 8];
        raw[..bytes.len()].copy_from_slice(bytes);
        u64::from_ne_bytes(raw)
    };
    match ty.kind() {
        MirTypeKind::Int => Ok(runtime_int_result(read_uint(bytes) as i64)),
        MirTypeKind::IntN { signed, bits } => {
            let value = read_uint(bytes);
            let value = match (*signed, *bits) {
                (true, 8) => (value as i8) as i64,
                (true, 16) => (value as i16) as i64,
                (true, 32) => (value as i32) as i64,
                (true, 64) => value as i64,
                (false, 8) => (value as u8) as i64,
                (false, 16) => (value as u16) as i64,
                (false, 32) => (value as u32) as i64,
                (false, 64) => value as i64,
                _ => {
                    return Err(ffi_diag(
                        wrapper,
                        "integer width has no checked C representation",
                        span,
                    ))
                }
            };
            Ok(runtime_int_result(value))
        }
        MirTypeKind::Float => {
            let mut raw = [0; 8];
            raw.copy_from_slice(&bytes[..8]);
            Ok(MirRuntimeValue::Float {
                value: f64::from_ne_bytes(raw),
                f32: false,
            })
        }
        MirTypeKind::Float32 => {
            let mut raw = [0; 4];
            raw.copy_from_slice(&bytes[..4]);
            Ok(MirRuntimeValue::Float {
                value: f32::from_ne_bytes(raw) as f64,
                f32: true,
            })
        }
        MirTypeKind::Bool => Ok(MirRuntimeValue::Bool(bytes[0] != 0)),
        MirTypeKind::Char => {
            let value = read_uint(bytes) as u32;
            char::from_u32(value)
                .map(MirRuntimeValue::Char)
                .ok_or_else(|| ffi_diag(wrapper, "native data is not a valid Char", span))
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => {
            decode_bytes(bytes, base, records, wrapper, span)
        }
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty.identity.or(Some(nominal.id)).ok_or_else(|| {
                ffi_diag(wrapper, "record has no checked type identity", span)
            })?;
            let descriptor = records.get(&id).ok_or_else(|| {
                ffi_diag(
                    wrapper,
                    format!("record `{}` has no checked C layout", nominal.name),
                    span,
                )
            })?;
            let mut fields = Vec::with_capacity(descriptor.fields.len());
            for field in &descriptor.fields {
                let (_, field_size) = type_size_align(&field.ty, records)
                    .map(|(size, _)| (field.offset, size))
                    .ok_or_else(|| {
                        ffi_diag(wrapper, "record field has no static C layout", span)
                    })?;
                let end = field.offset.checked_add(field_size).ok_or_else(|| {
                    ffi_diag(wrapper, "record field offset overflowed", span)
                })?;
                let field_bytes = bytes.get(field.offset..end).ok_or_else(|| {
                    ffi_diag(wrapper, "record field exceeds its checked C layout", span)
                })?;
                fields.push((
                    field.name.clone(),
                    decode_bytes(field_bytes, &field.ty, records, wrapper, span)?,
                ));
            }
            Ok(MirRuntimeValue::Struct {
                type_name: descriptor.name.clone(),
                fields,
            })
        }
        _ => Err(ffi_diag(
            wrapper,
            format!("type `{}` cannot cross the C bridge", ty.canonical_key()),
            span,
        )),
    }
}

fn prepare_runtime_call(
    args: &[MirRuntimeValue],
    entry: &FfiEntry,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    wrapper: &str,
    span: Span,
    target: MirForeignTarget,
) -> Result<(Vec<FfiSlot>, Vec<FfiStorage>), Diagnostic> {
    let mut slots = Vec::with_capacity(args.len());
    let mut storage = Vec::new();
    for (index, ((abi, ty), access)) in entry
        .params
        .iter()
        .zip(&entry.param_types)
        .zip(&entry.param_access)
        .enumerate()
    {
        let value = args.get(index).ok_or_else(|| {
            ffi_diag(wrapper, format!("argument {index} is missing"), span)
        })?;
        match abi {
            ParamAbi::Callback => {
                return Err(ffi_diag(
                    wrapper,
                    "managed callback start requires the callback registration adapter",
                    span,
                ));
            }
            ParamAbi::List => {
                let MirTypeKind::List(inner) = ty.kind() else {
                    return Err(ffi_diag(wrapper, "list carrier has a non-list type", span));
                };
                let MirRuntimeValue::List(values) = value else {
                    return Err(ffi_diag(wrapper, format!("argument {index} is not a List"), span));
                };
                let mut bytes = Vec::new();
                for value in values {
                    bytes.extend(encode_bytes(value, inner, records, wrapper, span)?);
                }
                let (_, element_align) = type_size_align(inner, records).ok_or_else(|| {
                    ffi_diag(wrapper, "list element has no static C layout", span)
                })?;
                let data = FfiStorage::from_bytes(&bytes, element_align);
                let ptr = if bytes.is_empty() {
                    std::ptr::null_mut()
                } else {
                    data.as_ptr()
                };
                let len = values.len();
                storage.push(data);
                slots.push(FfiSlot {
                    value: 0,
                    ptr,
                    len,
                });
            }
            ParamAbi::String => {
                let value = runtime_string_arg(args, index, wrapper, span)?;
                let data = FfiStorage::from_bytes(value.as_bytes(), 1);
                let ptr = data.as_ptr();
                let len = data.len;
                storage.push(data);
                slots.push(FfiSlot {
                    value: 0,
                    ptr,
                    len,
                });
            }
            ParamAbi::Handle => {
                let raw = runtime_handle_arg(args, index, wrapper, span, target)?;
                slots.push(FfiSlot {
                    value: raw as u64,
                    ptr: std::ptr::null_mut(),
                    len: 0,
                });
            }
            ParamAbi::Int | ParamAbi::Float | ParamAbi::Bool => {
                if *access == MirAccess::Write {
                    let (size, align) = type_size_align(ty, records).ok_or_else(|| {
                        ffi_diag(wrapper, "write parameter has no static C layout", span)
                    })?;
                    let mut data = FfiStorage::zeroed(size, align);
                    encode_bytes_at(value, ty, records, data.as_bytes_mut(), wrapper, span)?;
                    let ptr = data.as_ptr();
                    storage.push(data);
                    slots.push(FfiSlot {
                        value: 0,
                        ptr,
                        len: 0,
                    });
                } else {
                    let bits = match ty.kind() {
                        MirTypeKind::Float => {
                            let MirRuntimeValue::Float { value, .. } = value else {
                                return Err(ffi_diag(wrapper, "argument is not a Float", span));
                            };
                            value.to_bits()
                        }
                        MirTypeKind::Float32 => {
                            let MirRuntimeValue::Float { value, .. } = value else {
                                return Err(ffi_diag(wrapper, "argument is not a Float", span));
                            };
                            (*value as f32).to_bits() as u64
                        }
                        MirTypeKind::Bool => {
                            let MirRuntimeValue::Bool(value) = value else {
                                return Err(ffi_diag(wrapper, "argument is not a Bool", span));
                            };
                            u64::from(*value)
                        }
                        MirTypeKind::Int
                        | MirTypeKind::IntN { .. }
                        | MirTypeKind::InlineRange { .. }
                        | MirTypeKind::Tagged { .. }
                        | MirTypeKind::Quantity { .. } => {
                            value_int(value, wrapper, span)? as u64
                        }
                        _ => {
                            return Err(ffi_diag(
                                wrapper,
                                "parameter has no scalar C representation",
                                span,
                            ))
                        }
                    };
                    slots.push(FfiSlot {
                        value: bits,
                        ptr: std::ptr::null_mut(),
                        len: 0,
                    });
                }
            }
        }
    }
    Ok((slots, storage))
}

fn decode_runtime_result(
    state: &FfiState,
    entry: &FfiEntry,
    out: &FfiSlot,
    wrapper: &str,
    span: Span,
    target: MirForeignTarget,
    ret_f32: bool,
) -> Result<MirRuntimeValue, Diagnostic> {
    match entry.ret {
        RetAbi::Unit => Ok(MirRuntimeValue::Unit),
        RetAbi::Int => Ok(runtime_int_result(out.value as i64)),
        RetAbi::Float => {
            let value = if entry
                .ret_type
                .as_ref()
                .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Float32))
            {
                f32::from_bits(out.value as u32) as f64
            } else {
                f64::from_bits(out.value)
            };
            Ok(runtime_float_result(value, ret_f32))
        }
        RetAbi::Bool => Ok(MirRuntimeValue::Bool(out.value != 0)),
        RetAbi::Handle => runtime_handle_result(
            entry.handle,
            out.value as i64,
            target,
            wrapper,
            span,
        ),
        RetAbi::String => {
            if out.ptr.is_null() && out.len != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if out.ptr.is_null() {
                Vec::new()
            } else {
                // SAFETY: the generated trampoline owns this result buffer
                // until the host copies it and invokes its matching free hook.
                unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec()
            };
            release_cabi_buffer(state.free_fn, out.ptr, out.len);
            String::from_utf8(bytes)
                .map(MirRuntimeValue::String)
                .map_err(|_| ffi_diag(wrapper, "returned invalid UTF-8", span))
        }
        RetAbi::List => {
            let MirTypeKind::List(inner) = entry
                .ret_type
                .as_ref()
                .ok_or_else(|| ffi_diag(wrapper, "list result has no type", span))?
                .kind()
            else {
                return Err(ffi_diag(wrapper, "list result has a non-list type", span));
            };
            let (element_size, _) = type_size_align(inner, &state.records).ok_or_else(|| {
                ffi_diag(wrapper, "list element has no static C layout", span)
            })?;
            let total = element_size.checked_mul(out.len).ok_or_else(|| {
                ffi_diag(wrapper, "returned list length overflowed its C layout", span)
            })?;
            if out.ptr.is_null() && total != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null list buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if total == 0 {
                Vec::new()
            } else {
                // SAFETY: the C trampoline reports the native pointer and
                // element count; copy before any owner can be invalidated.
                unsafe { std::slice::from_raw_parts(out.ptr, total) }.to_vec()
            };
            let values = (0..out.len)
                .map(|index| {
                    let start = index * element_size;
                    decode_bytes(
                        &bytes[start..start + element_size],
                        inner,
                        &state.records,
                        wrapper,
                        span,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(MirRuntimeValue::List(values))
        }
    }
}

fn call_runtime_inner(
    wrapper: &str,
    args: &[MirRuntimeValue],
    ret_f32: bool,
    span: Span,
    target: MirForeignTarget,
) -> Result<MirRuntimeValue, Diagnostic> {
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
    let (slots, _storage) =
        prepare_runtime_call(args, entry, &state.records, wrapper, span, target)?;
    let mut out = FfiSlot {
        value: 0,
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    call_uniform(state, wrapper, entry, &slots, &mut out, span)?;
    let value = decode_runtime_result(state, entry, &out, wrapper, span, target, ret_f32)?;
    retire_handle_args(args, &entry.params, entry.close_handle, target);
    Ok(value)
}
fn managed_callback_type(ty: &MirType) -> bool {
    let MirTypeKind::Fn(signature) = ty.kind() else {
        return false;
    };
    signature.params.len() == 1
        && signature.ret.is_none()
        && matches!(
            signature.params[0].kind(),
            MirTypeKind::Apply { name, args }
                if name.name == "FfiCallbackEvent"
                    && args.len() == 1
                    && matches!(args[0].kind(), MirTypeKind::Int)
        )
}

fn mir_param_abi(ty: &MirType, is_handle: bool) -> Option<ParamAbi> {
    if managed_callback_type(ty) {
        return Some(ParamAbi::Callback);
    }
    if is_handle && matches!(ty.kind(), MirTypeKind::Apply { .. }) {
        return Some(ParamAbi::Handle);
    }
    if ty.is_string() {
        return Some(ParamAbi::String);
    }
    match ty.kind() {
        MirTypeKind::List(_) => Some(ParamAbi::List),
        _ => match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer) => Some(ParamAbi::Int),
            MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => Some(ParamAbi::Float),
            MirAbi::Scalar(MirScalarKind::Bool) => Some(ParamAbi::Bool),
            MirAbi::Scalar(MirScalarKind::Char)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic
            | MirAbi::Never => None,
        },
    }
}

fn mir_ret_abi(ty: Option<&MirType>, is_handle: bool) -> Option<RetAbi> {
    let Some(ty) = ty else {
        return Some(RetAbi::Unit);
    };
    if is_handle && matches!(ty.kind(), MirTypeKind::Apply { .. }) {
        return Some(RetAbi::Handle);
    }
    match ty.kind() {
        MirTypeKind::List(_) => Some(RetAbi::List),
        _ => match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer) => Some(RetAbi::Int),
            MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => Some(RetAbi::Float),
            MirAbi::Scalar(MirScalarKind::Bool) => Some(RetAbi::Bool),
            MirAbi::Scalar(MirScalarKind::Char)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic
            | MirAbi::Never => None,
        },
    }
}

#[derive(Clone, Copy)]
enum MirForeignTarget {
    Cranelift,
    Interpreter,
}

fn validate_mir_bridge(
    foreign: &MirForeign,
    span: Span,
    target: MirForeignTarget,
) -> Result<bool, Diagnostic> {
    let wrapper = bridge_wrapper_name(foreign);
    let (applicable, target_name) = match target {
        MirForeignTarget::Cranelift => (
            foreign.target_applicability.cranelift,
            "Cranelift",
        ),
        MirForeignTarget::Interpreter => (
            foreign.target_applicability.interpreter,
            "interpreter",
        ),
    };
    if !applicable {
        return Err(ffi_diag(
            &wrapper,
            format!("is not applicable to the {target_name} target"),
            span,
        ));
    }
    if foreign.path.is_empty() || foreign.symbol.is_empty() {
        return Err(ffi_diag(
            &wrapper,
            "has no checked bridge identity",
            span,
        ));
    }
    if !matches!(
        foreign.foreign_language,
        MirForeignLanguage::C | MirForeignLanguage::Cpp
    ) || !matches!(
        foreign.foreign_abi,
        MirForeignAbi::C | MirForeignAbi::CUnwind
    ) {
        return Err(ffi_diag(
            &wrapper,
            "has no supported native bridge ABI",
            span,
        ));
    }
    let expected_params = foreign
        .params
        .iter()
        .map(|parameter| {
            mir_param_abi(
                &parameter.ty,
                foreign.handle.is_some() && matches!(parameter.ty.kind(), MirTypeKind::Apply { .. }),
            )
            .ok_or_else(|| {
                ffi_diag(
                    &wrapper,
                    format!("parameter `{}` has no supported bridge carrier", parameter.name),
                    span,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected_ret = mir_ret_abi(
        foreign.return_type.as_ref(),
        foreign
            .handle
            .is_some()
            && foreign
                .return_type
                .as_ref()
                .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Apply { .. })),
    )
    .ok_or_else(|| {
        ffi_diag(
            &wrapper,
            "return type has no supported bridge carrier",
            span,
        )
    })?;
    let state = FFI_STATE.lock().unwrap_or_else(|error| error.into_inner());
    let Some(state) = state.as_ref() else {
        return Err(ffi_diag(&wrapper, "has no prepared bridge", span));
    };
    let Some(entry) = state.by_wrapper.get(&wrapper) else {
        return Err(ffi_diag(
            &wrapper,
            "is not bound in the prepared bridge",
            span,
        ));
    };
    let expected_access = foreign
        .params
        .iter()
        .map(|parameter| parameter.access)
        .collect::<Vec<_>>();
    let same_types = entry
        .param_types
        .iter()
        .zip(foreign.params.iter().map(|parameter| &parameter.ty))
        .all(|(left, right)| left.same_checked_type(right));
    let same_ret_type = match (entry.ret_type.as_ref(), foreign.return_type.as_ref()) {
        (None, None) => true,
        (Some(left), Some(right)) => left.same_checked_type(right),
        _ => false,
    };
    if entry.params != expected_params
        || entry.param_access != expected_access
        || !same_types
        || entry.ret != expected_ret
        || !same_ret_type
    {
        return Err(ffi_diag(
            &wrapper,
            "does not match the checked bridge signature",
            span,
        ));
    }
    Ok(foreign
        .return_type
        .as_ref()
        .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Float32)))
}

/// Call the exact typed bridge selected by a canonical MIR foreign row.
fn call_mir_foreign_for_target(
    foreign: &MirForeign,
    args: Vec<MirRuntimeValue>,
    span: Span,
    target: MirForeignTarget,
) -> Result<MirRuntimeValue, Diagnostic> {
    let wrapper = bridge_wrapper_name(foreign);
    let ret_f32 = validate_mir_bridge(foreign, span, target)?;
    if args.len() != foreign.params.len() {
        return Err(ffi_diag(
            &wrapper,
            format!("argc {} != {}", args.len(), foreign.params.len()),
            span,
        ));
    }
    call_runtime_inner(&wrapper, &args, ret_f32, span, target)
}

/// Ambient provider for canonical MIR foreign calls made by the interpreter.
pub(crate) fn ambient_mir_extern_call(
    foreign: &MirForeign,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    Some(call_mir_foreign_for_target(
        foreign,
        args,
        span,
        MirForeignTarget::Interpreter,
    ))
}

/// Ambient provider for canonical MIR foreign calls made by Cranelift.
pub(crate) fn ambient_mir_extern_call_cranelift(
    foreign: &MirForeign,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    Some(call_mir_foreign_for_target(
        foreign,
        args,
        span,
        MirForeignTarget::Cranelift,
    ))
}

/// Convert one erased heap cell using the checked MIR element descriptor.
fn heap_cell_to_runtime(
    heap: &jet_rt::JetArena,
    cell: &JetVal,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Result<MirRuntimeValue, String> {
    match ty.kind() {
        MirTypeKind::Int
        | MirTypeKind::IntN { .. }
        | MirTypeKind::InlineRange { .. }
        | MirTypeKind::Tagged { .. }
        | MirTypeKind::Quantity { .. } => match cell {
            JetVal::Int(value) => Ok(MirRuntimeValue::Int(*value)),
            _ => Err(format!("expected integer cell for `{}`", ty.canonical_key())),
        },
        MirTypeKind::Float | MirTypeKind::Float32 => match cell {
            JetVal::Float(value) => Ok(MirRuntimeValue::Float {
                value: *value,
                f32: matches!(ty.kind(), MirTypeKind::Float32),
            }),
            _ => Err(format!("expected float cell for `{}`", ty.canonical_key())),
        },
        MirTypeKind::Bool => match cell {
            JetVal::Bool(value) => Ok(MirRuntimeValue::Bool(*value)),
            JetVal::Int(value) => Ok(MirRuntimeValue::Bool(*value != 0)),
            _ => Err(format!("expected bool cell for `{}`", ty.canonical_key())),
        },
        MirTypeKind::Char => match cell {
            JetVal::Char(value) => Ok(MirRuntimeValue::Char(*value)),
            JetVal::Int(value) => char::from_u32(*value as u32)
                .map(MirRuntimeValue::Char)
                .ok_or_else(|| "character cell is not valid Unicode".to_string()),
            _ => Err(format!("expected character cell for `{}`", ty.canonical_key())),
        },
        MirTypeKind::String => match cell {
            JetVal::String(value) => Ok(MirRuntimeValue::String(value.clone())),
            JetVal::StringView { owner, start, end } => heap
                .get_string(*owner)
                .and_then(|text| text.get(*start..*end))
                .map(|text| MirRuntimeValue::String(text.to_string()))
                .ok_or_else(|| "string view cell is invalid".to_string()),
            JetVal::Int(value) => heap
                .clone_string(*value)
                .map(MirRuntimeValue::String)
                .ok_or_else(|| "string handle cell is invalid".to_string()),
            _ => Err(format!("expected string cell for `{}`", ty.canonical_key())),
        },
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty
                .identity
                .or(Some(nominal.id))
                .ok_or_else(|| "record has no checked type identity".to_string())?;
            let descriptor = records
                .get(&id)
                .ok_or_else(|| format!("record `{}` has no checked C layout", nominal.name))?;
            let fields = match cell {
                JetVal::Record(fields) => fields.clone(),
                JetVal::Int(handle) | JetVal::RecordRef(handle) => heap
                    .clone_record_values(*handle)
                    .ok_or_else(|| "record handle cell is invalid".to_string())?,
                _ => return Err(format!("expected record cell for `{}`", nominal.name)),
            };
            if fields.len() < descriptor.fields.len() {
                return Err(format!("record `{}` is missing fields", nominal.name));
            }
            let mut out = Vec::with_capacity(descriptor.fields.len());
            for (index, field) in descriptor.fields.iter().enumerate() {
                out.push((
                    field.name.clone(),
                    heap_cell_to_runtime(heap, &fields[index], &field.ty, records)?,
                ));
            }
            Ok(MirRuntimeValue::Struct {
                type_name: descriptor.name.clone(),
                fields: out,
            })
        }
        MirTypeKind::List(inner) => {
            let JetVal::List(values) = cell else {
                return Err(format!("nested list `{}` is not a boxed list", ty.canonical_key()));
            };
            values
                .iter()
                .map(|value| heap_cell_to_runtime(heap, value, inner, records))
                .collect::<Result<Vec<_>, _>>()
                .map(MirRuntimeValue::List)
        }
        _ => Err(format!("type `{}` cannot cross the C bridge", ty.canonical_key())),
    }
}

fn heap_list_to_runtime(
    heap: &jet_rt::JetArena,
    list: i64,
    inner: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Result<MirRuntimeValue, String> {
    let carrier = heap
        .clone_value(list)
        .ok_or_else(|| "list handle is invalid".to_string())?;
    let values = match carrier {
        JetVal::IntList(values) => values
            .into_iter()
            .map(JetVal::Int)
            .collect::<Vec<_>>(),
        JetVal::List(values) => values,
        JetVal::UninitList {
            values,
            initialized,
        } => {
            if initialized.iter().any(|initialized| !initialized) {
                return Err("list contains uninitialized elements".to_string());
            }
            values
        }
        _ => return Err("list handle does not contain a list".to_string()),
    };
    values
        .iter()
        .map(|value| heap_cell_to_runtime(heap, value, inner, records))
        .collect::<Result<Vec<_>, _>>()
        .map(MirRuntimeValue::List)
}

fn heap_arg_to_runtime(
    heap: &jet_rt::JetArena,
    value: i64,
    abi: ParamAbi,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Result<MirRuntimeValue, String> {
    match abi {
        ParamAbi::Callback => {
            Err("managed callback arguments require the callback registration adapter".to_string())
        }
        ParamAbi::Int | ParamAbi::Handle => heap
            .int_to_i64(value)
            .map(MirRuntimeValue::Int)
            .ok_or_else(|| "integer argument is not representable".to_string()),
        ParamAbi::Float => {
            let bits = value as u64;
            Ok(MirRuntimeValue::Float {
                value: if matches!(ty.kind(), MirTypeKind::Float32) {
                    f32::from_bits(bits as u32) as f64
                } else {
                    f64::from_bits(bits)
                },
                f32: matches!(ty.kind(), MirTypeKind::Float32),
            })
        }
        ParamAbi::Bool => heap
            .int_to_i64(value)
            .map(|value| MirRuntimeValue::Bool(value != 0))
            .ok_or_else(|| "bool argument is not an integer word".to_string()),
        ParamAbi::String => heap
            .clone_string(value)
            .map(MirRuntimeValue::String)
            .ok_or_else(|| "string argument is not a valid string handle".to_string()),
        ParamAbi::List => {
            let MirTypeKind::List(inner) = ty.kind() else {
                return Err("list carrier has a non-list type".to_string());
            };
            heap_list_to_runtime(heap, value, inner, records)
        }
    }
}

fn heap_runtime_to_jetval(
    heap: &mut jet_rt::JetArena,
    value: &MirRuntimeValue,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Result<JetVal, String> {
    match ty.kind() {
        MirTypeKind::Int
        | MirTypeKind::IntN { .. }
        | MirTypeKind::InlineRange { .. }
        | MirTypeKind::Tagged { .. }
        | MirTypeKind::Quantity { .. } => match value {
            MirRuntimeValue::Int(value) => Ok(JetVal::Int(*value)),
            MirRuntimeValue::BigInt(value) => heap
                .int_from_str(value)
                .map(JetVal::Int),
            _ => Err(format!("native result is not integer `{}`", ty.canonical_key())),
        },
        MirTypeKind::Float | MirTypeKind::Float32 => match value {
            MirRuntimeValue::Float { value, .. } => Ok(JetVal::Float(*value)),
            _ => Err(format!("native result is not float `{}`", ty.canonical_key())),
        },
        MirTypeKind::Bool => match value {
            MirRuntimeValue::Bool(value) => Ok(JetVal::Bool(*value)),
            _ => Err(format!("native result is not bool `{}`", ty.canonical_key())),
        },
        MirTypeKind::Char => match value {
            MirRuntimeValue::Char(value) => Ok(JetVal::Char(*value)),
            _ => Err(format!("native result is not char `{}`", ty.canonical_key())),
        },
        MirTypeKind::String => match value {
            MirRuntimeValue::String(value) => Ok(JetVal::String(value.clone())),
            _ => Err(format!("native result is not string `{}`", ty.canonical_key())),
        },
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty
                .identity
                .or(Some(nominal.id))
                .ok_or_else(|| "record has no checked type identity".to_string())?;
            let descriptor = records
                .get(&id)
                .ok_or_else(|| format!("record `{}` has no checked C layout", nominal.name))?;
            let MirRuntimeValue::Struct { fields, .. } = value else {
                return Err(format!("native result is not record `{}`", nominal.name));
            };
            let mut cells = Vec::with_capacity(descriptor.fields.len());
            for (index, field) in descriptor.fields.iter().enumerate() {
                let field_value = fields
                    .iter()
                    .find(|(name, _)| name == &field.name)
                    .or_else(|| fields.get(index))
                    .map(|(_, value)| value)
                    .ok_or_else(|| format!("record `{}` is missing `{}`", nominal.name, field.name))?;
                cells.push(heap_runtime_to_jetval(heap, field_value, &field.ty, records)?);
            }
            Ok(JetVal::RecordRef(heap.alloc_record_values(cells)))
        }
        MirTypeKind::List(inner) => {
            let MirRuntimeValue::List(values) = value else {
                return Err(format!("native result is not list `{}`", ty.canonical_key()));
            };
            values
                .iter()
                .map(|value| heap_runtime_to_jetval(heap, value, inner, records))
                .collect::<Result<Vec<_>, _>>()
                .map(JetVal::List)
        }
        _ => Err(format!("type `{}` cannot cross the C bridge", ty.canonical_key())),
    }
}

fn heap_restore_list(
    heap: &mut jet_rt::JetArena,
    list: i64,
    inner: &MirType,
    values: &[MirRuntimeValue],
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Result<(), String> {
    let original = heap
        .clone_value(list)
        .ok_or_else(|| "mutable list handle is invalid".to_string())?;
    let original_len = match &original {
        JetVal::IntList(values) => values.len(),
        JetVal::List(values) | JetVal::UninitList { values, .. } => values.len(),
        _ => return Err("mutable list handle does not contain a list".to_string()),
    };
    if original_len != values.len() {
        return Err("native mutable list changed its element count".to_string());
    }

    if matches!(inner.kind(), MirTypeKind::Int | MirTypeKind::IntN { .. })
        && values.iter().all(|value| matches!(value, MirRuntimeValue::Int(_)))
    {
        let values = values
            .iter()
            .map(|value| match value {
                MirRuntimeValue::Int(value) => *value,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        if matches!(&original, JetVal::IntList(_)) {
            heap.replace_int_list(list, values)
                .ok_or_else(|| "mutable integer list cannot be restored".to_string())?;
            return Ok(());
        }
    }
    if matches!(inner.kind(), MirTypeKind::Float | MirTypeKind::Float32)
        && values
            .iter()
            .all(|value| matches!(value, MirRuntimeValue::Float { .. }))
        && matches!(&original, JetVal::List(_))
    {
        let values = values
            .iter()
            .map(|value| match value {
                MirRuntimeValue::Float { value, .. } => *value,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        heap.replace_float_list(list, values)
            .ok_or_else(|| "mutable float list cannot be restored".to_string())?;
        return Ok(());
    }
    if matches!(inner.kind(), MirTypeKind::String)
        && matches!(&original, JetVal::IntList(_))
        && values
            .iter()
            .all(|value| matches!(value, MirRuntimeValue::String(_)))
    {
        let handles = values
            .iter()
            .map(|value| match value {
                MirRuntimeValue::String(value) => heap.alloc_string(value.clone()),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        heap.replace_int_list(list, handles)
            .ok_or_else(|| "mutable string list cannot be restored".to_string())?;
        return Ok(());
    }

    let cells = values
        .iter()
        .map(|value| heap_runtime_to_jetval(heap, value, inner, records))
        .collect::<Result<Vec<_>, _>>()?;
    let mut preserve_old = vec![false; cells.len()];
    if matches!(inner.kind(), MirTypeKind::Apply { .. }) {
        let old_cells = match &original {
            JetVal::IntList(values) => values.iter().copied().map(JetVal::Int).collect(),
            JetVal::List(values) | JetVal::UninitList { values, .. } => values.clone(),
            _ => Vec::new(),
        };
        for (index, old_cell) in old_cells.iter().enumerate() {
            let Some(old_record) = (match old_cell {
                JetVal::Int(handle) | JetVal::RecordRef(handle) => Some(*handle),
                _ => None,
            }) else {
                continue;
            };
            let Some(JetVal::RecordRef(new_record)) = cells.get(index) else {
                continue;
            };
            if heap.record_assign_from(old_record, *new_record).is_some() {
                preserve_old[index] = true;
            }
        }
    }
    let slots = heap
        .list_values_mut(list)
        .ok_or_else(|| "mutable list cannot be restored".to_string())?;
    for (index, (slot, cell)) in slots.iter_mut().zip(cells).enumerate() {
        if !preserve_old[index] {
            *slot = cell;
        }
    }
    Ok(())
}

fn heap_result_value(
    heap: &mut jet_rt::JetArena,
    value: MirRuntimeValue,
    entry: &FfiEntry,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Result<i64, String> {
    match value {
        MirRuntimeValue::Unit => Ok(0),
        MirRuntimeValue::Int(value) => Ok(heap.int_from_i64(value)),
        MirRuntimeValue::BigInt(value) => heap.int_from_str(&value),
        MirRuntimeValue::Float { value, .. } => Ok(value.to_bits() as i64),
        MirRuntimeValue::Bool(value) => Ok(i64::from(value)),
        MirRuntimeValue::Char(value) => Ok(i64::from(value as u32)),
        MirRuntimeValue::String(value) => Ok(heap.alloc_string(value)),
        MirRuntimeValue::List(values) => {
            let Some(ret_type) = entry.ret_type.as_ref() else {
                return Err("native list result has no list type".to_string());
            };
            let MirTypeKind::List(inner) = ret_type.kind() else {
                return Err("native list result has a non-list type".to_string());
            };
            let cells = values
                .iter()
                .map(|value| heap_runtime_to_jetval(heap, value, inner, records))
                .collect::<Result<Vec<_>, _>>()?;
            if matches!(inner.kind(), MirTypeKind::Int | MirTypeKind::IntN { .. })
                && cells.iter().all(|cell| matches!(cell, JetVal::Int(_)))
            {
                Ok(heap.alloc_int_list(
                    cells
                        .into_iter()
                        .map(|cell| match cell {
                            JetVal::Int(value) => value,
                            _ => unreachable!(),
                        })
                        .collect(),
                ))
            } else {
                Ok(heap.alloc_list_values(cells))
            }
        }
        MirRuntimeValue::Moved => Err("native bridge returned a moved value".to_string()),
        _ => Err("native bridge returned an unsupported value".to_string()),
    }
}

/// `wrapper` / `args` are heap string / int-list handles. Returns a heap
/// carrier for the canonical MIR runtime value.
fn jet_jit_extern_call(wrapper: i64, args: i64) -> i64 {
    let name = clone_string(wrapper);
    let Some(argv) = Concurrency::with_runtime_mut(|rt| {
        let values = rt.heap.clone_list_values(args)?;
        values
            .into_iter()
            .map(|value| match value {
                JetVal::Int(value) => Some(value),
                JetVal::Float(value) => Some(value.to_bits() as i64),
                JetVal::Bool(value) => Some(i64::from(value)),
                JetVal::Char(value) => Some(i64::from(value as u32)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
    }) else {
        trap("jit ffi: extern call arguments are not a valid value list");
        return 0;
    };
    let Some(entry) = ({
        let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
        state
            .as_ref()
            .and_then(|state| state.by_wrapper.get(&name).cloned())
    }) else {
        trap(&format!("jit ffi: unbound wrapper `{name}`"));
        return 0;
    };
    let records = {
        let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
        state
            .as_ref()
            .map(|state| state.records.clone())
            .unwrap_or_default()
    };
    if argv.len() != entry.params.len() {
        trap(&format!(
            "jit ffi: `{name}` argc {} != {}",
            argv.len(),
            entry.params.len()
        ));
        return 0;
    }
    let runtime_args = Concurrency::with_runtime_string(|rt| {
        entry
            .params
            .iter()
            .zip(&entry.param_types)
            .zip(argv.iter().copied())
            .map(|((abi, ty), value)| heap_arg_to_runtime(&rt.heap, value, *abi, ty, &records))
            .collect::<Result<Vec<_>, _>>()
    });
    let runtime_args = match runtime_args {
        Ok(args) => args,
        Err(error) => {
            trap(&format!("jit ffi: `{name}` {error}"));
            return 0;
        }
    };
    let list_bindings = {
        let mut storage_index = 0usize;
        let mut bindings = Vec::new();
        for (index, ((abi, _), access)) in entry
            .params
            .iter()
            .zip(&entry.param_types)
            .zip(&entry.param_access)
            .enumerate()
        {
            match abi {
                ParamAbi::List => {
                    if *access == MirAccess::Write {
                        bindings.push((index, storage_index));
                    }
                    storage_index += 1;
                }
                ParamAbi::String => storage_index += 1,
                ParamAbi::Int | ParamAbi::Float | ParamAbi::Bool
                    if *access == MirAccess::Write =>
                {
                    storage_index += 1;
                }
                ParamAbi::Int
                | ParamAbi::Float
                | ParamAbi::Bool
                | ParamAbi::Handle
                | ParamAbi::Callback => {}
            }
        }
        bindings
    };
    let bridge_result = {
        let state_guard = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
        match state_guard.as_ref() {
            None => Err(ffi_diag(&name, "has no prepared bridge", Span::new(0, 0))),
            Some(state) => {
                let prepared = prepare_runtime_call(
                    &runtime_args,
                    &entry,
                    &state.records,
                    &name,
                    Span::new(0, 0),
                    MirForeignTarget::Cranelift,
                );
                match prepared {
                    Ok((slots, storage)) => {
                        let mut out = FfiSlot {
                            value: 0,
                            ptr: std::ptr::null_mut(),
                            len: 0,
                        };
                        let result = call_uniform(
                            state,
                            &name,
                            &entry,
                            &slots,
                            &mut out,
                            Span::new(0, 0),
                        )
                        .and_then(|()| {
                            let value = decode_runtime_result(
                                state,
                                &entry,
                                &out,
                                &name,
                                Span::new(0, 0),
                                MirForeignTarget::Cranelift,
                                entry.ret_type.as_ref().is_some_and(|ty| {
                                    matches!(ty.kind(), MirTypeKind::Float32)
                                }),
                            )?;
                            let mut writebacks = Vec::new();
                            for (param_index, storage_index) in &list_bindings {
                                let ty = &entry.param_types[*param_index];
                                let MirTypeKind::List(inner) = ty.kind() else {
                                    return Err(ffi_diag(
                                        &name,
                                        "list binding has a non-list type",
                                        Span::new(0, 0),
                                    ));
                                };
                                let (element_size, _) =
                                    type_size_align(inner, &state.records).ok_or_else(|| {
                                        ffi_diag(
                                            &name,
                                            "mutable list element has no static C layout",
                                            Span::new(0, 0),
                                        )
                                    })?;
                                let slot = slots[*param_index];
                                let bytes = storage
                                    .get(*storage_index)
                                    .map(FfiStorage::as_bytes)
                                    .ok_or_else(|| {
                                        ffi_diag(
                                            &name,
                                            "mutable list storage is missing",
                                            Span::new(0, 0),
                                        )
                                    })?;
                                let mut values = Vec::with_capacity(slot.len);
                                for index in 0..slot.len {
                                    let start =
                                        index.checked_mul(element_size).ok_or_else(|| {
                                            ffi_diag(
                                                &name,
                                                "mutable list length overflowed its C layout",
                                                Span::new(0, 0),
                                            )
                                        })?;
                                    let end = start.checked_add(element_size).ok_or_else(|| {
                                        ffi_diag(
                                            &name,
                                            "mutable list element range overflowed",
                                            Span::new(0, 0),
                                        )
                                    })?;
                                    let element = bytes.get(start..end).ok_or_else(|| {
                                        ffi_diag(
                                            &name,
                                            "mutable list storage is shorter than its count",
                                            Span::new(0, 0),
                                        )
                                    })?;
                                    values.push(decode_bytes(
                                        element,
                                        inner,
                                        &state.records,
                                        &name,
                                        Span::new(0, 0),
                                    )?);
                                }
                                writebacks.push((
                                    *param_index,
                                    entry.param_types[*param_index].clone(),
                                    values,
                                ));
                            }
                            retire_handle_args(
                                &runtime_args,
                                &entry.params,
                                entry.close_handle,
                                MirForeignTarget::Cranelift,
                            );
                            Ok((value, writebacks))
                        });
                        let _storage = storage;
                        result
                    }
                    Err(error) => Err(error),
                }
            }
        }
    };
    let (value, writebacks) = match bridge_result {
        Ok(value) => value,
        Err(error) if error.code == "E3014" => {
            Concurrency::with_runtime_mut(|rt| rt.set_ffi_runtime_stop(FFI_RUNTIME_PANIC));
            return 0;
        }
        Err(error) if error.runtime_host_fault_parts().is_some() => {
            trap(&error.what);
            return 0;
        }
        Err(error) => {
            trap(&error.what);
            return 0;
        }
    };
    Concurrency::with_runtime_mut(|rt| {
        for (param_index, ty, values) in writebacks {
            let MirTypeKind::List(inner) = ty.kind() else {
                continue;
            };
            let list = argv[param_index];
            if let Err(error) = heap_restore_list(&mut rt.heap, list, inner, &values, &records) {
                rt.set_host_fault(&format!("jit ffi: `{name}` {error}"));
                return 0;
            }
        }
        match heap_result_value(&mut rt.heap, value, &entry, &records) {
            Ok(value) => value,
            Err(error) => {
                rt.set_host_fault(&format!("jit ffi: `{name}` {error}"));
                0
            }
        }
    })
}

fn jet_jit_ffi_drop(token_id: i64) {
    let result = {
        let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.as_ref().map_or_else(
            || {
                let _ = take_ffi_handle(token_id);
                Ok(())
            },
            |state| close_ffi_token(state, token_id, Span::new(0, 0)),
        )
    };
    if let Err(error) = result {
        trap(&error.what);
    }
}

fn jet_jit_ffi_callback_register(
    callback_handle: i64,
    native_start_handle: i64,
    native_stop_handle: i64,
    identity_handle: i64,
    digest_handle: i64,
) -> i64 {
    let callback = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback_handle)
    });
    let Some(callback) = callback else {
        trap("managed callback registration received an invalid callable");
        return 0;
    };
    let native_start_wrapper = clone_string(native_start_handle);
    let native_stop_wrapper = clone_string(native_stop_handle);
    let identity = clone_string(identity_handle);
    let digest = clone_string(digest_handle);
    if native_start_wrapper.is_empty()
        || native_stop_wrapper.is_empty()
        || identity.is_empty()
        || digest.is_empty()
        || digest
            != jet_foundation::SHA256::sha256_hex(
                format!("jet-ffi-callback-plan-v1\0{identity}").as_bytes(),
            )
    {
        trap("managed callback registration has incomplete or invalid bridge metadata");
        return 0;
    }
    let native_start = {
        let state = FFI_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .as_ref()
            .and_then(|state| state.by_wrapper.get(&native_start_wrapper))
            .and_then(|entry| entry.callback_start)
    };
    let Some(native_start) = native_start else {
        trap("managed callback registration has no native-start helper");
        return 0;
    };
    let state = Arc::new(JitCallbackState {
        callback,
        native_stop_wrapper,
        native_token: AtomicUsize::new(0),
        start_complete: AtomicBool::new(false),
        phase: AtomicU8::new(JIT_CALLBACK_ACTIVE),
        in_flight: AtomicUsize::new(0),
        stop_started: AtomicBool::new(false),
        owner_consumed: AtomicBool::new(false),
        callback_failure: Mutex::new(None),
        wait: Mutex::new(()),
        drained: Condvar::new(),
    });
    let raw = Arc::into_raw(Arc::clone(&state)) as usize;
    let start_state = Arc::clone(&state);
    JIT_CALLBACKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(raw, state);
    let token = unsafe { native_start(Some(jet_jit_ffi_callback_trampoline), raw as *mut c_void) };
    if token.is_null() {
        start_state.quarantine("managed callback native start returned a null token");
        start_state.start_complete.store(true, Ordering::Release);
        start_state.drained.notify_all();
        remove_jit_callback(raw, false);
        trap("managed callback native start returned a null token");
        return 0;
    }
    start_state.native_token.store(token as usize, Ordering::Release);
    start_state.start_complete.store(true, Ordering::Release);
    start_state.drained.notify_all();
    raw as i64
}

fn jet_jit_ffi_callback_unsubscribe(handle: i64) -> i64 {
    let Ok(raw) = usize::try_from(handle) else {
        trap("managed callback unsubscribe received an invalid registration");
        return 0;
    };
    let state = {
        let callbacks = JIT_CALLBACKS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        callbacks.get(&raw).cloned()
    };
    let Some(state) = state else {
        trap("managed callback unsubscribe received an unknown registration");
        return 0;
    };
    if state.owner_consumed.swap(true, Ordering::AcqRel) {
        trap("managed callback registration was already consumed");
        return 0;
    }
    // SAFETY: the successful registry lookup above proves the raw Arc owner is
    // live; consuming registration is the only path that calls `from_raw`.
    let owner = unsafe { Arc::from_raw(raw as *const JitCallbackState) };
    Concurrency::spawn_ffi_task(move || {
        let result = owner.stop_and_drain();
        let stopped = owner.phase.load(Ordering::Acquire) == JIT_CALLBACK_STOPPED;
        if stopped {
            remove_jit_callback(raw, true);
        }
        if let Err(error) = result {
            Concurrency::set_task_trap(&error);
        }
        0
    })
}

fn jet_jit_ffi_callback_event_stop_host() -> i64 {
    jet_jit_ffi_callback_event_stop()
}

fn jet_jit_ffi_emit_task(wrapper_handle: i64, value: i64) -> i64 {
    let wrapper = clone_string(wrapper_handle);
    if wrapper.is_empty() {
        trap("managed callback emit task has no native bridge identity");
        return 0;
    }
    Concurrency::spawn_ffi_task(move || {
        match call_runtime_inner(
            &wrapper,
            &[MirRuntimeValue::Int(value)],
            false,
            Span::new(0, 0),
            MirForeignTarget::Cranelift,
        ) {
            Ok(MirRuntimeValue::Int(value)) => value,
            Ok(_) => {
                Concurrency::set_task_trap("managed callback emit returned a non-integer value");
                0
            }
            Err(error) => {
                Concurrency::set_task_trap(&error.what);
                0
            }
        }
    })
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
        let mut sig_callback_register = Signature::new(cc);
        for _ in 0..5 {
            sig_callback_register
                .params
                .push(AbiParam::new(types::I64));
        }
        sig_callback_register.returns.push(AbiParam::new(types::I64));
        let mut sig_callback_unsubscribe = Signature::new(cc);
        sig_callback_unsubscribe
            .params
            .push(AbiParam::new(types::I64));
        sig_callback_unsubscribe
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_callback_event_stop = Signature::new(cc);
        sig_callback_event_stop
            .returns
            .push(AbiParam::new(types::I64));
        let sig_emit_task = sig.clone();
        let mut sig_drop_handle = Signature::new(cc);
        sig_drop_handle.params.push(AbiParam::new(types::I64));
        let mut sig_atomic_load = Signature::new(cc);
        sig_atomic_load.params.push(AbiParam::new(types::I64));
        sig_atomic_load.returns.push(AbiParam::new(types::I64));
        let mut sig_atomic_store = Signature::new(cc);
        sig_atomic_store.params.push(AbiParam::new(types::I64));
        sig_atomic_store.params.push(AbiParam::new(types::I64));
        sig_atomic_store.returns.push(AbiParam::new(types::I64));
        let mut sig_atomic_compare_exchange = Signature::new(cc);
        sig_atomic_compare_exchange
            .params
            .push(AbiParam::new(types::I64));
        sig_atomic_compare_exchange
            .params
            .push(AbiParam::new(types::I64));
        sig_atomic_compare_exchange
            .params
            .push(AbiParam::new(types::I64));
        sig_atomic_compare_exchange
            .returns
            .push(AbiParam::new(types::I64));
        let sig_atomic_try_new = sig_atomic_store.clone();
        let sig_atomic_try_add = sig_atomic_store.clone();
    }
    call: "jet_jit_extern_call" => jet_jit_extern_call: sig;
    callback_register: "jet_jit_ffi_callback_register" => jet_jit_ffi_callback_register: sig_callback_register;
    callback_unsubscribe: "jet_jit_ffi_callback_unsubscribe" => jet_jit_ffi_callback_unsubscribe: sig_callback_unsubscribe;
    callback_event_stop: "jet_jit_ffi_callback_event_stop" => jet_jit_ffi_callback_event_stop: sig_callback_event_stop;
    emit_task: "jet_jit_ffi_emit_task" => jet_jit_ffi_emit_task: sig_emit_task;
    drop_handle: "jet_jit_ffi_drop" => jet_jit_ffi_drop: sig_drop_handle;
    atomic_new: "JetAtomic::new" => jet_atomic_new: sig_atomic_store;
    atomic_try_new: "JetAtomic::try_new" => jet_atomic_try_new: sig_atomic_try_new;
    atomic_try_add: "jet_atomic_try_add" => jet_atomic_try_add: sig_atomic_try_add;
    atomic_load: "jet_atomic_load" => jet_atomic_load: sig_atomic_load;
    atomic_store: "jet_atomic_store" => jet_atomic_store: sig_atomic_store;
    atomic_add: "jet_atomic_add" => jet_atomic_add: sig_atomic_store;
    atomic_compare_exchange: "jet_atomic_compare_exchange" => jet_atomic_compare_exchange: sig_atomic_compare_exchange;
    atomic_publish: "jet_atomic_publish" => jet_atomic_publish: sig_atomic_store;
    atomic_observe: "jet_atomic_observe" => jet_atomic_observe: sig_atomic_load;
}

