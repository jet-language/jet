// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use jet_rt::JetVal;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Module};
use jet_codegen::embedded_hardware::{
    self as hardware_bridge, JetHardwareErasedHost, JetHardwareHost,
    JetHardwareInterruptHandler, JetHardwareInterruptRegistry,
    JetHardwareReplayHost as SharedHardwareReplayHost, JET_HARDWARE_UNAVAILABLE,
    JET_HARDWARE_SETUP_DMA_CONFIGURE, JET_HARDWARE_SETUP_INTERRUPT_BIND,
};
use jet_codegen::scheduler::{
    jet_scheduler_current_task_control, JetKeyedStream, JetSchedulerChannel, JetSchedulerJoin,
    JetSchedulerSender, JetStream, JetStreamEvent, JetStreamSender, JetStreamWindow,
    JetTaskControl,
};
use jet_foundation::AST::{CtFloat, CtValue};
use jet_foundation::MIR::*;
use jet_foundation::Shape::{ShapeFieldNames, ShapeProjectionKind};
use jet_foundation::TestingHistory::HistoryStrategyBehavior;
use jet_foundation::TestingComparison::{
    compare_samples, compare_samples_with_discarded, ComparisonIdentity, ComparisonObservation,
    ComparisonRecord, ComparisonSample, ComparisonStatus, ObservationRelation,
};
use jet_foundation::MatchScan::{
    jet_binary_pattern_match, jet_text_pattern_match, JetBinMatchPart, JetPatternCapture,
    JetTextHoleKind, JetTextMatchPart,
};
use jet_pkg_model::{ModelPackageCompiler, Package::ReleaseDevtoolsPolicy};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::panic::{catch_unwind, AssertUnwindSafe};

use super::resident::resident_teardown;
use super::{
    Archive, Cell as LocalCell, Collections, Compress, Compute, Concurrency, CoreHost, Crypto,
    Encoding, Fmt, JitResultValue, Memory, Net, Numeric, Process, Random, Solver, Text,
    Time, RESIDENT_JIT_RUN_LOCK,
};

pub(crate) use jet_codegen::Comptime::ClockRuntime as clock_rt;

fn history_capture_visit(
    state: &mut Option<&mut crate::Receipt::EncodeState>,
    depth: usize,
) -> Option<()> {
    if let Some(state) = state.as_deref_mut() {
        state.visit(depth).ok()?;
    }
    Some(())
}

fn history_capture_tree(
    rt: &mut JitRuntime,
    environment: i64,
    index: usize,
    descriptor: &RuntimeTypeDescriptor,
    capture_by_address: bool,
    mut state: Option<&mut crate::Receipt::EncodeState>,
    depth: usize,
) -> Option<crate::Encoding::json_rt::DataTree> {
    let index = i64::try_from(index).ok()?;
    if capture_by_address {
        let address = rt.heap.record_get_int(environment, index)?;
        if address == 0 {
            return None;
        }
        return match descriptor.kind {
            RuntimeValueKind::Int => {
                let raw = unsafe { std::ptr::read(address as *const i64) };
                let tree = crate::Receipt::encode_int(rt, raw, descriptor).ok()?;
                history_capture_visit(&mut state, depth)?;
                Some(tree)
            }
            RuntimeValueKind::String => {
                let raw = unsafe { std::ptr::read(address as *const i64) };
                let tree = rt
                    .heap
                    .clone_string(raw)
                    .map(crate::Encoding::json_rt::DataTree::Text)?;
                history_capture_visit(&mut state, depth)?;
                Some(tree)
            }
            RuntimeValueKind::Float => {
                let value = if descriptor.abi == RuntimeValueAbi::Float32 {
                    unsafe { f32::from_bits(std::ptr::read(address as *const u32)) as f64 }
                } else {
                    unsafe { f64::from_bits(std::ptr::read(address as *const u64)) }
                };
                let tree = value
                    .is_finite()
                    .then_some(crate::Encoding::json_rt::DataTree::Float(value))?;
                history_capture_visit(&mut state, depth)?;
                Some(tree)
            }
            RuntimeValueKind::Bool => {
                let value = unsafe { std::ptr::read(address as *const u8) != 0 };
                history_capture_visit(&mut state, depth)?;
                Some(crate::Encoding::json_rt::DataTree::Bool(value))
            }
            RuntimeValueKind::Char => {
                let value = unsafe { std::ptr::read(address as *const u32) };
                let value =
                    char::from_u32(value).map(|value| value.to_string())?;
                history_capture_visit(&mut state, depth)?;
                Some(crate::Encoding::json_rt::DataTree::Text(value))
            }
            RuntimeValueKind::Unit => {
                history_capture_visit(&mut state, depth)?;
                Some(crate::Encoding::json_rt::DataTree::Null)
            }
            _ => {
                let raw = unsafe { std::ptr::read(address as *const i64) };
                match state {
                    Some(state) => crate::Receipt::encode_jit_value_nested(
                        rt, raw, descriptor, state, depth,
                    )
                    .ok(),
                    None => crate::Receipt::encode_jit_value_with_history_callback(
                        rt,
                        raw,
                        descriptor,
                        history_encode_callable,
                    )
                    .ok(),
                }
            }
        };
    }
    match descriptor.kind {
        RuntimeValueKind::Int => {
            let raw = rt.heap.record_get_int(environment, index)?;
            let tree = crate::Receipt::encode_int(rt, raw, descriptor).ok()?;
            history_capture_visit(&mut state, depth)?;
            Some(tree)
        }
        RuntimeValueKind::String => {
            let value = rt
                .heap
                .record_clone_string(environment, index)
                .or_else(|| {
                    rt.heap
                        .record_get_int(environment, index)
                        .and_then(|raw| rt.heap.clone_string(raw))
                })?;
            history_capture_visit(&mut state, depth)?;
            Some(crate::Encoding::json_rt::DataTree::Text(value))
        }
        RuntimeValueKind::Float => {
            let value = rt
                .heap
                .record_get_float(environment, index)
                .filter(|value| value.is_finite())?;
            history_capture_visit(&mut state, depth)?;
            Some(crate::Encoding::json_rt::DataTree::Float(value))
        }
        RuntimeValueKind::Bool => {
            let value = rt.heap.record_get_bool(environment, index)?;
            history_capture_visit(&mut state, depth)?;
            Some(crate::Encoding::json_rt::DataTree::Bool(value))
        }
        RuntimeValueKind::Char => {
            let value = rt.heap.record_get_char(environment, index)?;
            history_capture_visit(&mut state, depth)?;
            Some(crate::Encoding::json_rt::DataTree::Text(value.to_string()))
        }
        RuntimeValueKind::Unit => {
            history_capture_visit(&mut state, depth)?;
            Some(crate::Encoding::json_rt::DataTree::Null)
        }
        _ => {
            let raw = rt.heap.record_get_int(environment, index)?;
            match state {
                Some(state) => crate::Receipt::encode_jit_value_nested(
                    rt, raw, descriptor, state, depth,
                )
                .ok(),
                None => crate::Receipt::encode_jit_value_with_history_callback(
                    rt,
                    raw,
                    descriptor,
                    history_encode_callable,
                )
                .ok(),
            }
        }
    }
}

fn history_encode_callable(
    rt: &mut JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut crate::Receipt::EncodeState,
    depth: usize,
) -> Result<crate::Encoding::json_rt::DataTree, String> {
    let slot = jit_callable_slot(rt, raw)
        .ok_or_else(|| "history capture callable is invalid".to_string())?;
    let target = rt
        .history_callable_targets
        .get(&slot.fn_ptr)
        .cloned()
        .ok_or_else(|| "history capture callable has no checked target".to_string())?;
    if target.capture_type_ids.len() != target.capture_place_flags.len() {
        return Err("history capture callable metadata is inconsistent".to_string());
    }
    if target.capture_type_ids.is_empty() {
        if slot.has_env {
            return Err("history capture callable has an unexpected environment".to_string());
        }
    } else if !slot.has_env {
        return Err("history capture callable has no environment".to_string());
    }
    let owned = if target.capture_type_ids.is_empty() {
        false
    } else {
        slot.history_captures_owned
            .ok_or_else(|| "history capture callable ownership is unavailable".to_string())?
    };
    state.enter(descriptor, raw)?;
    let result = (|| {
        let mut captures = Vec::with_capacity(target.capture_type_ids.len());
        for (index, type_id) in target.capture_type_ids.iter().enumerate() {
            let capture_descriptor = rt
                .runtime_type_descriptor(*type_id)
                .cloned()
                .ok_or_else(|| {
                    format!("history capture callable type descriptor {type_id} is unavailable")
                })?;
            let by_address = target.capture_place_flags[index] && !owned;
            let capture = history_capture_tree(
                rt,
                slot.env,
                index,
                &capture_descriptor,
                by_address,
                Some(&mut *state),
                depth.saturating_add(1),
            )
            .ok_or_else(|| "history capture callable value is unavailable".to_string())?;
            state.bytes(capture_descriptor.canonical.len())?;
            captures.push(crate::Encoding::json_rt::DataTree::Array(vec![
                crate::Encoding::json_rt::DataTree::Text(capture_descriptor.canonical.clone()),
                capture,
            ]));
        }
        let function_identity = format!("function:{}:{}", target.function_key, target.function_id);
        state.bytes("function_identity".len())?;
        state.bytes("captures".len())?;
        state.bytes(function_identity.len())?;
        Ok(crate::Encoding::json_rt::DataTree::Object(vec![
            (
                "function_identity".to_string(),
                crate::Encoding::json_rt::DataTree::Text(function_identity),
            ),
            (
                "captures".to_string(),
                crate::Encoding::json_rt::DataTree::Array(captures),
            ),
        ]))
    })();
    state.leave(descriptor, raw);
    result
}

fn history_callable_fingerprint(
    rt: &mut JitRuntime,
    slot: JitCallableSlot,
) -> Option<String> {
    let target = rt.history_callable_targets.get(&slot.fn_ptr)?.clone();
    if target.capture_type_ids.len() != target.capture_place_flags.len() {
        return None;
    }
    if target.capture_type_ids.is_empty() {
        if slot.has_env {
            return None;
        }
    } else if !slot.has_env || slot.history_captures_owned.is_none() {
        return None;
    }
    let mut material = Vec::new();
    let function_id = target.function_id.to_string();
    for part in [&target.function_key, &function_id] {
        material.extend_from_slice(&(part.len() as u64).to_le_bytes());
        material.extend_from_slice(part.as_bytes());
    }
    for (index, type_id) in target.capture_type_ids.iter().enumerate() {
        let descriptor = rt.runtime_type_descriptor(*type_id)?.clone();
        let by_address = target.capture_place_flags[index] && !slot.history_captures_owned?;
        let tree = history_capture_tree(rt, slot.env, index, &descriptor, by_address, None, 0)?;
        let encoded = crate::Encoding::json_rt::render_datatree_json(&tree, false, 0);
        let descriptor_key = descriptor.canonical;
        for part in [&descriptor_key, &encoded] {
            material.extend_from_slice(&(part.len() as u64).to_le_bytes());
            material.extend_from_slice(part.as_bytes());
        }
    }
    Some(jet_foundation::SHA256::sha256_hex(&material))
}

fn history_strategy_schema_fingerprint(descriptor: Option<&RuntimeTypeDescriptor>) -> String {
    let schema = descriptor
        .map(|descriptor| descriptor.canonical.as_str())
        .unwrap_or("DataTree");
    let mut material = b"history-strategy-schema".to_vec();
    material.extend_from_slice(&(schema.len() as u64).to_le_bytes());
    material.extend_from_slice(schema.as_bytes());
    jet_foundation::SHA256::sha256_hex(&material)
}

enum HistoryCallbackIdentity {
    Slot(JitCallableSlot),
    Fingerprint(String),
}

fn history_comparison_identity(
    case_id: String,
    input_id: String,
    seed: u64,
    callbacks: &[(&str, HistoryCallbackIdentity)],
) -> Option<ComparisonIdentity> {
    Concurrency::with_runtime_mut(|rt| {
        let provenance = rt.history_provenance.clone()?;
        if callbacks.is_empty() {
            return None;
        }
        let callback_identities = callbacks
            .iter()
            .map(|(role, identity)| {
                let fingerprint = match identity {
                    HistoryCallbackIdentity::Slot(slot) => history_callable_fingerprint(rt, *slot)?,
                    HistoryCallbackIdentity::Fingerprint(fingerprint) => fingerprint.clone(),
                };
                Some(((*role).to_string(), fingerprint))
            })
            .collect::<Option<Vec<_>>>()?;
        let callback_refs = callback_identities
            .iter()
            .map(|(role, fingerprint)| (role.as_str(), fingerprint.as_str()))
            .collect::<Vec<_>>();
        let provenance = provenance.bind_callbacks(&callback_refs).ok()?;
        let mut identity = ComparisonIdentity::new(
            case_id,
            input_id,
            provenance.source,
            provenance.tool,
            provenance.target,
        );
        identity.seed = Some(seed);
        Some(identity)
    })
}

pub(crate) mod duration_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/Duration.rs");
}

mod measurement_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/Measurement.rs");
}

pub(crate) mod contract_kernel {
    use jet_foundation::Outcome::{
        jet_err, jet_render_err, jet_render_runtime_stop, JetAbsent, JetErr, JetRuntimeDiagnostic,
    };
    include!("../../../jet-codegen/src/Prelude/Core/Contracts.rs");
}

pub(crate) mod fixed_arithmetic_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/FixedArithmetic.rs");
}

pub(crate) mod service_prelude {
    include!("../../../jet-codegen/src/Prelude/Service.rs");
}

pub(crate) mod inline_range_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/InlineRange.rs");
}

mod string_slice_kernel {
    use jet_foundation::StructuralDebug::jet_debug_range;
    include!("../../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
}

mod string_bytes_semantics {
    include!("../../../jet-codegen/src/Prelude/Core/StringBytes.rs");
}

thread_local! {
    static STRUCT_NEW_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[doc(hidden)]
pub fn reset_struct_new_count_for_test() {
    STRUCT_NEW_COUNT.with(|count| count.set(0));
}

#[doc(hidden)]
pub fn struct_new_count_for_test() -> usize {
    STRUCT_NEW_COUNT.with(Cell::get)
}

/// Move a compiler worker's `struct_new` tally back onto its caller's thread,
/// so a test that resets before the boundary and asserts after it sees the
/// count the run actually produced instead of a silent zero.
pub(crate) fn add_struct_new_count_for_test(extra: usize) {
    STRUCT_NEW_COUNT.with(|count| count.set(count.get() + extra));
}

thread_local! {
    /// How many [`catch_jit_panic`] windows are live on THIS thread.
    ///
    /// The panic hook is necessarily process-global; the suppression must not
    /// be. A test binary runs unrelated threads alongside a resident JIT run,
    /// and their panics have to keep printing their message and location, so
    /// the hook asks the *panicking* thread whether it is inside a window
    /// instead of relying on a global install/restore period (#1995).
    static JIT_PANIC_WINDOW: Cell<u32> = const { Cell::new(0) };

    /// Message from the last panic [`catch_jit_panic`] silenced.
    ///
    /// A panic that tries to escape an `extern "C"` JIT host raises the hook
    /// TWICE: once for the real failure, and once at the frame edge for
    /// `core::panicking::panic_cannot_unwind`. By the time the second one
    /// arrives the first message is the only evidence of what actually broke,
    /// so keep it until the window closes.
    static SILENCED_JIT_PANIC: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Whether the panicking thread is inside a [`catch_jit_panic`] window.
///
/// `try_with`, not `with`: a panic can be raised from a thread-local
/// destructor, when this key is already destroyed, and `with` would panic
/// again from inside the hook.
fn jit_panic_window_open() -> bool {
    JIT_PANIC_WINDOW
        .try_with(|depth| depth.get() != 0)
        .unwrap_or(false)
}

fn take_silenced_jit_panic() -> Option<String> {
    SILENCED_JIT_PANIC
        .try_with(|slot| slot.borrow_mut().take())
        .unwrap_or(None)
}

fn record_silenced_jit_panic(text: String) {
    let _ = SILENCED_JIT_PANIC.try_with(|slot| *slot.borrow_mut() = Some(text));
}

/// Marks its thread as running resident JIT work for the hook's benefit.
struct JitPanicWindow;

impl JitPanicWindow {
    fn enter() -> Self {
        JIT_PANIC_WINDOW.with(|depth| {
            let outer = depth.get();
            if outer == 0 {
                let _ = SILENCED_JIT_PANIC.try_with(|slot| *slot.borrow_mut() = None);
            }
            depth.set(outer + 1);
        });
        Self
    }
}

impl Drop for JitPanicWindow {
    fn drop(&mut self) {
        JIT_PANIC_WINDOW.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Installed once per process, wrapping whatever hook was live at that point,
/// and never taken back off — the same shape as
/// `Prelude/Scheduler.rs`'s `JET_SCHEDULER_PANIC_HOOK`. Install/restore around
/// each run was not merely over-broad: a scheduler hook installed *during* a
/// JIT window was silently discarded by the restore, permanently, because that
/// side installs exactly once too.
static JIT_PANIC_HOOK: std::sync::LazyLock<()> = std::sync::LazyLock::new(|| {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !jit_panic_window_open() {
            // Nothing to do with this JIT run — report it normally.
            previous(info);
            return;
        }
        let message = info.payload_as_str().unwrap_or("unknown panic payload");
        if !is_nounwind_abort(Some(message)) {
            // Recoverable: the caller converts this into a named gap and
            // deopts, so the user sees no Rust panic banner. Keep the text
            // in case the same run then hits a frame edge it cannot cross.
            let recorded = match info.location() {
                Some(at) => format!("{message} (at {at})"),
                None => message.to_string(),
            };
            record_silenced_jit_panic(recorded);
        }
    }));
});
fn is_nounwind_abort(message: Option<&str>) -> bool {
    matches!(
        message,
        Some("panic in a function that cannot unwind" | "panic in a destructor during cleanup")
    )
}

/// Run resident JIT work, converting an unwinding panic into a named gap so
/// the caller deopts instead of dying.
///
/// The name is honest only for panics that can unwind. A panic that reaches an
/// `extern "C"` frame edge is turned into a non-unwinding panic that aborts
/// inside `panic_with_hook`, so this function neither catches nor converts it.
/// The fix for that class is not here: every `extern "C"` frame this crate
/// exposes to generated code is now the generated `host_seam` shim, which
/// catches and converts *inside* its own C frame (`src/host_seam.rs`, #1997).
pub(crate) fn catch_jit_panic<R>(
    context: &str,
    f: impl FnOnce() -> Result<R, String>,
) -> Result<R, String> {
    let result = {
        let _serialize = RESIDENT_JIT_RUN_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::sync::LazyLock::force(&JIT_PANIC_HOOK);
        let _window = JitPanicWindow::enter();
        catch_unwind(AssertUnwindSafe(f))
    };
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            resident_teardown();
            Err(err)
        }
        Err(payload) => {
            resident_teardown();
            let detail = take_silenced_jit_panic().unwrap_or_else(|| {
                if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else {
                    "unknown panic payload".into()
                }
            });
            Err(format!(
                "jit {context} panicked before returning an unsupported reason: {detail}"
            ))
        }
    }
}

/// Live heap carried across type-stable hot_swap (M2). `invocations` counts
/// how many times `main` ran without a clean restart — preserved on swap,
/// reset on restart.
#[derive(Clone)]
pub(crate) struct ReflectSlot {
    /// Present only for a field projection. It points at the nested typed
    /// Value returned by `Field.value()`; text is only `display`.
    pub field_name: Option<String>,
    pub type_name: String,
    pub path: String,
    pub display: String,
    pub fields: Vec<(String, i64)>,
    pub value: Option<i64>,
}

/// Stable checked identity for one compiled function target.  Runtime closure
/// environments are decoded only with the capture type identities selected by
/// MIR; raw function addresses never enter evidence.
#[derive(Clone)]
pub(crate) struct JitHistoryCallableTarget {
    pub(crate) function_id: u64,
    pub(crate) function_key: String,
    pub(crate) capture_type_ids: Vec<u64>,
    pub(crate) capture_place_flags: Vec<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct JitCallableSlot {
    pub fn_ptr: i64,
    pub env: i64,
    pub has_env: bool,
    /// Whether closure capture places were copied into the environment
    /// (`true`) or retained as addresses (`false`). The compiler supplies
    /// this checked fact after binding; unknown means the slot is not safe to
    /// use for capture provenance.
    pub history_captures_owned: Option<bool>,
    /// Typed pointers for generated universal callback thunks. Exactly one of
    /// these fields is set for a slot bound at a callback site. Arity zero and
    /// arities wider than two use `raw_many`, whose second argument points at
    /// an i64 payload array.
    pub raw_unary: Option<unsafe extern "C" fn(i64, i64) -> i64>,
    pub raw_pair: Option<unsafe extern "C" fn(i64, i64, i64) -> i64>,
    pub raw_many: Option<unsafe extern "C" fn(i64, *const i64) -> i64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JitZipMode {
    Short,
    Strict,
    Pad,
}

/// Deferred `Iter<T>` pipeline state. The carrier is deliberately separate
/// from `JitViewSlot`: a view has known bounds and random access, while an
/// iterator owns a pull cursor and may stop before its source is exhausted.

pub(crate) enum JitLazyIter {
    Source {
        source: i64,
        index: usize,
    },
    Map {
        source: i64,
        callback: JitCallableSlot,
    },
    Enumerate {
        source: i64,
        index: usize,
        callback: JitCallableSlot,
    },

    FilterMap {
        source: i64,
        callback: JitCallableSlot,
    },
    Take {
        source: i64,
        remaining: usize,
    },
    Skip {
        source: i64,
        remaining: usize,
    },
    Filter {
        source: i64,
        callback: JitCallableSlot,
    },
    FlatMap {
        source: i64,
        callback: JitCallableSlot,
        active: Option<i64>,
    },
    TakeWhile {
        source: i64,
        callback: JitCallableSlot,
        done: bool,
    },
    SkipWhile {
        source: i64,
        callback: JitCallableSlot,
        skipping: bool,
    },
    Scan {
        source: i64,
        callback: JitCallableSlot,
        accumulator: i64,
    },
    Zip {
        left: i64,
        right: i64,
        callback: JitCallableSlot,
        mode: JitZipMode,
        left_fill: Option<i64>,
        right_fill: Option<i64>,
    },

}

impl JitLazyIter {
    fn next_value(&mut self, rt: &mut JitRuntime) -> Option<i64> {
        match self {
            Self::Source { source, index } => {
                let value = sequence_get_raw(rt, *source, *index)?;
                *index = index.saturating_add(1);
                Some(value)
            }
            Self::Map { source, callback } => {
                let value = lazy_iter_next(rt, *source)?;
                let mapped = invoke_universal_unary(*callback, value);
                if runtime_stop_pending(rt) {
                    return None;
                }
                if mapped.is_none() {
                    rt.set_host_fault("lazy iterator map callback has no unary universal thunk");
                }
                mapped
            }
            Self::Enumerate { source, index, callback } => {
                let value = lazy_iter_next(rt, *source)?;
                let index_value = *index as i64;
                *index = index.saturating_add(1);
                let Some(mapped) = invoke_universal_pair(*callback, index_value, value) else {
                    rt.set_host_fault("lazy iterator enumerate callback has no pair universal thunk");
                    return None;
                };
                if runtime_stop_pending(rt) {
                    return None;
                }
                Some(mapped)
            }

            Self::FilterMap { source, callback } => loop {
                let value = lazy_iter_next(rt, *source)?;
                let Some(mapped) = invoke_universal_unary(*callback, value) else {
                    rt.set_host_fault("lazy iterator filter_map callback has no unary universal thunk");
                    return None;
                };
                if runtime_stop_pending(rt) {
                    return None;
                }
                let Some((ok, bits)) = jit_result_parts(rt, mapped) else {
                    rt.set_host_fault("lazy iterator filter_map callback returned an invalid Result");
                    return None;
                };
                if ok {
                    return Some(bits as i64);
                }
            },
            Self::Take { source, remaining } => {
                if *remaining == 0 {
                    return None;
                }
                *remaining -= 1;
                lazy_iter_next(rt, *source)
            }
            Self::Skip { source, remaining } => {
                while *remaining != 0 {
                    lazy_iter_next(rt, *source)?;
                    *remaining -= 1;
                }
                lazy_iter_next(rt, *source)
            }
            Self::Filter { source, callback } => loop {
                let value = lazy_iter_next(rt, *source)?;
                let keep = invoke_universal_unary(*callback, value);
                let Some(keep) = keep else {
                    rt.set_host_fault(
                        "lazy iterator filter callback has no unary universal thunk",
                    );
                    return None;
                };
                if runtime_stop_pending(rt) {
                    return None;
                }
                if keep != 0 {
                    return Some(value);
                }
            },
            Self::FlatMap {
                source,
                callback,
                active,
            } => loop {
                if let Some(nested) = *active {
                    if let Some(value) = lazy_iter_next(rt, nested) {
                        return Some(value);
                    }
                    *active = None;
                }
                let value = lazy_iter_next(rt, *source)?;
                let nested = invoke_universal_unary(*callback, value);
                let Some(nested) = nested else {
                    rt.set_host_fault(
                        "lazy iterator flat_map callback has no unary universal thunk",
                    );
                    return None;
                };
                if runtime_stop_pending(rt) {
                    return None;
                }
                let nested = if lazy_iter_index(rt, nested).is_some() {
                    nested
                } else if sequence_len(rt, nested).is_some() {
                    lazy_iter_source(rt, nested)
                } else {
                    rt.set_host_fault(
                        "lazy iterator flat_map callback returned a non-sequence handle",
                    );
                    return None;
                };
                *active = Some(nested);
            },
            Self::TakeWhile {
                source,
                callback,
                done,
            } => {
                if *done {
                    return None;
                }
                let value = lazy_iter_next(rt, *source)?;
                let keep = invoke_universal_unary(*callback, value);
                let Some(keep) = keep else {
                    rt.set_host_fault(
                        "lazy iterator take_while callback has no unary universal thunk",
                    );
                    return None;
                };
                if runtime_stop_pending(rt) {
                    return None;
                }
                if keep == 0 {
                    *done = true;
                    None
                } else {
                    Some(value)
                }
            }
            Self::SkipWhile {
                source,
                callback,
                skipping,
            } => {
                while *skipping {
                    let value = lazy_iter_next(rt, *source)?;
                    let keep = invoke_universal_unary(*callback, value);
                    let Some(keep) = keep else {
                        rt.set_host_fault(
                            "lazy iterator skip_while callback has no unary universal thunk",
                        );
                        return None;
                    };
                    if runtime_stop_pending(rt) {
                        return None;
                    }
                    if keep == 0 {
                        *skipping = false;
                        return Some(value);
                    }
                }
                lazy_iter_next(rt, *source)
            }
            Self::Scan {
                source,
                callback,
                accumulator,
            } => {
                let value = lazy_iter_next(rt, *source)?;
                let next = invoke_universal_pair(*callback, *accumulator, value);
                let Some(next) = next else {
                    rt.set_host_fault("lazy iterator scan callback has no pair universal thunk");
                    return None;
                };
                if runtime_stop_pending(rt) {
                    return None;
                }
                *accumulator = next;
                Some(next)
            },
            Self::Zip {
                left,
                right,
                callback,
                mode,
                left_fill,
                right_fill,
            } => {
                let pair = match *mode {
                    JitZipMode::Short => Collections::collection_semantics::jet_zip_short_step(
                        lazy_iter_next(rt, *left),
                        || lazy_iter_next(rt, *right),
                    ),
                    JitZipMode::Strict => {
                        match Collections::collection_semantics::jet_zip_strict_step(
                            lazy_iter_next(rt, *left),
                            lazy_iter_next(rt, *right),
                        ) {
                            Ok(pair) => pair,
                            Err(()) => {
                                rt.set_runtime_stop_at(
                                    "E3001",
                                    "<core.collections>",
                                    0,
                                    Collections::collection_semantics::
                                        jet_zip_length_mismatch_message(),
                                );
                                return None;
                            }
                        }
                    }
                    JitZipMode::Pad => {
                        let (Some(left_fill), Some(right_fill)) = (*left_fill, *right_fill) else {
                            rt.set_host_fault("lazy iterator zip padding has no fill values");
                            return None;
                        };
                        Collections::collection_semantics::jet_zip_pad_step(
                            lazy_iter_next(rt, *left),
                            lazy_iter_next(rt, *right),
                            left_fill,
                            right_fill,
                        )
                    }
                };
                let Some((left, right)) = pair else {
                    return None;
                };
                let mapped = invoke_universal_pair(*callback, left, right);
                if runtime_stop_pending(rt) {
                    return None;
                }
                if mapped.is_none() {
                    rt.set_host_fault("lazy iterator zip callback has no pair universal thunk");
                }
                mapped
            }
        }
    }
}

/// Checked adapter metadata for one queue-dispatched MIR job.
///
/// The compiled entrypoint is called only after the queue payload has been
/// decoded through its registered runtime descriptor. The ABI tags make the
/// host call use the same Cranelift scalar carriers as the compiled function;
/// no erased "all i64" call is permitted for floating or boolean values.
#[derive(Clone, Copy)]
pub(crate) struct JitJobAdapter {
    pub(crate) callback: JitCallableSlot,
    pub(crate) input_type: u64,
    pub(crate) input_abi: RuntimeValueAbi,
    pub(crate) return_type: u64,
    pub(crate) return_abi: RuntimeValueAbi,
}
/// Canonical iterable hook pointers installed for one compiled resident.
///
/// `iter_slot` and `next_slot` are finalized universal thunks with the
/// `(env, raw) -> raw` ABI. The source-wire key is the complete checked
/// `MirLoopSourceKind::Iterable` identity; the type names are retained for
/// runtime carrier validation and diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JitIterableHook {
    pub(crate) iter_slot: i64,
    pub(crate) next_slot: i64,
    pub(crate) coll_type: String,
    pub(crate) iter_type: String,
}

#[derive(Clone)]
pub(crate) enum JitPatternDescriptor {
    Text(Vec<MirTextPatternPart>),
    Binary(Vec<MirBinaryPatternPart>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeValueKind {
    Unit,
    Int,
    Float,
    Bool,
    Char,
    String,
    List,
    Map,
    Shared,
    Option,
    Result,
    Record,
    Enum,
    Closure,
    View,
    Iterator,
    Named,
    Handle,
}
/// Width facts for fixed-width integer carriers. Ordinary `Int` remains a
/// resident heap handle; `IntN` values use their direct scalar bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeIntegerWidth {
    pub(crate) signed: bool,
    pub(crate) bits: u8,
}

/// Machine carrier used by the resident universal Value ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeValueAbi {
    Unit,
    Int,
    Float,
    Float32,
    Bool,
    Char,
    Handle,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeFieldDescriptor {
    pub(crate) index: usize,
    pub(crate) source_name: String,
    pub(crate) shape_names: ShapeFieldNames,
    pub(crate) skip: bool,
    pub(crate) computed: bool,
    pub(crate) has_default: bool,
    pub(crate) type_id: u64,
}

impl RuntimeFieldDescriptor {
    pub(crate) fn name_for(&self, projection: ShapeProjectionKind) -> &str {
        self.shape_names
            .name_for(projection)
            .unwrap_or(self.source_name.as_str())
    }

    pub(crate) fn matches_name(&self, name: &str, projection: ShapeProjectionKind) -> bool {
        name == self.source_name || name == self.name_for(projection)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeVariantDescriptor {
    pub(crate) name: String,
    pub(crate) wire_name: String,
    pub(crate) discriminant: i64,
    pub(crate) fields: Vec<RuntimeFieldDescriptor>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuntimeTypeDescriptor {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) canonical: String,
    pub(crate) kind: RuntimeValueKind,
    pub(crate) abi: RuntimeValueAbi,
    pub(crate) integer_width: Option<RuntimeIntegerWidth>,
    pub(crate) integer_range: Option<(i128, i128)>,
    pub(crate) element: Option<u64>,
    pub(crate) key: Option<u64>,
    pub(crate) value: Option<u64>,
    pub(crate) ok: Option<u64>,
    pub(crate) err: Option<u64>,
    pub(crate) serde_tag: Option<String>,
    pub(crate) serde_untagged: bool,
    pub(crate) serde_deny_unknown: bool,
    pub(crate) cli: Option<jet_foundation::MIR::MirCliEntry>,
    pub(crate) fields: Vec<RuntimeFieldDescriptor>,
    pub(crate) variants: Vec<RuntimeVariantDescriptor>,
}

fn runtime_value_kind(ty: &MirTypeKind) -> RuntimeValueKind {
    match ty {
        MirTypeKind::Int | MirTypeKind::IntN { .. } | MirTypeKind::Measure(_) => {
            RuntimeValueKind::Int
        }
        MirTypeKind::Float | MirTypeKind::Float32 => RuntimeValueKind::Float,
        MirTypeKind::Bool => RuntimeValueKind::Bool,
        MirTypeKind::Char => RuntimeValueKind::Char,
        MirTypeKind::String => RuntimeValueKind::String,
        MirTypeKind::List(_) | MirTypeKind::FixedList { .. } => RuntimeValueKind::List,
        MirTypeKind::Map { .. } => RuntimeValueKind::Map,
        MirTypeKind::Shared(_) => RuntimeValueKind::Shared,
        MirTypeKind::Option(_) => RuntimeValueKind::Option,
        MirTypeKind::Result { .. } => RuntimeValueKind::Result,
        MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. } => RuntimeValueKind::Closure,
        // A bare nominal (`Foo`) is a checked user type with a descriptor;
        // applied List/Result/View/Iter types use their checked carrier kind.
        MirTypeKind::Apply { args, .. } if args.is_empty() => RuntimeValueKind::Named,
        MirTypeKind::Apply { name, args } => {
            if name.name == "List" && args.len() == 1 {
                RuntimeValueKind::List
            } else if name.name == "Result" && args.len() == 2 {
                RuntimeValueKind::Result
            } else {
                match name.name.as_str() {
                    "View" | "ViewMut" => RuntimeValueKind::View,
                    "Iter" | "ViewIter" => RuntimeValueKind::Iterator,
                    _ => RuntimeValueKind::Handle,
                }
            }
        },
        MirTypeKind::Tuple(_) => RuntimeValueKind::Record,
        MirTypeKind::TraitObject(_) => RuntimeValueKind::Handle,
        MirTypeKind::Union(_) => RuntimeValueKind::Enum,
        MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => runtime_value_kind(&inner.kind),
    }
}
fn runtime_integer_width(kind: &MirTypeKind) -> Option<RuntimeIntegerWidth> {
    match kind {
        MirTypeKind::IntN { signed, bits } => Some(RuntimeIntegerWidth {
            signed: *signed,
            bits: *bits,
        }),
        MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => runtime_integer_width(&inner.kind),
        _ => None,
    }
}

fn runtime_integer_range(kind: &MirTypeKind) -> Option<(i128, i128)> {
    match kind {
        MirTypeKind::IntN { signed, bits } => {
            let bits = u32::from(*bits);
            if bits == 0 || bits >= 127 {
                return None;
            }
            let max = if *signed {
                (1_i128 << (bits - 1)) - 1
            } else {
                (1_i128 << bits) - 1
            };
            Some(if *signed { (-(max + 1), max) } else { (0, max) })
        }
        MirTypeKind::InlineRange { lo, hi, .. } => Some((i128::from(*lo), i128::from(*hi))),
        MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
            runtime_integer_range(&inner.kind)
        }
        _ => None,
    }
}


fn runtime_value_abi(ty: &MirType) -> RuntimeValueAbi {
    match ty.layout.abi {
        MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer) => RuntimeValueAbi::Int,
        MirAbi::Scalar(MirScalarKind::Float) => RuntimeValueAbi::Float,
        MirAbi::Scalar(MirScalarKind::Float32) => RuntimeValueAbi::Float32,
        MirAbi::Scalar(MirScalarKind::Bool) => RuntimeValueAbi::Bool,
        MirAbi::Scalar(MirScalarKind::Char) => RuntimeValueAbi::Char,
        MirAbi::Aggregate
        | MirAbi::Sequence
        | MirAbi::Function
        | MirAbi::Nominal
        | MirAbi::Dynamic => RuntimeValueAbi::Handle,
        MirAbi::Never => RuntimeValueAbi::Unit,
    }
}

fn runtime_synthetic_type_id(ty: &MirType) -> u64 {
    stable_id("jit-runtime-type", &ty.kind.canonical_key()) | (1_u64 << 63)
}

pub(crate) fn runtime_type_id(ty: &MirType) -> Option<u64> {
    Some(
        ty.identity
            .map(|identity| identity.0)
            .unwrap_or_else(|| runtime_synthetic_type_id(ty)),
    )
}

fn runtime_field_descriptor(
    index: usize,
    field: &jet_foundation::MIR::MirField,
) -> Option<RuntimeFieldDescriptor> {
    Some(RuntimeFieldDescriptor {
        index,
        source_name: field.name.clone(),
        shape_names: field.shape_names.clone(),
        skip: field.skip,
        computed: field.computed,
        has_default: field.has_default,
        type_id: runtime_type_id(&field.ty)?,
    })
}

fn runtime_type_descriptor(ty: &MirType) -> Option<RuntimeTypeDescriptor> {
    let id = runtime_type_id(ty)?;
    let (element, key, value, ok, err) = match &ty.kind {
        MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
            (runtime_type_id(inner), None, None, None, None)
        }
        MirTypeKind::Map { key, value } => {
            (None, runtime_type_id(key), runtime_type_id(value), None, None)
        }
        MirTypeKind::Shared(inner) => (runtime_type_id(inner), None, None, None, None),
        MirTypeKind::Option(inner) => (None, None, None, runtime_type_id(inner), None),
        MirTypeKind::Apply { name, args } => {
            let kind = runtime_value_kind(&ty.kind);
            match (kind, args.as_slice()) {
                (
                    RuntimeValueKind::List | RuntimeValueKind::View | RuntimeValueKind::Iterator,
                    [inner],
                ) => (runtime_type_id(inner), None, None, None, None),
                _ if name.name == "Result" && args.len() == 2 => {
                    (None, None, None, runtime_type_id(&args[0]), runtime_type_id(&args[1]))
                }
                _ => (None, None, None, None, None),
            }
        }
        MirTypeKind::Result { ok, err } => {
            (None, None, None, runtime_type_id(ok), runtime_type_id(err))
        }
        MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => {
            let inner = runtime_type_descriptor(inner)?;
            (inner.element, inner.key, inner.value, inner.ok, inner.err)
        }
        _ => (None, None, None, None, None),
    };
    let fields = match &ty.kind {
        MirTypeKind::Tuple(fields) => fields
            .iter()
            .enumerate()
            .filter_map(|(index, (name, field_ty))| {
                Some(RuntimeFieldDescriptor {
                    index,
                    source_name: name.clone(),
                    shape_names: ShapeFieldNames::from_source(name),
                    skip: false,
                    computed: false,
                    has_default: false,
                    type_id: runtime_type_id(field_ty)?,
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    Some(RuntimeTypeDescriptor {
        id,
        name: ty.display_name(),
        canonical: ty.identity_key(),
        kind: runtime_value_kind(&ty.kind),
        abi: runtime_value_abi(ty),
        integer_width: runtime_integer_width(&ty.kind),
        integer_range: runtime_integer_range(&ty.kind),
        element,
        key,
        value,
        ok,
        err,
        serde_tag: None,
        serde_untagged: false,
        serde_deny_unknown: false,
        cli: None,
        fields,
        variants: Vec::new(),
    })
}

/// Convert the compiler's checked type table into the small resident registry.
/// This is the only compiler-to-runtime metadata boundary; the runtime never
/// stores `MirType` or `JitMeta`.
pub(crate) fn runtime_type_descriptors(program: &MirProgram) -> Vec<RuntimeTypeDescriptor> {
    let mut descriptors = program
        .type_instances
        .iter()
        .filter_map(runtime_type_descriptor)
        .map(|descriptor| (descriptor.id, descriptor))
        .collect::<HashMap<_, _>>();
    for definition in &program.types {
        descriptors.entry(definition.id.0).or_insert_with(|| RuntimeTypeDescriptor {
            id: definition.id.0,
            name: definition.name.clone(),
            canonical: definition.key.clone(),
            kind: RuntimeValueKind::Named,
            abi: RuntimeValueAbi::Handle,
            integer_width: None,
            integer_range: None,
            element: None,
            key: None,
            value: None,
            ok: None,
            err: None,
            serde_tag: None,
            serde_untagged: false,
            serde_deny_unknown: false,
            cli: None,
            fields: Vec::new(),
            variants: Vec::new(),
        });
        let descriptor = descriptors
            .get_mut(&definition.id.0)
            .expect("enum/struct descriptor row was just inserted");
        descriptor.name = definition.name.clone();
        descriptor.canonical = definition.key.clone();
        descriptor.cli = definition.cli.clone();
        descriptor.serde_tag = definition.serde.iter().find_map(|attribute| {
            (attribute.kind == jet_foundation::MIR::MirSerdeAttributeKind::Tag)
                .then(|| attribute.value.clone())
                .flatten()
        });
        descriptor.serde_untagged = definition
            .serde
            .iter()
            .any(|attribute| attribute.kind == jet_foundation::MIR::MirSerdeAttributeKind::Untagged);
        descriptor.serde_deny_unknown = definition
            .serde
            .iter()
            .any(|attribute| {
                attribute.kind
                    == jet_foundation::MIR::MirSerdeAttributeKind::DenyUnknownFields
            });
        match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                descriptor.kind = RuntimeValueKind::Record;
                descriptor.abi = RuntimeValueAbi::Handle;
                descriptor.fields = fields
                    .iter()
                    .enumerate()
                    .filter_map(|(index, field)| runtime_field_descriptor(index, field))
                    .collect();
            }
            MirTypeDefKind::Enum { variants, .. } => {
                descriptor.kind = RuntimeValueKind::Enum;
                descriptor.abi = RuntimeValueAbi::Handle;
                descriptor.variants = variants
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| {
                        let fields = match &variant.payload {
                            MirVariantPayload::Unit => Vec::new(),
                            MirVariantPayload::Single(ty) => runtime_type_id(ty)
                                .map(|type_id| {
                                    vec![RuntimeFieldDescriptor {
                                        index: 0,
                                        source_name: "value".to_string(),
                                        shape_names: ShapeFieldNames::from_source("value"),
                                        skip: false,
                                        computed: false,
                                        has_default: false,
                                        type_id,
                                    }]
                                })
                                .unwrap_or_default(),
                            MirVariantPayload::Named(fields) => fields
                                .iter()
                                .enumerate()
                                .filter_map(|(field_index, field)| {
                                    runtime_field_descriptor(field_index, field)
                                })
                                .collect(),
                        };
                        RuntimeVariantDescriptor {
                            name: variant.name.clone(),
                            wire_name: variant.wire_name.clone(),
                            discriminant: variant.discriminant.unwrap_or(index as i64),
                            fields,
                        }
                    })
                    .collect();
            }
            MirTypeDefKind::Distinct { base, range } => {
                descriptor.kind = runtime_value_kind(&base.kind);
                descriptor.abi = runtime_value_abi(base);
                descriptor.integer_width = runtime_integer_width(&base.kind);
                descriptor.integer_range = range
                    .map(|(lo, hi)| (i128::from(lo), i128::from(hi)))
                    .or_else(|| runtime_integer_range(&base.kind));
                descriptor.element = match &base.kind {
                    MirTypeKind::List(inner)
                    | MirTypeKind::FixedList { elem: inner, .. } => runtime_type_id(inner),
                    _ => descriptor.element,
                };
            }
            MirTypeDefKind::Alias { target: base } => {
                descriptor.kind = runtime_value_kind(&base.kind);
                descriptor.abi = runtime_value_abi(base);
                descriptor.integer_width = runtime_integer_width(&base.kind);
                descriptor.integer_range = runtime_integer_range(&base.kind);
                descriptor.element = match &base.kind {
                    MirTypeKind::List(inner)
                    | MirTypeKind::FixedList { elem: inner, .. } => runtime_type_id(inner),
                    _ => descriptor.element,
                };
            }
            MirTypeDefKind::UnitFamily { .. } => {
                descriptor.kind = RuntimeValueKind::Unit;
                descriptor.abi = RuntimeValueAbi::Unit;
            }
        }
    }
    let mut descriptors = descriptors.into_values().collect::<Vec<_>>();
    descriptors.sort_by_key(|descriptor| descriptor.id);
    descriptors
}

/// Project the checked types reachable from one directly compiled function.
/// This entry point intentionally accepts only the function: direct
/// `compile_mir_function` callers do not have to manufacture a second
/// compiler metadata table.
pub(crate) fn install_program_type_descriptors(
    runtime: &mut JitRuntime,
    program: &MirProgram,
    functions: &[&MirFunction],
) {
    runtime.install_type_descriptors(runtime_type_descriptors(program));
    let existing: HashSet<u64> = runtime.type_descriptors.keys().copied().collect();
    let extras = functions
        .iter()
        .flat_map(|function| runtime_type_descriptors_for_function(function))
        .filter(|descriptor| !existing.contains(&descriptor.id));
    runtime.install_type_descriptors(extras);
}

pub(crate) fn runtime_type_descriptors_for_function(
    function: &MirFunction,
) -> Vec<RuntimeTypeDescriptor> {
    fn collect(ty: &MirType, descriptors: &mut HashMap<u64, RuntimeTypeDescriptor>) {
        if let Some(descriptor) = runtime_type_descriptor(ty) {
            descriptors.entry(descriptor.id).or_insert(descriptor);
        }
        match &ty.kind {
            MirTypeKind::List(inner)
            | MirTypeKind::Shared(inner)
            | MirTypeKind::Option(inner)
            | MirTypeKind::InlineRange { base: inner, .. }
            | MirTypeKind::Tagged { inner, .. }
            | MirTypeKind::Quantity { base: inner, .. } => collect(inner, descriptors),
            MirTypeKind::Map { key, value } => {
                collect(key, descriptors);
                collect(value, descriptors);
            }
            MirTypeKind::Result { ok, err } => {
                collect(ok, descriptors);
                collect(err, descriptors);
            }
            MirTypeKind::Fn(signature) => {
                signature
                    .params
                    .iter()
                    .for_each(|param| collect(param, descriptors));
                if let Some(ret) = &signature.ret {
                    collect(ret, descriptors);
                }
            }
            MirTypeKind::SendFn { params, ret } => {
                params.iter().for_each(|param| collect(param, descriptors));
                if let Some(ret) = ret {
                    collect(ret, descriptors);
                }
            }
            MirTypeKind::Apply { args, .. } => {
                args.iter().for_each(|arg| collect(arg, descriptors));
            }
            MirTypeKind::Tuple(fields) => {
                fields
                    .iter()
                    .for_each(|(_, field)| collect(field, descriptors));
            }
            MirTypeKind::FixedList { elem, .. } => collect(elem, descriptors),
            MirTypeKind::Union(members) => {
                members.iter().for_each(|member| collect(member, descriptors));
            }
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::IntN { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Measure(_) => {}
        }
    }

    let mut descriptors = HashMap::new();
    function.params.iter().for_each(|param| collect(&param.ty, &mut descriptors));
    function
        .capture_params
        .iter()
        .for_each(|param| collect(&param.ty, &mut descriptors));
    function.locals.iter().for_each(|local| collect(&local.ty, &mut descriptors));
    function
        .values
        .iter()
        .for_each(|(_, ty, _, _)| collect(ty, &mut descriptors));
    collect(&function.return_type, &mut descriptors);
    if let Some(return_type) = &function.declared_return {
        collect(return_type, &mut descriptors);
    }
    let mut descriptors = descriptors.into_values().collect::<Vec<_>>();
    descriptors.sort_by_key(|descriptor| descriptor.id);
    descriptors
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JitZipValueKind {
    Int,
    Float,
    Bool,
    Char,
    String,
    Opaque,
}

impl JitZipValueKind {
    /// The ONE wire encoding for a column kind.
    ///
    /// `jet_jit_iter_zip_family` hands its kinds over in a registered
    /// `JitZipPlan`; `jet_jit_list_unzip` has exactly two columns and no fill
    /// mode, so it takes them as immediates instead of paying for a plan slot.
    /// The host cannot read a record field back without knowing the kind
    /// `jit_zip_set_value` wrote it in — reading a `record_set_string` field
    /// with `record_get_int` is how unzip used to answer two EMPTY lists.
    pub(crate) fn code(self) -> i64 {
        match self {
            Self::Int => 0,
            Self::Float => 1,
            Self::Bool => 2,
            Self::Char => 3,
            Self::String => 4,
            Self::Opaque => 5,
        }
    }

    pub(crate) fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Int),
            1 => Some(Self::Float),
            2 => Some(Self::Bool),
            3 => Some(Self::Char),
            4 => Some(Self::String),
            5 => Some(Self::Opaque),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct JitZipColumn {
    pub(crate) input: JitZipValueKind,
    pub(crate) field: JitZipValueKind,
    pub(crate) optional: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct JitZipPlan {
    pub(crate) mode: u8,
    pub(crate) fill_mode: u8,
    pub(crate) columns: Vec<JitZipColumn>,
}

/// A checked, read-only view over an existing JIT sequence or string.
///
/// View handles are tagged independently from heap handles. The slot records
/// only provenance and bounds; it never owns or copies the source bytes.
pub(crate) enum JitViewSlot {
    Sequence {
        source: i64,
        start: usize,
        end: usize,
    },
    String {
        owner: i64,
        start: usize,
        end: usize,
        bytes: bool,
    },
    Mapped {
        view: usize,
    },
}

#[derive(Clone, Copy)]
enum JitDmaElement {
    Int { signed: bool, bits: u8 },
    Float32,
    Float64,
}

impl JitDmaElement {
    fn width(self) -> usize {
        match self {
            Self::Int { bits, .. } => usize::from(bits / 8),
            Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}

pub(crate) struct JitPinnedDma {
    source: i64,
    buffer_ty: MirType,
    element: JitDmaElement,
    len: usize,
    bytes: Box<[u8]>,
    completion_handle:
        Option<jet_foundation::ResourceSchedule::JetFrameCompletionHandle>,
}

fn dma_element_type(ty: &MirType) -> Option<JitDmaElement> {
    match &ty.kind {
        MirTypeKind::Int => Some(JitDmaElement::Int {
            signed: true,
            bits: 64,
        }),
        MirTypeKind::IntN { signed, bits } if *bits >= 8 && *bits % 8 == 0 => {
            Some(JitDmaElement::Int {
                signed: *signed,
                bits: *bits,
            })
        }
        MirTypeKind::Float32 => Some(JitDmaElement::Float32),
        MirTypeKind::Float => Some(JitDmaElement::Float64),
        MirTypeKind::InlineRange { base, .. } | MirTypeKind::Tagged { inner: base, .. } => {
            dma_element_type(base)
        }
        _ => None,
    }
}

fn dma_payload_shape(ty: &MirType) -> Result<(JitDmaElement, Option<usize>), String> {
    match &ty.kind {
        MirTypeKind::List(element) => dma_element_type(element)
            .map(|element| (element, None))
            .ok_or_else(|| format!("MIR DMA payload `{}` is not a checked POD list", ty.display_name())),
        MirTypeKind::FixedList { elem, len } => {
            let element = dma_element_type(elem).ok_or_else(|| {
                format!(
                    "MIR DMA payload `{}` is not a checked POD fixed list",
                    ty.display_name()
                )
            })?;
            let len = len
                .literal_value()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    format!(
                        "MIR DMA payload `{}` has no static fixed length",
                        ty.display_name()
                    )
                })?;
            Ok((element, Some(len)))
        }
        MirTypeKind::Tagged { inner, .. } => dma_payload_shape(inner),
        _ => Err(format!(
            "MIR DMA payload `{}` is not a contiguous owning POD carrier",
            ty.display_name()
        )),
    }
}
fn dma_integer_bits(
    runtime: &JitRuntime,
    value: i64,
    signed: bool,
    bits: u8,
) -> Result<u64, String> {
    if !(8..=64).contains(&bits) {
        return Err(format!(
            "MIR DMA buffer element has unsupported integer width {bits}"
        ));
    }
    let value = runtime
        .heap
        .int_to_i128(value)
        .ok_or_else(|| "MIR DMA integer payload has an invalid integer carrier".to_string())?;
    if signed {
        let bound = 1_i128 << (u32::from(bits) - 1);
        if value < -bound || value >= bound {
            return Err(format!(
                "MIR DMA buffer value does not fit signed {bits}-bit element"
            ));
        }
        Ok((value as i64) as u64)
    } else {
        let value = u64::try_from(value)
            .map_err(|_| format!("MIR DMA buffer value does not fit unsigned {bits}-bit element"))?;
        if bits < 64 && value >= (1_u64 << bits) {
            return Err(format!(
                "MIR DMA buffer value does not fit unsigned {bits}-bit element"
            ));
        }
        Ok(value)
    }
}


fn encode_dma_payload(
    runtime: &mut JitRuntime,
    source: i64,
    buffer_ty: MirType,
) -> Result<JitPinnedDma, String> {
    let (element, fixed_len) = dma_payload_shape(&buffer_ty)?;
    let actual_len = runtime
        .heap
        .list_len(source)
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| "MIR DMA payload is not an owned list carrier".to_string())?;
    if fixed_len.is_some_and(|expected| expected != actual_len) {
        return Err("MIR DMA fixed-list carrier has the wrong runtime length".to_string());
    }
    let bytes_len = actual_len
        .checked_mul(element.width())
        .ok_or_else(|| "MIR DMA payload byte length overflowed".to_string())?;
    let mut bytes = vec![0u8; bytes_len].into_boxed_slice();
    for index in 0..actual_len {
        let offset = index * element.width();
        let index_i64 = i64::try_from(index)
            .map_err(|_| "MIR DMA payload index exceeded the runtime range".to_string())?;
        match element {
            JitDmaElement::Int { signed, bits } => {
                let value = runtime
                    .heap
                    .list_get_int(source, index_i64)
                    .ok_or_else(|| "MIR DMA integer payload has a non-integer element".to_string())?;
                let raw = dma_integer_bits(runtime, value, signed, bits)?.to_le_bytes();
                bytes[offset..offset + element.width()].copy_from_slice(&raw[..element.width()]);
            }
            JitDmaElement::Float32 => {
                let value = runtime
                    .heap
                    .list_get_float(source, index_i64)
                    .ok_or_else(|| "MIR DMA float payload has a non-float element".to_string())?;
                bytes[offset..offset + 4].copy_from_slice(&(value as f32).to_le_bytes());
            }
            JitDmaElement::Float64 => {
                let value = runtime
                    .heap
                    .list_get_float(source, index_i64)
                    .ok_or_else(|| "MIR DMA float payload has a non-float element".to_string())?;
                bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(JitPinnedDma {
        source,
        buffer_ty,
        element,
        len: actual_len,
        bytes,
        completion_handle: None,
    })
}

fn decode_dma_payload(runtime: &mut JitRuntime, pinned: &JitPinnedDma) -> Result<(), String> {
    let actual_len = runtime
        .heap
        .list_len(pinned.source)
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| "MIR DMA source carrier disappeared before wait".to_string())?;
    if actual_len != pinned.len {
        return Err("MIR DMA source carrier changed length while in flight".to_string());
    }
    let _ = &pinned.buffer_ty;
    for index in 0..pinned.len {
        let offset = index * pinned.element.width();
        let index_i64 = i64::try_from(index)
            .map_err(|_| "MIR DMA payload index exceeded the runtime range".to_string())?;
        match pinned.element {
            JitDmaElement::Int { signed, bits } => {
                let mut raw = [0u8; 8];
                let width = pinned.element.width();
                raw[..width].copy_from_slice(&pinned.bytes[offset..offset + width]);
                let mut value = u64::from_le_bytes(raw);
                if signed && bits < 64 && (value & (1u64 << (bits - 1))) != 0 {
                    value |= !((1u64 << bits) - 1);
                }
                let value = if signed {
                    runtime.heap.int_from_i64(value as i64)
                } else {
                    runtime.heap.int_from_u64(value)
                };
                runtime
                    .heap
                    .list_set_int(pinned.source, index_i64, value)
                    .ok_or_else(|| "MIR DMA integer payload could not be restored".to_string())?;
            }
            JitDmaElement::Float32 => {
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&pinned.bytes[offset..offset + 4]);
                runtime
                    .heap
                    .list_set_float(pinned.source, index_i64, f32::from_le_bytes(raw) as f64)
                    .ok_or_else(|| "MIR DMA float payload could not be restored".to_string())?;
            }
            JitDmaElement::Float64 => {
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&pinned.bytes[offset..offset + 8]);
                runtime
                    .heap
                    .list_set_float(pinned.source, index_i64, f64::from_le_bytes(raw))
                    .ok_or_else(|| "MIR DMA float payload could not be restored".to_string())?;
            }
        }
    }
    Ok(())
}

const JIT_HANDLE_TAG_MASK: u64 = 0xf000_0000_0000_0000;
const JIT_MAPPED_FILE_TAG: u64 = 0x1000_0000_0000_0000;
const JIT_VIEW_TAG: u64 = 0x2000_0000_0000_0000;
const JIT_FILE_SCOPE_TAG: u64 = 0x3000_0000_0000_0000;

fn encode_tagged(tag: u64, index: usize) -> i64 {
    (tag | (index as u64).saturating_add(1)) as i64
}

fn decode_tagged(value: i64, tag: u64) -> Option<usize> {
    let raw = value as u64;
    if raw & JIT_HANDLE_TAG_MASK != tag {
        return None;
    }
    usize::try_from((raw & !JIT_HANDLE_TAG_MASK).checked_sub(1)?).ok()
}

pub(crate) fn mapped_file_handle(index: usize) -> i64 {
    encode_tagged(JIT_MAPPED_FILE_TAG, index)
}

pub(crate) fn file_scope_handle(index: usize) -> i64 {
    encode_tagged(JIT_FILE_SCOPE_TAG, index)
}

pub(crate) fn file_scope_index(rt: &JitRuntime, handle: i64) -> Option<usize> {
    let index = decode_tagged(handle, JIT_FILE_SCOPE_TAG)?;
    (index < rt.file_scopes.len()).then_some(index)
}

pub(crate) fn view_handle(index: usize) -> i64 {
    encode_tagged(JIT_VIEW_TAG, index)
}

pub(crate) fn mapped_file_index(
    rt: &JitRuntime,
    handle: i64,
) -> Option<usize> {
    let index = decode_tagged(handle, JIT_MAPPED_FILE_TAG)?;
    (index < rt.mapped_files.len()).then_some(index)
}

pub(crate) fn view_index(rt: &JitRuntime, handle: i64) -> Option<usize> {
    let index = decode_tagged(handle, JIT_VIEW_TAG)?;
    (index < rt.view_slots.len()).then_some(index)
}
const JIT_ITER_TAG: u64 = 0x4000_0000_0000_0000;

pub(crate) fn lazy_iter_handle(index: usize) -> i64 {
    encode_tagged(JIT_ITER_TAG, index)
}

pub(crate) fn lazy_iter_index(rt: &JitRuntime, handle: i64) -> Option<usize> {
    let index = decode_tagged(handle, JIT_ITER_TAG)?;
    (index < rt.lazy_iters.len() && rt.lazy_iters[index].is_some()).then_some(index)
}

pub(crate) fn lazy_iter_exact_len(rt: &JitRuntime, handle: i64) -> Option<usize> {
    let Some(index) = lazy_iter_index(rt, handle) else {
        return sequence_len(rt, handle);
    };
    match rt.lazy_iters.get(index)?.as_ref()? {
        JitLazyIter::Source { source, index } => {
            sequence_len(rt, *source)?.checked_sub(*index)
        }
        JitLazyIter::Map { source, .. } => lazy_iter_exact_len(rt, *source),
        JitLazyIter::Enumerate { source, .. } => lazy_iter_exact_len(rt, *source),

        JitLazyIter::Take { source, remaining } => {
            Some(lazy_iter_exact_len(rt, *source)?.min(*remaining))
        }
        JitLazyIter::Skip { source, remaining } => {
            Some(lazy_iter_exact_len(rt, *source)?.saturating_sub(*remaining))
        }
        JitLazyIter::Filter { .. }
        | JitLazyIter::FilterMap { .. }
        | JitLazyIter::FlatMap { .. }
        | JitLazyIter::TakeWhile { .. }
        | JitLazyIter::SkipWhile { .. }
        | JitLazyIter::Scan { .. }
        | JitLazyIter::Zip { .. } => None,
    }
}

fn lazy_source_valid(rt: &JitRuntime, source: i64) -> bool {
    lazy_iter_index(rt, source).is_some() || sequence_len(rt, source).is_some()
}

fn lazy_iter_push(rt: &mut JitRuntime, state: JitLazyIter) -> i64 {
    let index = rt.lazy_iters.len();
    rt.lazy_iters.push(Some(state));
    lazy_iter_handle(index)
}

pub(crate) fn lazy_iter_source(rt: &mut JitRuntime, source: i64) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(rt, JitLazyIter::Source { source, index: 0 })
}

pub(crate) fn lazy_iter_map(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator map source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(rt, JitLazyIter::Map { source, callback })
}
pub(crate) fn lazy_iter_enumerate(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator enumerate source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::Enumerate {
            source,
            index: 0,
            callback,
        },
    )
}


pub(crate) fn lazy_iter_filter_map(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator filter_map source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(rt, JitLazyIter::FilterMap { source, callback })
}

pub(crate) fn lazy_iter_take(rt: &mut JitRuntime, source: i64, count: i64) -> i64 {
    if let Some(message) = Collections::collection_semantics::sequence_argument_message("take", count) {
        rt.set_runtime_stop_at("E3001", "<core.collections>", 0, message);
        return 0;
    }
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator take source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::Take {
            source,
            remaining: count as usize,
        },
    )
}

pub(crate) fn lazy_iter_skip(rt: &mut JitRuntime, source: i64, count: i64) -> i64 {
    if let Some(message) = Collections::collection_semantics::sequence_argument_message("skip", count) {
        rt.set_runtime_stop_at("E3001", "<core.collections>", 0, message);
        return 0;
    }
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator skip source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::Skip {
            source,
            remaining: count as usize,
        },
    )
}

pub(crate) fn lazy_iter_filter(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator filter source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(rt, JitLazyIter::Filter { source, callback })
}

pub(crate) fn lazy_iter_flat_map(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator flat_map source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::FlatMap {
            source,
            callback,
            active: None,
        },
    )
}

pub(crate) fn lazy_iter_take_while(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator take_while source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::TakeWhile {
            source,
            callback,
            done: false,
        },
    )
}

pub(crate) fn lazy_iter_skip_while(
    rt: &mut JitRuntime,
    source: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator skip_while source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::SkipWhile {
            source,
            callback,
            skipping: true,
        },
    )
}

pub(crate) fn lazy_iter_scan(
    rt: &mut JitRuntime,
    source: i64,
    accumulator: i64,
    callback: JitCallableSlot,
) -> i64 {
    if !lazy_source_valid(rt, source) {
        rt.set_host_fault("lazy iterator scan source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::Scan {
            source,
            callback,
            accumulator,
        },
    )
}

pub(crate) fn lazy_iter_zip(
    rt: &mut JitRuntime,
    left: i64,
    right: i64,
    callback: JitCallableSlot,
    mode: JitZipMode,
    left_fill: Option<i64>,
    right_fill: Option<i64>,
) -> i64 {
    if !lazy_source_valid(rt, left) || !lazy_source_valid(rt, right) {
        rt.set_host_fault("lazy iterator zip source is not a sequence handle");
        return 0;
    }
    lazy_iter_push(
        rt,
        JitLazyIter::Zip {
            left,
            right,
            callback,
            mode,
            left_fill,
            right_fill,
        },
    )
}

pub(crate) fn lazy_iter_next(rt: &mut JitRuntime, handle: i64) -> Option<i64> {
    if runtime_stop_pending(rt) {
        return None;
    }
    let index = lazy_iter_index(rt, handle)?;
    let mut state = rt.lazy_iters.get_mut(index)?.take()?;
    let value = state.next_value(rt);
    rt.lazy_iters[index] = Some(state);
    if runtime_stop_pending(rt) {
        return None;
    }
    match value {
        Some(value) => {
            crate::IO::jet_jit_io_progress_pull_n(handle, 1);
            Some(value)
        }
        None => {
            crate::IO::progress_exhaust_state(handle);
            None
        }
    }
}

pub(crate) fn lazy_iter_collect(rt: &mut JitRuntime, handle: i64) -> Option<Vec<i64>> {
    lazy_iter_index(rt, handle)?;
    let mut values = Vec::new();
    while let Some(value) = lazy_iter_next(rt, handle) {
        values.push(value);
    }
    Some(values)
}

fn sequence_len_in_runtime(rt: &JitRuntime, source: i64) -> Option<usize> {
    if let Some(index) = view_index(rt, source) {
        return view_len_in_runtime(rt, index);
    }
    usize::try_from(rt.heap.list_len(source)?).ok()
}

fn view_len_in_runtime(rt: &JitRuntime, index: usize) -> Option<usize> {
    match rt.view_slots.get(index)? {
        JitViewSlot::Sequence { start, end, .. } => end.checked_sub(*start),
        JitViewSlot::String { start, end, .. } => end.checked_sub(*start),
        JitViewSlot::Mapped { view } => rt.mapped_views.get(*view).map(|value| value.len()),
    }
}

pub(crate) fn sequence_len(rt: &JitRuntime, source: i64) -> Option<usize> {
    sequence_len_in_runtime(rt, source)
}

pub(crate) fn sequence_get_int(
    rt: &JitRuntime,
    source: i64,
    index: usize,
) -> Option<i64> {
    if let Some(view_index) = view_index(rt, source) {
        let slot = rt.view_slots.get(view_index)?;
        return match slot {
            JitViewSlot::Sequence { source, start, end } => {
                let absolute = start.checked_add(index)?;
                (absolute < *end).then(|| sequence_get_int(rt, *source, absolute))?
            }
            JitViewSlot::String {
                owner,
                start,
                end,
                bytes,
            } => {
                if !*bytes {
                    return None;
                }
                let absolute = start.checked_add(index)?;
                if absolute >= *end {
                    return None;
                }
                rt.heap.get_string(*owner)?.as_bytes().get(absolute).copied().map(i64::from)
            }
            JitViewSlot::Mapped { view } => rt
                .mapped_views
                .get(*view)?
                .as_bytes()
                .get(index)
                .copied()
                .map(i64::from),
        };
    }
    rt.heap.list_get_int(source, i64::try_from(index).ok()?)
}

pub(crate) fn sequence_get_float(
    rt: &JitRuntime,
    source: i64,
    index: usize,
) -> Option<f64> {
    if let Some(view_index) = view_index(rt, source) {
        let slot = rt.view_slots.get(view_index)?;
        return match slot {
            JitViewSlot::Sequence { source, start, end } => {
                let absolute = start.checked_add(index)?;
                (absolute < *end).then(|| sequence_get_float(rt, *source, absolute))?
            }
            JitViewSlot::String { .. } | JitViewSlot::Mapped { .. } => None,
        };
    }
    rt.heap.list_get_float(source, i64::try_from(index).ok()?)
}

pub(crate) fn sequence_get_string(
    rt: &mut JitRuntime,
    source: i64,
    index: usize,
) -> Option<i64> {
    if let Some(view_index) = view_index(rt, source) {
        let slot = rt.view_slots.get(view_index)?;
        return match slot {
            JitViewSlot::Sequence { source, start, end } => {
                let absolute = start.checked_add(index)?;
                if absolute >= *end {
                    return None;
                }
                sequence_get_string(rt, *source, absolute)
            }
            JitViewSlot::String {
                owner,
                start,
                end,
                bytes,
            } => {
                if *bytes {
                    return None;
                }
                let absolute_start = start.checked_add(index)?;
                if absolute_start >= *end {
                    return None;
                }
                rt.heap
                    .alloc_string_view(*owner, absolute_start, *end)
            }
            JitViewSlot::Mapped { .. } => None,
        };
    }
    rt.heap
        .list_get_int(source, i64::try_from(index).ok()?)
}
/// Read one sequence element in the universal callback carrier. Integer-like
/// values and handles stay unchanged; f64/f32 values are passed as IEEE bits.
pub(crate) fn sequence_get_raw(
    rt: &mut JitRuntime,
    source: i64,
    index: usize,
) -> Option<i64> {
    if let Some(value) = sequence_get_int(rt, source, index) {
        return Some(value);
    }
    if let Some(value) = sequence_get_float(rt, source, index) {
        return Some(value.to_bits() as i64);
    }
    sequence_get_string(rt, source, index)
}


pub(crate) fn view_string(rt: &JitRuntime, handle: i64) -> Option<String> {
    let index = view_index(rt, handle)?;
    match rt.view_slots.get(index)? {
        JitViewSlot::String {
            owner,
            start,
            end,
            bytes: false,
        } => rt.heap.get_string(*owner)?.get(*start..*end).map(str::to_owned),
        _ => None,
    }
}

pub(crate) fn view_bytes(rt: &JitRuntime, handle: i64) -> Option<Vec<u8>> {
    let index = view_index(rt, handle)?;
    match rt.view_slots.get(index)? {
        JitViewSlot::String {
            owner,
            start,
            end,
            bytes: true,
        } => rt
            .heap
            .get_string(*owner)?
            .as_bytes()
            .get(*start..*end)
            .map(|value| value.to_vec()),
        JitViewSlot::Mapped { view } => rt.mapped_views.get(*view).map(|value| value.to_vec()),
        _ => None,
    }
}
struct JitHardwareReplayHost {
    profile_id: String,
    replay: SharedHardwareReplayHost<jet_foundation::TargetMachine::TargetHardwareFacts>,
    strings: HashMap<i64, String>,
}


fn hardware_ownership_name(value: i64) -> Option<&'static str> {
    match value {
        0 => Some(hardware_bridge::JET_HARDWARE_DMA_BORROWED_NAME),
        1 => Some(hardware_bridge::JET_HARDWARE_DMA_TRANSFER_NAME),
        _ => None,
    }
}
impl JitHardwareReplayHost {
    fn new(
        profile_id: String,
        facts: jet_foundation::TargetMachine::TargetHardwareFacts,
    ) -> Self {
        Self {
            profile_id,
            replay: SharedHardwareReplayHost::new(facts),
            strings: HashMap::new(),
        }
    }

    fn metadata(&self, handle: i64) -> Option<&str> {
        self.strings.get(&handle).map(String::as_str)
    }

    fn profile_matches(&self, profile_handle: i64) -> bool {
        self.metadata(profile_handle) == Some(self.profile_id.as_str())
    }

    /// The string table and the replay engine are disjoint fields; borrowing
    /// them apart lets a call read its operand names while it drives the
    /// engine mutably.
    fn parts(
        &mut self,
    ) -> (
        &HashMap<i64, String>,
        &mut SharedHardwareReplayHost<jet_foundation::TargetMachine::TargetHardwareFacts>,
    ) {
        (&self.strings, &mut self.replay)
    }
}

impl JetHardwareErasedHost for JitHardwareReplayHost {
    fn setup(
        &mut self,
        profile_id: i64,
        setup_kind: i64,
        item: i64,
        width_or_vector: i64,
        ownership_or_handler: i64,
    ) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let (strings, replay) = self.parts();
        let metadata = |handle: i64| strings.get(&handle).map(String::as_str);
        let Some(profile) = metadata(profile_id) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(item) = metadata(item) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let (kind, operand) = match setup_kind {
            JET_HARDWARE_SETUP_DMA_CONFIGURE => (
                hardware_bridge::JET_HARDWARE_SETUP_DMA_CONFIGURE_NAME,
                hardware_ownership_name(ownership_or_handler),
            ),
            JET_HARDWARE_SETUP_INTERRUPT_BIND => (
                hardware_bridge::JET_HARDWARE_SETUP_INTERRUPT_BIND_NAME,
                metadata(ownership_or_handler),
            ),
            _ => return JET_HARDWARE_UNAVAILABLE,
        };
        let Some(operand) = operand else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        JetHardwareHost::setup(replay, profile, kind, item, width_or_vector, operand)
    }

    fn register_read(&mut self, profile_id: i64, block: i64, register: i64, width: i64) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let (strings, replay) = self.parts();
        let metadata = |handle: i64| strings.get(&handle).map(String::as_str);
        let Some(profile) = metadata(profile_id) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(block) = metadata(block) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(register) = metadata(register) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        JetHardwareHost::register_read(replay, profile, block, register, width)
    }

    fn register_write(
        &mut self,
        profile_id: i64,
        block: i64,
        register: i64,
        width: i64,
        value: i64,
    ) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let (strings, replay) = self.parts();
        let metadata = |handle: i64| strings.get(&handle).map(String::as_str);
        let Some(profile) = metadata(profile_id) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(block) = metadata(block) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(register) = metadata(register) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        JetHardwareHost::register_write(replay, profile, block, register, width, value)
    }

    fn dma_start(
        &mut self,
        profile_id: i64,
        channel: i64,
        address: i64,
        bytes: i64,
    ) -> i64 {
        if !self.profile_matches(profile_id) || address < 0 || bytes < 0 {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let (strings, replay) = self.parts();
        let metadata = |handle: i64| strings.get(&handle).map(String::as_str);
        let Some(profile) = metadata(profile_id) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(channel) = metadata(channel) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        JetHardwareHost::dma_start(replay, profile, channel, address as u64, bytes as u64)
    }

    fn dma_wait(&mut self, profile_id: i64, channel: i64, token: i64) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let (strings, replay) = self.parts();
        let metadata = |handle: i64| strings.get(&handle).map(String::as_str);
        let Some(profile) = metadata(profile_id) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(channel) = metadata(channel) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        JetHardwareHost::dma_wait(replay, profile, channel, token)
    }

    fn register_string_handle(&mut self, handle: i64, value: &str) {
        self.strings.insert(handle, value.to_string());
    }

    fn trigger_interrupt(&mut self, vector: u16) -> i64 {
        JetHardwareHost::trigger_interrupt(&mut self.replay, vector)
    }
}

pub(crate) struct JitRuntime {
    /// The canonical policy for this invocation/resident image. It is carried
    /// by the runtime instead of caller-thread TLS so game and web Prelude
    /// hooks observe the same profile across native and HTTP worker entries.
    pub(crate) release_devtools_policy: ReleaseDevtoolsPolicy,
    /// Stable heap-owned atomic cells. FFI handles are pointers into this
    /// resident-owned vector; cells remain live for the runtime lifetime.
    pub(crate) atomics: Vec<Box<crate::Ffi::FfiAtomicCell>>,
    pub(crate) source_file: String,
    pub(crate) source_text: String,
    pub(crate) current_function: String,
    pub(crate) current_line: u32,
    pub(crate) current_source_line: String,
    pub(crate) source_frames: Vec<JitSourceFrame>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) heap: jet_rt::JetArena,
    /// Canonical typed DataLoader lifecycle slots. Handles are one-based
    /// indices into this resident-owned vector; each slot owns the Foundation
    /// LoaderState and the checked row type key.
    pub(crate) data_loaders: Vec<crate::Data::DataLoaderSlot>,
    /// Checked model output bindings projected by the loader.  The JIT keeps
    /// these neutral facts and opens through the runtime-owned model provider.
    pub(crate) model_outputs: Vec<jet_foundation::AST::ModelOutputFact>,
    /// Resident model sessions. Handles are one-based indices into this
    /// vector and are returned as the checked trait-object carrier.
    pub(crate) model_sessions: Vec<jet_rt::model::provider::OnnxRuntimeSession>,
    /// Stable backing buffers for resident pure integer-list loops. A list
    /// already using `JetVal::IntList` is borrowed directly; erased list
    /// carriers are copied here once before native lowering reads them.
    pub(crate) int_list_views: Vec<Box<[i64]>>,
    /// Native read-only mappings. Handles are tagged one-based indices into
    /// this resident-owned vector; views keep their own Arc lease.
    pub(crate) mapped_files: Vec<crate::CoreHost::os_rt::jet_std::JetMappedFile>,
    /// Resource-qualified no-follow filesystem scopes. Handles are tagged
    /// one-based indices into this resident-owned vector.
    pub(crate) file_scopes: Vec<crate::Collections::authority_semantics::JetFileScope>,
    /// Read-only byte windows. Each carrier owns an Arc lease independently
    /// of the originating mapped-file handle.
    pub(crate) mapped_views: Vec<crate::CoreHost::os_rt::jet_std::JetMappedByteView>,
    /// Generic read-only views over lists and text. Slots retain provenance
    /// and ranges; source storage remains owned by the heap or mapped_views.
    pub(crate) view_slots: Vec<JitViewSlot>,
    /// Deferred iterator pipelines. Handles are tagged one-based indices;
    /// slots own cursor state and callback thunks until materialization.
    pub(crate) lazy_iters: Vec<Option<JitLazyIter>>,
    pub(crate) type_descriptors: HashMap<u64, RuntimeTypeDescriptor>,
    pub(crate) type_descriptor_names: HashMap<String, u64>,
    /// Concrete nominal type identity for each heap record promoted to a
    /// trait object. The Cranelift call site uses this side table to select
    /// the checked impl method without changing record field layout.
    pub(crate) trait_object_types: HashMap<i64, MirTypeId>,
    /// Exact checked payload types used by the JIT DMA marshalling boundary.
    pub(crate) dma_types: HashMap<String, MirType>,
    /// Owned contiguous carriers retained by transfer token until wait.
    pub(crate) dma_transfers: HashMap<i64, JitPinnedDma>,
    /// Canonical source-wire iterable hook table for this resident module.
    pub(crate) iterable_hooks: HashMap<String, JitIterableHook>,
    /// Shared Prelude hardware adapter installed for this resident module.
    /// The JIT only supplies erased metadata handles; the adapter owns the
    /// selected target's register, interrupt, and DMA behavior.
    pub(crate) hardware_host: Option<Box<dyn JetHardwareErasedHost>>,
    /// Checked package setup rows retained until the resident module is torn
    /// down. They are installed once before the first entry invocation.
    pub(crate) hardware_setups: Vec<MirHardwareSetup>,
    /// Finalized zero-argument handler pointers keyed by checked vector facts.
    pub(crate) hardware_handlers: JetHardwareInterruptRegistry,
    /// Typed static pattern descriptors referenced by MIR Prelude calls.
    /// Handles are one-based indices into this arena.
    pub(crate) pattern_descriptors: Vec<JitPatternDescriptor>,
    pub(crate) program_allocator:
        std::sync::Arc<jet_codegen::program_allocator::JetProgramAllocator>,
    pub(crate) compute: Compute::ComputeState,
    /// Compile-time string handles baked into Cranelift as `iconst` ids.
    /// `reset_run_heap` and the run-cache artifact must preserve these — clearing
    /// them leaves warm `jet run` hits with empty panic/require text (I9).
    pub(crate) compile_strings: Vec<(usize, String)>,
    /// Source labels for JIT task spawn sites. This is resident metadata, not
    /// a process-global registry: generated code passes the existing site
    /// number and the host resolves the label on the active runtime.
    pub(crate) task_labels: Vec<Option<String>>,
    /// Compile-time zip row schemas referenced by resident Cranelift code. The
    /// host receives only handles at run time; this table supplies the checked
    pub(crate) zip_plans: Vec<JitZipPlan>,
    pub(crate) invocations: u64,
    /// Semantic live-edit plan captured before the next resident reload.
    ///
    /// The IDs remain opaque: MIR owns the mapping from semantic callable
    /// identity to compiled symbol and the corresponding redefinition step.
    pub(crate) hot_swap_plan: Option<ResidentHotSwapPlan>,
    /// D-FIELDMEMO1=A: cached computed-field words keyed by record and the
    /// stable getter slot. The host only stores raw ABI words; the TIR/JIT
    /// lowering owns type packing and the Prelude owns cache policy elsewhere.
    pub(crate) memo_values: std::collections::HashMap<(i64, i64), i64>,
    /// D-MEMO1=A: per-function memo stores keyed by the Jet function name,
    /// the resident mirror of the one `static Mutex<JetMemo>` AOT emits per
    /// memoized function. `f.cache()` reads its stats here; the store itself
    /// is the shared Prelude carrier, so cache policy is never re-decided.
    pub(crate) memo_functions:
        std::collections::HashMap<String, jet_codegen::memo::JetMemo<Vec<i64>, i64>>,
    pub(crate) channels: Vec<JetSchedulerChannel<i64>>,
    /// Canonical MIR loop cursors. Handles are one-based and each slot owns
    /// exactly one typed source state until the loop completes or is reset.
    pub(crate) loop_cursors: Vec<Option<crate::Collections::JetLoopCursorState>>,
    pub(crate) senders: Vec<Option<JetSchedulerSender<i64>>>,
    /// Intermediate typed views stay in the shared Prelude until the next
    /// operator consumes them; only completed windows cross the scalar stream
    /// handle ABI.
    /// Generator stream receivers retained by negative channel handles.
    pub(crate) stream_consumers: HashMap<i64, JetStream<i64>>,
    /// Generator stream producers retained by negative channel handles.
    pub(crate) stream_producers:
        HashMap<i64, std::sync::Arc<JetStreamSender<i64>>>,
    /// Stream senders retained by negative sender handles.
    pub(crate) stream_senders:
        HashMap<i64, std::sync::Arc<JetStreamSender<i64>>>,
    pub(crate) stream_event_consumers:
        std::collections::HashMap<i64, JetStream<JetStreamEvent<i64>>>,
    pub(crate) stream_keyed_consumers:
        std::collections::HashMap<i64, JetKeyedStream<i64, i64>>,
    pub(crate) stream_windows:
        std::collections::HashMap<i64, JetStreamWindow<i64, i64>>,
    pub(crate) next_stream_channel: i64,
    pub(crate) next_stream_sender: i64,
    /// Unique names for JIT-local OptionLift2 factory/adapter functions.
    /// These functions are only ABI thunks; the operation they serve lives in
    /// the shared Option Prelude.
    pub(crate) next_option_lift2_thunk: u64,
    /// Unique names for deferred Shared transaction lambda callbacks.
    pub(crate) next_shared_txn_thunk: u64,
    /// Runtime function values. Every function value is a negative handle
    /// minted by `bind_jit_callable`; a raw Cranelift address is not a callable
    /// and is refused at the call boundary (`jet_jit_callable_normalize`).
    pub(crate) jit_callables: Vec<JitCallableSlot>,
    /// Process-edge callbacks. The resident adapter invokes these after all
    /// generated scope cleanup and before it returns the run outcome.
    pub(crate) atexit_handlers: Vec<JitCallableSlot>,
    pub(crate) tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    pub(crate) task_controls: Vec<std::sync::Arc<JetTaskControl>>,
    pub(crate) task_groups: Vec<Option<super::Concurrency::JitTaskGroup>>,
    /// D-LOCALCELL1=A: one-thread canonical Cell values and guards.
    pub(crate) cells: LocalCell::CellState,
    /// General `Result<T, E>` ABI arena. Handles are one-based indices; payload
    /// bits are interpreted from checked TIR types, never dynamically guessed.
    pub(crate) results: Vec<JitResultValue>,
    /// D-FAIL-ERROR1=A: Prelude-owned default error values. JIT code sees only
    /// one-based handles and marshals fields through the helpers below.
    pub(crate) errors: Vec<jet_foundation::Outcome::JetErr>,
    pub(crate) solvers: Vec<Solver::SolverState>,
    pub(crate) rngs: Vec<crate::Random::RngState>,
    /// Borrowed Foundation RNG pointers installed only while an explicit
    /// history generator callback is synchronously executing. Handles are
    /// positive one-based indices and never enter a case or artifact.
    pub(crate) history_rngs: Vec<usize>,
    /// Selected checked program/artifact provenance used by history evidence.
    pub(crate) history_provenance:
        Option<jet_foundation::TestingHistory::HistoryProvenance>,
    /// Stable function-target metadata used to decode live closure captures.
    /// Keys are only an internal lookup from finalized code pointers; pointers
    /// themselves never enter the provenance digest.
    pub(crate) history_callable_targets: HashMap<i64, JitHistoryCallableTarget>,
    pub(crate) fakes: Vec<crate::Random::FakeState>,
    /// Canonical Prelude clocks, addressed by 1-based resident handles.
    pub(crate) clocks: Vec<clock_rt::jet_std::Clock>,
    /// `ProcessSpec` handles — 1-based indices (#729 process builder).
    pub(crate) process_specs: Vec<Process::JitProcessSpec>,
    /// `ProcessChild` handles — 1-based indices (#729 process spawn).
    pub(crate) process_children: Vec<Process::JitProcessChild>,
    /// `core.data.sketch.*` handles (#729).
    pub(crate) sketches: Vec<crate::Sketch::SketchSlot>,
    /// `core.args` ArgsSpec / ParsedArgs handles (#729).
    pub(crate) args_specs: Vec<crate::Args::ArgsSpec>,
    pub(crate) args_parsed: Vec<crate::Args::ParsedArgs>,
    /// Encoding stream file / codec handles (#729 encoding_*_stream).
    pub(crate) file_readers: Vec<crate::enc_stream::FileReaderSlot>,
    pub(crate) file_writers: Vec<crate::enc_stream::FileWriterSlot>,
    pub(crate) json_readers: Vec<crate::enc_stream::JSONReaderSlot>,
    pub(crate) json_writers: Vec<crate::enc_stream::JSONWriterSlot>,
    pub(crate) jsonl_readers: Vec<crate::enc_stream::JsonlReaderSlot>,
    pub(crate) jsonl_writers: Vec<crate::enc_stream::JsonlWriterSlot>,
    pub(crate) csv_readers: Vec<crate::enc_stream::CSVReaderSlot>,
    pub(crate) csv_writers: Vec<crate::enc_stream::CSVWriterSlot>,
    pub(crate) xml_readers: Vec<crate::enc_stream::XmlReaderSlot>,
    pub(crate) xml_writers: Vec<crate::enc_stream::XmlWriterSlot>,
    pub(crate) cbor_readers: Vec<crate::enc_stream::CBORReaderSlot>,
    pub(crate) cbor_writers: Vec<crate::enc_stream::CBORWriterSlot>,
    /// Typed `core.data` pull streams (`csv_reader` → Event rows).
    pub(crate) data_streams: Vec<crate::Data::DataStreamSlot>,
    /// Typed `core.data.plot` carriers. Handles are one-based indices into this runtime-owned vector.
    pub(crate) data_plots: Vec<crate::Data::DataPlotSlot>,
    /// `core.jobs` queue carriers. Handles are one-based indices into slots
    /// owning the canonical queue and its native connection.
    pub(crate) job_queues: Vec<Option<crate::DB::JitJobQueueSlot>>,
    /// `Set<T>` handles — 1-based indices (#729 collections/set), with the
    /// parallel kind tag preserving String equality at the host boundary.
    pub(crate) sets: Vec<std::collections::HashSet<i64>>,
    /// Parallel element-kind tags: `true` means String, `false` means Int.
    pub(crate) set_string_kinds: Vec<bool>,
    /// `Queue<T>` handles — 1-based indices (#729 collections/queue). Int elems only.
    pub(crate) deques: Vec<std::collections::VecDeque<i64>>,
    /// `Tally<T>` handles — counted JIT-value bits, keyed by the checked element ABI.
    pub(crate) bags: Vec<std::collections::HashMap<i64, usize>>,
    pub(crate) sorted_sets: Vec<std::collections::BTreeSet<i64>>,
    pub(crate) sorted_set_string_kinds: Vec<bool>,
    pub(crate) priority_queues: Vec<std::collections::BinaryHeap<i64>>,
    pub(crate) lrus: Vec<Collections::LruState>,
    pub(crate) bit_sets: Vec<std::collections::BTreeSet<i64>>,
    pub(crate) byte_buffers: Vec<Collections::byte_buffer_semantics::JetByteBuffer>,
    pub(crate) allocators: Vec<Memory::AllocatorState>,
    pub(crate) allocator_views: Vec<Memory::AllocatorView>,
    pub(crate) gc_roots: Vec<jet_rt::__gc::AutomaticRoot<i64>>,
    pub(crate) gc_edges: Vec<Vec<jet_rt::__gc::ObjectId>>,
    pub(crate) pools: Vec<std::sync::Arc<std::sync::Mutex<Memory::PoolState>>>,
    pub(crate) shareds: Vec<std::sync::Arc<Memory::SharedState>>,
    pub(crate) conditions: Vec<std::sync::Arc<Memory::ConditionState>>,
    pub(crate) shared_guard_states: std::collections::HashMap<
        i64,
        std::sync::Arc<Memory::shared_protocol::JetSharedGuardState>,
    >,
    pub(crate) expirings: Vec<Memory::ExpiringState>,
    pub(crate) secrets: Vec<Option<Memory::SecretState>>,
    pub(crate) crypto_values: Vec<Option<Crypto::CryptoValue>>,
    /// `core.net.url` / `core.net.mime` / net handles (#1221).
    pub(crate) net_values: Vec<Option<Net::NetValue>>,
    /// Checked service worker function-value handles, keyed by source identity.
    pub(crate) service_callbacks: std::collections::HashMap<String, i64>,
    /// Finalized #Job entrypoints and checked payload/result carriers.
    pub(crate) job_adapters: std::collections::HashMap<String, JitJobAdapter>,
    /// `core.service` / `core.sync` opaque Prelude values keyed by their handle.
    pub(crate) service_values: Vec<Option<MirRuntimeValue>>,
    /// `core.game` scene / frame / replay / backend handles (#1218).
    pub(crate) game_scenes: Vec<crate::Game::GameSceneState>,
    pub(crate) game_frames: Vec<crate::Game::GameFrameState>,
    pub(crate) game_replays: Vec<crate::Game::GameReplayState>,
    pub(crate) game_backends: Vec<crate::Game::GameBackendState>,
    /// `core.game.raylib` window / color / atlas handles (#2850).
    pub(crate) raylib_windows: Vec<crate::Raylib::RaylibWindowState>,
    pub(crate) raylib_colors: Vec<crate::Raylib::RaylibColorState>,
    pub(crate) raylib_sounds: Vec<crate::Raylib::RaylibSoundState>,
    pub(crate) raylib_atlases: Vec<crate::Raylib::RaylibTextureAtlasState>,
    pub(crate) raylib_draw_calls: Vec<crate::Raylib::JetRaylibSpriteDrawCall>,
    pub(crate) time_values: Vec<Option<Time::TimeValue>>,
    /// Fixed-rate realtime streams keyed by 1-based handles.
    pub(crate) realtime_values: Vec<Option<Time::time_rt::JetRealtimeStream>>,
    /// Regex / Match handles for core.regex (#1219).
    pub(crate) regex_values: Vec<Option<Text::RegexValue>>,
    /// Decimal handles for D-DECIMAL1 (#1219) — side table of CtDecimal.
    pub(crate) decimal_values: Vec<Option<jet_foundation::Numeric::CtDecimal>>,
    /// Fraction handles for D-NUMTYPE1 (#1464) — side table of CtFraction.
    pub(crate) fraction_values: Vec<Option<jet_foundation::Numeric::CtFraction>>,
    /// Complex handles for D-TYPE2-IMAG1=A — values use the exact MathLibPure
    /// type extracted into the resident JIT module.
    pub(crate) complex_values: Vec<Option<crate::MathExtra::math_rt::JetComplex>>,
    /// Set by every trap payload write and cleared only by `take_trap`.
    ///
    /// The payload is written to `trapped` first, then this flag is published
    /// with `Release`; `jet_jit_is_trapped` reads it with `Acquire`. This is
    /// deliberately separate from the payload so hot trap polls do not take
    /// `E0953` diagnostic, exactly as the tier-0 interpreter reports the same
    /// panic. Keeps the FIRST message; later traps on the unwind path are noise.
    /// Host faults use this owned slot for their captured cause; `host_fault`
    /// selects ICE handling before ordinary runtime-stop handling.
    /// Publication bit for the trap payload stored in `trapped`.
    pub(crate) trapped_flag: AtomicBool,
    pub(crate) trapped: Option<String>,
    /// A Rust helper fault is a Jet defect. The resident driver renders the
    /// branded ICE report after generated control flow reaches the boundary.
    pub(crate) host_fault: bool,
    /// True when the host-fault path captured an owned helper message in
    /// `trapped`; false when a boundary supplied only an opaque trap token.
    pub(crate) host_fault_payload_captured: bool,
    /// Soft process exit for rich `require`/`panic` reports — stderr already
    /// holds the AOT-matching text; resident returns `Ran` with this code.
    pub(crate) exit_code: Option<i32>,

    /// Compiler-owned E3003 rendered after native code returns; never unwinds
    /// through a Cranelift frame.
    pub(crate) deadline_exceeded: Option<String>,
    pub(crate) readers: Vec<crate::Parse::ReaderSlot>,
    pub(crate) cursors: Vec<crate::Parse::CursorSlot>,
    pub(crate) reflect_values: Vec<ReflectSlot>,
    /// D-LAYOUT1: layout handles / LinExpr / Constraint slots (#1225).
    pub(crate) layout_slots: Vec<crate::Layout::LayoutSlot>,
    /// D-REACT1 / D-EVENT1: reactive + event opaque handles (#1225).
    pub(crate) reactive: crate::Reactive::ReactiveState,
    /// D-RENDERTGT*: UI backends / nodes / events (#1225).
    pub(crate) ui: crate::Ui::UiState,
    /// D-WEBAPP1 / c-devserver: web app + DevServer handles (#1226).
    pub(crate) web: crate::Web::WebState,
}

#[derive(Clone)]
pub(crate) struct JitSourceFrame {
    file: String,
    line: u32,
    fn_name: String,
    source_line: String,
}

/// Control transfer for a shared Prelude stop whose Rust signature is `!`.
/// The report is recorded first; the resident caller remains the owner of
/// cleanup and the final target exit.
#[derive(Debug)]
pub(crate) struct JitRuntimeStop;

pub(crate) fn runtime_stop_unwind(code: &'static str, line: u32, message: &str) -> ! {
    with_runtime_mut(|rt| rt.set_runtime_stop(code, line, message));
    std::panic::resume_unwind(Box::new(JitRuntimeStop));
}

pub(crate) fn runtime_stop_unwind_at(
    code: &'static str,
    file: &str,
    line: u32,
    message: &str,
) -> ! {
    with_runtime_mut(|rt| rt.set_runtime_stop_at(code, file, line, message));
    std::panic::resume_unwind(Box::new(JitRuntimeStop));
}
fn merge_runtime_optional<T: Clone + PartialEq>(
    current: &mut Option<T>,
    incoming: Option<T>,
) -> bool {
    match (current.as_ref(), incoming) {
        (Some(current), Some(incoming)) if current != &incoming => false,
        (Some(_), Some(_)) | (Some(_), None) => true,
        (None, Some(incoming)) => {
            *current = Some(incoming);
            true
        }
        (None, None) => true,
    }
}

fn merge_runtime_rows<T: PartialEq>(current: &mut Vec<T>, incoming: Vec<T>) -> bool {
    if incoming.is_empty() {
        true
    } else if current.is_empty() {
        *current = incoming;
        true
    } else {
        *current == incoming
    }
}

fn merge_runtime_type_descriptor(
    current: &mut RuntimeTypeDescriptor,
    incoming: RuntimeTypeDescriptor,
) -> bool {
    let kind_compatible = current.kind == incoming.kind
        || (current.kind == RuntimeValueKind::Named
            && matches!(incoming.kind, RuntimeValueKind::Record | RuntimeValueKind::Enum))
        || (incoming.kind == RuntimeValueKind::Named
            && matches!(current.kind, RuntimeValueKind::Record | RuntimeValueKind::Enum));
    if current.name != incoming.name
        || current.canonical != incoming.canonical
        || !kind_compatible
        || current.abi != incoming.abi
    {
        return false;
    }
    if current.kind == RuntimeValueKind::Named {
        current.kind = incoming.kind;
    }
    current.serde_untagged |= incoming.serde_untagged;
    current.serde_deny_unknown |= incoming.serde_deny_unknown;
    merge_runtime_optional(&mut current.integer_width, incoming.integer_width)
        && merge_runtime_optional(&mut current.integer_range, incoming.integer_range)
        && merge_runtime_optional(&mut current.element, incoming.element)
        && merge_runtime_optional(&mut current.key, incoming.key)
        && merge_runtime_optional(&mut current.value, incoming.value)
        && merge_runtime_optional(&mut current.ok, incoming.ok)
        && merge_runtime_optional(&mut current.err, incoming.err)
        && merge_runtime_optional(&mut current.serde_tag, incoming.serde_tag)
        && merge_runtime_optional(&mut current.cli, incoming.cli)
        && merge_runtime_rows(&mut current.fields, incoming.fields)
        && merge_runtime_rows(&mut current.variants, incoming.variants)
}


fn hardware_ownership_tag(
    ownership: jet_foundation::TargetMachine::TargetDmaOwnership,
) -> i64 {
    match ownership {
        jet_foundation::TargetMachine::TargetDmaOwnership::Borrowed => 0,
        jet_foundation::TargetMachine::TargetDmaOwnership::Transfer => 1,
    }
}
impl JitRuntime {
    pub(crate) fn clock_new_manual(&mut self, seed: i64) -> i64 {
        self.clocks.push(clock_rt::jet_std_clock_new(seed));
        self.clocks.len() as i64
    }

    pub(crate) fn clock_new_system(&mut self) -> i64 {
        self.clocks.push(clock_rt::jet_std_clock_system());
        self.clocks.len() as i64
    }

    pub(crate) fn clock_now(&mut self, handle: i64) -> i64 {
        let Some(clock) = self.clocks.get((handle as usize).wrapping_sub(1)) else {
            self.set_host_fault("Clock.now received an invalid resident handle");
            return 0;
        };
        clock_rt::jet_clock_now(clock)
    }

    pub(crate) fn clock_tick(&mut self, handle: i64, delta_ms: i64) -> i64 {
        let Some(clock) = self.clocks.get_mut((handle as usize).wrapping_sub(1)) else {
            self.set_host_fault("Clock.tick received an invalid resident handle");
            return 0;
        };
        clock_rt::jet_clock_tick(clock, delta_ms)
    }
    pub(crate) fn clock_clone(&mut self, handle: i64) -> i64 {
        let Some(clock) = self.clocks.get((handle as usize).wrapping_sub(1)) else {
            self.set_host_fault("Clock copy received an invalid resident handle");
            return 0;
        };
        let clock = clock.clone();
        self.clocks.push(clock);
        self.clocks.len() as i64
    }


    pub(crate) fn clock_advance(&mut self, handle: i64, to_ms: i64) -> i64 {
        let Some(clock) = self.clocks.get_mut((handle as usize).wrapping_sub(1)) else {
            self.set_host_fault("Clock.advance received an invalid resident handle");
            return 0;
        };
        clock_rt::jet_clock_advance(clock, to_ms)
    }

    pub(crate) fn clock_wait(&mut self, handle: i64, duration_ms: i64) -> i64 {
        self.clock_tick(handle, duration_ms)
    }

    /// Whether game development data is present for this invocation.
    ///
    /// Game diagnostics are a release-code decision, not a mutable host
    /// toggle. Read the canonical policy carried by this resident runtime.
    pub(crate) fn game_debug_data_enabled(&self) -> bool {
        !self.release_devtools_policy.is_release()
    }

    /// Canonical Web Prelude projection for streaming code/devtools behavior.
    pub(crate) fn web_runtime_devtools_enabled(&self) -> bool {
        !self.release_devtools_policy.is_release()
            || self.release_devtools_policy.stream_code
    }

    /// Canonical Web Prelude projection for local history capture.
    pub(crate) fn web_runtime_history_enabled(&self) -> bool {
        self.release_devtools_policy.local_rail
    }
    /// Install the shared Prelude adapter used by this resident module.
    /// Adapters provide marshalling only; hardware semantics remain in the
    /// shared embedded-hardware kernel behind this trait.
    pub(crate) fn install_hardware_host(&mut self, host: Box<dyn JetHardwareErasedHost>) {
        self.hardware_host = Some(host);
    }

    pub(crate) fn clear_hardware_host(&mut self) {
        self.hardware_host = None;
    }

    /// Construct the resident replay adapter from the canonical target profile
    /// registry. No target name or triple is interpreted as hardware identity.
    pub(crate) fn install_canonical_hardware_host(
        &mut self,
        profile_id: &str,
        facts: Option<jet_foundation::TargetMachine::TargetHardwareFacts>,
    ) -> Result<(), String> {
        hardware_bridge::jet_hardware_clear_pending();
        self.clear_hardware_interrupt_handlers();
        if profile_id.is_empty() {
            self.clear_hardware_host();
            return Ok(());
        }
        let Some(facts) = facts else {
            return Err(format!(
                "no checked hardware profile facts for `{profile_id}`"
            ));
        };
        let host = JitHardwareReplayHost::new(profile_id.to_string(), facts);
        self.install_hardware_host(Box::new(host));
        Ok(())
    }

    /// Register one finalized, checked zero-argument handler for a target
    /// vector. The callback pointer is never exposed as a user value.
    pub(crate) fn register_hardware_interrupt_handler(
        &mut self,
        vector: u16,
        fn_ptr: i64,
    ) -> Result<(), String> {
        if fn_ptr == 0 {
            return Err(format!("hardware interrupt vector {vector} has no handler"));
        }
        let handler = JetHardwareInterruptHandler {
            fn_ptr,
            env: 0,
            has_env: false,
        };
        if !self.hardware_handlers.register(vector, handler) {
            return Err(format!(
                "hardware interrupt vector {vector} has duplicate handlers"
            ));
        }
        Ok(())
    }

    pub(crate) fn clear_hardware_interrupt_handlers(&mut self) {
        self.hardware_handlers.clear();
    }

    /// Clone the checked pointer registry before the owner thread drains and
    /// invokes Foundation's queue outside the runtime borrow.
    pub(crate) fn hardware_interrupt_registry(&self) -> JetHardwareInterruptRegistry {
        self.hardware_handlers.clone()
    }

    /// Queue one deterministic canonical replay event. Physical adapters may
    /// publish the same event through `JetHardwareErasedHost`.
    pub(crate) fn trigger_hardware_interrupt(&mut self, vector: i64) -> i64 {
        let Ok(vector) = u16::try_from(vector) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        self.hardware_host
            .as_mut()
            .map(|host| host.trigger_interrupt(vector))
            .unwrap_or(JET_HARDWARE_UNAVAILABLE)
    }

    /// Install one checked package setup before entry invocation. String
    /// metadata becomes the same compile-string handle ABI used by calls.
    pub(crate) fn install_hardware_setup(
        &mut self,
        setup: &MirHardwareSetup,
    ) -> Result<(), String> {
        let (
            profile_id,
            setup_kind,
            item,
            width_or_vector,
            ownership_or_handler,
        ) = match setup {
            MirHardwareSetup::DmaConfigure {
                profile_id,
                channel,
                transfer_width,
                ownership,
            } => (
                profile_id,
                JET_HARDWARE_SETUP_DMA_CONFIGURE,
                channel,
                transfer_width.bytes() as i64,
                hardware_ownership_tag(*ownership),
            ),
            MirHardwareSetup::InterruptBind {
                profile_id,
                interrupt,
                vector,
                handler_symbol,
                ..
            } => (
                profile_id,
                JET_HARDWARE_SETUP_INTERRUPT_BIND,
                interrupt,
                i64::from(*vector),
                self.heap.alloc_string(handler_symbol.clone()),
            ),
        };
        let profile_handle = self.heap.alloc_string(profile_id.clone());
        let item_handle = self.heap.alloc_string(item.clone());
        let handler_text = if setup_kind == JET_HARDWARE_SETUP_INTERRUPT_BIND {
            self.heap.clone_string(ownership_or_handler)
        } else {
            None
        };
        let status = self
            .hardware_host
            .as_mut()
            .map(|host| {
                host.register_string_handle(profile_handle, profile_id);
                host.register_string_handle(item_handle, item);
                if let Some(handler) = handler_text.as_deref() {
                    host.register_string_handle(ownership_or_handler, handler);
                }
                hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
                    hardware_bridge::jet_hardware_setup(
                        profile_handle,
                        setup_kind,
                        item_handle,
                        width_or_vector,
                        ownership_or_handler,
                    )
                })
            })
            .unwrap_or(JET_HARDWARE_UNAVAILABLE);
        if status < 0 {
            return Err(format!(
                "JIT hardware setup unavailable for target profile `{profile_id}`"
            ));
        }
        self.hardware_setups.push(setup.clone());
        Ok(())
    }

    /// Install all checked package setup rows in source order before entry.
    pub(crate) fn install_hardware_setups(
        &mut self,
        setups: &[MirHardwareSetup],
    ) -> Result<(), String> {
        self.hardware_setups.clear();
        for setup in setups {
            self.install_hardware_setup(setup)?;
        }
        Ok(())
    }

    /// Snapshot string handles allocated during lowering (baked into code).
    pub(crate) fn snapshot_compile_strings(&mut self) {
        self.compile_strings = self.heap.string_slots();
        if let Some(host) = self.hardware_host.as_mut() {
            for (handle, value) in &self.compile_strings {
                host.register_string_handle(*handle as i64, value);
            }
        }
    }
    /// Install checked type facts used by universal Value ABI hosts.
    /// Re-installing a descriptor may only add missing shape rows. A nominal
    /// placeholder can be enriched once to a Record or Enum; divergent facts
    /// remain an internal compiler/runtime defect.
    pub(crate) fn install_type_descriptors(
        &mut self,
        descriptors: impl IntoIterator<Item = RuntimeTypeDescriptor>,
    ) {
        for descriptor in descriptors {
            let id = descriptor.id;
            if let Some(existing) = self.type_descriptors.get_mut(&id) {
                if !merge_runtime_type_descriptor(existing, descriptor.clone()) {
                    jet_foundation::ice!(
                        None,
                        "conflicting runtime type descriptor for identity {}",
                        id
                    );
                }
                self.type_descriptor_names
                    .insert(descriptor.name.clone(), id);
                self.type_descriptor_names
                    .insert(descriptor.canonical.clone(), id);
                continue;
            }
            self.type_descriptor_names
                .insert(descriptor.name.clone(), id);
            self.type_descriptor_names
                .insert(descriptor.canonical.clone(), id);
            self.type_descriptors.insert(id, descriptor);
        }
    }

    /// Retain the exact checked type needed to restore a completed DMA carrier.
    pub(crate) fn register_dma_type(&mut self, buffer_ty: &MirType) {
        let key = buffer_ty.identity_key();
        if let Some(existing) = self.dma_types.get(&key) {
            if existing.canonical_key() != buffer_ty.canonical_key() {
                jet_foundation::ice!(
                    None,
                    "conflicting checked DMA carrier type for `{key}`"
                );
            }
            return;
        }
        self.dma_types.insert(key, buffer_ty.clone());
    }

    pub(crate) fn runtime_type_descriptor(
        &self,
        id: u64,
    ) -> Option<&RuntimeTypeDescriptor> {
        self.type_descriptors.get(&id)
    }

    pub(crate) fn runtime_type_descriptor_by_name(
        &self,
        name: &str,
    ) -> Option<&RuntimeTypeDescriptor> {
        let id = self.type_descriptor_names.get(name)?;
        self.type_descriptors.get(id)
    }

    /// Drop the previous program's source-wire hook identities before a fresh
    /// resident artifact installs its finalized thunk pointers.
    pub(crate) fn clear_iterable_hooks(&mut self) {
        self.iterable_hooks.clear();
    }
    pub(crate) fn clear_lazy_iters(&mut self) {
        self.lazy_iters.clear();
    }

    /// Install one checked Iterable implementation for this resident.
    ///
    /// The complete source wire is the identity. Repeating the exact row is
    /// idempotent; replacing any pointer or checked type fact under that key is
    /// an internal compiler conflict.
    pub(crate) fn register_iterable_hook(
        &mut self,
        source_wire: &str,
        iter_slot: i64,
        next_slot: i64,
        coll_type: &str,
        iter_type: &str,
    ) -> Result<(), String> {
        if source_wire.is_empty()
            || iter_slot == 0
            || next_slot == 0
            || coll_type.is_empty()
            || iter_type.is_empty()
        {
            return Err("invalid empty or null MIR iterable hook registration".to_string());
        }
        let parsed = MirLoopSourceKind::from_wire(source_wire)
            .ok_or_else(|| format!("invalid MIR iterable source wire `{source_wire}`"))?;
        let MirLoopSourceKind::Iterable {
            coll_type: wire_coll_type,
            iter_type: wire_iter_type,
            ..
        } = parsed
        else {
            return Err(format!(
                "MIR iterable hook registration used non-Iterable source wire `{source_wire}`"
            ));
        };
        if wire_coll_type != coll_type || wire_iter_type != iter_type {
            return Err(format!(
                "MIR iterable hook types disagree with source wire `{source_wire}`"
            ));
        }
        let incoming = JitIterableHook {
            iter_slot,
            next_slot,
            coll_type: coll_type.to_owned(),
            iter_type: iter_type.to_owned(),
        };
        if let Some(existing) = self.iterable_hooks.get(source_wire) {
            if existing == &incoming {
                return Ok(());
            }
            jet_foundation::ice!(
                None,
                "conflicting JIT iterable hook registration for source wire `{}`",
                source_wire
            );
        }
        self.iterable_hooks.insert(source_wire.to_owned(), incoming);
        Ok(())
    }

    pub(crate) fn iterable_hook(&self, source_wire: &str) -> Option<&JitIterableHook> {
        self.iterable_hooks.get(source_wire)
    }
    /// Register a typed MIR text pattern and return its one-based arena handle.
    pub(crate) fn register_text_pattern(&mut self, parts: &[MirTextPatternPart]) -> i64 {
        self.pattern_descriptors
            .push(JitPatternDescriptor::Text(parts.to_vec()));
        self.pattern_descriptors.len() as i64
    }

    /// Register a typed MIR binary pattern and return its one-based arena handle.
    pub(crate) fn register_binary_pattern(&mut self, parts: &[MirBinaryPatternPart]) -> i64 {
        self.pattern_descriptors
            .push(JitPatternDescriptor::Binary(parts.to_vec()));
        self.pattern_descriptors.len() as i64
    }

    /// Store a trap payload, then publish the lock-free poll flag.
    ///
    /// `trapped_flag` is a publication bit, not an independent source of
    /// truth: callers take the payload through [`Self::take_trap`], which is
    /// the only operation that clears the bit.
    pub(crate) fn set_trap_message(&mut self, msg: String) {
        if self.trapped.is_none() {
            self.trapped = Some(msg);
            self.trapped_flag.store(true, Ordering::Release);
        }
    }

    /// Take the published trap payload and clear its poll bit as one boundary
    /// operation. The bit is never cleared before the payload is taken.
    pub(crate) fn take_trap(&mut self) -> Option<String> {
        let payload = self.trapped.take();
        if payload.is_some() {
            self.trapped_flag.store(false, Ordering::Release);
        }
        payload
    }

    /// Read the hot-path trap bit without reacquiring `RUNTIME_ACCESS`.
    pub(crate) fn trap_pending(&self) -> bool {
        self.trapped_flag.load(Ordering::Acquire)
    }

    /// Record a runtime panic. Keeps the first message (the unwind branch may
    /// re-enter trap sites with dummy values before the epilogue is reached).
    fn store_trap(&mut self, msg: &str) {
        if Concurrency::in_scheduler_task() {
            Concurrency::set_task_trap(msg);
            return;
        }
        self.set_trap_message(msg.to_string());
    }

    /// Legacy host failures still enter the one runtime-stop renderer. The
    /// caller supplies no source facts for an engine failure, so the report
    /// keeps the generic E3001 location shape.
    pub(crate) fn set_trap(&mut self, msg: &str) {
        if let Some(reason) = jet_codegen::scheduler::jet_scheduler_host_fault_reason(msg) {
            self.set_host_fault(reason);
        } else {
            self.set_runtime_stop("E3001", 0, msg);
        }
    }

    /// A foreign bridge panic is a program-side runtime stop. Render it with
    /// the active Jet frame instead of the generic host E3001 fallback.
    pub(crate) fn set_ffi_runtime_stop(&mut self, msg: &str) {
        let line = self.current_line.max(1);
        if Concurrency::in_scheduler_task() {
            let source_line = self
                .source_text
                .lines()
                .nth((line as usize).saturating_sub(1))
                .filter(|source| !source.is_empty())
                .unwrap_or(self.current_source_line.as_str());
            self.set_child_runtime_stop(
                "E3014",
                &self.source_file,
                line,
                &self.current_function,
                source_line,
                msg,
            );
            return;
        }
        self.set_runtime_stop_with_source_line("E3014", line, None, msg);
    }

    /// Carry a rendered child failure through the scheduler result without
    /// mutating the resident process outcome. The task shares this runtime
    /// with its parent, so writing `stderr` or `exit_code` here would make a
    /// handled child failure stop an unrelated parent operation.
    fn set_child_runtime_stop(
        &self,
        code: &'static str,
        file: &str,
        line: u32,
        fn_name: &str,
        source_line: &str,
        message: &str,
    ) {
        if Concurrency::local_rich_panic_pending() || Concurrency::task_trap_pending() {
            return;
        }
        let report = contract_kernel::jet_runtime_stop_report(
            code,
            file,
            line,
            fn_name,
            source_line,
            1,
            1,
            message,
            "",
        );
        // The child result owns this failure. Keep the full shared-Prelude
        // report available to stream/task adapters, but leave resident output
        // and exit status untouched.
        Concurrency::set_rich_panic_reason(message.to_string());
        Concurrency::set_rich_panic_report(report.rendered);
        Concurrency::set_local_rich_panic();
    }

    /// A Rust helper panic is an engine fault, not a user runtime stop. Keep
    /// the ICE status and trap boundary, while carrying the owned message to
    /// the resident ICE renderer as data.
    pub(crate) fn set_host_fault(&mut self, msg: impl AsRef<str>) {
        let msg = msg.as_ref();
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        let msg = jet_codegen::scheduler::jet_scheduler_host_fault_reason(msg).unwrap_or(msg);
        if Concurrency::in_scheduler_task() {
            Concurrency::set_task_trap(msg);
        }
        self.host_fault = true;
        self.host_fault_payload_captured = true;
        self.exit_code = Some(jet_foundation::ExitCodes::ICE);
        self.set_trap_message(msg.to_string());
    }

    pub(crate) fn set_deadline(&mut self, rendered: String) {
        if self.deadline_exceeded.is_none() {
            self.deadline_exceeded = Some(rendered);
        }
    }

    /// Marshal a runtime breach into the Foundation Prelude renderer. JIT
    /// hosts provide only source facts and keep no user-facing wording.
    pub(crate) fn set_runtime_stop(&mut self, code: &'static str, line: u32, message: &str) {
        self.set_runtime_stop_with_source_line(code, line, None, message);
    }

    pub(crate) fn set_runtime_stop_at(
        &mut self,
        code: &'static str,
        file: &str,
        line: u32,
        message: &str,
    ) {
        if Concurrency::in_scheduler_task() {
            self.set_child_runtime_stop(code, file, line, "", "", message);
            return;
        }
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        let report =
            contract_kernel::jet_runtime_stop_report(code, file, line, "", "", 1, 1, message, "");
        let _ = jet_codegen::development_receipt::jet_production_failure_receipt_write(
            code, file, line, "",
        );
        self.stderr.push_str(&report.rendered);
        self.exit_code = Some(report.exit_code);
        self.store_trap(message);
    }

    /// Same renderer, with a source line captured at the TIR stop site. A host
    /// can otherwise fall back to the enclosing function's prologue line when
    /// its run-level source text is unavailable (the Pool stale-id path is the
    /// concrete case).
    pub(crate) fn set_runtime_stop_with_source_line(
        &mut self,
        code: &'static str,
        line: u32,
        source_override: Option<&str>,
        message: &str,
    ) {
        if Concurrency::in_scheduler_task() {
            let source_line = source_override
                .filter(|source| !source.is_empty())
                .or_else(|| {
                    self.source_text
                        .lines()
                        .nth((line as usize).saturating_sub(1))
                        .filter(|source| !source.is_empty())
                })
                .unwrap_or(self.current_source_line.as_str());
            self.set_child_runtime_stop(
                code,
                &self.source_file,
                line,
                &self.current_function,
                source_line,
                message,
            );
            return;
        }
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        let source_line = source_override
            .filter(|source| !source.is_empty())
            .or_else(|| {
                self.source_text
                    .lines()
                    .nth((line as usize).saturating_sub(1))
                    .filter(|source| !source.is_empty())
            })
            .unwrap_or(self.current_source_line.as_str());
        let report = contract_kernel::jet_runtime_stop_report(
            code,
            &self.source_file,
            line,
            &self.current_function,
            source_line,
            1,
            1,
            message,
            "",
        );
        let _ = jet_codegen::development_receipt::jet_production_failure_receipt_write(
            code,
            &self.source_file,
            line,
            &self.current_function,
        );
        self.stderr.push_str(&report.rendered);
        self.exit_code = Some(report.exit_code);
        self.store_trap(message);
    }

    pub(crate) fn set_arithmetic_stop(&mut self, line: u32, message: &str) {
        self.set_runtime_stop(contract_kernel::JET_ARITHMETIC_CODE, line, message);
    }

    pub(crate) fn set_rendered_runtime_stop(&mut self, rendered: String, exit_code: i32) {
        if Concurrency::in_scheduler_task() {
            if Concurrency::local_rich_panic_pending() || Concurrency::task_trap_pending() {
                return;
            }
            let reason = rendered
                .lines()
                .next()
                .and_then(|line| line.split_once("]: ").map(|(_, message)| message))
                .unwrap_or_else(|| rendered.lines().next().unwrap_or("runtime failure"));
            // A report already rendered by the shared Prelude is the child
            // result. Preserve it for stream/task adapters, but never copy it
            // into the resident process stderr or exit status.
            Concurrency::set_rich_panic_reason(reason.to_string());
            Concurrency::set_rich_panic_report(rendered);
            Concurrency::set_local_rich_panic();
            return;
        }
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        self.stderr.push_str(&rendered);
        self.exit_code = Some(exit_code);
        self.set_trap_message("__jet_rich_panic__".to_string());
    }

    /// An explicit `process.exit(code)`: the resident twin of AOT's
    /// `JetExplicitExit`. Both fields are written here, in this order, for the
    /// same reason `set_rendered_runtime_stop` above does: `exit_code` alone is
    /// invisible to generated code, which leaves a run only when
    /// `jet_jit_is_trapped` answers 1 at an `emit_trap_check`. Setting
    /// `exit_code` and then calling `set_trap` cannot work — that path returns
    /// early once `exit_code` is set, so `trapped` stayed empty, every check
    /// passed, and the program ran ON past its own exit (`print` after
    /// `process.exit(0)` still printed, and a signal handler's exit never ended
    /// the process while the program sat in a `loop`).
    ///
    /// This is also the resident half of the rule
    /// `Prelude/CoreLib/Top/Interrupt.rs::jet_interrupt_dispatch` states: a tier
    /// MUST END whatever control transfer its handler raises. The handler runs on
    /// the interrupt dispatcher thread, which shares this runtime, so recording
    /// the trap here is what ends the program that is still running.
    pub(crate) fn set_explicit_exit(&mut self, code: i32) {
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        self.exit_code = Some(code);
        self.set_trap_message("__jet_process_exit__".to_string());
    }

    pub(crate) fn stack_enter(&mut self, file: &str, line: u32, fn_name: &str, src_line: &str) {
        self.source_frames.push(JitSourceFrame {
            file: self.source_file.clone(),
            line: self.current_line,
            fn_name: self.current_function.clone(),
            source_line: self.current_source_line.clone(),
        });
        self.source_file = file.to_string();
        self.current_line = line;
        self.current_function = fn_name.to_string();
        self.current_source_line = src_line.to_string();
        if jet_codegen::runtime_stack::jet_runtime_stack_enter() {
            let message = jet_foundation::Outcome::jet_stack_overflow_message(fn_name);
            self.set_runtime_stop_with_source_line("E3012", line, Some(src_line), &message);
        }
    }

    pub(crate) fn stack_leave(&mut self) {
        jet_codegen::runtime_stack::jet_runtime_stack_leave();
        if let Some(frame) = self.source_frames.pop() {
            self.source_file = frame.file;
            self.current_line = frame.line;
            self.current_function = frame.fn_name;
            self.current_source_line = frame.source_line;
        } else {
            self.current_line = 0;
            self.current_function.clear();
            self.current_source_line.clear();
        }
    }
}

/// Resident-side metadata for one semantically accepted live edit.
///
/// Function identities come from sema and stay opaque here. MIR is the owner
/// of resolving them to compiled symbols and preparing those symbols for
/// redefinition.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResidentHotSwapPlan {
    pub(crate) module: String,
    pub(crate) changed_functions: Vec<String>,
    pub(crate) preserved_state_keys: Vec<String>,
    pub(crate) fresh_state_keys: Vec<String>,
    pub(crate) rechecked_items: Vec<String>,
}

pub(crate) struct ResidentModule {
    pub(crate) module: JITModule,
    pub(crate) host: HostFns,
    pub(crate) main_id: FuncId,
    pub(crate) main_returns_result: bool,
    pub(crate) main_returns_app: bool,
    pub(crate) main_returns_default_err: bool,
    pub(crate) main_error_type: Option<String>,
    pub(crate) main_error_is_packed: bool,
}

fn with_runtime_mut<F: FnOnce(&mut JitRuntime)>(f: F) {
    Concurrency::with_runtime_mut(f);
}

/// Route resident output to the process's stream when the program owns it — a
/// terminal, or a one-shot `jet run`/`jet dev` that hands its streams over.
/// Otherwise keep it in `JitRuntime` so an embedder that reads the run's output
/// back from `RunOutcome` gets one ordered buffer. This is an engine adapter;
/// the ownership fact and the terminal framing stay in `Prelude/Term.rs`.
pub(crate) fn write_jit_stdout(text: &str, flush: bool) -> Result<(), String> {
    let direct = crate::IO::term_prelude::jet_term_stdout_is_program_stream()
        || Concurrency::active_runtime_ptr().is_none();
    if direct {
        crate::IO::term_prelude::jet_term_write_stdout(text, flush)
            .map_err(|error| format!("write stdout: {error}"))?;
    } else {
        with_runtime_mut(|rt| rt.stdout.push_str(text));
    }
    Ok(())
}

fn write_jit_stdout_line(text: &str, flush: bool) -> Result<(), String> {
    let frame = crate::IO::term_prelude::jet_term_print_frame(text);
    write_jit_stdout(&frame, flush)
}

pub(crate) fn write_jit_stderr(text: &str, flush: bool) -> Result<(), String> {
    let direct = crate::IO::term_prelude::jet_term_stderr_is_program_stream()
        || Concurrency::active_runtime_ptr().is_none();
    if direct {
        crate::IO::term_prelude::jet_term_write_stderr(text, flush)
            .map_err(|error| format!("write stderr: {error}"))?;
    } else {
        with_runtime_mut(|rt| rt.stderr.push_str(text));
    }
    Ok(())
}

/// `eprint(x)`'s line framing. AOT emits the call as
/// `jet_term_write_stderr_line(&((x).jet_show()), false)`; that Prelude entry is
/// `jet_term_print_frame` + `jet_term_write_stderr`, which is exactly what the
/// stdout twin above marshals. Keep the framing in the one Prelude kernel and
/// only pick the stream here (I9).
fn write_jit_stderr_line(text: &str, flush: bool) -> Result<(), String> {
    let frame = crate::IO::term_prelude::jet_term_print_frame(text);
    write_jit_stderr(&frame, flush)
}

fn with_runtime_trap<F: FnOnce(&mut JitRuntime)>(f: F) {
    Concurrency::with_runtime_mut(|rt| {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| f(rt))) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&'static str>()
                        .map(|message| (*message).to_string())
                })
                .unwrap_or_else(|| "the JIT runtime helper panicked".to_string());
            rt.set_host_fault(&message);
        }
    });
}

fn with_runtime_result<R: Default, F: FnOnce(&mut JitRuntime) -> R>(default: R, f: F) -> R {
    Concurrency::with_runtime_mut(|rt| match catch_unwind(AssertUnwindSafe(|| f(rt))) {
        Ok(value) => value,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&'static str>()
                        .map(|message| (*message).to_string())
                })
                .unwrap_or_else(|| "the JIT runtime helper panicked".to_string());
            rt.set_host_fault(&message);
            default
        }
    })
}

/// Record an arithmetic overflow/div-by-zero trap. Returns normally (the
/// caller yields a dummy `0`); JIT code branches to its epilogue at the next
/// `emit_trap_check`. Message text is unchanged from the old exit-70 path.
fn jet_trap_overflow(op: &str, line: u32) {
    let msg = contract_kernel::jet_arithmetic_message(op);
    with_runtime_mut(|rt| rt.set_arithmetic_stop(line, msg));
}

// JIT ABI aliases. Values come from the shared Prelude table; lowering carries
// them across the Cranelift boundary but does not define arithmetic meaning.
pub(crate) const INTN_OP_ADD: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_ADD;
pub(crate) const INTN_OP_SUB: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_SUB;
pub(crate) const INTN_OP_MUL: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_MUL;
pub(crate) const INTN_OP_DIV: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_DIV;
pub(crate) const INTN_OP_REM: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_REM;
pub(crate) const INTN_OP_BIT_AND: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_BIT_AND;
pub(crate) const INTN_OP_BIT_OR: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_BIT_OR;
pub(crate) const INTN_OP_BIT_XOR: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_BIT_XOR;
pub(crate) const INTN_OP_SHL: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_SHL;
pub(crate) const INTN_OP_SHR: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_SHR;
pub(crate) const INTN_OP_POW: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_POW;
pub(crate) const INTN_OP_FLOOR_DIV: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_FLOOR_DIV;
pub(crate) const INTN_OP_MOD: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_MOD;
pub(crate) const INTN_OP_ROTATE_LEFT: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_ROTATE_LEFT;
pub(crate) const INTN_OP_ROTATE_RIGHT: i64 = fixed_arithmetic_kernel::JET_FIXED_OP_ROTATE_RIGHT;
pub(crate) const INTN_MODE_TRAP: i64 = fixed_arithmetic_kernel::JET_FIXED_MODE_TRAP;
pub(crate) const INTN_MODE_WRAPPING: i64 = fixed_arithmetic_kernel::JET_FIXED_MODE_WRAPPING;
pub(crate) const INTN_MODE_SATURATING: i64 = fixed_arithmetic_kernel::JET_FIXED_MODE_SATURATING;
pub(crate) const INTN_MODE_CHECKED: i64 = fixed_arithmetic_kernel::JET_FIXED_MODE_CHECKED;

pub(crate) fn runtime_stop_pending(rt: &JitRuntime) -> bool {
    rt.trap_pending()
        || Concurrency::local_rich_panic_pending()
        || (Concurrency::in_scheduler_task() && Concurrency::task_trap_pending())
        || Concurrency::jet_jit_pending_exit_status() != 0
}

/// Reads the native stop state. `1` branches to the epilogue; `0` keeps going.
fn jet_jit_is_trapped() -> i64 {
    if Concurrency::jet_jit_pending_exit_status() != 0 || Concurrency::local_rich_panic_pending() {
        1
    } else if Concurrency::in_scheduler_task() {
        i64::from(Concurrency::task_trap_pending())
    } else {
        Concurrency::active_runtime_ptr()
            .and_then(|ptr| {
                // SAFETY: resident_invoke publishes this pointer only while
                // the runtime is live. `trapped_flag` is atomic, so this poll
                // does not race the guarded payload mutation.
                unsafe { ptr.as_ref().map(|rt| rt.trap_pending()) }
            })
            .map_or(0, i64::from)
    }
}

fn jet_jit_stack_enter(file: i64, line: i64, fn_name: i64, src_line: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let src_line = rt.heap.clone_string(src_line).unwrap_or_default();
        rt.stack_enter(&file, line.max(0) as u32, &fn_name, &src_line);
        i64::from(rt.trapped.is_some())
    })
}

fn jet_jit_stack_leave() {
    Concurrency::with_runtime_mut(JitRuntime::stack_leave);
}

fn persist_key(rt: &JitRuntime, handle: i64) -> String {
    rt.heap.clone_string(handle).unwrap_or_default()
}

fn jet_jit_persist_read_i64(key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        match jet_foundation::Persist::shared_read_runtime_key(&key) {
            Some(MirRuntimeValue::Int(value)) => rt.heap.int_from_i64(value),
            _ => {
                rt.set_host_fault("persistent Int slot was missing or had the wrong shape");
                0
            }
        }
    })
}

fn jet_jit_persist_write_i64(key: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        let Some(value) = rt.heap.int_to_i64(value) else {
            rt.set_host_fault("persistent Int slot received an invalid value");
            return;
        };
        if !jet_foundation::Persist::shared_write_runtime_key(&key, &MirRuntimeValue::Int(value)) {
            rt.set_host_fault("persistent Int slot was missing");
        }
    });
}

fn jet_jit_persist_read_f64(key: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        match jet_foundation::Persist::shared_read_runtime_key(&key) {
            Some(MirRuntimeValue::Float { value, .. }) => value,
            _ => {
                rt.set_host_fault("persistent Float slot was missing or had the wrong shape");
                0.0
            }
        }
    })
}

fn jet_jit_persist_write_f64(key: i64, value: f64) {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        if !jet_foundation::Persist::shared_write_runtime_key(
            &key,
            &MirRuntimeValue::Float {
                value,
                f32: false,
            },
        ) {
            rt.set_host_fault("persistent Float slot was missing");
        }
    });
}

fn jet_jit_persist_read_bool(key: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        match jet_foundation::Persist::shared_read_runtime_key(&key) {
            Some(MirRuntimeValue::Bool(value)) => i8::from(value),
            _ => {
                rt.set_host_fault("persistent Bool slot was missing or had the wrong shape");
                0
            }
        }
    })
}

fn jet_jit_persist_write_bool(key: i64, value: i8) {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        if !jet_foundation::Persist::shared_write_runtime_key(
            &key,
            &MirRuntimeValue::Bool(value != 0),
        ) {
            rt.set_host_fault("persistent Bool slot was missing");
        }
    });
}

fn jet_jit_persist_read_char(key: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        match jet_foundation::Persist::shared_read_runtime_key(&key) {
            Some(MirRuntimeValue::Char(value)) => value as i32,
            _ => {
                rt.set_host_fault("persistent Char slot was missing or had the wrong shape");
                0
            }
        }
    })
}

fn jet_jit_persist_write_char(key: i64, value: i32) {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        let Some(value) = char::from_u32(value as u32) else {
            rt.set_host_fault("persistent Char slot received an invalid value");
            return;
        };
        if !jet_foundation::Persist::shared_write_runtime_key(
            &key,
            &MirRuntimeValue::Char(value),
        ) {
            rt.set_host_fault("persistent Char slot was missing");
        }
    });
}

fn jet_jit_persist_read_str(key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        match jet_foundation::Persist::shared_read_runtime_key(&key) {
            Some(MirRuntimeValue::String(value)) => rt.heap.alloc_string(value),
            _ => {
                rt.set_host_fault("persistent String slot was missing or had the wrong shape");
                0
            }
        }
    })
}

fn jet_jit_persist_write_str(key: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let key = persist_key(rt, key);
        let Some(value) = rt.heap.clone_string(value) else {
            rt.set_host_fault("persistent String slot received an invalid value");
            return;
        };
        if !jet_foundation::Persist::shared_write_runtime_key(
            &key,
            &MirRuntimeValue::String(value),
        ) {
            rt.set_host_fault("persistent String slot was missing");
        }
    });
}
const PERSIST_MAX_DEPTH: usize = 128;
const PERSIST_MAX_NODES: usize = 100_000;

#[derive(Default)]
struct PersistDecodeState {
    active: HashSet<(u64, i64)>,
    nodes: usize,
    skip_computed: bool,
}

impl PersistDecodeState {
    fn for_debug() -> Self {
        Self {
            skip_computed: true,
            ..Self::default()
        }
    }

    fn enter(
        &mut self,
        descriptor: &RuntimeTypeDescriptor,
        raw: i64,
        depth: usize,
    ) -> Result<bool, String> {
        if depth > PERSIST_MAX_DEPTH {
            return Err("persistent value exceeds the descriptor depth limit".to_string());
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| "persistent value exceeds the node limit".to_string())?;
        if self.nodes > PERSIST_MAX_NODES {
            return Err("persistent value exceeds the node limit".to_string());
        }
        let tracked = descriptor.abi == RuntimeValueAbi::Handle
            && !matches!(persist_effective_kind(descriptor), RuntimeValueKind::String);
        if tracked && !self.active.insert((descriptor.id, raw)) {
            return Err("persistent value contains a cyclic heap carrier".to_string());
        }
        Ok(tracked)
    }

    fn leave(&mut self, descriptor: &RuntimeTypeDescriptor, raw: i64, tracked: bool) {
        if tracked {
            self.active.remove(&(descriptor.id, raw));
        }
    }
}

#[derive(Default)]
struct PersistEncodeState {
    nodes: usize,
}

impl PersistEncodeState {
    fn enter(&mut self, depth: usize) -> Result<(), String> {
        if depth > PERSIST_MAX_DEPTH {
            return Err("persistent value exceeds the descriptor depth limit".to_string());
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| "persistent value exceeds the node limit".to_string())?;
        if self.nodes > PERSIST_MAX_NODES {
            return Err("persistent value exceeds the node limit".to_string());
        }
        Ok(())
    }
}

fn persist_builtin_descriptor(type_name: &str) -> Option<RuntimeTypeDescriptor> {
    let (kind, abi) = if type_name == "Int"
        || (type_name.len() > 1
            && matches!(type_name.as_bytes()[0], b'I' | b'U')
            && type_name[1..].parse::<u8>().is_ok())
    {
        (RuntimeValueKind::Int, RuntimeValueAbi::Int)
    } else if type_name == "Float" {
        (RuntimeValueKind::Float, RuntimeValueAbi::Float)
    } else if matches!(type_name, "F32" | "Float32") {
        (RuntimeValueKind::Float, RuntimeValueAbi::Float32)
    } else if type_name == "Bool" {
        (RuntimeValueKind::Bool, RuntimeValueAbi::Bool)
    } else if type_name == "Char" {
        (RuntimeValueKind::Char, RuntimeValueAbi::Char)
    } else if type_name == "String" {
        (RuntimeValueKind::String, RuntimeValueAbi::Handle)
    } else if matches!(type_name, "Unit" | "()") {
        (RuntimeValueKind::Unit, RuntimeValueAbi::Unit)
    } else {
        return None;
    };
    Some(RuntimeTypeDescriptor {
        id: 0,
        name: type_name.to_string(),
        canonical: type_name.to_string(),
        kind,
        abi,
        integer_width: None,
        integer_range: None,
        element: None,
        key: None,
        value: None,
        ok: None,
        err: None,
        serde_tag: None,
        serde_untagged: false,
        serde_deny_unknown: false,
        cli: None,
        fields: Vec::new(),
        variants: Vec::new(),
    })
}

fn persist_effective_kind(descriptor: &RuntimeTypeDescriptor) -> RuntimeValueKind {
    if descriptor.kind == RuntimeValueKind::Handle
        && descriptor.ok.is_some()
        && descriptor.err.is_some()
    {
        RuntimeValueKind::Result
    } else if matches!(descriptor.kind, RuntimeValueKind::Named | RuntimeValueKind::Handle)
        && !descriptor.fields.is_empty()
    {
        RuntimeValueKind::Record
    } else if matches!(descriptor.kind, RuntimeValueKind::Named | RuntimeValueKind::Handle)
        && !descriptor.variants.is_empty()
    {
        RuntimeValueKind::Enum
    } else {
        descriptor.kind
    }
}

fn runtime_report_is_clean(rt: &JitRuntime, type_id: Option<u64>) -> bool {
    let Some(descriptor) = type_id.and_then(|type_id| rt.runtime_type_descriptor(type_id)) else {
        return false;
    };
    let name = runtime_source_name(&descriptor.name);
    name == "JetAbsent"
        || descriptor.canonical.ends_with("::JetAbsent")
        || descriptor.canonical.ends_with(".JetAbsent")
}

fn persist_descriptor(
    rt: &JitRuntime,
    type_name: &str,
) -> Option<RuntimeTypeDescriptor> {
    if let Some(id) = type_name
        .strip_prefix("id:")
        .and_then(|id| id.parse::<u64>().ok())
    {
        if let Some(descriptor) = rt.runtime_type_descriptor(id) {
            return Some(descriptor.clone());
        }
    }
    rt.runtime_type_descriptor_by_name(type_name)
        .cloned()
        .or_else(|| persist_builtin_descriptor(type_name))
}

fn persist_child_descriptor(
    rt: &JitRuntime,
    id: Option<u64>,
    parent: &RuntimeTypeDescriptor,
    label: &str,
) -> Result<RuntimeTypeDescriptor, String> {
    id.and_then(|id| rt.runtime_type_descriptor(id).cloned())
        .ok_or_else(|| {
            format!(
                "persistent {} descriptor `{}` has no {} descriptor",
                parent.name, parent.id, label
            )
        })
}

fn persist_slot_i64(rt: &JitRuntime, slot: &JetVal) -> Result<i64, String> {
    match slot {
        JetVal::Int(value) | JetVal::RecordRef(value) => rt
            .heap
            .int_to_i64(*value)
            .ok_or_else(|| "persistent integer discriminant is out of range".to_string()),
        JetVal::ExactInt(value) => value
            .to_string_rep()
            .parse::<i64>()
            .map_err(|_| "persistent integer discriminant is out of range".to_string()),
        _ => Err("persistent record discriminant has the wrong carrier".to_string()),
    }
}

fn persist_compat_type(
    rt: &JitRuntime,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<MirType, String> {
    let kind = persist_effective_kind(descriptor);
    let kind = match kind {
        RuntimeValueKind::Unit => MirTypeKind::Tuple(Vec::new()),
        RuntimeValueKind::Int => MirTypeKind::Int,
        RuntimeValueKind::Float => {
            if descriptor.abi == RuntimeValueAbi::Float32 {
                MirTypeKind::Float32
            } else {
                MirTypeKind::Float
            }
        }
        RuntimeValueKind::Bool => MirTypeKind::Bool,
        RuntimeValueKind::Char => MirTypeKind::Char,
        RuntimeValueKind::String => MirTypeKind::String,
        RuntimeValueKind::List => MirTypeKind::List(Box::new(
            persist_compat_type(
                rt,
                &persist_child_descriptor(rt, descriptor.element, descriptor, "element")?,
            )?,
        )),
        RuntimeValueKind::Map => MirTypeKind::Map {
            key: Box::new(persist_compat_type(
                rt,
                &persist_child_descriptor(rt, descriptor.key, descriptor, "key")?,
            )?),
            value: Box::new(persist_compat_type(
                rt,
                &persist_child_descriptor(rt, descriptor.value, descriptor, "value")?,
            )?),
        },
        RuntimeValueKind::Shared => MirTypeKind::Shared(Box::new(persist_compat_type(
            rt,
            &persist_child_descriptor(rt, descriptor.element, descriptor, "element")?,
        )?)),
        RuntimeValueKind::Option => MirTypeKind::Option(Box::new(persist_compat_type(
            rt,
            &persist_child_descriptor(rt, descriptor.ok, descriptor, "option")?,
        )?)),
        RuntimeValueKind::Result => MirTypeKind::Result {
            ok: Box::new(persist_compat_type(
                rt,
                &persist_child_descriptor(rt, descriptor.ok, descriptor, "ok")?,
            )?),
            err: Box::new(persist_compat_type(
                rt,
                &persist_child_descriptor(rt, descriptor.err, descriptor, "err")?,
            )?),
        },
        RuntimeValueKind::Record
        | RuntimeValueKind::Enum
        | RuntimeValueKind::Named
        | RuntimeValueKind::Handle => MirTypeKind::Apply { name: jet_foundation::MIR::MirNominalRef::from_name(descriptor.name.clone()), args: Vec::new() },
        RuntimeValueKind::Closure | RuntimeValueKind::View | RuntimeValueKind::Iterator => {
            return Err(format!(
                "persistent type `{}` is not a persistable runtime carrier",
                descriptor.name
            ));
        }
    };
    Ok(MirType::from_kind(kind))
}

fn persist_decode_record_slots(
    rt: &mut JitRuntime,
    slots: &[JetVal],
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistDecodeState,
    depth: usize,
) -> Result<MirRuntimeValue, String> {
    let kind = persist_effective_kind(descriptor);
    match kind {
        RuntimeValueKind::Record => {
            let mut fields = Vec::with_capacity(descriptor.fields.len());
            let skip_computed = state.skip_computed;
            for field in descriptor
                .fields
                .iter()
                .filter(|field| !skip_computed || !field.computed)
            {
                let slot = slots.get(field.index).ok_or_else(|| {
                    format!(
                        "persistent record `{}` is missing field {}",
                        descriptor.name, field.source_name
                    )
                })?;
                let child = rt
                    .runtime_type_descriptor(field.type_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "persistent record field `{}` has no descriptor",
                            field.source_name
                        )
                    })?;
                fields.push((
                    field.source_name.clone(),
                    persist_decode_slot(rt, slot, &child, state, depth + 1)?,
                ));
            }
            Ok(MirRuntimeValue::Struct {
                type_name: descriptor.name.clone(),
                fields,
            })
        }
        RuntimeValueKind::Enum => {
            let discriminant = persist_slot_i64(
                rt,
                slots
                    .first()
                    .ok_or_else(|| "persistent enum has no discriminant".to_string())?,
            )?;
            let variant = descriptor
                .variants
                .iter()
                .find(|variant| variant.discriminant == discriminant)
                .ok_or_else(|| {
                    format!(
                        "persistent enum `{}` has unknown discriminant {discriminant}",
                        descriptor.name
                    )
                })?;
            let mut args = Vec::with_capacity(variant.fields.len());
            let skip_computed = state.skip_computed;
            for field in variant
                .fields
                .iter()
                .filter(|field| !skip_computed || !field.computed)
            {
                let slot = slots.get(field.index + 1).ok_or_else(|| {
                    format!(
                        "persistent enum `{}` is missing field {}",
                        descriptor.name, field.source_name
                    )
                })?;
                let child = rt
                    .runtime_type_descriptor(field.type_id)
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "persistent enum field `{}` has no descriptor",
                            field.source_name
                        )
                    })?;
                let name = (variant.fields.len() != 1 || field.source_name != "value")
                    .then(|| field.source_name.clone());
                args.push((
                    name,
                    persist_decode_slot(rt, slot, &child, state, depth + 1)?,
                ));
            }
            Ok(MirRuntimeValue::Enum {
                type_name: descriptor.name.clone(),
                variant: variant.name.clone(),
                args,
            })
        }
        _ => Err(format!(
            "persistent descriptor `{}` is not a record carrier",
            descriptor.name
        )),
    }
}

fn persist_decode_list_slots(
    rt: &mut JitRuntime,
    slots: &[JetVal],
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistDecodeState,
    depth: usize,
) -> Result<MirRuntimeValue, String> {
    let child = rt
        .runtime_type_descriptor(descriptor.element.ok_or_else(|| {
            format!("persistent list `{}` has no element descriptor", descriptor.name)
        })?)
        .cloned()
        .ok_or_else(|| format!("persistent list `{}` element is unknown", descriptor.name))?;
    slots
        .iter()
        .map(|slot| persist_decode_slot(rt, slot, &child, state, depth + 1))
        .collect::<Result<Vec<_>, _>>()
        .map(MirRuntimeValue::List)
}

fn persist_decode_map(
    rt: &mut JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistDecodeState,
    depth: usize,
) -> Result<MirRuntimeValue, String> {
    let key_descriptor = persist_child_descriptor(rt, descriptor.key, descriptor, "key")?;
    let value_descriptor = persist_child_descriptor(rt, descriptor.value, descriptor, "value")?;
    let len = rt
        .heap
        .map_len(raw)
        .ok_or_else(|| "persistent map handle was invalid".to_string())?;
    let mut values = Vec::with_capacity(len as usize);
    for index in 0..len {
        let key_raw = rt
            .heap
            .map_key_at(raw, index)
            .ok_or_else(|| "persistent map key was invalid".to_string())?;
        let key = match persist_effective_kind(&key_descriptor) {
            RuntimeValueKind::Int => jet_foundation::MIR::MirConstKey::Int(
                rt.heap
                    .int_to_i64(key_raw)
                    .ok_or_else(|| "persistent map integer key is out of range".to_string())?,
            ),
            RuntimeValueKind::String => jet_foundation::MIR::MirConstKey::String(
                rt.heap
                    .clone_string(key_raw)
                    .ok_or_else(|| "persistent map string key was invalid".to_string())?,
            ),
            RuntimeValueKind::Bool => jet_foundation::MIR::MirConstKey::Bool(key_raw != 0),
            _ => {
                return Err(format!(
                    "persistent map key type `{}` is not a scalar key",
                    key_descriptor.name
                ))
            }
        };
        let value_raw = rt
            .heap
            .map_value_at(raw, index)
            .ok_or_else(|| "persistent map value was invalid".to_string())?;
        values.push((
            key,
            persist_decode_raw(rt, value_raw, &value_descriptor, state, depth + 1)?,
        ));
    }
    Ok(MirRuntimeValue::Map(values))
}

fn persist_decode_slot(
    rt: &mut JitRuntime,
    slot: &JetVal,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistDecodeState,
    depth: usize,
) -> Result<MirRuntimeValue, String> {
    let kind = persist_effective_kind(descriptor);
    match slot {
        JetVal::Int(raw) | JetVal::RecordRef(raw) => {
            persist_decode_raw(rt, *raw, descriptor, state, depth)
        }
        JetVal::ExactInt(value) if kind == RuntimeValueKind::Int => {
            Ok(MirRuntimeValue::BigInt(value.to_string_rep()))
        }
        JetVal::String(value) if kind == RuntimeValueKind::String => {
            Ok(MirRuntimeValue::String(value.clone()))
        }
        JetVal::Float(value) if kind == RuntimeValueKind::Float => {
            Ok(MirRuntimeValue::Float {
                value: *value,
                f32: descriptor.abi == RuntimeValueAbi::Float32,
            })
        }
        JetVal::Bool(value) if kind == RuntimeValueKind::Bool => {
            Ok(MirRuntimeValue::Bool(*value))
        }
        JetVal::Char(value) if kind == RuntimeValueKind::Char => {
            Ok(MirRuntimeValue::Char(*value))
        }
        JetVal::List(values) if kind == RuntimeValueKind::List => {
            persist_decode_list_slots(rt, values, descriptor, state, depth)
        }
        JetVal::Record(values)
            if matches!(kind, RuntimeValueKind::Record | RuntimeValueKind::Enum) =>
        {
            persist_decode_record_slots(rt, values, descriptor, state, depth)
        }
        _ => Err(format!(
            "persistent `{}` value had an incompatible arena slot",
            descriptor.name
        )),
    }
}

fn persist_decode_raw(
    rt: &mut JitRuntime,
    raw: i64,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistDecodeState,
    depth: usize,
) -> Result<MirRuntimeValue, String> {
    let tracked = state.enter(descriptor, raw, depth)?;
    let kind = persist_effective_kind(descriptor);
    let value = match kind {
        RuntimeValueKind::Unit => Ok(MirRuntimeValue::Unit),
        RuntimeValueKind::Int => match rt.heap.int_to_i64(raw) {
            Some(value) => Ok(MirRuntimeValue::Int(value)),
            None => Ok(MirRuntimeValue::BigInt(rt.heap.int_to_string(raw))),
        },
        RuntimeValueKind::Float => match descriptor.abi {
            RuntimeValueAbi::Float => Ok(MirRuntimeValue::Float {
                value: f64::from_bits(raw as u64),
                f32: false,
            }),
            RuntimeValueAbi::Float32 => Ok(MirRuntimeValue::Float {
                value: f32::from_bits(raw as u32) as f64,
                f32: true,
            }),
            _ => Err(format!("persistent float `{}` had the wrong ABI", descriptor.name)),
        },
        RuntimeValueKind::Bool => Ok(MirRuntimeValue::Bool(raw != 0)),
        RuntimeValueKind::Char => char::from_u32(raw as u32)
            .map(MirRuntimeValue::Char)
            .ok_or_else(|| "persistent char value was invalid".to_string()),
        RuntimeValueKind::String => rt
            .heap
            .clone_string(raw)
            .map(MirRuntimeValue::String)
            .ok_or_else(|| "persistent string handle was invalid".to_string()),
        RuntimeValueKind::List => {
            let slots = rt
                .heap
                .clone_list_values(raw)
                .ok_or_else(|| "persistent list handle was invalid".to_string())?;
            persist_decode_list_slots(rt, &slots, descriptor, state, depth + 1)
        }
        RuntimeValueKind::Map => persist_decode_map(rt, raw, descriptor, state, depth + 1),
        RuntimeValueKind::Shared => {
            let child = persist_child_descriptor(rt, descriptor.element, descriptor, "element")?;
            let value = Memory::shared_value(rt, raw)
                .ok_or_else(|| "persistent Shared handle was invalid".to_string())?;
            persist_decode_raw(rt, value, &child, state, depth + 1)
        }
        RuntimeValueKind::Option => {
            let child = persist_child_descriptor(rt, descriptor.ok, descriptor, "option")?;
            let (present, value) = jit_result_parts(rt, raw)
                .ok_or_else(|| "persistent Option handle was invalid".to_string())?;
            if present {
                Ok(MirRuntimeValue::Present(Box::new(persist_decode_raw(
                    rt,
                    value as i64,
                    &child,
                    state,
                    depth + 1,
                )?)))
            } else {
                Ok(MirRuntimeValue::Absent {
                    element: persist_compat_type(rt, &child)?,
                })
            }
        }
        RuntimeValueKind::Result => {
            let ok = persist_child_descriptor(rt, descriptor.ok, descriptor, "ok")?;
            let err = persist_child_descriptor(rt, descriptor.err, descriptor, "err")?;
            let (success, value) = jit_result_parts(rt, raw)
                .ok_or_else(|| "persistent Result handle was invalid".to_string())?;
            if success {
                Ok(MirRuntimeValue::Present(Box::new(persist_decode_raw(
                    rt,
                    value as i64,
                    &ok,
                    state,
                    depth + 1,
                )?)))
            } else if runtime_report_is_clean(rt, Some(err.id)) {
                Ok(MirRuntimeValue::Absent {
                    element: persist_compat_type(rt, &ok)?,
                })
            } else {
                Ok(MirRuntimeValue::FailedTold(Box::new(persist_decode_raw(
                    rt,
                    value as i64,
                    &err,
                    state,
                    depth + 1,
                )?)))
            }
        }
        RuntimeValueKind::Record | RuntimeValueKind::Enum => {
            let slots = rt
                .heap
                .clone_record_values(raw)
                .ok_or_else(|| "persistent record handle was invalid".to_string())?;
            persist_decode_record_slots(rt, &slots, descriptor, state, depth + 1)
        }
        RuntimeValueKind::Named | RuntimeValueKind::Handle => Err(format!(
            "persistent type `{}` has no checked runtime descriptor",
            descriptor.name
        )),
        RuntimeValueKind::Closure | RuntimeValueKind::View | RuntimeValueKind::Iterator => Err(
            format!(
                "persistent type `{}` is not a persistable runtime carrier",
                descriptor.name
            ),
        ),
    };
    state.leave(descriptor, raw, tracked);
    value
}

fn persist_name_matches(descriptor: &RuntimeTypeDescriptor, name: &str) -> bool {
    name == descriptor.name
        || name == descriptor.canonical
        || descriptor
            .canonical
            .strip_prefix("id:")
            .is_some_and(|_| name == descriptor.name)
}

fn persist_encode_record(
    rt: &mut JitRuntime,
    value: &MirRuntimeValue,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistEncodeState,
    depth: usize,
) -> Result<i64, String> {
    let MirRuntimeValue::Struct { type_name, fields } = value else {
        return Err(format!("persistent `{}` expects a record value", descriptor.name));
    };
    if !persist_name_matches(descriptor, type_name) {
        return Err(format!(
            "persistent record type changed from `{}` to `{}`",
            descriptor.name, type_name
        ));
    }
    if fields.len() != descriptor.fields.len() {
        return Err(format!(
            "persistent record `{}` field count changed",
            descriptor.name
        ));
    }
    let mut slots = Vec::with_capacity(descriptor.fields.len());
    for field in &descriptor.fields {
        let (_, value) = fields
            .iter()
            .find(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
            .ok_or_else(|| {
                format!(
                    "persistent record `{}` is missing field {}",
                    descriptor.name, field.source_name
                )
            })?;
        let child = rt
            .runtime_type_descriptor(field.type_id)
            .cloned()
            .ok_or_else(|| format!("persistent field `{}` has no descriptor", field.source_name))?;
        slots.push(persist_encode_slot(rt, value, &child, state, depth + 1)?);
    }
    Ok(rt.heap.alloc_record_values(slots))
}

fn persist_encode_enum(
    rt: &mut JitRuntime,
    value: &MirRuntimeValue,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistEncodeState,
    depth: usize,
) -> Result<i64, String> {
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(format!("persistent `{}` expects an enum value", descriptor.name));
    };
    if !persist_name_matches(descriptor, type_name) {
        return Err(format!(
            "persistent enum type changed from `{}` to `{}`",
            descriptor.name, type_name
        ));
    }
    let variant = descriptor
        .variants
        .iter()
        .find(|candidate| {
            candidate.name == variant.as_str() || candidate.wire_name == variant.as_str()
        })
        .ok_or_else(|| format!("persistent enum `{}` has no variant `{variant}`", descriptor.name))?;
    if args.len() != variant.fields.len() {
        return Err(format!(
            "persistent enum `{}` payload arity changed",
            descriptor.name
        ));
    }
    let mut slots = Vec::with_capacity(args.len() + 1);
    slots.push(JetVal::Int(variant.discriminant));
    for (index, field) in variant.fields.iter().enumerate() {
        let arg = &args[index];
        let expected_name = (variant.fields.len() != 1 || field.source_name != "value")
            .then_some(field.source_name.as_str());
        if arg.0.as_deref() != expected_name {
            return Err(format!(
                "persistent enum `{}` payload field order changed",
                descriptor.name
            ));
        }
        let child = rt
            .runtime_type_descriptor(field.type_id)
            .cloned()
            .ok_or_else(|| format!("persistent enum field `{}` has no descriptor", field.source_name))?;
        slots.push(persist_encode_slot(rt, &arg.1, &child, state, depth + 1)?);
    }
    Ok(rt.heap.alloc_record_values(slots))
}

fn persist_encode_map(
    rt: &mut JitRuntime,
    values: &[(jet_foundation::MIR::MirConstKey, MirRuntimeValue)],
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistEncodeState,
    depth: usize,
) -> Result<i64, String> {
    let key_descriptor = persist_child_descriptor(rt, descriptor.key, descriptor, "key")?;
    let value_descriptor = persist_child_descriptor(rt, descriptor.value, descriptor, "value")?;
    let map = rt.heap.alloc_empty_map();
    for (key, value) in values {
        let value = persist_encode_raw(rt, value, &value_descriptor, state, depth + 1)?;
        match (key, key_descriptor.kind) {
            (jet_foundation::MIR::MirConstKey::Int(key), RuntimeValueKind::Int) => {
                rt.heap
                    .map_insert_int(map, *key, value)
                    .ok_or_else(|| "persistent integer map allocation failed".to_string())?;
            }
            (jet_foundation::MIR::MirConstKey::String(key), RuntimeValueKind::String) => {
                let key = rt.heap.alloc_string(key.clone());
                rt.heap
                    .map_insert(map, key, value)
                    .ok_or_else(|| "persistent string map allocation failed".to_string())?;
            }
            (jet_foundation::MIR::MirConstKey::Bool(key), RuntimeValueKind::Bool) => {
                rt.heap
                    .map_insert_bool(map, *key, value)
                    .ok_or_else(|| "persistent bool map allocation failed".to_string())?;
            }
            _ => {
                return Err(format!(
                    "persistent map key `{}` does not match descriptor `{}`",
                    key_descriptor.name, descriptor.name
                ))
            }
        }
    }
    Ok(map)
}

fn persist_encode_slot(
    rt: &mut JitRuntime,
    value: &MirRuntimeValue,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistEncodeState,
    depth: usize,
) -> Result<JetVal, String> {
    let kind = persist_effective_kind(descriptor);
    match kind {
        RuntimeValueKind::String => match value {
            MirRuntimeValue::String(value) => Ok(JetVal::String(value.clone())),
            _ => Err(format!("persistent `{}` expects a string", descriptor.name)),
        },
        RuntimeValueKind::Float => match value {
            MirRuntimeValue::Float { value, f32 } => {
                if (*f32) != (descriptor.abi == RuntimeValueAbi::Float32) {
                    return Err(format!("persistent `{}` float width changed", descriptor.name));
                }
                Ok(JetVal::Float(if descriptor.abi == RuntimeValueAbi::Float32 {
                    *value as f32 as f64
                } else {
                    *value
                }))
            }
            _ => Err(format!("persistent `{}` expects a float", descriptor.name)),
        },
        RuntimeValueKind::Bool => match value {
            MirRuntimeValue::Bool(value) => Ok(JetVal::Bool(*value)),
            _ => Err(format!("persistent `{}` expects a bool", descriptor.name)),
        },
        RuntimeValueKind::Char => match value {
            MirRuntimeValue::Char(value) => Ok(JetVal::Char(*value)),
            _ => Err(format!("persistent `{}` expects a char", descriptor.name)),
        },
        RuntimeValueKind::Unit => Ok(JetVal::Int(0)),
        _ => persist_encode_raw(rt, value, descriptor, state, depth).map(JetVal::Int),
    }
}

fn persist_encode_raw(
    rt: &mut JitRuntime,
    value: &MirRuntimeValue,
    descriptor: &RuntimeTypeDescriptor,
    state: &mut PersistEncodeState,
    depth: usize,
) -> Result<i64, String> {
    state.enter(depth)?;
    let kind = persist_effective_kind(descriptor);
    match kind {
        RuntimeValueKind::Unit => match value {
            MirRuntimeValue::Unit => Ok(0),
            _ => Err(format!("persistent `{}` expects unit", descriptor.name)),
        },
        RuntimeValueKind::Int => match value {
            MirRuntimeValue::Int(value) => Ok(rt.heap.int_from_i64(*value)),
            MirRuntimeValue::BigInt(value) => rt
                .heap
                .int_from_str(value)
                .map_err(|error| format!("persistent BigInt is invalid: {error}")),
            _ => Err(format!("persistent `{}` expects an integer", descriptor.name)),
        },
        RuntimeValueKind::Float => match value {
            MirRuntimeValue::Float { value, f32 } => match descriptor.abi {
                RuntimeValueAbi::Float if !*f32 => Ok(value.to_bits() as i64),
                RuntimeValueAbi::Float32 if *f32 => Ok((*value as f32).to_bits() as i64),
                _ => Err(format!("persistent `{}` float width changed", descriptor.name)),
            },
            _ => Err(format!("persistent `{}` expects a float", descriptor.name)),
        },
        RuntimeValueKind::Bool => match value {
            MirRuntimeValue::Bool(value) => Ok(i64::from(*value)),
            _ => Err(format!("persistent `{}` expects a bool", descriptor.name)),
        },
        RuntimeValueKind::Char => match value {
            MirRuntimeValue::Char(value) => Ok(i64::from(*value as u32)),
            _ => Err(format!("persistent `{}` expects a char", descriptor.name)),
        },
        RuntimeValueKind::String => match value {
            MirRuntimeValue::String(value) => Ok(rt.heap.alloc_string(value.clone())),
            _ => Err(format!("persistent `{}` expects a string", descriptor.name)),
        },
        RuntimeValueKind::List => {
            let child = persist_child_descriptor(rt, descriptor.element, descriptor, "element")?;
            let values = match value {
                MirRuntimeValue::List(values) => values.clone(),
                MirRuntimeValue::Bytes(values) => values
                    .iter()
                    .map(|value| MirRuntimeValue::Int(i64::from(*value)))
                    .collect(),
                _ => return Err(format!("persistent `{}` expects a list", descriptor.name)),
            };
            let slots = values
                .iter()
                .map(|value| persist_encode_slot(rt, value, &child, state, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rt.heap.alloc_list_values(slots))
        }
        RuntimeValueKind::Map => match value {
            MirRuntimeValue::Map(values) => {
                persist_encode_map(rt, values, descriptor, state, depth + 1)
            }
            _ => Err(format!("persistent `{}` expects a map", descriptor.name)),
        },
        RuntimeValueKind::Shared => {
            let child = persist_child_descriptor(rt, descriptor.element, descriptor, "element")?;
            let value = persist_encode_raw(rt, value, &child, state, depth + 1)?;
            Ok(Memory::shared_alloc_for_persist(rt, value))
        }
        RuntimeValueKind::Option => match value {
            MirRuntimeValue::Present(value) => {
                let child =
                    persist_child_descriptor(rt, descriptor.ok, descriptor, "option")?;
                let value = persist_encode_raw(rt, value, &child, state, depth + 1)?;
                Ok(alloc_jit_result(rt, true, value as u64))
            }
            MirRuntimeValue::Absent { .. } => Ok(alloc_jit_result(rt, false, 0)),
            _ => Err(format!("persistent `{}` expects an option carrier", descriptor.name)),
        },
        RuntimeValueKind::Result => match value {
            MirRuntimeValue::Present(value) => {
                let child = persist_child_descriptor(rt, descriptor.ok, descriptor, "ok")?;
                let value = persist_encode_raw(rt, value, &child, state, depth + 1)?;
                Ok(alloc_jit_result(rt, true, value as u64))
            }
            MirRuntimeValue::FailedTold(value) => {
                let child = persist_child_descriptor(rt, descriptor.err, descriptor, "err")?;
                let value = persist_encode_raw(rt, value, &child, state, depth + 1)?;
                Ok(alloc_jit_result(rt, false, value as u64))
            }
            MirRuntimeValue::Absent { .. } => Ok(alloc_jit_result(rt, false, 0)),
            _ => Err(format!("persistent `{}` expects a result carrier", descriptor.name)),
        },
        RuntimeValueKind::Record => {
            persist_encode_record(rt, value, descriptor, state, depth + 1)
        }
        RuntimeValueKind::Enum => persist_encode_enum(rt, value, descriptor, state, depth + 1),
        RuntimeValueKind::Named | RuntimeValueKind::Handle => Err(format!(
            "persistent type `{}` has no checked runtime descriptor",
            descriptor.name
        )),
        RuntimeValueKind::Closure | RuntimeValueKind::View | RuntimeValueKind::Iterator => Err(
            format!(
                "persistent type `{}` is not a persistable runtime carrier",
                descriptor.name
            ),
        ),
    }
}

fn persist_text(rt: &JitRuntime, handle: i64) -> Option<String> {
    rt.heap
        .clone_string(handle)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
}

/// Decode one raw resident carrier into an owned typed value before it crosses
/// into the shared Persist store.
fn jet_persist_runtime_set(key_handle: i64, type_handle: i64, raw: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(key) = persist_text(rt, key_handle) else {
            rt.set_host_fault("persistent key handle was invalid");
            return -1;
        };
        let Some(type_name) = persist_text(rt, type_handle) else {
            rt.set_host_fault("persistent type handle was invalid");
            return -1;
        };
        let Some(descriptor) = persist_descriptor(rt, &type_name) else {
            rt.set_host_fault("persistent type descriptor was missing");
            return -1;
        };
        let mut state = PersistDecodeState::default();
        let Ok(value) = persist_decode_raw(rt, raw, &descriptor, &mut state, 0) else {
            rt.set_host_fault("persistent value could not be decoded by its descriptor");
            return -1;
        };
        if !jet_foundation::Persist::shared_write_runtime_key(&key, &value) {
            rt.set_host_fault("persistent value was rejected by the shared typed store");
            return -1;
        }
        0
    })
}

/// Materialize a fresh resident carrier from the shared typed Persist value.
/// No arena handle is retained across runs or hot swaps.
fn jet_persist_runtime_get(key_handle: i64, type_handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(key) = persist_text(rt, key_handle) else {
            rt.set_host_fault("persistent key handle was invalid");
            return 0;
        };
        let Some(type_name) = persist_text(rt, type_handle) else {
            rt.set_host_fault("persistent type handle was invalid");
            return 0;
        };
        let Some(descriptor) = persist_descriptor(rt, &type_name) else {
            rt.set_host_fault("persistent type descriptor was missing");
            return 0;
        };
        let Some(value) = jet_foundation::Persist::shared_read_runtime_key(&key) else {
            rt.set_host_fault("persistent value was missing from the shared typed store");
            return 0;
        };
        let mut state = PersistEncodeState::default();
        match persist_encode_raw(rt, &value, &descriptor, &mut state, 0) {
            Ok(raw) => raw,
            Err(_) => {
                rt.set_host_fault("persistent value could not be materialized by its descriptor");
                0
            }
        }
    })
}
fn jet_jit_observe_live_value_update(
    key_handle: i64,
    type_handle: i64,
    rendered_handle: i64,
) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(key) = rt.heap.clone_string(key_handle) else {
            rt.set_host_fault("live value key handle was invalid");
            return;
        };
        let Some(type_identity) = rt.heap.clone_string(type_handle) else {
            rt.set_host_fault("live value type handle was invalid");
            return;
        };
        let rendered = if rendered_handle == 0 {
            None
        } else {
            let Some(value) = rt.heap.clone_string(rendered_handle) else {
                rt.set_host_fault("live value rendered handle was invalid");
                return;
            };
            Some(value)
        };
        jet_foundation::Devtools::jet_observe_live_value_update(
            &key,
            &type_identity,
            rendered.as_deref(),
        );
    });
}

fn jet_jit_add_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("add", line);
            0
        }
    }
}

fn jet_jit_sub_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_sub(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("sub", line);
            0
        }
    }
}

fn jet_jit_mul_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_mul(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("mul", line);
            0
        }
    }
}

fn jet_jit_div_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_div(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("div", line);
            0
        }
    }
}

/// D-EXPSEM1=A: the same exact, trapping whole-number power the Prelude runs
/// (`Prelude/Core/Power.rs`). A negative exponent has no whole-number result.
fn jet_jit_pow_i64(a: i64, b: i64, line: u32) -> i64 {
    if b < 0 {
        let message = contract_kernel::jet_arithmetic_message("pow_negative");
        with_runtime_mut(|rt| rt.set_arithmetic_stop(line, message));
        return 0;
    }
    match u32::try_from(b).ok().and_then(|e| a.checked_pow(e)) {
        Some(value) => value,
        None => {
            jet_trap_overflow("pow", line);
            0
        }
    }
}

/// D-EXPSEM1=A: `^` on floats is the ordinary floating-point power.
fn jet_jit_pow_f64(a: f64, b: f64) -> f64 {
    a.powf(b)
}

/// D-FLOORDIV1=A: the same rounding-down division the Prelude runs
/// (`Prelude/Core/Division.rs`), through the one shared rule.
fn jet_jit_floordiv_i64(a: i64, b: i64, line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b == 0 {
        let message = contract_kernel::jet_arithmetic_message("divide_zero");
        with_runtime_mut(|rt| rt.set_arithmetic_stop(line, message));
        return 0;
    }
    match MathLayout::floor_div(a as i128, b as i128).and_then(|v| i64::try_from(v).ok()) {
        Some(value) => value,
        None => {
            let message = contract_kernel::jet_arithmetic_message("divide_overflow");
            with_runtime_mut(|rt| rt.set_arithmetic_stop(line, message));
            0
        }
    }
}

/// D-FLOORDIV1=A: on floats `/%` divides and rounds the answer down.
fn jet_jit_floordiv_f64(a: f64, b: f64) -> f64 {
    (a / b).floor()
}

/// D-MODSEM1=A: the floored modulo the Prelude runs
/// (`Prelude/Core/Division.rs`), through the one shared rule.
fn jet_jit_mod_i64(a: i64, b: i64, line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b == 0 {
        let message = contract_kernel::jet_arithmetic_message("divide_zero");
        with_runtime_mut(|rt| rt.set_arithmetic_stop(line, message));
        return 0;
    }
    match MathLayout::floored_mod(a as i128, b as i128).and_then(|v| i64::try_from(v).ok()) {
        Some(value) => value,
        None => {
            let message = contract_kernel::jet_arithmetic_message("divide_overflow");
            with_runtime_mut(|rt| rt.set_arithmetic_stop(line, message));
            0
        }
    }
}

fn jet_jit_rem_i64(a: i64, b: i64, line: u32) -> i64 {
    if b == 0 {
        let message = contract_kernel::jet_arithmetic_message("divide_zero");
        with_runtime_mut(|rt| rt.set_arithmetic_stop(line, message));
        return 0;
    }
    a.wrapping_rem(b)
}

fn jet_jit_intn_binop(
    left: i64,
    right: i64,
    op: i64,
    mode: i64,
    signed: i64,
    bits: i64,
    right_signed: i64,
    line: u32,
) -> i64 {
    match fixed_arithmetic_kernel::jet_fixed_arithmetic(
        left,
        right as i128,
        op,
        mode,
        signed != 0,
        bits as u8,
        right_signed != 0,
    ) {
        fixed_arithmetic_kernel::JetFixedArithmeticResult::Value(value) => value,
        fixed_arithmetic_kernel::JetFixedArithmeticResult::Absent => {
            Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, false, 0))
        }
        fixed_arithmetic_kernel::JetFixedArithmeticResult::Trap(error) => {
            let message = error.to_string();
            with_runtime_mut(|rt| rt.set_arithmetic_stop(line, &message));
            0
        }
    }
}

fn jet_jit_intn_to_string(value: i64, signed: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .alloc_string(jet_codegen::Comptime::MathLayout::integer_show(
                value,
                signed != 0,
            ))
    })
}

fn jet_jit_print_i64(v: i64) {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.int_to_string(v));
    let _ = write_jit_stdout_line(&text, false);
}

fn jet_jit_print_f64(v: f64) {
    let text = jet_rt::display_f64(v);
    let _ = write_jit_stdout_line(&text, false);
}

fn jet_jit_print_bool(v: i8) {
    let _ = write_jit_stdout_line(if v == 0 { "false" } else { "true" }, false);
}

fn jet_jit_print_char(v: i32) {
    let ch = char::from_u32(v as u32).unwrap_or('?');
    let _ = write_jit_stdout_line(&ch.to_string(), false);
}

fn jet_jit_print_str(id: i64) {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id));
    if let Some(text) = text {
        let _ = write_jit_stdout_line(&text, false);
    }
}

/// MIR `print`: the canonical Prelude entry `jet_term_write_stdout_line(text,
/// flush)`. AOT calls the Prelude function directly; the resident JIT
/// registers the same symbol name and routes through the engine's ordered
/// stdout adapter (I9).
fn jet_jit_term_write_stdout_line(id: i64, flush: i64) {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id));
    if let Some(text) = text {
        let _ = write_jit_stdout_line(&text, flush != 0);
    }
}

/// `core.term.eprint`'s marshalling half: the rendered line, on stderr. The
/// rendering is `lower_jet_show`, the same JetShow route AOT's
/// `(x).jet_show()` takes, so no engine re-encodes a value's text.
fn jet_jit_eprint_str(id: i64) {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id));
    if let Some(text) = text {
        let _ = write_jit_stderr_line(&text, false);
    }
}

fn display_slot_text(
    rt: &JitRuntime,
    slot: &JetVal,
    type_id: u64,
    depth: usize,
    debug: bool,
) -> String {
    match slot {
        JetVal::Float(value) => jet_rt::display_f64(*value),
        JetVal::Bool(value) => value.to_string(),
        JetVal::Char(value) => format!("{value:?}"),
        JetVal::String(value) => format!("{value:?}"),
        JetVal::ExactInt(value) => value.to_string_rep(),
        JetVal::Record(_) => "<invalid>".to_string(),
        JetVal::Int(raw) => display_handle_text_mode(rt, *raw, type_id, depth, debug),
        JetVal::RecordRef(raw) => display_handle_text_mode(rt, *raw, type_id, depth, debug),
        JetVal::StringView { owner, start, end } => rt
            .heap
            .clone_string(*owner)
            .map(|text| {
                let start = (*start).min(text.len());
                let end = (*end).min(text.len()).max(start);
                format!("{:?}", &text[start..end])
            })
            .unwrap_or_else(|| "<invalid>".to_string()),
        _ => "<invalid>".to_string(),
    }
}

pub(crate) fn display_handle_text(rt: &JitRuntime, handle: i64, type_id: u64, depth: usize) -> String {
    display_handle_text_mode(rt, handle, type_id, depth, false)
}

fn display_handle_text_mode(
    rt: &JitRuntime,
    handle: i64,
    type_id: u64,
    depth: usize,
    debug: bool,
) -> String {
    let kind = rt
        .runtime_type_descriptor(type_id)
        .map(|descriptor| descriptor.kind);
    match kind {
        Some(RuntimeValueKind::Int) => match rt
            .runtime_type_descriptor(type_id)
            .and_then(|descriptor| descriptor.integer_width)
        {
            Some(width) if !width.signed => (handle as u64).to_string(),
            Some(_) => handle.to_string(),
            None => rt.heap.int_to_string(handle),
        },
        Some(RuntimeValueKind::String) => rt
            .heap
            .clone_string(handle)
            .map(|text| format!("{text:?}"))
            .unwrap_or_else(|| "<invalid>".to_string()),
        Some(RuntimeValueKind::Bool) => {
            if handle != 0 { "true" } else { "false" }.to_string()
        }
        Some(RuntimeValueKind::Enum | RuntimeValueKind::Record) => {
            nominal_handle_text(rt, handle, type_id, depth, debug)
                .unwrap_or_else(|| "<invalid>".to_string())
        }
        Some(RuntimeValueKind::Named | RuntimeValueKind::Handle) => {
            let walk = rt.runtime_type_descriptor(type_id).is_some_and(|descriptor| {
                !descriptor.variants.is_empty() || !descriptor.fields.is_empty()
            });
            if walk {
                nominal_handle_text(rt, handle, type_id, depth, debug)
                    .unwrap_or_else(|| handle.to_string())
            } else {
                handle.to_string()
            }
        }
        _ => handle.to_string(),
    }
}

fn display_nominal_handle(
    rt: &JitRuntime,
    handle: i64,
    type_id: u64,
    depth: usize,
) -> Option<String> {
    nominal_handle_text(rt, handle, type_id, depth, false)
}

fn debug_nominal_handle(
    rt: &mut JitRuntime,
    handle: i64,
    type_id: u64,
    depth: usize,
) -> Option<String> {
    let descriptor = rt.runtime_type_descriptor(type_id)?.clone();
    let mut state = PersistDecodeState::for_debug();
    let value = persist_decode_raw(rt, handle, &descriptor, &mut state, depth).ok()?;
    structural_debug_value(rt, &value, &descriptor, depth)
}

fn runtime_source_name(name: &str) -> &str {
    name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(name)
}

fn structural_debug_field_redacted(type_name: &str, field_name: &str) -> bool {
    jet_foundation::StructuralDebug::jet_debug_field_metadata(runtime_source_name(type_name))
        .and_then(|metadata| {
            metadata
                .iter()
                .find(|(name, _)| *name == runtime_source_name(field_name))
        })
        .is_some_and(|(_, redacted)| *redacted)
}

fn nominal_handle_text(
    rt: &JitRuntime,
    handle: i64,
    type_id: u64,
    depth: usize,
    debug: bool,
) -> Option<String> {
    if depth > 64 {
        return Some("...".to_string());
    }
    let descriptor = rt.runtime_type_descriptor(type_id)?.clone();
    match descriptor.kind {
        RuntimeValueKind::Enum => {
            let slots = rt.heap.clone_record_values(handle)?;
            let discriminant = match slots.first()? {
                JetVal::Int(value) => *value,
                JetVal::ExactInt(value) => value.to_string_rep().parse().ok()?,
                _ => return None,
            };
            let variant = descriptor
                .variants
                .iter()
                .find(|variant| variant.discriminant == discriminant)?;
            let fields = variant
                .fields
                .iter()
                .filter(|field| !debug || !field.computed)
                .collect::<Vec<_>>();
            let variant_name = if debug {
                runtime_source_name(&variant.name)
            } else {
                variant.name.as_str()
            };
            if fields.is_empty() {
                return if debug {
                    Some(jet_foundation::StructuralDebug::jet_debug_variant(
                        variant_name,
                        None,
                    ))
                } else {
                    Some(variant.name.clone())
                };
            }
            let tuple_style = fields.len() == 1 && fields[0].source_name == "value";
            let mut parts = Vec::new();
            let mut debug_fields = Vec::new();
            for field in fields {
                let slot = slots.get(field.index + 1)?;
                let rendered = display_slot_text(rt, slot, field.type_id, depth + 1, debug);
                if tuple_style {
                    parts.push(rendered);
                } else if debug {
                    debug_fields.push(jet_foundation::StructuralDebug::JetDebugField {
                        name: runtime_source_name(&field.source_name).to_string(),
                        value: rendered,
                        storage_index: field.index,
                        redacted: structural_debug_field_redacted(
                            &descriptor.name,
                            &field.source_name,
                        ),
                    });
                } else {
                    parts.push(format!("{}: {}", field.source_name, rendered));
                }
            }
            Some(if debug {
                if tuple_style {
                    jet_foundation::StructuralDebug::jet_debug_variant(
                        variant_name,
                        Some(parts.join(", ")),
                    )
                } else {
                    jet_foundation::StructuralDebug::jet_debug_record_fields(
                        variant_name,
                        debug_fields,
                    )
                }
            } else if tuple_style {
                format!("{}({})", variant.name, parts.join(", "))
            } else {
                format!("{} {{ {} }}", variant.name, parts.join(", "))
            })
        }
        RuntimeValueKind::Record => {
            let slots = rt.heap.clone_record_values(handle)?;
            let mut parts = Vec::new();
            let mut debug_fields = Vec::new();
            let fields = descriptor
                .fields
                .iter()
                .filter(|field| !debug || !field.computed);
            for field in fields {
                let slot = slots.get(field.index)?;
                let rendered = display_slot_text(rt, slot, field.type_id, depth + 1, debug);
                if debug {
                    debug_fields.push(jet_foundation::StructuralDebug::JetDebugField {
                        name: runtime_source_name(&field.source_name).to_string(),
                        value: rendered,
                        storage_index: field.index,
                        redacted: structural_debug_field_redacted(
                            &descriptor.name,
                            &field.source_name,
                        ),
                    });
                } else {
                    parts.push(format!("{}: {}", field.source_name, rendered));
                }
            }
            if debug {
                Some(jet_foundation::StructuralDebug::jet_debug_record_fields(
                    runtime_source_name(&descriptor.name),
                    debug_fields,
                ))
            } else {
                Some(format!("{} {{ {} }}", descriptor.name, parts.join(", ")))
            }
        }
        _ => None,
    }
}

fn structural_debug_key(key: &MirConstKey) -> String {
    match key {
        MirConstKey::Int(value) => value.to_string(),
        MirConstKey::String(value) => format!("{value:?}"),
        MirConstKey::Bool(value) => value.to_string(),
        MirConstKey::Char(value) => format!("{value:?}"),
        MirConstKey::Tuple(fields) => jet_foundation::StructuralDebug::jet_debug_record(
            "tuple",
            fields.iter().map(|(name, value)| {
                (
                    runtime_source_name(name).to_string(),
                    structural_debug_key(value),
                )
            }),
        ),
        MirConstKey::Struct { type_name, fields } => {
            jet_foundation::StructuralDebug::jet_debug_record(
                runtime_source_name(type_name),
                fields.iter().map(|(name, value)| {
                    (
                        runtime_source_name(name).to_string(),
                        structural_debug_key(value),
                    )
                }),
            )
        }
        MirConstKey::Enum { variant, .. } => runtime_source_name(variant).to_string(),
    }
}

fn structural_debug_record_value<'a>(
    values: &'a [(String, MirRuntimeValue)],
    field: &RuntimeFieldDescriptor,
) -> Option<&'a MirRuntimeValue> {
    values
        .iter()
        .find(|(name, _)| {
            name == &field.source_name
                || runtime_source_name(name) == runtime_source_name(&field.source_name)
        })
        .map(|(_, value)| value)
}

fn structural_debug_enum_value<'a>(
    args: &'a [(Option<String>, MirRuntimeValue)],
    field: &RuntimeFieldDescriptor,
    visible_index: usize,
) -> Option<&'a MirRuntimeValue> {
    args.iter()
        .find(|(name, _)| {
            name.as_deref().is_some_and(|name| {
                name == field.source_name.as_str()
                    || runtime_source_name(name) == runtime_source_name(&field.source_name)
            })
        })
        .map(|(_, value)| value)
        .or_else(|| args.get(visible_index).map(|(_, value)| value))
}

fn structural_debug_value(
    rt: &JitRuntime,
    value: &MirRuntimeValue,
    descriptor: &RuntimeTypeDescriptor,
    depth: usize,
) -> Option<String> {
    match persist_effective_kind(descriptor) {
        RuntimeValueKind::Unit => matches!(value, MirRuntimeValue::Unit).then_some("()".to_string()),
        RuntimeValueKind::Int => match value {
            MirRuntimeValue::Int(value) => Some(
                descriptor
                    .integer_width
                    .is_some_and(|width| !width.signed)
                    .then(|| (*value as u64).to_string())
                    .unwrap_or_else(|| value.to_string()),
            ),
            MirRuntimeValue::BigInt(value) => Some(value.clone()),
            _ => None,
        },
        RuntimeValueKind::Float => match value {
            MirRuntimeValue::Float { value, f32 } => Some(if *f32
                || descriptor.abi == RuntimeValueAbi::Float32
            {
                jet_rt::display_f32(*value as f32)
            } else {
                jet_rt::display_f64(*value)
            }),
            _ => None,
        },
        RuntimeValueKind::Bool => match value {
            MirRuntimeValue::Bool(value) => Some(value.to_string()),
            _ => None,
        },
        RuntimeValueKind::Char => match value {
            MirRuntimeValue::Char(value) => Some(format!("{value:?}")),
            _ => None,
        },
        RuntimeValueKind::String => match value {
            MirRuntimeValue::String(value) => Some(format!("{value:?}")),
            _ => None,
        },
        RuntimeValueKind::List => {
            let child = rt
                .runtime_type_descriptor(descriptor.element?)
                .cloned()?;
            if let MirRuntimeValue::Bytes(values) = value {
                return Some(format!("{values:?}"));
            }
            let MirRuntimeValue::List(values) = value else {
                return None;
            };
            let values = values
                .iter()
                .map(|value| structural_debug_value(rt, value, &child, depth + 1))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[{}]", values.join(", ")))
        }
        RuntimeValueKind::Map => {
            let MirRuntimeValue::Map(values) = value else {
                return None;
            };
            let child = rt
                .runtime_type_descriptor(descriptor.value?)
                .cloned()?;
            let values = values
                .iter()
                .map(|(key, value)| {
                    Some((
                        structural_debug_key(key),
                        structural_debug_value(rt, value, &child, depth + 1)?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_foundation::StructuralDebug::jet_debug_map(values))
        }
        RuntimeValueKind::Shared => {
            let child = rt
                .runtime_type_descriptor(descriptor.element?)
                .cloned()?;
            structural_debug_value(rt, value, &child, depth + 1)
        }
        RuntimeValueKind::Option => {
            let child = rt
                .runtime_type_descriptor(descriptor.ok?)
                .cloned()?;
            match value {
                MirRuntimeValue::Present(value) => Some(
                    jet_foundation::StructuralDebug::jet_debug_optional(Some(
                        structural_debug_value(rt, value, &child, depth + 1)?,
                    )),
                ),
                MirRuntimeValue::Absent { .. } => Some(
                    jet_foundation::StructuralDebug::jet_debug_optional(None),
                ),
                _ => None,
            }
        }
        RuntimeValueKind::Result => {
            let clean = runtime_report_is_clean(rt, descriptor.err);
            match value {
                MirRuntimeValue::Present(value) => {
                    let child = rt
                        .runtime_type_descriptor(descriptor.ok?)
                        .cloned()?;
                    let value = structural_debug_value(rt, value, &child, depth + 1)?;
                    Some(if clean {
                        jet_foundation::StructuralDebug::jet_debug_optional(Some(value))
                    } else {
                        format!("Ok({value})")
                    })
                }
                MirRuntimeValue::FailedTold(value) if !clean => {
                    let child = rt
                        .runtime_type_descriptor(descriptor.err?)
                        .cloned()?;
                    Some(format!(
                        "Err({})",
                        structural_debug_value(rt, value, &child, depth + 1)?
                    ))
                }
                MirRuntimeValue::Absent { .. } if clean => Some(
                    jet_foundation::StructuralDebug::jet_debug_optional(None),
                ),
                _ => None,
            }
        }
        RuntimeValueKind::Record => {
            let MirRuntimeValue::Struct { fields: values, .. } = value else {
                return None;
            };
            let special_field = match runtime_source_name(&descriptor.name) {
                "TextError" => Some("message"),
                "RangeError" => Some("reason"),
                _ => None,
            };
            if let Some(field_name) = special_field {
                let field = descriptor
                    .fields
                    .iter()
                    .find(|field| runtime_source_name(&field.source_name) == field_name)?;
                let MirRuntimeValue::String(value) =
                    structural_debug_record_value(values, field)?
                else {
                    return None;
                };
                return Some(value.clone());
            }
            let fields = descriptor
                .fields
                .iter()
                .filter(|field| !field.computed)
                .map(|field| {
                    let value = structural_debug_record_value(values, field)?;
                    let child = rt.runtime_type_descriptor(field.type_id).cloned()?;
                    Some(jet_foundation::StructuralDebug::JetDebugField {
                        name: runtime_source_name(&field.source_name).to_string(),
                        value: structural_debug_value(rt, value, &child, depth + 1)?,
                        storage_index: field.index,
                        redacted: structural_debug_field_redacted(
                            &descriptor.name,
                            &field.source_name,
                        ),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_foundation::StructuralDebug::jet_debug_record_fields(
                runtime_source_name(&descriptor.name),
                fields,
            ))
        }
        RuntimeValueKind::Enum => {
            let MirRuntimeValue::Enum {
                variant: active_variant,
                args,
                ..
            } = value
            else {
                return None;
            };
            let variant = descriptor.variants.iter().find(|variant| {
                variant.name == *active_variant
                    || variant.wire_name == *active_variant
                    || runtime_source_name(&variant.name) == runtime_source_name(active_variant)
            })?;
            let variant_name = runtime_source_name(&variant.name);
            if variant.fields.is_empty() {
                return Some(jet_foundation::StructuralDebug::jet_debug_variant(
                    variant_name,
                    None,
                ));
            }
            let visible = variant
                .fields
                .iter()
                .filter(|field| !field.computed)
                .collect::<Vec<_>>();
            if visible.is_empty() {
                return Some(jet_foundation::StructuralDebug::jet_debug_record_fields(
                    variant_name,
                    std::iter::empty(),
                ));
            }
            let tuple_style = variant.fields.len() == 1
                && !variant.fields[0].computed
                && variant.fields[0].source_name == "value";
            if tuple_style {
                let field = visible[0];
                let value = structural_debug_enum_value(args, field, 0)?;
                let child = rt.runtime_type_descriptor(field.type_id).cloned()?;
                return Some(jet_foundation::StructuralDebug::jet_debug_variant(
                    variant_name,
                    Some(structural_debug_value(rt, value, &child, depth + 1)?),
                ));
            }
            let fields = visible
                .iter()
                .enumerate()
                .map(|(visible_index, field)| {
                    let value = structural_debug_enum_value(args, field, visible_index)?;
                    let child = rt.runtime_type_descriptor(field.type_id).cloned()?;
                    Some(jet_foundation::StructuralDebug::JetDebugField {
                        name: runtime_source_name(&field.source_name).to_string(),
                        value: structural_debug_value(rt, value, &child, depth + 1)?,
                        storage_index: field.index,
                        redacted: structural_debug_field_redacted(
                            &descriptor.name,
                            &field.source_name,
                        ),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_foundation::StructuralDebug::jet_debug_record_fields(
                variant_name, fields,
            ))
        }
        RuntimeValueKind::Named
        | RuntimeValueKind::Handle
        | RuntimeValueKind::Closure
        | RuntimeValueKind::View
        | RuntimeValueKind::Iterator => None,
    }
}

fn jet_jit_display_nominal(handle: i64, type_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rendered = display_nominal_handle(rt, handle, type_id as u64, 0)
            .unwrap_or_else(|| "<invalid>".to_string());
        rt.heap.alloc_string(rendered)
    })
}

fn jet_jit_debug_nominal(handle: i64, type_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(rendered) = debug_nominal_handle(rt, handle, type_id as u64, 0) else {
            rt.set_host_fault("MIR nominal debug value has an invalid checked carrier");
            return 0;
        };
        rt.heap.alloc_string(rendered)
    })
}

fn jet_jit_str_begin() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_string())
}

fn jet_jit_str_push_lit(buf_id: i64, lit_id: i64) {
    with_runtime_mut(|rt| {
        let Some(lit) = rt.heap.clone_string(lit_id) else {
            return;
        };
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&lit);
        }
    });
}

fn jet_jit_str_push_i64(buf_id: i64, v: i64) {
    with_runtime_mut(|rt| {
        let text = rt.heap.int_to_string(v);
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&text);
        }
    });
}

fn jet_jit_str_push_f64(buf_id: i64, v: f64) {
    with_runtime_trap(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&jet_rt::display_f64(v));
        }
    });
}

fn jet_jit_str_push_compact_f64(buf_id: i64, v: f64) {
    with_runtime_trap(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&v.to_string());
        }
    });
}

fn jet_jit_str_push_bool(buf_id: i64, v: i8) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(if v == 0 { "false" } else { "true" });
        }
    });
}

fn jet_jit_str_push_char(buf_id: i64, v: i32) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            match char::from_u32(v as u32) {
                Some(ch) => buf.push(ch),
                None => buf.push('?'),
            }
        }
    });
}

fn jet_jit_str_push_str(buf_id: i64, str_id: i64) {
    with_runtime_mut(|rt| {
        let Some(s) = rt.heap.clone_string(str_id) else {
            return;
        };
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&s);
        }
    });
}

pub(crate) fn runtime_clone_value(
    runtime: &mut JitRuntime,
    value: i64,
    type_id: u64,
) -> Result<i64, String> {
    let descriptor = runtime
        .runtime_type_descriptor(type_id)
        .cloned()
        .ok_or_else(|| format!("JIT copy type descriptor {type_id} is unavailable"))?;
    runtime_clone_with_descriptor(runtime, value, &descriptor)
}

pub(crate) fn runtime_clone_with_descriptor(
    runtime: &mut JitRuntime,
    value: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, String> {
    match descriptor.kind {
        RuntimeValueKind::Named | RuntimeValueKind::Handle
            if descriptor.name == "Clock" =>
        {
            Ok(runtime.clock_clone(value))
        }
        RuntimeValueKind::List => runtime_clone_list(runtime, value, &descriptor),
        RuntimeValueKind::Map => runtime_clone_map(runtime, value, &descriptor),
        RuntimeValueKind::Record => runtime_clone_record(runtime, value, &descriptor),
        RuntimeValueKind::Enum => runtime_clone_enum(runtime, value, &descriptor),
        RuntimeValueKind::Option | RuntimeValueKind::Result => {
            runtime_clone_result(runtime, value, &descriptor)
        }
        RuntimeValueKind::Named | RuntimeValueKind::Handle
            if !descriptor.fields.is_empty() =>
        {
            runtime_clone_record(runtime, value, &descriptor)
        }
        RuntimeValueKind::Named | RuntimeValueKind::Handle
            if !descriptor.variants.is_empty() =>
        {
            runtime_clone_enum(runtime, value, &descriptor)
        }
        RuntimeValueKind::Unit
        | RuntimeValueKind::Int
        | RuntimeValueKind::Float
        | RuntimeValueKind::Bool
        | RuntimeValueKind::Char
        | RuntimeValueKind::String
        | RuntimeValueKind::Shared
        | RuntimeValueKind::Closure
        | RuntimeValueKind::View
        | RuntimeValueKind::Iterator
        | RuntimeValueKind::Named
        | RuntimeValueKind::Handle => Ok(value),
    }
}

fn runtime_clone_cell(
    runtime: &mut JitRuntime,
    value: &JetVal,
    type_id: u64,
) -> Result<JetVal, String> {
    match value {
        JetVal::Int(raw) => Ok(JetVal::Int(runtime_clone_value(runtime, *raw, type_id)?)),
        JetVal::RecordRef(raw) => Ok(JetVal::RecordRef(runtime_clone_value(
            runtime, *raw, type_id,
        )?)),
        JetVal::Record(fields) => {
            let descriptor = runtime
                .runtime_type_descriptor(type_id)
                .cloned()
                .ok_or_else(|| format!("JIT copy type descriptor {type_id} is unavailable"))?;
            if matches!(descriptor.kind, RuntimeValueKind::Record)
                || (matches!(descriptor.kind, RuntimeValueKind::Named | RuntimeValueKind::Handle)
                    && !descriptor.fields.is_empty())
            {
                let fields = runtime_clone_record_cells(runtime, fields.clone(), &descriptor)?;
                Ok(JetVal::Record(fields))
            } else if matches!(descriptor.kind, RuntimeValueKind::Enum)
                || (matches!(descriptor.kind, RuntimeValueKind::Named | RuntimeValueKind::Handle)
                    && !descriptor.variants.is_empty())
            {
                let fields = runtime_clone_enum_cells(runtime, fields.clone(), &descriptor)?;
                Ok(JetVal::Record(fields))
            } else {
                Ok(value.clone())
            }
        }
        JetVal::List(fields) => {
            let descriptor = runtime
                .runtime_type_descriptor(type_id)
                .cloned()
                .ok_or_else(|| format!("JIT copy type descriptor {type_id} is unavailable"))?;
            if !matches!(descriptor.kind, RuntimeValueKind::List) {
                return Ok(value.clone());
            }
            let element = descriptor
                .element
                .ok_or_else(|| format!("JIT copy `{}` has no element descriptor", descriptor.name))?;
            let mut cloned = Vec::with_capacity(fields.len());
            for field in fields {
                cloned.push(runtime_clone_cell(runtime, field, element)?);
            }
            Ok(JetVal::List(cloned))
        }
        _ => Ok(value.clone()),
    }
}

fn runtime_clone_record_cells(
    runtime: &mut JitRuntime,
    mut fields: Vec<JetVal>,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<Vec<JetVal>, String> {
    for field in &descriptor.fields {
        let slot = fields
            .get(field.index)
            .cloned()
            .ok_or_else(|| format!("JIT copy `{}` field {} is unavailable", descriptor.name, field.index))?;
        fields[field.index] = runtime_clone_cell(runtime, &slot, field.type_id)?;
    }
    Ok(fields)
}

fn runtime_clone_enum_cells(
    runtime: &mut JitRuntime,
    mut fields: Vec<JetVal>,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<Vec<JetVal>, String> {
    let discriminant = fields
        .first()
        .and_then(runtime_eq_discriminant)
        .ok_or_else(|| format!("JIT copy `{}` enum discriminant is invalid", descriptor.name))?;
    let variant = descriptor
        .variants
        .iter()
        .find(|variant| variant.discriminant == discriminant)
        .ok_or_else(|| format!("JIT copy `{}` enum discriminant is unknown", descriptor.name))?;
    for field in &variant.fields {
        let index = field.index + 1;
        let slot = fields
            .get(index)
            .cloned()
            .ok_or_else(|| format!("JIT copy `{}` enum field {} is unavailable", descriptor.name, index))?;
        fields[index] = runtime_clone_cell(runtime, &slot, field.type_id)?;
    }
    Ok(fields)
}

fn runtime_clone_record(
    runtime: &mut JitRuntime,
    value: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, String> {
    let fields = runtime
        .heap
        .clone_record_values(value)
        .ok_or_else(|| format!("JIT copy `{}` value is not a record", descriptor.name))?;
    let fields = runtime_clone_record_cells(runtime, fields, descriptor)?;
    Ok(runtime.heap.alloc_record_values(fields))
}

fn runtime_clone_enum(
    runtime: &mut JitRuntime,
    value: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, String> {
    let fields = runtime
        .heap
        .clone_record_values(value)
        .ok_or_else(|| format!("JIT copy `{}` value is not an enum", descriptor.name))?;
    let fields = runtime_clone_enum_cells(runtime, fields, descriptor)?;
    Ok(runtime.heap.alloc_record_values(fields))
}

fn runtime_clone_list(
    runtime: &mut JitRuntime,
    value: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, String> {
    let values = runtime
        .heap
        .clone_list_values(value)
        .ok_or_else(|| format!("JIT copy `{}` value is not a list", descriptor.name))?;
    let element = descriptor
        .element
        .ok_or_else(|| format!("JIT copy `{}` has no element descriptor", descriptor.name))?;
    let mut cloned = Vec::with_capacity(values.len());
    for value in values {
        cloned.push(runtime_clone_cell(runtime, &value, element)?);
    }
    Ok(runtime.heap.alloc_list_values(cloned))
}

fn runtime_clone_map(
    runtime: &mut JitRuntime,
    value: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, String> {
    let key = descriptor
        .key
        .and_then(|type_id| runtime.runtime_type_descriptor(type_id).cloned())
        .ok_or_else(|| format!("JIT copy `{}` has no key descriptor", descriptor.name))?;
    let value_type = descriptor
        .value
        .ok_or_else(|| format!("JIT copy `{}` has no value descriptor", descriptor.name))?;
    let length = runtime
        .heap
        .map_len(value)
        .ok_or_else(|| format!("JIT copy `{}` value is not a map", descriptor.name))?;
    let output = runtime.heap.alloc_empty_map();
    for index in 0..length {
        let key_value = runtime
            .heap
            .map_key_at(value, index)
            .ok_or_else(|| format!("JIT copy `{}` map key {} is unavailable", descriptor.name, index))?;
        let map_value = runtime
            .heap
            .map_value_at(value, index)
            .ok_or_else(|| format!("JIT copy `{}` map value {} is unavailable", descriptor.name, index))?;
        let map_value = runtime_clone_value(runtime, map_value, value_type)?;
        let inserted = match key.kind {
            RuntimeValueKind::String => runtime.heap.map_insert(output, key_value, map_value),
            RuntimeValueKind::Bool => runtime.heap.map_insert_bool(output, key_value != 0, map_value),
            RuntimeValueKind::Record
            | RuntimeValueKind::Enum
            | RuntimeValueKind::Map
            | RuntimeValueKind::List
            | RuntimeValueKind::Named
            | RuntimeValueKind::Handle => {
                runtime.heap.map_insert_composite(output, key_value, map_value)
            }
            RuntimeValueKind::Unit
            | RuntimeValueKind::Int
            | RuntimeValueKind::Float
            | RuntimeValueKind::Char
            | RuntimeValueKind::Shared
            | RuntimeValueKind::Option
            | RuntimeValueKind::Result
            | RuntimeValueKind::Closure
            | RuntimeValueKind::View
            | RuntimeValueKind::Iterator => runtime.heap.map_insert_int(output, key_value, map_value),
        };
        inserted.ok_or_else(|| format!("JIT copy `{}` map insertion failed", descriptor.name))?;
    }
    Ok(output)
}

fn runtime_clone_result(
    runtime: &mut JitRuntime,
    value: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, String> {
    if let Some(result) = jit_result(runtime, value) {
        let payload_type = if result.ok { descriptor.ok } else { descriptor.err };
        let payload = payload_type
            .map(|type_id| runtime_clone_value(runtime, result.bits as i64, type_id))
            .transpose()?
            .unwrap_or(result.bits as i64);
        return Ok(alloc_jit_result(runtime, result.ok, payload as u64));
    }
    if descriptor.kind == RuntimeValueKind::Option {
        if value == 0 {
            return Ok(0);
        }
        let payload_type = descriptor
            .ok
            .ok_or_else(|| format!("JIT copy `{}` has no option payload descriptor", descriptor.name))?;
        let payload = runtime_clone_value(runtime, value.wrapping_sub(1), payload_type)?;
        return Ok(payload.wrapping_add(1));
    }
    Err(format!("JIT copy `{}` value is not a result", descriptor.name))
}

fn jet_jit_typed_clone(value: i64, type_id: i64) -> i64 {
    with_runtime_result(value, |runtime| {
        match runtime_clone_value(runtime, value, type_id as u64) {
            Ok(value) => value,
            Err(message) => {
                runtime.set_host_fault(&message);
                value
            }
        }
    })
}

#[derive(Clone, Copy)]
enum RuntimeEqSlot<'a> {
    Raw(i64),
    Cell(&'a JetVal),
}

enum RuntimeEqInt<'a> {
    Raw(i64),
    Big(&'a jet_foundation::Numeric::CtBigInt),
}

fn runtime_eq_descriptor(
    runtime: &JitRuntime,
    type_id: u64,
) -> Result<&RuntimeTypeDescriptor, String> {
    runtime
        .runtime_type_descriptor(type_id)
        .ok_or_else(|| format!("JIT equality type descriptor {type_id} is unavailable"))
}

fn runtime_eq_list_carrier(value: &JetVal) -> bool {
    matches!(
        value,
        JetVal::IntList(_) | JetVal::List(_) | JetVal::UninitList { .. }
    )
}

fn runtime_eq_record_carrier(value: &JetVal) -> bool {
    matches!(value, JetVal::Record(_))
}

fn runtime_eq_list_len(value: &JetVal) -> Option<usize> {
    match value {
        JetVal::IntList(values) => Some(values.len()),
        JetVal::List(values) => Some(values.len()),
        JetVal::UninitList { values, .. } => Some(values.len()),
        _ => None,
    }
}

fn runtime_eq_list_slot<'a>(value: &'a JetVal, index: usize) -> Option<RuntimeEqSlot<'a>> {
    match value {
        JetVal::IntList(values) => values.get(index).copied().map(RuntimeEqSlot::Raw),
        JetVal::List(values) | JetVal::UninitList { values, .. } => {
            values.get(index).map(RuntimeEqSlot::Cell)
        }
        _ => None,
    }
}

fn runtime_eq_handle(slot: RuntimeEqSlot<'_>) -> Option<i64> {
    match slot {
        RuntimeEqSlot::Raw(value) => Some(value),
        RuntimeEqSlot::Cell(JetVal::Int(value) | JetVal::RecordRef(value)) => Some(*value),
        RuntimeEqSlot::Cell(_) => None,
    }
}

fn runtime_eq_int_slot(slot: RuntimeEqSlot<'_>) -> Result<RuntimeEqInt<'_>, String> {
    match slot {
        RuntimeEqSlot::Raw(value) => Ok(RuntimeEqInt::Raw(value)),
        RuntimeEqSlot::Cell(JetVal::Int(value)) => Ok(RuntimeEqInt::Raw(*value)),
        RuntimeEqSlot::Cell(JetVal::ExactInt(value)) => Ok(RuntimeEqInt::Big(value)),
        RuntimeEqSlot::Cell(_) => Err("JIT equality integer field has an invalid carrier".to_string()),
    }
}

fn runtime_eq_fixed_int(
    left: RuntimeEqSlot<'_>,
    right: RuntimeEqSlot<'_>,
    width: RuntimeIntegerWidth,
) -> Result<bool, String> {
    if width.bits == 0 || width.bits > 64 {
        return Err(format!("JIT equality has invalid integer width {}", width.bits));
    }
    let RuntimeEqInt::Raw(left) = runtime_eq_int_slot(left)? else {
        return Err("JIT equality fixed integer field has an exact carrier".to_string());
    };
    let RuntimeEqInt::Raw(right) = runtime_eq_int_slot(right)? else {
        return Err("JIT equality fixed integer field has an exact carrier".to_string());
    };
    if width.bits == 64 {
        return Ok(left as u64 == right as u64);
    }
    let mask = (1_u64 << u32::from(width.bits)) - 1;
    Ok((left as u64 & mask) == (right as u64 & mask))
}

fn runtime_eq_int(
    left: RuntimeEqSlot<'_>,
    right: RuntimeEqSlot<'_>,
    width: Option<RuntimeIntegerWidth>,
) -> Result<bool, String> {
    if let Some(width) = width {
        return runtime_eq_fixed_int(left, right, width);
    }
    match (runtime_eq_int_slot(left)?, runtime_eq_int_slot(right)?) {
        (RuntimeEqInt::Raw(left), RuntimeEqInt::Raw(right)) => {
            Ok(jet_rt::exact_int_compare(left, right) == 0)
        }
        (RuntimeEqInt::Big(left), RuntimeEqInt::Big(right)) => {
            Ok(left.compare(right) == std::cmp::Ordering::Equal)
        }
        (RuntimeEqInt::Big(left), RuntimeEqInt::Raw(right))
        | (RuntimeEqInt::Raw(right), RuntimeEqInt::Big(left)) => {
            Ok(left.compare(&jet_rt::exact_int_value(right)) == std::cmp::Ordering::Equal)
        }
    }
}

fn runtime_eq_float(slot: RuntimeEqSlot<'_>) -> Result<f64, String> {
    match slot {
        RuntimeEqSlot::Raw(value) => Ok(f64::from_bits(value as u64)),
        RuntimeEqSlot::Cell(JetVal::Float(value)) => Ok(*value),
        RuntimeEqSlot::Cell(_) => Err("JIT equality float field has an invalid carrier".to_string()),
    }
}

fn runtime_eq_bool(slot: RuntimeEqSlot<'_>) -> Result<bool, String> {
    match slot {
        RuntimeEqSlot::Raw(value) => Ok(value != 0),
        RuntimeEqSlot::Cell(JetVal::Bool(value)) => Ok(*value),
        RuntimeEqSlot::Cell(JetVal::Int(value)) => Ok(*value != 0),
        RuntimeEqSlot::Cell(_) => Err("JIT equality Bool field has an invalid carrier".to_string()),
    }
}

fn runtime_eq_char(slot: RuntimeEqSlot<'_>) -> Result<char, String> {
    let value = match slot {
        RuntimeEqSlot::Raw(value) => value,
        RuntimeEqSlot::Cell(JetVal::Char(value)) => return Ok(*value),
        RuntimeEqSlot::Cell(JetVal::Int(value)) => *value,
        RuntimeEqSlot::Cell(_) => {
            return Err("JIT equality Char field has an invalid carrier".to_string())
        }
    };
    char::from_u32(value as u32)
        .ok_or_else(|| "JIT equality Char field has an invalid scalar value".to_string())
}

fn runtime_eq_string<'a>(runtime: &'a JitRuntime, slot: RuntimeEqSlot<'a>) -> Option<&'a str> {
    match slot {
        RuntimeEqSlot::Raw(value) => runtime.heap.get_string(value),
        RuntimeEqSlot::Cell(JetVal::String(value)) => Some(value.as_str()),
        RuntimeEqSlot::Cell(JetVal::StringView { owner, start, end }) => {
            runtime.heap.get_string(*owner)?.get(*start..*end)
        }
        RuntimeEqSlot::Cell(JetVal::Int(value) | JetVal::RecordRef(value)) => {
            runtime.heap.get_string(*value)
        }
        RuntimeEqSlot::Cell(_) => None,
    }
}


fn runtime_eq_discriminant(value: &JetVal) -> Option<i64> {
    match value {
        JetVal::Int(value) | JetVal::RecordRef(value) => Some(*value),
        _ => None,
    }
}

fn runtime_eq_list_handles(
    runtime: &JitRuntime,
    left: i64,
    right: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    let left = runtime
        .heap
        .list_value(left)
        .ok_or_else(|| format!("JIT equality `{}` left value is not a list", descriptor.name))?;
    let right = runtime
        .heap
        .list_value(right)
        .ok_or_else(|| format!("JIT equality `{}` right value is not a list", descriptor.name))?;
    runtime_eq_list_carriers(runtime, left, right, descriptor)
}

fn runtime_eq_list_carriers(
    runtime: &JitRuntime,
    left: &JetVal,
    right: &JetVal,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    let left_len = runtime_eq_list_len(left)
        .ok_or_else(|| format!("JIT equality `{}` left value is not a list", descriptor.name))?;
    let right_len = runtime_eq_list_len(right)
        .ok_or_else(|| format!("JIT equality `{}` right value is not a list", descriptor.name))?;
    if left_len != right_len {
        return Ok(false);
    }
    let element = descriptor
        .element
        .ok_or_else(|| format!("JIT equality `{}` has no element descriptor", descriptor.name))?;
    for index in 0..left_len {
        let left = runtime_eq_list_slot(left, index)
            .ok_or_else(|| format!("JIT equality `{}` left slot is unavailable", descriptor.name))?;
        let right = runtime_eq_list_slot(right, index)
            .ok_or_else(|| format!("JIT equality `{}` right slot is unavailable", descriptor.name))?;
        if !runtime_eq_slots(runtime, left, right, element)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn runtime_eq_list_slots(
    runtime: &JitRuntime,
    left: RuntimeEqSlot<'_>,
    right: RuntimeEqSlot<'_>,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    if let (RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right)) = (left, right) {
        if runtime_eq_list_carrier(left) && runtime_eq_list_carrier(right) {
            return runtime_eq_list_carriers(runtime, left, right, descriptor);
        }
    }
    let left = runtime_eq_handle(left)
        .ok_or_else(|| format!("JIT equality `{}` left list handle is invalid", descriptor.name))?;
    let right = runtime_eq_handle(right)
        .ok_or_else(|| format!("JIT equality `{}` right list handle is invalid", descriptor.name))?;
    runtime_eq_list_handles(runtime, left, right, descriptor)
}

fn runtime_eq_record_handles(
    runtime: &JitRuntime,
    left: i64,
    right: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    if descriptor.fields.is_empty() {
        return Ok(true);
    }
    for field in &descriptor.fields {
        let left = runtime
            .heap
            .record_get(left, field.index as i64)
            .ok_or_else(|| format!("JIT equality `{}` left field is unavailable", descriptor.name))?;
        let right = runtime
            .heap
            .record_get(right, field.index as i64)
            .ok_or_else(|| format!("JIT equality `{}` right field is unavailable", descriptor.name))?;
        if !runtime_eq_slots(runtime, RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right), field.type_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn runtime_eq_record_carriers(
    runtime: &JitRuntime,
    left: &JetVal,
    right: &JetVal,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    let (JetVal::Record(left), JetVal::Record(right)) = (left, right) else {
        return Err(format!("JIT equality `{}` value is not a record", descriptor.name));
    };
    if left.len() != right.len() || left.len() != descriptor.fields.len() {
        return Ok(false);
    }
    for field in &descriptor.fields {
        let left = left
            .get(field.index)
            .ok_or_else(|| format!("JIT equality `{}` left field is unavailable", descriptor.name))?;
        let right = right
            .get(field.index)
            .ok_or_else(|| format!("JIT equality `{}` right field is unavailable", descriptor.name))?;
        if !runtime_eq_slots(runtime, RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right), field.type_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn runtime_eq_record_slots(
    runtime: &JitRuntime,
    left: RuntimeEqSlot<'_>,
    right: RuntimeEqSlot<'_>,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    if let (RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right)) = (left, right) {
        if runtime_eq_record_carrier(left) && runtime_eq_record_carrier(right) {
            return runtime_eq_record_carriers(runtime, left, right, descriptor);
        }
    }
    let left = runtime_eq_handle(left)
        .ok_or_else(|| format!("JIT equality `{}` left record handle is invalid", descriptor.name))?;
    let right = runtime_eq_handle(right)
        .ok_or_else(|| format!("JIT equality `{}` right record handle is invalid", descriptor.name))?;
    runtime_eq_record_handles(runtime, left, right, descriptor)
}

fn runtime_eq_enum_handles(
    runtime: &JitRuntime,
    left: i64,
    right: i64,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    let left_discriminant = runtime
        .heap
        .record_get_int(left, 0)
        .ok_or_else(|| format!("JIT equality `{}` left enum discriminant is invalid", descriptor.name))?;
    let right_discriminant = runtime
        .heap
        .record_get_int(right, 0)
        .ok_or_else(|| format!("JIT equality `{}` right enum discriminant is invalid", descriptor.name))?;
    if left_discriminant != right_discriminant {
        return Ok(false);
    }
    let variant = descriptor
        .variants
        .iter()
        .find(|variant| variant.discriminant == left_discriminant)
        .ok_or_else(|| format!("JIT equality `{}` enum discriminant is unknown", descriptor.name))?;
    for field in &variant.fields {
        let left = runtime
            .heap
            .record_get(left, (field.index + 1) as i64)
            .ok_or_else(|| format!("JIT equality `{}` left payload is unavailable", descriptor.name))?;
        let right = runtime
            .heap
            .record_get(right, (field.index + 1) as i64)
            .ok_or_else(|| format!("JIT equality `{}` right payload is unavailable", descriptor.name))?;
        if !runtime_eq_slots(runtime, RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right), field.type_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn runtime_eq_enum_carriers(
    runtime: &JitRuntime,
    left: &JetVal,
    right: &JetVal,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    let (JetVal::Record(left), JetVal::Record(right)) = (left, right) else {
        return Err(format!("JIT equality `{}` value is not an enum", descriptor.name));
    };
    if left.len() != right.len() {
        return Ok(false);
    }
    let left_discriminant = left
        .first()
        .and_then(runtime_eq_discriminant)
        .ok_or_else(|| format!("JIT equality `{}` left enum discriminant is invalid", descriptor.name))?;
    let right_discriminant = right
        .first()
        .and_then(runtime_eq_discriminant)
        .ok_or_else(|| format!("JIT equality `{}` right enum discriminant is invalid", descriptor.name))?;
    if left_discriminant != right_discriminant {
        return Ok(false);
    }
    let variant = descriptor
        .variants
        .iter()
        .find(|variant| variant.discriminant == left_discriminant)
        .ok_or_else(|| format!("JIT equality `{}` enum discriminant is unknown", descriptor.name))?;
    if left.len() != variant.fields.len() + 1 {
        return Ok(false);
    }
    for field in &variant.fields {
        let left = left
            .get(field.index + 1)
            .ok_or_else(|| format!("JIT equality `{}` left payload is unavailable", descriptor.name))?;
        let right = right
            .get(field.index + 1)
            .ok_or_else(|| format!("JIT equality `{}` right payload is unavailable", descriptor.name))?;
        if !runtime_eq_slots(runtime, RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right), field.type_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn runtime_eq_enum_slots(
    runtime: &JitRuntime,
    left: RuntimeEqSlot<'_>,
    right: RuntimeEqSlot<'_>,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<bool, String> {
    if let (RuntimeEqSlot::Cell(left), RuntimeEqSlot::Cell(right)) = (left, right) {
        if runtime_eq_record_carrier(left) && runtime_eq_record_carrier(right) {
            return runtime_eq_enum_carriers(runtime, left, right, descriptor);
        }
    }
    let left = runtime_eq_handle(left)
        .ok_or_else(|| format!("JIT equality `{}` left enum handle is invalid", descriptor.name))?;
    let right = runtime_eq_handle(right)
        .ok_or_else(|| format!("JIT equality `{}` right enum handle is invalid", descriptor.name))?;
    runtime_eq_enum_handles(runtime, left, right, descriptor)
}

fn runtime_eq_slots(
    runtime: &JitRuntime,
    left: RuntimeEqSlot<'_>,
    right: RuntimeEqSlot<'_>,
    type_id: u64,
) -> Result<bool, String> {
    let descriptor = runtime_eq_descriptor(runtime, type_id)?;
    match descriptor.kind {
        RuntimeValueKind::Unit => Ok(true),
        RuntimeValueKind::Int => runtime_eq_int(left, right, descriptor.integer_width),
        RuntimeValueKind::Float => {
            let left = runtime_eq_float(left)?;
            let right = runtime_eq_float(right)?;
            if descriptor.abi == RuntimeValueAbi::Float32 {
                Ok((left as f32) == (right as f32))
            } else {
                Ok(left == right)
            }
        }
        RuntimeValueKind::Bool => Ok(runtime_eq_bool(left)? == runtime_eq_bool(right)?),
        RuntimeValueKind::Char => Ok(runtime_eq_char(left)? == runtime_eq_char(right)?),
        RuntimeValueKind::String => Ok(runtime_eq_string(runtime, left) == runtime_eq_string(runtime, right)),
        RuntimeValueKind::List => runtime_eq_list_slots(runtime, left, right, descriptor),
        RuntimeValueKind::Record => runtime_eq_record_slots(runtime, left, right, descriptor),
        RuntimeValueKind::Enum => runtime_eq_enum_slots(runtime, left, right, descriptor),
        RuntimeValueKind::Named => Err(format!(
            "JIT equality `{}` has no resolved structural descriptor",
            descriptor.name
        )),
        kind => Err(format!("JIT equality does not support {kind:?} values")),
    }
}

fn runtime_eq_value(
    runtime: &JitRuntime,
    left: i64,
    right: i64,
    type_id: u64,
) -> Result<bool, String> {
    runtime_eq_slots(
        runtime,
        RuntimeEqSlot::Raw(left),
        RuntimeEqSlot::Raw(right),
        type_id,
    )
}

fn jet_jit_clock_clone(handle: i64) -> i64 {
    with_runtime_result(0, |runtime| runtime.clock_clone(handle))
}

fn jet_jit_typed_eq(left: i64, right: i64, type_id: i64) -> i8 {
    with_runtime_result(0, |runtime| {
        match runtime_eq_value(runtime, left, right, type_id as u64) {
            Ok(equal) => i8::from(equal),
            Err(message) => {
                runtime.set_host_fault(&message);
                0
            }
        }
    })
}

fn jet_jit_str_eq(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| match (rt.heap.get_string(a), rt.heap.get_string(b)) {
        (Some(x), Some(y)) => i8::from(x == y),
        _ => 0,
    })
}

fn jet_jit_str_order(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let left = rt.heap.get_string(a).expect("MIR String comparison left handle");
        let right = rt.heap.get_string(b).expect("MIR String comparison right handle");
        crate::Text::text_rt::compare(left, right)
    })
}

fn jet_jit_str_contains(hay: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        match (rt.heap.get_string(hay), rt.heap.get_string(needle)) {
            (Some(h), Some(n)) => {
                i8::from(crate::Collections::collection_semantics::jet_string_contains(h, n))
            }
            _ => 0,
        }
    })
}

fn jet_jit_str_starts_with(hay: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        match (rt.heap.get_string(hay), rt.heap.get_string(needle)) {
            (Some(h), Some(n)) => i8::from(h.starts_with(n)),
            _ => 0,
        }
    })
}
fn jet_jit_str_repeat(text_id: i64, count: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(text_id).unwrap_or_default();
        rt.heap.alloc_string(text.repeat(count.max(0) as usize))
    })
}


fn jet_jit_str_ends_with(hay: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        match (rt.heap.get_string(hay), rt.heap.get_string(needle)) {
            (Some(h), Some(n)) => i8::from(h.ends_with(n)),
            _ => 0,
        }
    })
}

fn jet_jit_str_len(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        rt.heap
            .get_string(id)
            .map(jet_rt::string_len_chars)
            .unwrap_or(0)
    })
}

fn jet_jit_str_byte_len(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        rt.heap.get_string(id).map(|s| s.len() as i64).unwrap_or(0)
    })
}
fn jet_jit_str_is_empty(id: i64) -> i8 {
    i8::from(jet_jit_str_len(id) == 0)
}


fn jet_jit_str_is_ascii(id: i64) -> i8 {
    with_runtime_result(0, |rt| {
        i8::from(
            rt.heap
                .get_string(id)
                .map(|s| s.is_ascii())
                .unwrap_or(false),
        )
    })
}

/// `core.text.scalars` — list of one-scalar strings (AOT `Vec<String>`).
fn jet_jit_str_scalar_strings(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for ch in text.chars() {
            let sid = rt.heap.alloc_string(ch.to_string());
            rt.heap
                .list_push_int(list, sid)
                .expect("jit str scalar strings: bad list handle");
        }
        list
    })
}

fn jet_jit_str_clone(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_str_trim(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(jet_rt::string_trim)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_str_to_upper(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(jet_rt::string_to_upper)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_str_to_lower(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(jet_rt::string_to_lower)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_str_to_ascii_upper(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(Text::text_rt::ascii_upper)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_str_to_ascii_lower(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(Text::text_rt::ascii_lower)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_str_replace(id: i64, from_id: i64, to_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let from = rt.heap.clone_string(from_id).unwrap_or_default();
        let to = rt.heap.clone_string(to_id).unwrap_or_default();
        rt.heap
            .alloc_string(jet_rt::string_replace(&text, &from, &to))
    })
}

fn jet_jit_str_lines(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for line in text.lines() {
            let sid = rt.heap.alloc_string(line.to_string());
            rt.heap
                .list_push_int(list, sid)
                .expect("jit str lines: bad list handle");
        }
        list
    })
}

/// `String.split` JIT host: same piece sequence as AOT `jet_iter_string_split`.
///
/// AOT returns a lazy `JetIter<String>`; Cranelift host shims can only pass i64
/// handles, so this eagerly materializes that sequence into a list handle typed
/// as `Iter<String>`. Observable values for split + adapters + `to_list` match
/// AOT; true pull-based laziness waits on an Iter-capable JIT ABI.
fn jet_jit_str_split(id: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        // Match AOT `jet_iter_string_split` / Rust `str::split` piece order
        // (including empty-sep Char split with leading/trailing empties).
        for part in text.split(&sep) {
            let sid = rt.heap.alloc_string(part.to_string());
            rt.heap
                .list_push_int(list, sid)
                .expect("jit str split: bad list handle");
        }
        list
    })
}

fn jet_jit_str_rsplit(id: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        if sep.is_empty() {
            for part in text.split(&sep) {
                let sid = rt.heap.alloc_string(part.to_string());
                rt.heap
                    .list_push_int(list, sid)
                    .expect("jit str rsplit: bad list handle");
            }
        } else {
            let mut parts: Vec<String> = text.rsplit(&sep).map(|p| p.to_string()).collect();
            parts.reverse();
            for part in parts {
                let sid = rt.heap.alloc_string(part);
                rt.heap
                    .list_push_int(list, sid)
                    .expect("jit str rsplit: bad list handle");
            }
        }
        list
    })
}

fn jet_jit_str_chars(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for ch in text.chars() {
            rt.heap
                .list_push_int(list, ch as i64)
                .expect("jit str chars: bad list handle");
        }
        list
    })
}

fn jet_jit_str_bytes(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let bytes = rt.heap
            .get_string(id)
            .expect("checked String.bytes receiver")
            .bytes()
            .map(i64::from)
            .collect();
        rt.heap.alloc_int_list(bytes)
    })
}

fn jet_jit_str_from_bytes(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(id).unwrap_or(0);
        let bytes = (0..len)
            .map(|index| rt.heap.list_get_int(id, index).unwrap_or_default() as u8)
            .collect::<Vec<_>>();
        match string_bytes_semantics::jet_string_decode_utf8(&bytes) {
            Ok(text) => {
                let value = rt.heap.alloc_string(text);
                alloc_jit_result(rt, true, value as u64)
            }
            Err(message) => {
                let error = rt.heap.alloc_record(1);
                let message = rt.heap.alloc_string(message);
                let _ = rt.heap.record_set_string(error, 0, message);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_str_from_bytes_lossy(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(id).unwrap_or(0);
        let bytes = (0..len)
            .map(|index| rt.heap.list_get_int(id, index).unwrap_or_default() as u8)
            .collect::<Vec<_>>();
        let text = string_bytes_semantics::jet_string_decode_utf8_lossy(&bytes);
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_list_join(list: i64, sep_id: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let len = sequence_len(rt, list).unwrap_or(0);
        let mut parts = Vec::new();
        for index in 0..len {
            let part = match kind {
                1 => {
                    let value = sequence_get_int(rt, list, index).unwrap_or(0);
                    rt.heap.clone_string(value).unwrap_or_default()
                }
                4 | 7 => sequence_get_float(rt, list, index)
                    .unwrap_or(0.0)
                    .to_string(),
                5 => {
                    if sequence_get_int(rt, list, index).unwrap_or(0) != 0 {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }
                _ => sequence_get_int(rt, list, index)
                    .unwrap_or(0)
                    .to_string(),
            };
            parts.push(part);
        }
        rt.heap.alloc_string(parts.join(&sep))
    })
}

fn jet_jit_slice_range_value(id: i64, range: i64, _file: i64, line: i32) -> i64 {
    let (start, end, exclusive) = Concurrency::with_runtime_mut(|rt| {
        let start = rt.heap.record_get_int(range, 0).unwrap_or(0);
        let end = rt.heap.record_get_int(range, 1).unwrap_or(0);
        let exclusive = rt
            .heap
            .record_get_bool(range, 2)
            .or_else(|| rt.heap.record_get_int(range, 2).map(|value| value != 0))
            .unwrap_or(false);
        (start, end, exclusive)
    });
    jet_jit_str_slice_value(id, start, end, exclusive, line.max(0) as u32)
}

fn jet_jit_str_slice_direct(id: i64, start: i64, end: i64, _file: i64, line: i32) -> i64 {
    jet_jit_str_slice_value(id, start, end, false, line.max(0) as u32)
}

fn jet_jit_str_after(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        rt.heap.alloc_string(jet_rt::string_after(&text, &sep))
    })
}

fn jet_jit_str_before(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        rt.heap.alloc_string(jet_rt::string_before(&text, &sep))
    })
}

fn jet_jit_str_trim_view(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let Some(text) = rt.heap.get_string(id) else {
            return 0;
        };
        let start = text.len() - text.trim_start().len();
        let end = text.trim_end().len();
        rt.heap.alloc_string_view(id, start, end).unwrap_or(0)
    })
}

fn jet_jit_str_after_view(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let Some(text) = rt.heap.get_string(id) else {
            return 0;
        };
        let start = text.find(&sep).map_or(0, |index| index + sep.len());
        rt.heap
            .alloc_string_view(id, start, text.len())
            .unwrap_or(0)
    })
}

fn jet_jit_str_before_view(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let Some(text) = rt.heap.get_string(id) else {
            return 0;
        };
        let end = text.find(&sep).unwrap_or(text.len());
        rt.heap.alloc_string_view(id, 0, end).unwrap_or(0)
    })
}

fn jet_jit_str_slice_value(id: i64, start: i64, end: i64, exclusive: bool, line: u32) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        match string_slice_kernel::jet_string_slice_value(&text, start, end, exclusive) {
            Ok(sliced) => rt.heap.alloc_string(sliced),
            Err(message) => {
                rt.set_runtime_stop("E3001", line, &message);
                0
            }
        }
    })
}

/// Inclusive string slice (`s.slice(lo, hi)`). Same start/end = one char.
fn jet_jit_str_slice(id: i64, start: i64, end: i64) -> i64 {
    jet_jit_str_slice_value(id, start, end, false, 0)
}

fn jet_jit_str_slice_range(id: i64, start: i64, end: i64, exclusive: i8, line: i32) -> i64 {
    jet_jit_str_slice_value(id, start, end, exclusive != 0, line.max(0) as u32)
}

fn jet_jit_rich_panic(
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    msg: i64,
    locals: i64,
) -> i64 {
    let in_task = Concurrency::in_jit_task();
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let src_line = rt.heap.clone_string(src_line).unwrap_or_default();
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let locals = rt.heap.clone_string(locals).unwrap_or_default();
        Concurrency::set_rich_panic_reason(msg.clone());
        // I9: Cranelift marshals the same failure facts into the canonical
        // Prelude receipt writer; it does not own a second receipt path.
        let _ = jet_codegen::development_receipt::jet_production_failure_receipt_write(
            "E3001",
            &file,
            line.max(0) as u32,
            &fn_name,
        );
        let report = contract_kernel::jet_runtime_stop_report(
            "E3001",
            &file,
            line.max(0) as u32,
            &fn_name,
            &src_line,
            col.max(1) as u32,
            caret.max(1) as u32,
            &msg,
            &locals,
        );
        if in_task {
            // A child failure is a typed TaskFailure. Its trap must remain
            // thread-local: the resident runtime is shared with the parent,
            // and a shared trap would make the parent skip unrelated joins.
            Concurrency::set_rich_panic_report(report.rendered);
            Concurrency::set_local_rich_panic();
        } else {
            // The report is already rendered, so it takes the pre-rendered
            // stop seam (`jet_jit_contract_fail` uses the same one). Writing
            // `exit_code` by hand and then calling `set_trap` recorded the
            // report but left `trapped` empty: `set_runtime_stop` keeps the
            // FIRST stop and returns early once `exit_code` is set, so
            // `store_trap` never ran, `jet_jit_is_trapped` kept answering 0,
            // and generated code sailed past `emit_trap_check` — the stop
            // reported but did not stop (I9: AOT's `jet_panic_rich` is `!`
            // and the evaluator raises).
            rt.set_rendered_runtime_stop(report.rendered, report.exit_code);
        }
        0
    })
}

fn jet_jit_require(
    condition: i64,
    msg: i64,
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    locals: i64,
) -> i64 {
    if condition != 0 {
        return 0;
    }
    jet_jit_rich_panic(file, line, fn_name, src_line, col, caret, msg, locals)
}
fn jet_jit_require_eq(
    condition: i64,
    left_debug: i64,
    right_debug: i64,
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    locals: i64,
) -> i64 {
    if condition != 0 {
        return 0;
    }
    let msg = Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.heap.clone_string(left_debug) else {
            rt.set_host_fault("MIR require_eq left debug value has an invalid handle");
            return 0;
        };
        let Some(right) = rt.heap.clone_string(right_debug) else {
            rt.set_host_fault("MIR require_eq right debug value has an invalid handle");
            return 0;
        };
        rt.heap
            .alloc_string(format!("expected: {right}, got: {left}"))
    });
    jet_jit_rich_panic(file, line, fn_name, src_line, col, caret, msg, locals)
}

fn jet_jit_test_require_eq(
    condition: i64,
    left_debug: i64,
    right_debug: i64,
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    locals: i64,
) -> i64 {
    if condition != 0 {
        return jet_jit_result_new_i64(1, 0);
    }
    let msg = Concurrency::with_runtime_mut(|rt| {
        let Some(left) = rt.heap.clone_string(left_debug) else {
            rt.set_host_fault("MIR require_eq left debug value has an invalid handle");
            return 0;
        };
        let Some(right) = rt.heap.clone_string(right_debug) else {
            rt.set_host_fault("MIR require_eq right debug value has an invalid handle");
            return 0;
        };
        rt.heap
            .alloc_string(format!("expected: {right}, got: {left}"))
    });
    jet_jit_test_failure_result(file, line, fn_name, src_line, col, caret, msg, locals)
}



fn jet_jit_test_failure_result(
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    msg: i64,
    locals: i64,
) -> i64 {
    let rendered = Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let src_line = rt.heap.clone_string(src_line).unwrap_or_default();
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let locals = rt.heap.clone_string(locals).unwrap_or_default();
        contract_kernel::jet_runtime_stop_report(
            "E3001",
            &file,
            line.max(0) as u32,
            &fn_name,
            &src_line,
            col.max(1) as u32,
            caret.max(1) as u32,
            &msg,
            &locals,
        )
        .rendered
    });
    let rendered = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(rendered));
    jet_jit_result_new_i64(0, rendered)
}

fn jet_jit_test_require(
    condition: i64,
    msg: i64,
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    locals: i64,
) -> i64 {
    if condition != 0 {
        jet_jit_result_new_i64(1, 0)
    } else {
        jet_jit_test_failure_result(file, line, fn_name, src_line, col, caret, msg, locals)
    }
}


fn jet_jit_debug_i64(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(crate::Collections::jet_debug_i64(value)))
}

fn jet_jit_debug_f64(value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(crate::Collections::jet_debug_f64(value)))
}

fn jet_jit_debug_f32(value: f32) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(crate::Collections::jet_debug_f32(value)))
}

fn jet_jit_debug_bool(value: i8) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(crate::Collections::jet_debug_bool(value != 0)))
}

fn jet_jit_debug_char(value: i32) -> i64 {
    let Some(value) = char::from_u32(value as u32) else {
        Concurrency::with_runtime_mut(|rt| rt.set_host_fault("MIR Char local is not a Unicode scalar"));
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(crate::Collections::jet_debug_char(value)))
}

fn jet_jit_debug_string(value: i64) -> i64 {
    let Some(value) = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(value)) else {
        Concurrency::with_runtime_mut(|rt| rt.set_host_fault("MIR String local has an invalid handle"));
        return 0;
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(crate::Collections::jet_debug_string(value)))
}

fn jet_jit_debug_local_append(current: i64, name: i64, value: i64, first: i8) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(mut text) = rt.heap.clone_string(current) else {
            rt.set_host_fault("MIR panic locals accumulator has an invalid handle");
            return 0;
        };
        let Some(name) = rt.heap.clone_string(name) else {
            rt.set_host_fault("MIR panic local name has an invalid handle");
            return 0;
        };
        let Some(value) = rt.heap.clone_string(value) else {
            rt.set_host_fault("MIR panic local value has an invalid handle");
            return 0;
        };
        if first == 0 && !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(&name);
        text.push_str(" = ");
        text.push_str(&value);
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_todo_stop(line: i64, expected_type: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let expected_type = rt.heap.clone_string(expected_type).unwrap_or_default();
        let message = jet_foundation::Outcome::jet_todo_message(
            &rt.source_file,
            line.max(0) as u32,
            &expected_type,
        );
        rt.set_runtime_stop("E3011", line.max(0) as u32, &message);
        0
    })
}

fn jet_jit_trap_panic(_unused: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap("panic");
        0
    })
}

fn jet_jit_index_miss(line: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_runtime_stop("E3001", line.max(0) as u32, "index miss");
        0
    })
}

/// D-FAIL-TIER1: the JIT only marshals contract values.  The predicate and
/// rendered report are the same Prelude functions used by AOT and TIR-eval.
fn jet_jit_contract_check(condition: i8) -> i8 {
    i8::from(contract_kernel::jet_contract_check(condition != 0))
}

fn jet_jit_contract_fail(msg: i64, file: i64, line: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let clause = if kind == 0 { "Pre" } else { "Post" };
        let report = contract_kernel::jet_contract_report(clause, &msg, &file, line as u32);
        rt.set_rendered_runtime_stop(report.rendered, report.exit_code);
        0
    })
}

/// E3002 frames ACCUMULATE here and are printed once at the entry boundary
/// (`resident.rs` -> `jet_journey_report`), exactly as the AOT Prelude's
/// `jet_trace_err` does. Pushing the frame onto `rt.stderr` here was a second
/// reporting policy: a failure later recovered by `??` still printed its
/// journey under the resident tier while AOT printed nothing (I9).
fn jet_jit_trace_err(file: i64, line: i64, fn_name: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        jet_foundation::Outcome::jet_journey_frame(&file, line as u32, &fn_name, String::new);
    });
}

fn jet_jit_trace_err_note(file: i64, line: i64, fn_name: i64, note: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let note = rt.heap.clone_string(note).unwrap_or_default();
        jet_foundation::Outcome::jet_journey_frame(&file, line as u32, &fn_name, || note);
    })
}

fn jet_jit_trace_reset() {
    jet_foundation::Outcome::jet_journey_reset();
}

fn jet_jit_parse_i64(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        match rt.heap.int_from_str(text.trim()) {
            Ok(value) => alloc_jit_result(rt, true, value as u64),
            Err(_) => {
                let error = rt
                    .heap
                    .alloc_string(format!("cannot parse `{text}` as an integer"));
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_parse_f64(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        match text.trim().parse::<f64>() {
            Ok(value) => alloc_jit_result(rt, true, value.to_bits()),
            Err(_) => {
                let error = rt
                    .heap
                    .alloc_string(format!("cannot parse `{text}` as a float"));
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}


fn jet_jit_numeric_try_i64(value: i64, source_signed: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match jet_foundation::NumericConversion::jet_numeric_try_from_fixed(
            value as u64,
            source_signed != 0,
            kind,
        ) {
            Ok(value) => alloc_jit_result(rt, true, value as u64),
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_numeric_try_int(value: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_try_from(value, kind) {
        Some(value) => alloc_jit_result(rt, true, value as u64),
        None => {
            let error = rt
                .heap
                .alloc_string(jet_foundation::NumericConversion::JET_NUMERIC_CONVERSION_ERROR);
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

fn jit_numeric_stop(rt: &mut JitRuntime, file: i64, line: i64, message: &str) {
    let Some(file) = rt.heap.get_string(file) else {
        rt.set_host_fault("checked numeric conversion has an invalid source file handle");
        return;
    };
    let report = contract_kernel::jet_runtime_stop_report(
        contract_kernel::JET_ARITHMETIC_CODE,
        file,
        line as u32,
        &rt.current_function,
        &rt.current_source_line,
        1,
        1,
        message,
        "",
    );
    rt.set_rendered_runtime_stop(report.rendered, report.exit_code);
}

fn jet_jit_numeric_checked_int(value: i64, kind: i64, file: i64, line: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match rt.heap.int_try_from(value, kind) {
        Some(value) => value as u64 as i64,
        None => {
            jit_numeric_stop(
                rt, file, line,
                jet_foundation::NumericConversion::JET_NUMERIC_CONVERSION_TRAP,
            );
            0
        }
    })
}

fn jet_jit_numeric_float_to_int(value: f64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match jet_foundation::NumericConversion::jet_numeric_float_to_int(value, kind) {
            Ok(value) => alloc_jit_result(rt, true, value as u64),
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_numeric_float_narrow(value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match jet_foundation::NumericConversion::jet_numeric_float_narrow(value) {
            Ok(value) => alloc_jit_result(rt, true, f64::from(value).to_bits()),
            Err(error) => {
                let error = rt.heap.alloc_string(error);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_numeric_checked_widen(raw: i64, source_signed: i64, target_f32: i64, file: i64, line: i64) -> f64 {
    Concurrency::with_runtime_mut(
        |rt| match jet_foundation::NumericConversion::jet_numeric_checked_widen(
            raw as u64,
            source_signed != 0,
            target_f32 != 0,
        ) {
            Some(value) => value,
            None => {
                jit_numeric_stop(rt, file, line, jet_foundation::NumericConversion::JET_NUMERIC_WIDEN_TRAP);
                0.0
            }
        },
    )
}

fn jet_jit_numeric_int_checked_widen(value: i64, target_f32: i64, file: i64, line: i64) -> f64 {
    Concurrency::with_runtime_mut(
        |rt| match rt.heap.int_checked_widen(value, target_f32 != 0) {
            Some(value) => value,
            None => {
                jit_numeric_stop(rt, file, line, jet_foundation::NumericConversion::JET_NUMERIC_WIDEN_TRAP);
                0.0
            }
        },
    )
}

fn jet_jit_distinct_range(value: i64, lo: i64, hi: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if value >= lo && value <= hi {
            alloc_jit_result(rt, true, value as u64)
        } else {
            let error = rt
                .heap
                .alloc_string("value is outside the distinct type's range");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

fn jet_jit_distinct_range_result(handle: i64, lo: i64, hi: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(result) = jit_result(rt, handle) else {
            return 0;
        };
        if !result.ok {
            return handle;
        }
        let value = result.bits as i64;
        if value >= lo && value <= hi {
            handle
        } else {
            let error = rt
                .heap
                .alloc_string("value is outside the distinct type's range");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

fn jet_jit_inline_range(value: i64, lo: i64, hi: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match inline_range_kernel::jet_inline_range_from_int(value, lo, hi) {
            Ok(value) => value,
            Err(message) => {
                rt.set_trap(&message);
                value
            }
        }
    })
}

fn jet_jit_inline_range_result(value: i64, lo: i64, hi: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match inline_range_kernel::jet_inline_range_from_int(value, lo, hi) {
            Ok(value) => alloc_jit_result(rt, true, value as u64),
            Err(message) => {
                let error = rt.heap.alloc_string(message);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_numeric_bit_count(value: i64, op: i64, width: i64) -> i64 {
    jet_foundation::NumericConversion::jet_numeric_bit_count(value, op, width)
}

fn jet_jit_numeric_int_bit_count(value: i64, op: i64, width: i64) -> i64 {
    let method = match op {
        0 => "count_ones",
        1 => "count_zeros",
        2 => "leading_zeros",
        _ => "trailing_zeros",
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .int_bit_count(value, width as u32, method)
            .unwrap_or(0)
    })
}

fn jet_jit_struct_new(n: i64) -> i64 {
    STRUCT_NEW_COUNT.with(|count| count.set(count.get() + 1));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(n as usize))
}
fn jet_jit_trait_object_tag(record: i64, type_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.trait_object_types
            .insert(record, MirTypeId(type_id as u64));
    });
    record
}

fn jet_jit_trait_object_type(record: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.trait_object_types
            .get(&record)
            .map(|type_id| type_id.0 as i64)
            .unwrap_or(-1)
    })
}

fn jet_jit_struct_assign(dst: i64, src: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_assign_from(dst, src);
    });
}

fn jet_jit_struct_get_i64(h: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .record_get_int(h, idx)
            .or_else(|| rt.heap.record_get_string(h, idx))
            .unwrap_or(0)
    })
}

fn jet_jit_struct_get_f64(h: i64, idx: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_float(h, idx).unwrap_or(0.0))
}

fn jet_jit_struct_get_bool(h: i64, idx: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(rt.heap.record_get_bool(h, idx).unwrap_or(false)))
}

fn jet_jit_struct_get_char(h: i64, idx: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .record_get_char(h, idx)
            .map(|c| c as i32)
            .unwrap_or(0)
    })
}

fn jet_jit_struct_field_address(h: i64, idx: i64, kind: i64) -> i64 {
    with_runtime_result(0, |rt| {
        match rt.heap.record_field_address(h, idx, kind) {
            Some(address) => address,
            None => {
                rt.set_host_fault("MIR native record field address has an invalid carrier");
                0
            }
        }
    })
}

fn jet_jit_pattern_capture_char(h: i64, idx: i64) -> i32 {
    with_runtime_result(0, |rt| {
        let Some(handle) = rt.heap.record_get_string(h, idx) else {
            rt.set_host_fault("MIR text pattern capture is not Char");
            return 0;
        };
        let Some(text) = rt.heap.clone_string(handle) else {
            rt.set_host_fault("MIR text pattern Char capture has an invalid string handle");
            return 0;
        };
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(value), None) => value as i32,
            _ => {
                rt.set_host_fault("MIR text pattern Char capture is not one scalar");
                0
            }
        }
    })
}


fn jet_jit_struct_get_str(h: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_string(h, idx).unwrap_or(0))
}

fn jet_jit_struct_set_i64(h: i64, idx: i64, v: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(h, idx, v);
    });
}

fn jet_jit_struct_set_record(h: i64, idx: i64, v: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_record(h, idx, v);
    });
}

fn jet_jit_struct_set_f64(h: i64, idx: i64, v: f64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_float(h, idx, v);
    });
}

fn jet_jit_struct_set_bool(h: i64, idx: i64, v: i8) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_bool(h, idx, v != 0);
    });
}

fn jet_jit_struct_set_char(h: i64, idx: i64, v: i32) {
    with_runtime_mut(|rt| {
        let Some(ch) = char::from_u32(v as u32) else {
            return;
        };
        let _ = rt.heap.record_set_char(h, idx, ch);
    });
}

fn jet_jit_struct_set_str(h: i64, idx: i64, v: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_string(h, idx, v);
    });
}

fn jet_jit_receipt_attach(
    value: i64,
    section_name: i64,
    type_name: i64,
    schema_digest: i64,
) {
    let encoded = Concurrency::with_runtime_string(|rt| {
        let section_name = rt
            .heap
            .clone_string(section_name)
            .ok_or_else(|| "receipt.attach received an invalid section name".to_string())?;
        let type_name = rt
            .heap
            .clone_string(type_name)
            .ok_or_else(|| "receipt.attach received an invalid type name".to_string())?;
        let schema_digest = rt
            .heap
            .clone_string(schema_digest)
            .ok_or_else(|| "receipt.attach received an invalid schema digest".to_string())?;
        let descriptor = rt
            .runtime_type_descriptor_by_name(&type_name)
            .cloned()
            .ok_or_else(|| {
                format!("receipt.attach has no checked runtime descriptor for `{type_name}`")
            })?;
        let tree = crate::Receipt::encode_jit_value(rt, value, &descriptor)?;
        Ok((section_name, type_name, schema_digest, tree))
    });
    match encoded {
        Ok((section_name, type_name, schema_digest, tree)) => {
            let payload = crate::Encoding::json_rt::render_datatree_json(&tree, false, 0);
            if payload.len() > 64 * 1024 * 1024 {
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_host_fault("receipt.attach payload exceeds 64 MiB");
                });
                return;
            }
            crate::Receipt::kernel::jet_receipt_attach_encoded(
                &payload,
                &section_name,
                &type_name,
                &schema_digest,
            );
        }
        Err(error) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_host_fault(format!("receipt.attach: {error}"));
            });
        }
    }
}

fn jet_jit_memo_probe(record: i64, slot: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(rt.memo_values.contains_key(&(record, slot))))
}

fn jet_jit_memo_get(record: i64, slot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.memo_values.get(&(record, slot)).copied().unwrap_or(0))
}

fn jet_jit_memo_put(record: i64, slot: i64, value: i64) {
    with_runtime_mut(|rt| {
        rt.memo_values.insert((record, slot), value);
    });
}

fn jet_jit_memo_clear(record: i64) {
    with_runtime_mut(|rt| {
        rt.memo_values.retain(|(owner, _), _| *owner != record);
    });
}

fn jet_jit_memo_clear_slot(record: i64, slot: i64) {
    with_runtime_mut(|rt| {
        rt.memo_values.remove(&(record, slot));
    });
}

/// D-MEMO1=A / I9: `f.cache()` reads the shared Prelude memo store for one
/// memoized function; no cache policy lives here. `name` is a string handle
/// for the Jet function name and `bound` its ratified bound, where a negative
/// word spells `bound: none`. The result is the four-slot `MemoStats` record
/// whose field order `types_meta`'s core table declares.
fn jet_jit_memo_stats(name: i64, bound: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let bound = usize::try_from(bound).ok();
        let stats = rt
            .memo_functions
            .entry(name)
            .or_insert_with(|| jet_codegen::memo::JetMemo::with_bound(bound))
            .stats();
        let record = rt.heap.alloc_record(4);
        let _ = rt.heap.record_set_int(record, 0, stats.hits);
        let _ = rt.heap.record_set_int(record, 1, stats.misses);
        let _ = rt.heap.record_set_int(record, 2, stats.size);
        let text = rt.heap.alloc_string(stats.bound);
        let _ = rt.heap.record_set_string(record, 3, text);
        record
    })
}

fn jet_jit_err_new(message: i64, code: i64, cause: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        use jet_foundation::Outcome::{jet_err, JetAbsent};

        let message = rt.heap.clone_string(message).unwrap_or_default();
        let code = if code == 0 {
            Err(JetAbsent)
        } else {
            rt.heap.clone_string(code - 1).ok_or(JetAbsent)
        };
        let cause = if cause == 0 {
            Err(JetAbsent)
        } else {
            let handle = cause - 1;
            rt.errors
                .get(handle.saturating_sub(1) as usize)
                .cloned()
                .ok_or(JetAbsent)
        };
        rt.errors.push(jet_err(message, code, cause));
        rt.errors.len() as i64
    })
}

fn jet_jit_err_from_message(message: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(message) = rt.heap.clone_string(message) else {
            rt.set_host_fault("default Err construction has an invalid message handle");
            return 0;
        };
        rt.errors.push(jet_foundation::Outcome::jet_err_from_message(message));
        rt.errors.len() as i64
    })
}

fn jet_jit_err_with_context_frame(
    handle: i64, file: i64, line: i64, function: i64, note: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (Some(error), Some(file), Some(function), Some(note)) = (
            jit_error(rt, handle),
            rt.heap.get_string(file),
            rt.heap.get_string(function),
            rt.heap.clone_string(note),
        ) else {
            rt.set_host_fault("default Err context has an invalid checked carrier");
            return 0;
        };
        let error = jet_foundation::Outcome::jet_err_with_context_frame(
            error, file, line as u32, function, note,
        );
        rt.errors.push(error);
        rt.errors.len() as i64
    })
}

fn jet_jit_entry_error_exit(handle: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = jit_error(rt, handle) else {
            rt.set_host_fault("error propagation exit has an invalid default Err carrier");
            return;
        };
        let report = jet_foundation::Outcome::jet_error_report(&error).render();
        rt.set_rendered_runtime_stop(report, 1);
    });
}

/// Apply a declared error conversion to the existing shared carrier. The JIT
/// owns only handle/string marshalling; the Prelude preserves every existing
/// structured field and adds the crossing identity/history.
fn jet_jit_err_apply_conversion(handle: i64, source: i64, target: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let source = rt.heap.clone_string(source).unwrap_or_default();
        let target = rt.heap.clone_string(target).unwrap_or_default();
        if let Some(error) = rt.errors.get_mut(handle.saturating_sub(1) as usize) {
            jet_foundation::Outcome::jet_err_apply_conversion(error, source, target);
        }
    })
}

fn jet_jit_err_add_context(handle: i64, text: i64, file: i64, line: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(text).unwrap_or_default();
        let file = rt.heap.clone_string(file).unwrap_or_default();
        if let Some(error) = rt.errors.get_mut(handle.saturating_sub(1) as usize) {
            jet_foundation::Outcome::jet_err_add_context(error, text, file, line as u32);
        }
    });
}

fn jet_jit_err_message(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = rt.errors.get(handle.saturating_sub(1) as usize) else {
            return 0;
        };
        rt.heap
            .alloc_string(jet_foundation::Outcome::jet_err_message(error))
    })
}

fn jet_jit_err_code(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = rt.errors.get(handle.saturating_sub(1) as usize) else {
            return 0;
        };
        match jet_foundation::Outcome::jet_err_code(error) {
            Ok(code) => rt.heap.alloc_string(code) + 1,
            Err(_) => 0,
        }
    })
}

fn jet_jit_err_cause(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = rt.errors.get(handle.saturating_sub(1) as usize) else {
            return 0;
        };
        match jet_foundation::Outcome::jet_err_cause(error) {
            Ok(cause) => {
                rt.errors.push(cause);
                rt.errors.len() as i64 + 1
            }
            Err(_) => 0,
        }
    })
}

fn alloc_measurement(rt: &mut JitRuntime, value: (f64, f64)) -> i64 {
    let handle = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_float(handle, 0, value.0);
    let _ = rt.heap.record_set_float(handle, 1, value.1);
    handle
}

fn read_measurement(rt: &mut JitRuntime, handle: i64) -> Option<(f64, f64)> {
    Some((
        rt.heap.record_get_float(handle, 0)?,
        rt.heap.record_get_float(handle, 1)?,
    ))
}

fn jet_jit_measurement_new(value: f64, uncertainty: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        alloc_measurement(
            rt,
            measurement_kernel::jet_measurement_kernel_new(value, uncertainty),
        )
    })
}

fn jet_jit_measurement_arithmetic(left: i64, right: i64, op: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (Some(left), Some(right)) = (read_measurement(rt, left), read_measurement(rt, right))
        else {
            rt.set_trap("the JIT received an invalid Measurement handle");
            return 0;
        };
        let value = match op {
            0 => measurement_kernel::jet_measurement_kernel_add(left, right),
            1 => measurement_kernel::jet_measurement_kernel_sub(left, right),
            2 => measurement_kernel::jet_measurement_kernel_mul(left, right),
            3 => measurement_kernel::jet_measurement_kernel_div(left, right),
            4 => measurement_kernel::jet_measurement_kernel_sqrt(left),
            _ => {
                rt.set_trap("the JIT received an invalid Measurement operation");
                return 0;
            }
        };
        alloc_measurement(rt, value)
    })
}

fn jet_jit_measurement_get(handle: i64, field: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap.record_get_float(handle, field).unwrap_or_else(|| {
            rt.set_trap("the JIT received an invalid Measurement handle");
            0.0
        })
    })
}

fn jet_jit_measurement_value(handle: i64) -> f64 {
    jet_jit_measurement_get(handle, 0)
}

fn jet_jit_measurement_uncertainty(handle: i64) -> f64 {
    jet_jit_measurement_get(handle, 1)
}

fn jet_jit_measurement_add(left: i64, right: i64) -> i64 {
    jet_jit_measurement_arithmetic(left, right, 0)
}

fn jet_jit_measurement_sub(left: i64, right: i64) -> i64 {
    jet_jit_measurement_arithmetic(left, right, 1)
}

fn jet_jit_measurement_mul(left: i64, right: i64) -> i64 {
    jet_jit_measurement_arithmetic(left, right, 2)
}

fn jet_jit_measurement_div(left: i64, right: i64) -> i64 {
    jet_jit_measurement_arithmetic(left, right, 3)
}

fn jet_jit_measurement_sqrt(handle: i64) -> i64 {
    jet_jit_measurement_arithmetic(handle, handle, 4)
}

fn jet_jit_measurement_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(value) = read_measurement(rt, handle) else {
            rt.set_trap("the JIT received an invalid Measurement handle");
            return 0;
        };
        let rendered = measurement_kernel::jet_measurement_kernel_show(value);
        rt.heap.alloc_string(rendered)
    })
}

const JIT_PERF_DEFAULT_FIDELITY_BITS: u32 = 1.0f32.to_bits();
// D-FIDELITY-API1=A: this signal is deliberately outside `JitRuntime` — it
// must survive `resident_teardown()` + a fresh `JitRuntime` (a "restart"),
// exactly like the AOT binary's process-global static survives every read/
// write for the life of that one running program. What must NOT happen is
// leaking one resident-JIT execution's override into a DIFFERENT one running
// concurrently on another thread (the actual bug: a process-wide
// `AtomicU32` let a parallel `cargo test` thread observe another thread's
// override mid-battery). Thread-local scoping keeps every restart/hot-swap/
// relaunch sequence within one session exactly as before while giving each
// independent resident-JIT session its own signal.
//
// The session is the thread that OWNS the session, and since 17d04068e that
// is no longer automatically the thread the signal is written on: an outermost
// `CraneliftBackend::run` hops onto a fresh `jet-compiler` worker per call, so
// a worker's storage spans one CALL, not one session. The cell below is
// therefore the owner's, and [`crate::on_compiler_stack`] lends it to the
// worker for the duration of the hop and takes the worker's value back — see
// [`perf_fidelity_bits`]. The entries that never hop (`hot_swap`, `restart`,
// `run_cached_module`) already read and write the owner's cell directly, so
// every entry agrees on one home.
thread_local! {
    static JIT_PERF_FIDELITY: std::cell::Cell<u32> =
        const { std::cell::Cell::new(JIT_PERF_DEFAULT_FIDELITY_BITS) };
}

/// This session's `core.perf` fidelity signal, for the compiler-worker hop.
///
/// Read on the session owner before the hop and on the worker once `work` has
/// returned; [`set_perf_fidelity_bits`] installs it on the other side. Raw
/// bits rather than `f32` so the relay reproduces exactly what was stored,
/// with no round-trip through a value the cell does not hold.
pub(crate) fn perf_fidelity_bits() -> u32 {
    JIT_PERF_FIDELITY.with(std::cell::Cell::get)
}

pub(crate) fn set_perf_fidelity_bits(bits: u32) {
    JIT_PERF_FIDELITY.with(|slot| slot.set(bits));
}

fn jet_jit_pattern_text_match(subject: i64, descriptor: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let Some(subject) = rt.heap.clone_string(subject) else {
            rt.set_host_fault("MIR text pattern subject was not a string handle");
            return 0;
        };
        let Some(index) = descriptor.checked_sub(1).and_then(|value| usize::try_from(value).ok())
        else {
            rt.set_host_fault("MIR text pattern descriptor handle is invalid");
            return 0;
        };
        let Some(JitPatternDescriptor::Text(mir_parts)) =
            rt.pattern_descriptors.get(index).cloned()
        else {
            rt.set_host_fault("MIR text pattern descriptor has the wrong kind");
            return 0;
        };
        let parts = mir_parts
            .iter()
            .map(|part| match part {
                MirTextPatternPart::Literal(value) => JetTextMatchPart::Literal(value.as_str()),
                MirTextPatternPart::Hole { kind, .. } => JetTextMatchPart::Hole {
                    kind: match kind {
                        jet_foundation::MIR::MirTextHoleKind::Text => JetTextHoleKind::Text,
                        jet_foundation::MIR::MirTextHoleKind::Int => JetTextHoleKind::Int,
                        jet_foundation::MIR::MirTextHoleKind::Float => JetTextHoleKind::Float,
                        jet_foundation::MIR::MirTextHoleKind::Bool => JetTextHoleKind::Bool,
                        jet_foundation::MIR::MirTextHoleKind::InlineRange { lo, hi } => {
                            JetTextHoleKind::InlineRange { lo: *lo, hi: *hi }
                        }
                    },
                },
            })
            .collect::<Vec<_>>();
        let Some(bindings) = jet_text_pattern_match(&subject, &parts) else {
            return alloc_jit_result(rt, false, 0);
        };
        let captures = rt.heap.alloc_empty_list();
        for binding in bindings {
            let capture = alloc_pattern_capture(rt, binding);
            if rt.heap.list_push_int(captures, capture).is_none() {
                rt.set_host_fault("MIR text pattern capture list rejected a capture record");
                return 0;
            }
        }
        alloc_jit_result(rt, true, captures as u64)
    })
}

fn jet_jit_pattern_binary_match(subject: i64, descriptor: i64) -> i64 {
    with_runtime_result(0, |rt| {
        // Bytes live in the resident heap as a `[Int]` word list (the same
        // carrier `Marshal::clone_bytes` reads); every word must be a byte.
        let bytes = match view_bytes(rt, subject) {
            Some(bytes) => bytes,
            None => {
                let Some(words) = rt.heap.clone_int_list(subject) else {
                    rt.set_host_fault("MIR binary pattern subject was not an integer-list handle");
                    return 0;
                };
                let Ok(bytes) = words
                    .iter()
                    .map(|word| u8::try_from(*word))
                    .collect::<Result<Vec<u8>, _>>()
                else {
                    rt.set_host_fault("MIR binary pattern subject is not Bytes");
                    return 0;
                };
                bytes
            }
        };
        let Some(index) = descriptor.checked_sub(1).and_then(|value| usize::try_from(value).ok())
        else {
            rt.set_host_fault("MIR binary pattern descriptor handle is invalid");
            return 0;
        };
        let Some(JitPatternDescriptor::Binary(mir_parts)) =
            rt.pattern_descriptors.get(index).cloned()
        else {
            rt.set_host_fault("MIR binary pattern descriptor has the wrong kind");
            return 0;
        };
        let parts = mir_parts
            .iter()
            .map(|part| match part {
                MirBinaryPatternPart::Literal(value) => JetBinMatchPart::Lit(value.as_slice()),
                MirBinaryPatternPart::Bits { width, little, .. } => JetBinMatchPart::Bits {
                    width: usize::from(*width),
                    little: *little,
                },
                MirBinaryPatternPart::Rest { .. } => JetBinMatchPart::Rest,
            })
            .collect::<Vec<_>>();
        let Some(bindings) = jet_binary_pattern_match(&bytes, &parts) else {
            return alloc_jit_result(rt, false, 0);
        };
        let captures = rt.heap.alloc_empty_list();
        for binding in bindings {
            let capture = alloc_pattern_capture(rt, binding);
            if rt.heap.list_push_int(captures, capture).is_none() {
                rt.set_host_fault("MIR binary pattern capture list rejected a capture record");
                return 0;
            }
        }
        alloc_jit_result(rt, true, captures as u64)
    })
}


fn alloc_pattern_capture(rt: &mut JitRuntime, capture: JetPatternCapture) -> i64 {
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, 0);
    match capture {
        JetPatternCapture::Text(value) => {
            let value = rt.heap.alloc_string(value);
            let _ = rt.heap.record_set_string(record, 1, value);
        }
        JetPatternCapture::Int(value) => {
            let _ = rt.heap.record_set_int(record, 1, value);
        }
        JetPatternCapture::Float(value) => {
            let _ = rt.heap.record_set_float(record, 1, value);
        }
        JetPatternCapture::Bool(value) => {
            let _ = rt.heap.record_set_bool(record, 1, value);
        }
        JetPatternCapture::Bytes(bytes) => {
            let list = rt.heap.alloc_empty_list();
            for byte in bytes {
                let _ = rt.heap.list_push_int(list, i64::from(byte));
            }
            let _ = rt.heap.record_set_int(record, 1, list);
        }
    }
    record
}

pub(crate) fn alloc_jit_result(rt: &mut JitRuntime, ok: bool, bits: u64) -> i64 {
    rt.results.push(JitResultValue { ok, bits });
    rt.results.len() as i64
}

/// Read one checked `Err` payload from the shared one-based error arena.
pub(crate) fn jit_error(rt: &JitRuntime, handle: i64) -> Option<jet_foundation::Outcome::JetErr> {
    handle
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| rt.errors.get(index).cloned())
}

    pub(crate) fn write_typed_record_field(
        rt: &mut JitRuntime,
        record: i64,
        index: usize,
        raw: i64,
        descriptor: &RuntimeTypeDescriptor,
    ) -> Result<(), String> {
        let index = i64::try_from(index).map_err(|_| "typed record field index overflowed".to_string())?;
        let kind = persist_effective_kind(descriptor);
        let written = match kind {
            RuntimeValueKind::Float => rt.heap.record_set_float(
                record,
                index,
                if descriptor.abi == RuntimeValueAbi::Float32 {
                    f32::from_bits(raw as u32) as f64
                } else {
                    f64::from_bits(raw as u64)
                },
            ),
            RuntimeValueKind::Bool => rt.heap.record_set_bool(record, index, raw != 0),
            RuntimeValueKind::Char => {
                let value = char::from_u32(raw as u32).ok_or_else(|| {
                    format!("field `{}` contains an invalid Char", descriptor.name)
                })?;
                rt.heap.record_set_char(record, index, value)
            }
            RuntimeValueKind::String => rt.heap.record_set_string(record, index, raw),
            RuntimeValueKind::Record | RuntimeValueKind::Enum => {
                rt.heap.record_set_record(record, index, raw)
            }
            _ => rt.heap.record_set_int(record, index, raw),
        };
        written.ok_or_else(|| {
            format!(
                "field `{}` could not be stored in its checked record",
                descriptor.name
            )
        })
    }

mod service_adapter {
    use super::{
        alloc_jit_result, service_prelude, JitJobAdapter, JitRuntime, RuntimeTypeDescriptor,
        RuntimeValueAbi, RuntimeValueKind,
    };
    use jet_foundation::CborKernel::{self, Value as CborValue};
    use jet_foundation::Diagnostics::Span;
    use jet_foundation::MIR::{MirConstKey, MirNominalRef, MirRuntimeValue, MirType, MirTypeKind};
    use jet_foundation::Shape::ShapeProjectionKind;

    const SERVICES_MODULE: i64 = 0;
    const SYNC_MODULE: i64 = 1;
    const SERVICE_AUTHORITY_MODULE: i64 = 2;

    #[derive(Clone, Copy)]
    enum ArgKind {
        String,
        Int,
        StringList,
        Slot,
        Restart,
        Delivery,
        DeliveryHandle,
        TaskOutcome,
        DurationNs,
        WorkflowId,
    }

    fn arg_kind(module: i64, method: &str, index: usize) -> Option<ArgKind> {
        match module {
            SERVICES_MODULE => match method {
                "runtime" => match index {
                    0 => Some(ArgKind::String),
                    1 => Some(ArgKind::DurationNs),
                    _ => None,
                },
                "tree" | "state_store" if index == 0 => Some(ArgKind::String),
                "set_restart" if index == 0 => Some(ArgKind::Slot),
                "set_restart" if index == 1 => Some(ArgKind::Restart),
                "set_delivery" if index == 0 => Some(ArgKind::Slot),
                "worker" if index == 2 => Some(ArgKind::Int),
                "worker" if index == 3 => Some(ArgKind::String),
                "worker" if index == 4 => Some(ArgKind::Int),
                "group" if index == 0 => Some(ArgKind::Slot),
                "group" if index == 1 => Some(ArgKind::String),
                "group" if index == 2 => Some(ArgKind::StringList),
                "start" | "stop" if index == 0 => Some(ArgKind::Slot),
                "send" if index == 0 => Some(ArgKind::Slot),
                "send" if index == 1 => Some(ArgKind::Slot),
                "send" if index == 2 => Some(ArgKind::String),
                "receive" | "mailbox_depth" | "restarts" | "fail_worker" | "drain_worker"
                | "partition_worker" | "reconcile_worker"
                    if index < 2 =>
                {
                    Some(ArgKind::Slot)
                }
                "endpoint_send" | "endpoint_receive" | "endpoint_show"
                | "tree_show"
                | "dead_letter_count"
                | "drain_dead_letters"
                | "set_state_empty"
                | "restore_snapshot"
                | "event_count"
                | "replay_events"
                | "directory_generation"
                | "handoff_generation"
                | "rollback_generation"
                | "chaos_fail"
                | "upgrade_receipt"
                | "observe"
                    if index == 0 =>
                {
                    Some(ArgKind::Slot)
                }
                "endpoint_send" if index == 1 => Some(ArgKind::String),
                "send_durable" if index < 2 => Some(ArgKind::Slot),
                "send_durable" if index < 4 => Some(ArgKind::String),
                "delivery_wait" | "delivery_status" | "delivery_retry" | "delivery_cancel"
                | "delivery_receipt" | "delivery_events"
                    if index == 0 =>
                {
                    Some(ArgKind::Slot)
                }
                "set_state_snapshot" | "set_state_event_log" if index == 0 => Some(ArgKind::Slot),
                "set_state_snapshot" | "set_state_event_log" if index == 1 => Some(ArgKind::Slot),
                "set_state_snapshot" | "set_state_event_log" if index == 2 => Some(ArgKind::String),
                "set_state_snapshot" | "set_state_event_log" if index == 3 => Some(ArgKind::Int),
                "set_state_snapshot" | "set_state_event_log" if index == 4 => Some(ArgKind::String),
                "commit_snapshot" if index == 0 => Some(ArgKind::Slot),
                "commit_snapshot" if index == 1 => Some(ArgKind::String),
                "append_event" if index == 0 => Some(ArgKind::Slot),
                "append_event" if index == 1 => Some(ArgKind::String),
                "workflow_start" if index == 0 => Some(ArgKind::Slot),
                "workflow_start" if index == 1 => Some(ArgKind::String),
                "workflow_start" if index == 2 => Some(ArgKind::Int),
                "workflow_sleep" if index == 0 => Some(ArgKind::Slot),
                "workflow_sleep" if index == 1 => Some(ArgKind::DurationNs),
                "workflow_activity_wait" if index == 0 => Some(ArgKind::Slot),
                "workflow_activity_wait" if index == 1 || index == 2 => Some(ArgKind::String),
                "workflow_all" if index == 0 => Some(ArgKind::Slot),
                "workflow_all" if index == 1 => Some(ArgKind::StringList),
                "workflow_step" if index == 0 => Some(ArgKind::Slot),
                "workflow_step" if index == 1 => Some(ArgKind::WorkflowId),
                "workflow_step" if index == 2 => Some(ArgKind::String),
                "workflow_activity" if index == 0 => Some(ArgKind::Slot),
                "workflow_activity" if index == 1 => Some(ArgKind::WorkflowId),
                "workflow_activity" if index == 2 || index == 3 => Some(ArgKind::String),
                "workflow_activity" if index == 4 => Some(ArgKind::Int),
                "workflow_activity_retry" if index == 0 => Some(ArgKind::Slot),
                "workflow_activity_retry" if index == 1 => Some(ArgKind::WorkflowId),
                "workflow_activity_retry" if index == 2 => Some(ArgKind::String),
                "workflow_activity_retry" if index == 3 => Some(ArgKind::TaskOutcome),
                "workflow_activity_complete" if index == 0 => Some(ArgKind::Slot),
                "workflow_activity_complete" if index == 1 => Some(ArgKind::WorkflowId),
                "workflow_activity_complete" if index == 2 => Some(ArgKind::String),
                "workflow_activity_complete" if index == 3 => Some(ArgKind::TaskOutcome),
                "workflow_history" if index == 0 => Some(ArgKind::Slot),
                "workflow_history" if index == 1 => Some(ArgKind::WorkflowId),
                "workflow_outcome" if index == 0 => Some(ArgKind::Slot),
                "workflow_outcome" if index == 1 => Some(ArgKind::WorkflowId),
                "directory_register" if index == 0 => Some(ArgKind::Slot),
                "directory_register" if index == 1 => Some(ArgKind::String),
                "directory_register" if index == 2 => Some(ArgKind::Slot),
                "directory_resolve" if index == 0 => Some(ArgKind::Slot),
                "directory_resolve" if index == 1 => Some(ArgKind::String),
                _ => None,
            },
            SERVICE_AUTHORITY_MODULE => match method {
                "send" if index == 0 => Some(ArgKind::Slot),
                "send" if index == 1 => Some(ArgKind::Slot),
                "send" if index >= 2 => Some(ArgKind::String),
                "retry" | "dead_letter" | "retain" if index < 2 => {
                    if index == 0 {
                        Some(ArgKind::Slot)
                    } else {
                        Some(ArgKind::DeliveryHandle)
                    }
                }
                "commit" if index == 0 => Some(ArgKind::Slot),
                "commit" if index == 1 => Some(ArgKind::DeliveryHandle),
                _ => None,
            },
            SYNC_MODULE => match method {
                "text_new" if index < 2 => Some(ArgKind::String),
                "text_set" if index == 0 => Some(ArgKind::Slot),
                "text_set" if index > 0 => Some(ArgKind::String),
                "text_merge" if index < 2 => Some(ArgKind::Slot),
                "text_show" | "text_metadata" if index == 0 => Some(ArgKind::Slot),
                "text_edit" if index == 0 => Some(ArgKind::Slot),
                "text_edit" if index == 1 || index == 4 => Some(ArgKind::String),
                "text_edit" if index == 2 || index == 3 => Some(ArgKind::Int),
                "counter_new" if index == 0 => Some(ArgKind::String),
                "counter_new" if index == 1 => Some(ArgKind::Int),
                "counter_inc" if index == 0 => Some(ArgKind::Slot),
                "counter_inc" if index == 1 => Some(ArgKind::String),
                "counter_inc" if index == 2 => Some(ArgKind::Int),
                "counter_merge" if index < 2 => Some(ArgKind::Slot),
                "counter_value" if index == 0 => Some(ArgKind::Slot),
                "map_set" if index == 0 => Some(ArgKind::Slot),
                "map_set" if index > 0 => Some(ArgKind::String),
                "map_get" if index == 0 => Some(ArgKind::Slot),
                "map_get" if index == 1 => Some(ArgKind::String),
                "map_merge" if index < 2 => Some(ArgKind::Slot),
                "map_show" if index == 0 => Some(ArgKind::Slot),
                "list_push" if index == 0 => Some(ArgKind::Slot),
                "list_push" if index > 0 => Some(ArgKind::String),
                "list_merge" if index < 2 => Some(ArgKind::Slot),
                "list_show" if index == 0 => Some(ArgKind::Slot),
                "policy_new" if index < 2 => Some(ArgKind::String),
                "policy_allows" if index == 0 => Some(ArgKind::Slot),
                "policy_allows" if index > 0 => Some(ArgKind::String),
                "policy_show" if index == 0 => Some(ArgKind::Slot),
                "sync" if index < 2 => Some(ArgKind::String),
                "map_new" | "list_new" => None,
                _ => None,
            },
            _ => None,
        }
    }

    fn service_value(rt: &JitRuntime, handle: i64) -> Option<MirRuntimeValue> {
        let index = usize::try_from(handle).ok()?;
        rt.service_values.get(index)?.as_ref().cloned()
    }

    fn remember_service_value(rt: &mut JitRuntime, handle: i64, value: MirRuntimeValue) {
        let Ok(index) = usize::try_from(handle) else {
            return;
        };
        if rt.service_values.len() <= index {
            rt.service_values.resize_with(index + 1, || None);
        }
        rt.service_values[index] = Some(value);
    }

    fn replace_service_value(rt: &mut JitRuntime, handle: i64, value: MirRuntimeValue) {
        let Ok(index) = usize::try_from(handle) else {
            return;
        };
        if index < rt.service_values.len() {
            rt.service_values[index] = Some(value.clone());
            refresh_service_record(rt, handle, &value);
        }
    }
    pub(super) fn start(tree_handle: i64) -> i64 {
        use jet_codegen::Comptime::ServicesLite::JetServiceError;

        let tree = super::Concurrency::with_runtime_mut(|rt| service_value(rt, tree_handle));
        let Some(tree) = tree else {
            super::Concurrency::with_runtime_mut(|rt| {
                rt.set_trap("JIT service.start received an invalid ServiceTree handle");
            });
            return 0;
        };
        let result = service_prelude::services_start_runtime(
            &tree,
            Span::new(0, 0),
            |handler_name, endpoint| {
                let handler = super::Concurrency::with_runtime_result(
                    JetServiceError::Unknown(format!(
                        "JIT service worker `{handler_name}` has no active resident runtime"
                    )),
                    |rt| {
                    let handle = rt.service_callbacks.get(handler_name).copied().ok_or_else(|| {
                        JetServiceError::Unknown(format!(
                            "JIT service worker `{handler_name}` has no checked callback handle"
                        ))
                    })?;
                    super::jit_callable_slot(rt, handle).ok_or_else(|| {
                        JetServiceError::Unknown(format!(
                            "JIT service worker `{handler_name}` has an invalid callback handle"
                        ))
                    })
                })?;
                let _scope = service_prelude::jet_services_execution_scope(endpoint)?;
                super::invoke_jit_callable_zero(&handler);
                if super::jet_jit_is_trapped() != 0 {
                    let reason = super::Concurrency::with_runtime_mut(|rt| rt.trapped.clone())
                        .unwrap_or_else(|| "service worker callback stopped".to_string());
                    return Err(JetServiceError::Unknown(reason));
                }
                tick_worker_queue(endpoint).map_err(JetServiceError::Unknown)
            },
        );
        super::Concurrency::with_runtime_mut(|rt| {
            let value = result.unwrap_or_else(diagnostic_value);
            let value = apply_services_mutation(rt, &[tree_handle], value);
            marshal_result(rt, value)
        })
    }

    fn convert_arg(rt: &JitRuntime, kind: ArgKind, raw: i64) -> Option<MirRuntimeValue> {
        match kind {
            ArgKind::String => rt.heap.clone_string(raw).map(MirRuntimeValue::String),
            ArgKind::Int => Some(MirRuntimeValue::Int(raw)),
            ArgKind::WorkflowId => service_value(rt, raw)
                .filter(|value| {
                    matches!(
                        value,
                        MirRuntimeValue::Struct { type_name, .. } if type_name == "ServiceWorkflow"
                    )
                })
                .or_else(|| Some(MirRuntimeValue::Int(raw))),
            ArgKind::StringList => {
                let length = rt.heap.list_len(raw)?;
                if length < 0 {
                    return None;
                }
                let mut values = Vec::with_capacity(usize::try_from(length).ok()?);
                for index in 0..length {
                    let handle = rt.heap.list_get_int(raw, index)?;
                    values.push(MirRuntimeValue::String(rt.heap.clone_string(handle)?));
                }
                Some(MirRuntimeValue::List(values))
            }
            ArgKind::Slot => service_value(rt, raw),
            ArgKind::Restart => Some(MirRuntimeValue::Enum {
                type_name: "ServiceRestart".to_string(),
                variant: match raw {
                    0 => "OneForOne",
                    1 => "OneForAll",
                    2 => "RestForOne",
                    _ => return None,
                }
                .to_string(),
                args: Vec::new(),
            }),
            ArgKind::Delivery => Some(MirRuntimeValue::Enum {
                type_name: "ServiceDelivery".to_string(),
                variant: match raw {
                    0 => "AtMostOnce",
                    1 => "DurableAtLeastOnce",
                    _ => return None,
                }
                .to_string(),
                args: Vec::new(),
            }),
            ArgKind::DeliveryHandle => service_value(rt, raw).filter(|value| {
                matches!(
                    value,
                    MirRuntimeValue::Struct { type_name, .. } if type_name == "Delivery"
                )
            }),
            ArgKind::TaskOutcome => {
                let variant = match raw & 0xff {
                    0 => "Finished",
                    2 => "Cancelled",
                    3 => "DeadlineBlown",
                    1 => {
                        let reason = rt.heap.clone_string(raw >> 8)?;
                        return Some(MirRuntimeValue::Enum {
                            type_name: "TaskOutcome".to_string(),
                            variant: "Panicked".to_string(),
                            args: vec![(None, MirRuntimeValue::String(reason))],
                        });
                    }
                    _ => return None,
                };
                Some(MirRuntimeValue::Enum {
                    type_name: "TaskOutcome".to_string(),
                    variant: variant.to_string(),
                    args: Vec::new(),
                })
            }
            ArgKind::DurationNs => Some(MirRuntimeValue::Struct {
                type_name: "Duration".to_string(),
                fields: vec![("ns".to_string(), MirRuntimeValue::Int(raw))],
            }),
        }
    }

    fn set_record_field(rt: &mut JitRuntime, record: i64, index: usize, value: &MirRuntimeValue) {
        let Ok(index) = i64::try_from(index) else {
            return;
        };
        match value {
            MirRuntimeValue::Int(value) => {
                let _ = rt.heap.record_set_int(record, index, *value);
            }
            MirRuntimeValue::Float { value, .. } => {
                let _ = rt.heap.record_set_float(record, index, *value);
            }
            MirRuntimeValue::Bool(value) => {
                let _ = rt.heap.record_set_bool(record, index, *value);
            }
            MirRuntimeValue::Char(value) => {
                let _ = rt.heap.record_set_char(record, index, *value);
            }
            MirRuntimeValue::String(value) => {
                let handle = rt.heap.alloc_string(value.clone());
                let _ = rt.heap.record_set_string(record, index, handle);
            }
            MirRuntimeValue::Struct { type_name, fields } => {
                let handle = marshal_struct(rt, type_name, fields);
                let _ = rt.heap.record_set_record(record, index, handle);
            }
            MirRuntimeValue::Present(value) => {
                let bits = marshal_scalar(rt, value).wrapping_add(1);
                let _ = rt.heap.record_set_int(record, index, bits);
            }
            MirRuntimeValue::Absent { .. } => {
                let _ = rt.heap.record_set_int(record, index, 0);
            }
            MirRuntimeValue::FailedTold(value) => {
                let bits = marshal_scalar(rt, value);
                let _ = rt.heap.record_set_int(record, index, bits);

            }
            MirRuntimeValue::Moved => {
                rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
                return;
            }
            MirRuntimeValue::Bytes(_)
            | MirRuntimeValue::List(_)
            | MirRuntimeValue::Map(_)
            | MirRuntimeValue::Enum { .. }
            | MirRuntimeValue::BigInt(_)
            | MirRuntimeValue::Closure(_)
            | MirRuntimeValue::Unit => {
                let bits = marshal_scalar(rt, value);
                let _ = rt.heap.record_set_int(record, index, bits);
            }
        }
    }

    fn contains_moved(value: &MirRuntimeValue) -> bool {
        match value {
            MirRuntimeValue::Moved => true,
            MirRuntimeValue::List(values) => values.iter().any(contains_moved),
            MirRuntimeValue::Map(entries) => entries.iter().any(|(_, value)| contains_moved(value)),
            MirRuntimeValue::Struct { fields, .. } => {
                fields.iter().any(|(_, value)| contains_moved(value))
            }
            MirRuntimeValue::Enum { args, .. } => {
                args.iter().any(|(_, value)| contains_moved(value))
            }
            MirRuntimeValue::Present(value) | MirRuntimeValue::FailedTold(value) => {
                contains_moved(value)
            }
            MirRuntimeValue::Closure(closure) => closure.captures.iter().any(contains_moved),
            MirRuntimeValue::Int(_)
            | MirRuntimeValue::BigInt(_)
            | MirRuntimeValue::Float { .. }
            | MirRuntimeValue::Bool(_)
            | MirRuntimeValue::Char(_)
            | MirRuntimeValue::String(_)
            | MirRuntimeValue::Bytes(_)
            | MirRuntimeValue::Absent { .. }
            | MirRuntimeValue::Unit => false,
        }
    }

    fn reject_moved(rt: &mut JitRuntime, value: &MirRuntimeValue) -> bool {
        if contains_moved(value) {
            rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
            true
        } else {
            false
        }
    }

    fn refresh_service_record(rt: &mut JitRuntime, record: i64, value: &MirRuntimeValue) {
        let MirRuntimeValue::Struct { fields, .. } = value else {
            return;
        };
        if reject_moved(rt, value) {
            return;
        }
        for (index, (_, value)) in fields.iter().enumerate() {
            set_record_field(rt, record, index, value);
        }
    }

    fn marshal_list(rt: &mut JitRuntime, values: &[MirRuntimeValue]) -> i64 {
        if values.iter().any(contains_moved) {
            rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
            return 0;
        }
        let list = rt.heap.alloc_empty_list();
        for value in values {
            let bits = marshal_scalar(rt, value);
            let _ = rt.heap.list_push_int(list, bits);
        }
        list
    }

    fn marshal_struct(rt: &mut JitRuntime, type_name: &str, fields: &[(String, MirRuntimeValue)]) -> i64 {
        if fields
            .iter()
            .any(|(_, value)| contains_moved(value))
        {
            rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
            return 0;
        }
        let record = rt.heap.alloc_record(fields.len());
        remember_service_value(
            rt,
            record,
            MirRuntimeValue::Struct {
                type_name: type_name.to_string(),
                fields: fields.to_vec(),
            },
        );
        for (index, (_, value)) in fields.iter().enumerate() {
            set_record_field(rt, record, index, value);
        }
        record
    }

    fn enum_discriminant(type_name: &str, variant: &str) -> Option<i64> {
        let variants: &[&str] = match type_name {
            "DeliveryState" => &[
                "Pending",
                "Accepted",
                "Delivering",
                "Delivered",
                "DeadLettered",
                "Cancelled",
            ],
            "ServiceError" => &[
                "Full",
                "Ambiguous",
                "Unknown",
                "NotStarted",
                "Policy",
                "Unavailable",
                "Partitioned",
                "Revoked",
                "Stale",
                "Expired",
            ],
            "TaskOutcome" => &["Finished", "Panicked", "Cancelled", "DeadlineBlown"],
            "TaskStatus" => &["Running", "Paused", "CancelRequested"],
            "ServiceRestart" => &["OneForOne", "OneForAll", "RestForOne"],
            "ServiceDelivery" => &["AtMostOnce", "DurableAtLeastOnce"],
            "ServiceStateAdapter" => &["Empty", "Snapshot", "EventLog"],
            _ => return None,
        };
        variants
            .iter()
            .position(|name| *name == variant)
            .and_then(|index| i64::try_from(index).ok())
    }

    fn marshal_enum(
        rt: &mut JitRuntime,
        type_name: &str,
        variant: &str,
        args: &[(Option<String>, MirRuntimeValue)],
    ) -> i64 {
        if args.iter().any(|(_, value)| contains_moved(value)) {
            rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
            return 0;
        }
        let Some(discriminant) = enum_discriminant(type_name, variant) else {
            return 0;
        };
        if args.len() <= 1 {
            if matches!(
                args.first().map(|(_, value)| value),
                Some(MirRuntimeValue::Moved)
            ) {
                rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
                return 0;
            }
            let payload = args
                .first()
                .map(|(_, value)| marshal_scalar(rt, value))
                .unwrap_or(0);
            return payload.wrapping_shl(8) | discriminant;
        }
        let record = rt.heap.alloc_record(args.len() + 1);
        let _ = rt.heap.record_set_int(record, 0, discriminant);
        for (index, (_, value)) in args.iter().enumerate() {
            let field = i64::try_from(index + 1).unwrap_or(i64::MAX);
            match value {
                MirRuntimeValue::Int(value) => {
                    let _ = rt.heap.record_set_int(record, field, *value);
                }
                MirRuntimeValue::Float { value, .. } => {
                    let _ = rt.heap.record_set_float(record, field, *value);
                }
                MirRuntimeValue::Bool(value) => {
                    let _ = rt.heap.record_set_bool(record, field, *value);
                }
                MirRuntimeValue::Char(value) => {
                    let _ = rt.heap.record_set_char(record, field, *value);
                }
                MirRuntimeValue::String(value) => {
                    let handle = rt.heap.alloc_string(value.clone());
                    let _ = rt.heap.record_set_string(record, field, handle);
                }
                MirRuntimeValue::Moved => {
                    rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
                    return 0;
                }
                MirRuntimeValue::BigInt(_)
                | MirRuntimeValue::Bytes(_)
                | MirRuntimeValue::List(_)
                | MirRuntimeValue::Map(_)
                | MirRuntimeValue::Struct { .. }
                | MirRuntimeValue::Enum { .. }
                | MirRuntimeValue::Present(_)
                | MirRuntimeValue::FailedTold(_)
                | MirRuntimeValue::Absent { .. }
                | MirRuntimeValue::Unit
                | MirRuntimeValue::Closure(_) => {
                    let bits = marshal_scalar(rt, value);
                    let _ = rt.heap.record_set_int(record, field, bits);
                }
            }
        }
        record
    }

    fn marshal_scalar(rt: &mut JitRuntime, value: &MirRuntimeValue) -> i64 {
        match value {
            MirRuntimeValue::Moved => {
                rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
                0
            }
            MirRuntimeValue::Int(value) => *value,
            MirRuntimeValue::BigInt(_) => 0,
            MirRuntimeValue::Float { value, .. } => value.to_bits() as i64,
            MirRuntimeValue::Bool(value) => i64::from(*value),
            MirRuntimeValue::Char(value) => i64::from(*value as u32),
            MirRuntimeValue::String(value) => rt.heap.alloc_string(value.clone()),
            MirRuntimeValue::Bytes(values) => marshal_list(
                rt,
                &values
                    .iter()
                    .map(|value| MirRuntimeValue::Int(i64::from(*value)))
                    .collect::<Vec<_>>(),
            ),
            MirRuntimeValue::List(values) => marshal_list(rt, values),
            MirRuntimeValue::Map(_) => 0,
            MirRuntimeValue::Struct { type_name, fields } => marshal_struct(rt, type_name, fields),
            MirRuntimeValue::Enum {
                type_name,
                variant,
                args,
            } => marshal_enum(rt, type_name, variant, args),
            MirRuntimeValue::Present(value) => marshal_scalar(rt, value),
            MirRuntimeValue::FailedTold(_) => 0,
            MirRuntimeValue::Absent { .. } => 0,
            MirRuntimeValue::Unit => 0,
            MirRuntimeValue::Closure(_) => 0,
        }
    }

    fn marshal_result(rt: &mut JitRuntime, value: MirRuntimeValue) -> i64 {
        if contains_moved(&value) {
            rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
            return 0;
        }
        match value {
            MirRuntimeValue::Moved => {
                rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
                0
            }
            MirRuntimeValue::Present(value) => {
                let bits = marshal_scalar(rt, &value);
                alloc_jit_result(rt, true, bits as u64)
            }
            MirRuntimeValue::FailedTold(value) => {
                let bits = marshal_scalar(rt, &value);
                alloc_jit_result(rt, false, bits as u64)
            }
            MirRuntimeValue::Absent { .. } => alloc_jit_result(rt, false, 0),
            MirRuntimeValue::Int(_)
            | MirRuntimeValue::BigInt(_)
            | MirRuntimeValue::Float { .. }
            | MirRuntimeValue::Bool(_)
            | MirRuntimeValue::Char(_)
            | MirRuntimeValue::String(_)
            | MirRuntimeValue::Bytes(_)
            | MirRuntimeValue::List(_)
            | MirRuntimeValue::Map(_)
            | MirRuntimeValue::Struct { .. }
            | MirRuntimeValue::Enum { .. }
            | MirRuntimeValue::Unit
            | MirRuntimeValue::Closure(_) => marshal_scalar(rt, &value),
        }
    }

    fn marshal_option(rt: &mut JitRuntime, value: MirRuntimeValue) -> i64 {
        if contains_moved(&value) {
            rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
            return 0;
        }
        match value {
            MirRuntimeValue::Moved => {
                rt.set_host_fault("moved runtime value cannot cross the JIT service boundary");
                0
            }
            MirRuntimeValue::Present(value) => marshal_scalar(rt, &value).wrapping_add(1),
            MirRuntimeValue::Absent { .. } => 0,
            MirRuntimeValue::Int(_)
            | MirRuntimeValue::BigInt(_)
            | MirRuntimeValue::Float { .. }
            | MirRuntimeValue::Bool(_)
            | MirRuntimeValue::Char(_)
            | MirRuntimeValue::String(_)
            | MirRuntimeValue::Bytes(_)
            | MirRuntimeValue::List(_)
            | MirRuntimeValue::Map(_)
            | MirRuntimeValue::Struct { .. }
            | MirRuntimeValue::Enum { .. }
            | MirRuntimeValue::FailedTold(_)
            | MirRuntimeValue::Unit
            | MirRuntimeValue::Closure(_) => 0,
        }
    }

    fn diagnostic_value(diagnostic: jet_foundation::Diagnostics::Diagnostic) -> MirRuntimeValue {
        MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(format!(
            "{}: {}",
            diagnostic.code, diagnostic.what
        ))))
    }

    fn apply_services_mutation(
        rt: &mut JitRuntime,
        raw_args: &[i64],
        value: MirRuntimeValue,
    ) -> MirRuntimeValue {
        match service_prelude::services_take_mut_runtime(value) {
            Ok((tree, value)) => {
                if !matches!(tree, MirRuntimeValue::Unit) {
                    if let Some(handle) = raw_args.first() {
                        replace_service_value(rt, *handle, tree);
                    }
                }
                value
            }
            Err(value) => value,
        }
    }

    fn call_runtime(
        rt: &mut JitRuntime,
        module: i64,
        method: &str,
        raw_args: &[i64],
    ) -> MirRuntimeValue {
        let span = Span::new(0, 0);
        if module == SERVICES_MODULE && method == "runtime" {
            let Some(store) = raw_args
                .first()
                .and_then(|handle| rt.heap.clone_string(*handle))
            else {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                    "core.service.runtime expects a store path".to_string(),
                )));
            };
            let Some(retention_ns) = raw_args.get(1).copied() else {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                    "core.service.runtime expects a Duration".to_string(),
                )));
            };
            return service_prelude::service_runtime_runtime(store, retention_ns / 1_000_000);
        }

        if module == SERVICES_MODULE && method == "worker" && raw_args.len() != 5 {
            return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                "core.service.worker expects five arguments".to_string(),
            )));
        }
        let mut args = Vec::with_capacity(raw_args.len());
        for (index, raw) in raw_args.iter().copied().enumerate() {
            let Some(kind) = arg_kind(module, method, index) else {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(format!(
                    "unsupported service adapter call: {module}.{method}"
                ))));
            };
            let Some(value) = convert_arg(rt, kind, raw) else {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(format!(
                    "invalid service adapter argument: {module}.{method}[{index}]"
                ))));
            };
            args.push(value);
        }

        let result = match module {
            SERVICES_MODULE => service_prelude::services_apply_runtime(method, &args, span),
            SYNC_MODULE => service_prelude::sync_apply_runtime(method, &args, span),
            SERVICE_AUTHORITY_MODULE => {
                let Some((receiver, args)) = args.split_first() else {
                    return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                        "ServiceRuntime receiver is missing".to_string(),
                    )));
                };
                service_prelude::services_runtime_apply_runtime(receiver, method, args, span)
            }
            _ => {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                    "unknown service adapter module".to_string(),
                )))
            }
        };
        let value = match result {
            Ok(value) => value,
            Err(diagnostic) => diagnostic_value(diagnostic),
        };
        if module != SERVICES_MODULE {
            return value;
        }
        let value = apply_services_mutation(rt, raw_args, value);
        if method == "worker"
            && raw_args.len() == 5
            && matches!(value, MirRuntimeValue::Present(_))
        {
            let Some(handler_handle) = raw_args.get(2).copied() else {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                    "service.worker callback handle is missing".to_string(),
                )));
            };
            if super::jit_callable_slot(rt, handler_handle).is_none() {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                    "service.worker callback handle is invalid".to_string(),
                )));
            }
            let Some(handler_name) = raw_args
                .get(3)
                .and_then(|handle| rt.heap.clone_string(*handle))
            else {
                return MirRuntimeValue::FailedTold(Box::new(MirRuntimeValue::String(
                    "service.worker handler identity is invalid".to_string(),
                )));
            };
            rt.service_callbacks.insert(handler_name, handler_handle);
        }
        value
    }
    fn job_descriptor(
        rt: &JitRuntime,
        id: u64,
        owner: &str,
    ) -> Result<RuntimeTypeDescriptor, String> {
        rt.runtime_type_descriptor(id)
            .cloned()
            .ok_or_else(|| format!("queued #Job `{owner}` has no runtime descriptor {id}"))
    }

    fn job_child_descriptor(
        rt: &JitRuntime,
        parent: &RuntimeTypeDescriptor,
        id: Option<u64>,
        label: &str,
    ) -> Result<RuntimeTypeDescriptor, String> {
        id.and_then(|id| rt.runtime_type_descriptor(id).cloned())
            .ok_or_else(|| {
                format!(
                    "queued #Job type `{}` has no checked {label} descriptor",
                    parent.name
                )
            })
    }

    fn job_absent(descriptor: &RuntimeTypeDescriptor) -> MirRuntimeValue {
        MirRuntimeValue::Absent {
            element: MirType::from_kind(MirTypeKind::Apply { name: MirNominalRef::from_name(
                &descriptor.name,
            ), args: Vec::new() }),
        }
    }

    fn job_decode_record(
        rt: &JitRuntime,
        descriptor: &RuntimeTypeDescriptor,
        entries: &[(String, CborValue)],
        depth: usize,
    ) -> Result<MirRuntimeValue, String> {
        let mut fields = Vec::with_capacity(descriptor.fields.len());
        for field in &descriptor.fields {
            if field.skip {
                return Err(format!(
                    "queued #Job record `{}` contains a skipped field `{}`",
                    descriptor.name, field.source_name
                ));
            }
            let child = job_child_descriptor(rt, descriptor, Some(field.type_id), &field.source_name)?;
            let value = entries
                .iter()
                .find(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
                .map(|(_, value)| job_decode_value(rt, value, &child, depth + 1))
                .transpose()?
                .or_else(|| {
                    (super::persist_effective_kind(&child) == RuntimeValueKind::Option)
                        .then(|| job_absent(&child))
                })
                .ok_or_else(|| {
                    format!(
                        "queued #Job record `{}` is missing field `{}`",
                        descriptor.name,
                        field.name_for(ShapeProjectionKind::Json)
                    )
                })?;
            fields.push((field.source_name.clone(), value));
        }
        if entries.iter().any(|(name, _)| {
            !descriptor.fields.iter().any(|field| {
                field.matches_name(name, ShapeProjectionKind::Json)
            })
        }) {
            return Err(format!(
                "queued #Job record `{}` contains an unknown field",
                descriptor.name
            ));
        }
        Ok(MirRuntimeValue::Struct {
            type_name: descriptor.name.clone(),
            fields,
        })
    }

    fn job_decode_enum_variant(
        rt: &JitRuntime,
        descriptor: &RuntimeTypeDescriptor,
        variant: &super::RuntimeVariantDescriptor,
        payload: Option<&CborValue>,
        named: Option<&[(String, CborValue)]>,
        depth: usize,
    ) -> Result<MirRuntimeValue, String> {
        let mut args = Vec::with_capacity(variant.fields.len());
        match variant.fields.as_slice() {
            [] => {
                if payload.is_some_and(|value| !matches!(value, CborValue::Null)) {
                    return Err(format!(
                        "queued #Job enum `{}` variant `{}` has an unexpected payload",
                        descriptor.name, variant.name
                    ));
                }
            }
            [field] => {
                let child = job_child_descriptor(rt, descriptor, Some(field.type_id), &field.source_name)?;
                let value = payload
                    .or_else(|| {
                        named.and_then(|entries| {
                            entries
                                .iter()
                                .find(|(name, _)| {
                                    name == "value"
                                        || field.matches_name(name, ShapeProjectionKind::Json)
                                })
                                .map(|(_, value)| value)
                        })
                    })
                    .ok_or_else(|| {
                        format!(
                            "queued #Job enum `{}` variant `{}` has no payload",
                            descriptor.name, variant.name
                        )
                    })?;
                args.push((None, job_decode_value(rt, value, &child, depth + 1)?));
            }
            fields => {
                let entries = named.or_else(|| match payload {
                    Some(CborValue::Object(entries)) => Some(entries.as_slice()),
                    _ => None,
                }).ok_or_else(|| {
                    format!(
                        "queued #Job enum `{}` variant `{}` needs named fields",
                        descriptor.name, variant.name
                    )
                })?;
                for field in fields {
                    let child =
                        job_child_descriptor(rt, descriptor, Some(field.type_id), &field.source_name)?;
                    let value = entries
                        .iter()
                        .find(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
                        .map(|(_, value)| job_decode_value(rt, value, &child, depth + 1))
                        .transpose()?
                        .ok_or_else(|| {
                            format!(
                                "queued #Job enum `{}` variant `{}` is missing field `{}`",
                                descriptor.name,
                                variant.name,
                                field.name_for(ShapeProjectionKind::Json)
                            )
                        })?;
                    args.push((Some(field.source_name.clone()), value));
                }
            }
        }
        Ok(MirRuntimeValue::Enum {
            type_name: descriptor.name.clone(),
            variant: variant.name.clone(),
            args,
        })
    }

    fn job_decode_enum(
        rt: &JitRuntime,
        descriptor: &RuntimeTypeDescriptor,
        value: &CborValue,
        depth: usize,
    ) -> Result<MirRuntimeValue, String> {
        if descriptor.serde_untagged {
            for variant in &descriptor.variants {
                if let Ok(value) = job_decode_enum_variant(rt, descriptor, variant, Some(value), None, depth) {
                    return Ok(value);
                }
            }
            return Err(format!(
                "queued #Job enum `{}` has no variant matching its payload",
                descriptor.name
            ));
        }
        if let Some(tag) = descriptor.serde_tag.as_deref() {
            let CborValue::Object(entries) = value else {
                return Err(format!(
                    "queued #Job enum `{}` requires tagged object encoding",
                    descriptor.name
                ));
            };
            let variant_name = entries
                .iter()
                .find_map(|(name, value)| (name == tag).then(|| match value {
                    CborValue::Text(name) => Some(name.as_str()),
                    _ => None,
                }))
                .flatten()
                .ok_or_else(|| format!("queued #Job enum `{}` has no `{tag}` tag", descriptor.name))?;
            let variant = descriptor
                .variants
                .iter()
                .find(|variant| variant.wire_name == variant_name || variant.name == variant_name)
                .ok_or_else(|| format!("queued #Job enum `{}` has unknown variant `{variant_name}`", descriptor.name))?;
            let payload = entries.iter().find(|(name, _)| name == "value").map(|(_, value)| value);
            let named = (variant.fields.len() > 1).then(|| {
                entries
                    .iter()
                    .filter(|(name, _)| name != tag)
                    .cloned()
                    .collect::<Vec<_>>()
            });
            return job_decode_enum_variant(
                rt,
                descriptor,
                variant,
                payload,
                named.as_deref(),
                depth,
            );
        }
        match value {
            CborValue::Text(name) => {
                let variant = descriptor
                    .variants
                    .iter()
                    .find(|variant| variant.wire_name == *name || variant.name == *name)
                    .ok_or_else(|| format!("queued #Job enum `{}` has unknown variant `{name}`", descriptor.name))?;
                job_decode_enum_variant(rt, descriptor, variant, None, None, depth)
            }
            CborValue::Object(entries) if entries.len() == 1 => {
                let (name, payload) = &entries[0];
                let variant = descriptor
                    .variants
                    .iter()
                    .find(|variant| variant.wire_name == *name || variant.name == *name)
                    .ok_or_else(|| format!("queued #Job enum `{}` has unknown variant `{name}`", descriptor.name))?;
                job_decode_enum_variant(rt, descriptor, variant, Some(payload), None, depth)
            }
            _ => Err(format!(
                "queued #Job enum `{}` has invalid external-tag encoding",
                descriptor.name
            )),
        }
    }

    fn job_decode_value(
        rt: &JitRuntime,
        value: &CborValue,
        descriptor: &RuntimeTypeDescriptor,
        depth: usize,
    ) -> Result<MirRuntimeValue, String> {
        if depth > 128 {
            return Err("queued #Job payload exceeds the descriptor depth limit".to_string());
        }
        let kind = super::persist_effective_kind(descriptor);
        match kind {
            RuntimeValueKind::Unit => match value {
                CborValue::Null => Ok(MirRuntimeValue::Unit),
                _ => Err(format!("queued #Job `{}` expects unit", descriptor.name)),
            },
            RuntimeValueKind::Int => match value {
                CborValue::Int(value) => Ok(MirRuntimeValue::Int(*value)),
                _ => Err(format!("queued #Job `{}` expects an integer", descriptor.name)),
            },
            RuntimeValueKind::Float => match value {
                CborValue::Float(value) => Ok(MirRuntimeValue::Float {
                    value: *value,
                    f32: descriptor.abi == RuntimeValueAbi::Float32,
                }),
                CborValue::Int(value) => Ok(MirRuntimeValue::Float {
                    value: *value as f64,
                    f32: descriptor.abi == RuntimeValueAbi::Float32,
                }),
                _ => Err(format!("queued #Job `{}` expects a float", descriptor.name)),
            },
            RuntimeValueKind::Bool => match value {
                CborValue::Bool(value) => Ok(MirRuntimeValue::Bool(*value)),
                _ => Err(format!("queued #Job `{}` expects a bool", descriptor.name)),
            },
            RuntimeValueKind::Char => match value {
                CborValue::Text(value) => {
                    let mut chars = value.chars();
                    let Some(character) = chars.next() else {
                        return Err(format!("queued #Job `{}` expects one character", descriptor.name));
                    };
                    if chars.next().is_some() {
                        return Err(format!("queued #Job `{}` expects one character", descriptor.name));
                    }
                    Ok(MirRuntimeValue::Char(character))
                }
                _ => Err(format!("queued #Job `{}` expects a character", descriptor.name)),
            },
            RuntimeValueKind::String => match value {
                CborValue::Text(value) => Ok(MirRuntimeValue::String(value.clone())),
                _ => Err(format!("queued #Job `{}` expects text", descriptor.name)),
            },
            RuntimeValueKind::List => {
                let child = job_child_descriptor(rt, descriptor, descriptor.element, "element")?;
                match value {
                    CborValue::Array(values) => values
                        .iter()
                        .map(|value| job_decode_value(rt, value, &child, depth + 1))
                        .collect::<Result<Vec<_>, _>>()
                        .map(MirRuntimeValue::List),
                    CborValue::Bytes(values)
                        if super::persist_effective_kind(&child) == RuntimeValueKind::Int =>
                    {
                        Ok(MirRuntimeValue::Bytes(values.clone()))
                    }
                    _ => Err(format!("queued #Job `{}` expects an array", descriptor.name)),
                }
            }
            RuntimeValueKind::Map => {
                let key = job_child_descriptor(rt, descriptor, descriptor.key, "key")?;
                let child = job_child_descriptor(rt, descriptor, descriptor.value, "value")?;
                if super::persist_effective_kind(&key) != RuntimeValueKind::String {
                    return Err(format!(
                        "queued #Job map `{}` needs string keys in canonical CBOR",
                        descriptor.name
                    ));
                }
                let CborValue::Object(entries) = value else {
                    return Err(format!("queued #Job `{}` expects an object map", descriptor.name));
                };
                entries
                    .iter()
                    .map(|(name, value)| {
                        Ok((
                            MirConstKey::String(name.clone()),
                            job_decode_value(rt, value, &child, depth + 1)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()
                    .map(MirRuntimeValue::Map)
            }
            RuntimeValueKind::Shared => {
                let child = job_child_descriptor(rt, descriptor, descriptor.element, "element")?;
                job_decode_value(rt, value, &child, depth + 1)
            }
            RuntimeValueKind::Option => {
                let child = job_child_descriptor(rt, descriptor, descriptor.ok, "option")?;
                if matches!(value, CborValue::Null) {
                    Ok(job_absent(&child))
                } else {
                    Ok(MirRuntimeValue::Present(Box::new(job_decode_value(
                        rt,
                        value,
                        &child,
                        depth + 1,
                    )?)))
                }
            }
            RuntimeValueKind::Result => {
                let ok = job_child_descriptor(rt, descriptor, descriptor.ok, "result ok")?;
                let err = job_child_descriptor(rt, descriptor, descriptor.err, "result err")?;
                if let CborValue::Object(entries) = value {
                    if let Some((_, payload)) = entries.iter().find(|(name, _)| name == "Ok") {
                        return Ok(MirRuntimeValue::Present(Box::new(job_decode_value(
                            rt, payload, &ok, depth + 1,
                        )?)));
                    }
                    if let Some((_, payload)) = entries.iter().find(|(name, _)| name == "Err") {
                        return Ok(MirRuntimeValue::FailedTold(Box::new(job_decode_value(
                            rt, payload, &err, depth + 1,
                        )?)));
                    }
                }
                Ok(MirRuntimeValue::Present(Box::new(job_decode_value(
                    rt, value, &ok, depth + 1,
                )?)))
            }
            RuntimeValueKind::Record => {
                let CborValue::Object(entries) = value else {
                    return Err(format!("queued #Job record `{}` expects an object", descriptor.name));
                };
                job_decode_record(rt, descriptor, entries, depth + 1)
            }
            RuntimeValueKind::Enum => job_decode_enum(rt, descriptor, value, depth + 1),
            RuntimeValueKind::Named | RuntimeValueKind::Handle => Err(format!(
                "queued #Job `{}` has no checked runtime carrier",
                descriptor.name
            )),
            RuntimeValueKind::Closure
            | RuntimeValueKind::View
            | RuntimeValueKind::Iterator => Err(format!(
                "queued #Job `{}` cannot carry a callable or iterator",
                descriptor.name
            )),
        }
    }


    fn job_marshal_value(
        rt: &mut JitRuntime,
        value: &MirRuntimeValue,
        descriptor: &RuntimeTypeDescriptor,
        depth: usize,
    ) -> Result<i64, String> {
        if depth > 128 {
            return Err("queued #Job payload exceeds the descriptor depth limit".to_string());
        }
        let kind = super::persist_effective_kind(descriptor);
        match kind {
            RuntimeValueKind::Unit => matches!(value, MirRuntimeValue::Unit)
                .then_some(0)
                .ok_or_else(|| format!("queued #Job `{}` expects unit", descriptor.name)),
            RuntimeValueKind::Int => match value {
                MirRuntimeValue::Int(value) => Ok(*value),
                _ => Err(format!("queued #Job `{}` expects an integer", descriptor.name)),
            },
            RuntimeValueKind::Float => match value {
                MirRuntimeValue::Float { value, f32 } => {
                    if descriptor.abi == RuntimeValueAbi::Float32 {
                        Ok((*value as f32).to_bits() as i64)
                    } else if !*f32 {
                        Ok(value.to_bits() as i64)
                    } else {
                        Err(format!("queued #Job `{}` float width changed", descriptor.name))
                    }
                }
                _ => Err(format!("queued #Job `{}` expects a float", descriptor.name)),
            },
            RuntimeValueKind::Bool => match value {
                MirRuntimeValue::Bool(value) => Ok(i64::from(*value)),
                _ => Err(format!("queued #Job `{}` expects a bool", descriptor.name)),
            },
            RuntimeValueKind::Char => match value {
                MirRuntimeValue::Char(value) => Ok(i64::from(*value as u32)),
                _ => Err(format!("queued #Job `{}` expects a character", descriptor.name)),
            },
            RuntimeValueKind::String => match value {
                MirRuntimeValue::String(value) => Ok(rt.heap.alloc_string(value.clone())),
                _ => Err(format!("queued #Job `{}` expects text", descriptor.name)),
            },
            RuntimeValueKind::List => {
                let child = job_child_descriptor(rt, descriptor, descriptor.element, "element")?;
                let values = match value {
                    MirRuntimeValue::List(values) => values.clone(),
                    MirRuntimeValue::Bytes(values) => values
                        .iter()
                        .map(|value| MirRuntimeValue::Int(i64::from(*value)))
                        .collect(),
                    _ => return Err(format!("queued #Job `{}` expects a list", descriptor.name)),
                };
                let list = rt.heap.alloc_empty_list();
                for value in &values {
                    let raw = job_marshal_value(rt, value, &child, depth + 1)?;
                    rt.heap
                        .list_push_int(list, raw)
                        .ok_or_else(|| "queued #Job list allocation failed".to_string())?;
                }
                Ok(list)
            }
            RuntimeValueKind::Map => {
                let key = job_child_descriptor(rt, descriptor, descriptor.key, "key")?;
                let child = job_child_descriptor(rt, descriptor, descriptor.value, "value")?;
                let MirRuntimeValue::Map(values) = value else {
                    return Err(format!("queued #Job `{}` expects a map", descriptor.name));
                };
                let map = rt.heap.alloc_empty_map();
                for (entry_key, entry_value) in values {
                    let raw = job_marshal_value(rt, entry_value, &child, depth + 1)?;
                    match (entry_key, super::persist_effective_kind(&key)) {
                        (MirConstKey::String(key), RuntimeValueKind::String) => {
                            let key = rt.heap.alloc_string(key.clone());
                            rt.heap
                                .map_insert(map, key, raw)
                                .ok_or_else(|| "queued #Job map allocation failed".to_string())?;
                        }
                        (MirConstKey::Int(key), RuntimeValueKind::Int) => {
                            rt.heap
                                .map_insert_int(map, *key, raw)
                                .ok_or_else(|| "queued #Job map allocation failed".to_string())?;
                        }
                        (MirConstKey::Bool(key), RuntimeValueKind::Bool) => {
                            rt.heap
                                .map_insert_bool(map, *key, raw)
                                .ok_or_else(|| "queued #Job map allocation failed".to_string())?;
                        }
                        _ => {
                            return Err(format!(
                                "queued #Job map key does not match `{}`",
                                key.name
                            ));
                        }
                    }
                }
                Ok(map)
            }
            RuntimeValueKind::Shared => {
                let child = job_child_descriptor(rt, descriptor, descriptor.element, "element")?;
                let raw = job_marshal_value(rt, value, &child, depth + 1)?;
                Ok(super::Memory::shared_alloc_for_persist(rt, raw))
            }
            RuntimeValueKind::Option => {
                let child = job_child_descriptor(rt, descriptor, descriptor.ok, "option")?;
                match value {
                    MirRuntimeValue::Present(value) => {
                        let raw = job_marshal_value(rt, value, &child, depth + 1)?;
                        Ok(alloc_jit_result(rt, true, raw as u64))
                    }
                    MirRuntimeValue::Absent { .. } => Ok(alloc_jit_result(rt, false, 0)),
                    _ => Err(format!("queued #Job `{}` expects an option", descriptor.name)),
                }
            }
            RuntimeValueKind::Result => {
                let child_ok = job_child_descriptor(rt, descriptor, descriptor.ok, "result ok")?;
                let child_err = job_child_descriptor(rt, descriptor, descriptor.err, "result err")?;
                match value {
                    MirRuntimeValue::Present(value) => {
                        let raw = job_marshal_value(rt, value, &child_ok, depth + 1)?;
                        Ok(alloc_jit_result(rt, true, raw as u64))
                    }
                    MirRuntimeValue::FailedTold(value) => {
                        let raw = job_marshal_value(rt, value, &child_err, depth + 1)?;
                        Ok(alloc_jit_result(rt, false, raw as u64))
                    }
                    _ => Err(format!("queued #Job `{}` expects a result", descriptor.name)),
                }
            }
            RuntimeValueKind::Record => {
                let MirRuntimeValue::Struct { fields, .. } = value else {
                    return Err(format!("queued #Job `{}` expects a record", descriptor.name));
                };
                let record = rt.heap.alloc_record(descriptor.fields.len());
                for field in &descriptor.fields {
                    if field.skip {
                        return Err(format!(
                            "queued #Job record `{}` contains a skipped field `{}`",
                            descriptor.name, field.source_name
                        ));
                    }
                    let child = job_child_descriptor(rt, descriptor, Some(field.type_id), &field.source_name)?;
                    let value = fields
                        .iter()
                        .find(|(name, _)| field.matches_name(name, ShapeProjectionKind::Json))
                        .map(|(_, value)| value)
                        .ok_or_else(|| {
                            format!(
                                "queued #Job record `{}` is missing field `{}`",
                                descriptor.name, field.source_name
                            )
                        })?;
                    let raw = job_marshal_value(rt, value, &child, depth + 1)?;
                    super::write_typed_record_field(rt, record, field.index, raw, &child)?;
                }
                Ok(record)
            }
            RuntimeValueKind::Enum => {
                let MirRuntimeValue::Enum { variant, args, .. } = value else {
                    return Err(format!("queued #Job `{}` expects an enum", descriptor.name));
                };
                let row = descriptor
                    .variants
                    .iter()
                    .find(|row| row.name == *variant || row.wire_name == *variant)
                    .ok_or_else(|| format!("queued #Job `{}` has unknown variant `{variant}`", descriptor.name))?;
                if args.len() != row.fields.len() {
                    return Err(format!(
                        "queued #Job enum `{}` variant `{variant}` payload arity changed",
                        descriptor.name
                    ));
                }
                if row.fields.len() <= 1 {
                    let payload = if let Some((_, value)) = args.first() {
                        let child = job_child_descriptor(rt, descriptor, Some(row.fields[0].type_id), "enum payload")?;
                        job_marshal_value(rt, value, &child, depth + 1)?
                    } else {
                        0
                    };
                    Ok(payload.wrapping_shl(8) | row.discriminant)
                } else {
                    let record = rt.heap.alloc_record(row.fields.len() + 1);
                    rt.heap
                        .record_set_int(record, 0, row.discriminant)
                        .ok_or_else(|| "queued #Job enum allocation failed".to_string())?;
                    for (index, field) in row.fields.iter().enumerate() {
                        let child = job_child_descriptor(rt, descriptor, Some(field.type_id), &field.source_name)?;
                        let raw = job_marshal_value(rt, &args[index].1, &child, depth + 1)?;
                        super::write_typed_record_field(rt, record, index + 1, raw, &child)?;
                    }
                    Ok(record)
                }
            }
            RuntimeValueKind::Named | RuntimeValueKind::Handle => Err(format!(
                "queued #Job `{}` has no checked runtime carrier",
                descriptor.name
            )),
            RuntimeValueKind::Closure
            | RuntimeValueKind::View
            | RuntimeValueKind::Iterator => Err(format!(
                "queued #Job `{}` cannot carry a callable or iterator",
                descriptor.name
            )),
        }
    }

    #[derive(Clone, Copy)]
    enum JobRaw {
        Int(i64),
        Float(f64),
        Float32(f32),
        Bool(i8),
        Char(i32),
    }

    fn job_raw(abi: RuntimeValueAbi, raw: i64) -> Result<JobRaw, String> {
        match abi {
            RuntimeValueAbi::Int | RuntimeValueAbi::Handle => Ok(JobRaw::Int(raw)),
            RuntimeValueAbi::Float => Ok(JobRaw::Float(f64::from_bits(raw as u64))),
            RuntimeValueAbi::Float32 => Ok(JobRaw::Float32(f32::from_bits(raw as u32))),
            RuntimeValueAbi::Bool => Ok(JobRaw::Bool(raw as i8)),
            RuntimeValueAbi::Char => Ok(JobRaw::Char(raw as i32)),
            RuntimeValueAbi::Unit => Err("queued #Job has a unit input without a machine ABI".to_string()),
        }
    }

    fn invoke_job_callback(adapter: &JitJobAdapter, arg: JobRaw) -> Result<Option<JobRaw>, String> {
        macro_rules! call {
            ($arg_ty:ty, $arg:expr, $ret_ty:ty, $wrap:expr) => {{
                let result = unsafe {
                    if adapter.callback.has_env {
                        let function: unsafe extern "C" fn(i64, $arg_ty) -> $ret_ty =
                            std::mem::transmute(adapter.callback.fn_ptr as usize);
                        function(adapter.callback.env, $arg)
                    } else {
                        let function: unsafe extern "C" fn($arg_ty) -> $ret_ty =
                            std::mem::transmute(adapter.callback.fn_ptr as usize);
                        function($arg)
                    }
                };
                $wrap(result)
            }};
        }
        macro_rules! call_unit {
            ($arg_ty:ty, $arg:expr) => {{
                unsafe {
                    if adapter.callback.has_env {
                        let function: unsafe extern "C" fn(i64, $arg_ty) =
                            std::mem::transmute(adapter.callback.fn_ptr as usize);
                        function(adapter.callback.env, $arg);
                    } else {
                        let function: unsafe extern "C" fn($arg_ty) =
                            std::mem::transmute(adapter.callback.fn_ptr as usize);
                        function($arg);
                    }
                }
                Ok(None)
            }};
        }
        match (arg, adapter.return_abi) {
            (JobRaw::Int(arg), RuntimeValueAbi::Unit) => call_unit!(i64, arg),
            (JobRaw::Float(arg), RuntimeValueAbi::Unit) => call_unit!(f64, arg),
            (JobRaw::Float32(arg), RuntimeValueAbi::Unit) => call_unit!(f32, arg),
            (JobRaw::Bool(arg), RuntimeValueAbi::Unit) => call_unit!(i8, arg),
            (JobRaw::Char(arg), RuntimeValueAbi::Unit) => call_unit!(i32, arg),
            (JobRaw::Int(arg), RuntimeValueAbi::Int | RuntimeValueAbi::Handle) => {
                call!(i64, arg, i64, |value| Ok(Some(JobRaw::Int(value))))
            }
            (JobRaw::Int(arg), RuntimeValueAbi::Float) => {
                call!(i64, arg, f64, |value| Ok(Some(JobRaw::Float(value))))
            }
            (JobRaw::Int(arg), RuntimeValueAbi::Float32) => {
                call!(i64, arg, f32, |value| Ok(Some(JobRaw::Float32(value))))
            }
            (JobRaw::Int(arg), RuntimeValueAbi::Bool) => {
                call!(i64, arg, i8, |value| Ok(Some(JobRaw::Bool(value))))
            }
            (JobRaw::Int(arg), RuntimeValueAbi::Char) => {
                call!(i64, arg, i32, |value| Ok(Some(JobRaw::Char(value))))
            }
            (JobRaw::Float(arg), RuntimeValueAbi::Int | RuntimeValueAbi::Handle) => {
                call!(f64, arg, i64, |value| Ok(Some(JobRaw::Int(value))))
            }
            (JobRaw::Float(arg), RuntimeValueAbi::Float) => {
                call!(f64, arg, f64, |value| Ok(Some(JobRaw::Float(value))))
            }
            (JobRaw::Float(arg), RuntimeValueAbi::Float32) => {
                call!(f64, arg, f32, |value| Ok(Some(JobRaw::Float32(value))))
            }
            (JobRaw::Float(arg), RuntimeValueAbi::Bool) => {
                call!(f64, arg, i8, |value| Ok(Some(JobRaw::Bool(value))))
            }
            (JobRaw::Float(arg), RuntimeValueAbi::Char) => {
                call!(f64, arg, i32, |value| Ok(Some(JobRaw::Char(value))))
            }
            (JobRaw::Float32(arg), RuntimeValueAbi::Int | RuntimeValueAbi::Handle) => {
                call!(f32, arg, i64, |value| Ok(Some(JobRaw::Int(value))))
            }
            (JobRaw::Float32(arg), RuntimeValueAbi::Float) => {
                call!(f32, arg, f64, |value| Ok(Some(JobRaw::Float(value))))
            }
            (JobRaw::Float32(arg), RuntimeValueAbi::Float32) => {
                call!(f32, arg, f32, |value| Ok(Some(JobRaw::Float32(value))))
            }
            (JobRaw::Float32(arg), RuntimeValueAbi::Bool) => {
                call!(f32, arg, i8, |value| Ok(Some(JobRaw::Bool(value))))
            }
            (JobRaw::Float32(arg), RuntimeValueAbi::Char) => {
                call!(f32, arg, i32, |value| Ok(Some(JobRaw::Char(value))))
            }
            (JobRaw::Bool(arg), RuntimeValueAbi::Int | RuntimeValueAbi::Handle) => {
                call!(i8, arg, i64, |value| Ok(Some(JobRaw::Int(value))))
            }
            (JobRaw::Bool(arg), RuntimeValueAbi::Float) => {
                call!(i8, arg, f64, |value| Ok(Some(JobRaw::Float(value))))
            }
            (JobRaw::Bool(arg), RuntimeValueAbi::Float32) => {
                call!(i8, arg, f32, |value| Ok(Some(JobRaw::Float32(value))))
            }
            (JobRaw::Bool(arg), RuntimeValueAbi::Bool) => {
                call!(i8, arg, i8, |value| Ok(Some(JobRaw::Bool(value))))
            }
            (JobRaw::Bool(arg), RuntimeValueAbi::Char) => {
                call!(i8, arg, i32, |value| Ok(Some(JobRaw::Char(value))))
            }
            (JobRaw::Char(arg), RuntimeValueAbi::Int | RuntimeValueAbi::Handle) => {
                call!(i32, arg, i64, |value| Ok(Some(JobRaw::Int(value))))
            }
            (JobRaw::Char(arg), RuntimeValueAbi::Float) => {
                call!(i32, arg, f64, |value| Ok(Some(JobRaw::Float(value))))
            }
            (JobRaw::Char(arg), RuntimeValueAbi::Float32) => {
                call!(i32, arg, f32, |value| Ok(Some(JobRaw::Float32(value))))
            }
            (JobRaw::Char(arg), RuntimeValueAbi::Bool) => {
                call!(i32, arg, i8, |value| Ok(Some(JobRaw::Bool(value))))
            }
            (JobRaw::Char(arg), RuntimeValueAbi::Char) => {
                call!(i32, arg, i32, |value| Ok(Some(JobRaw::Char(value))))
            }
        }
    }



    fn job_datatree_to_cbor(value: &crate::Encoding::json_rt::DataTree) -> CborValue {
        match value {
            crate::Encoding::json_rt::DataTree::Null => CborValue::Null,
            crate::Encoding::json_rt::DataTree::Bool(value) => CborValue::Bool(*value),
            crate::Encoding::json_rt::DataTree::Int(value) => CborValue::Int(*value),
            crate::Encoding::json_rt::DataTree::Float(value) => CborValue::Float(*value),
            crate::Encoding::json_rt::DataTree::Number(value)
            | crate::Encoding::json_rt::DataTree::TypedText(value)
            | crate::Encoding::json_rt::DataTree::Text(value) => CborValue::Text(value.clone()),
            crate::Encoding::json_rt::DataTree::Bytes(value) => CborValue::Bytes(value.clone()),
            crate::Encoding::json_rt::DataTree::Array(values) => {
                CborValue::Array(values.iter().map(job_datatree_to_cbor).collect())
            }
            crate::Encoding::json_rt::DataTree::Object(entries) => CborValue::Object(
                entries
                    .iter()
                    .map(|(name, value)| (name.clone(), job_datatree_to_cbor(value)))
                    .collect(),
            ),
        }
    }

    fn job_encode_payload(
        rt: &mut JitRuntime,
        raw: i64,
        descriptor: &RuntimeTypeDescriptor,
    ) -> Result<Vec<u8>, String> {
        let tree = crate::Receipt::encode_jit_value(rt, raw, descriptor)?;
        CborKernel::encode(&job_datatree_to_cbor(&tree), true)
            .map_err(|error| format!("queued #Job result CBOR encoding failed: {error:?}"))
    }

    fn job_error(
        payload: &jet_codegen::Comptime::ServicesLite::JetJobPayload,
        reason: &str,
        detail: impl Into<Option<String>>,
    ) -> jet_codegen::Comptime::ServicesLite::JetJobError {
        jet_codegen::Comptime::ServicesLite::JetJobError {
            type_id: payload.type_id.clone(),
            reason: reason.to_string(),
            detail: detail.into(),
        }
    }

    fn job_result(
        rt: &mut JitRuntime,
        payload: &jet_codegen::Comptime::ServicesLite::JetJobPayload,
        adapter: JitJobAdapter,
        raw: Option<JobRaw>,
    ) -> Result<jet_codegen::Comptime::ServicesLite::JetJobResult, jet_codegen::Comptime::ServicesLite::JetJobError> {
        let descriptor = job_descriptor(rt, adapter.return_type, &payload.type_id)
            .map_err(|error| job_error(payload, "encode", Some(error)))?;
        let kind = super::persist_effective_kind(&descriptor);
        if kind == RuntimeValueKind::Unit {
            return Ok(jet_codegen::Comptime::ServicesLite::JetJobResult {
                type_id: "Unit".to_string(),
                bytes: Vec::new(),
                publish: false,
            });
        }
        let Some(raw) = raw else {
            return Err(job_error(
                payload,
                "callback_failed",
                Some("compiled #Job returned no value for a non-Unit result".to_string()),
            ));
        };
        let raw = match raw {
            JobRaw::Int(value) => value,
            JobRaw::Float(value) => value.to_bits() as i64,
            JobRaw::Float32(value) => value.to_bits() as i64,
            JobRaw::Bool(value) => i64::from(value),
            JobRaw::Char(value) => i64::from(value),
        };
        if kind == RuntimeValueKind::Result {
            let Some((ok, value)) = super::jit_result_parts(rt, raw) else {
                return Err(job_error(
                    payload,
                    "callback_failed",
                    Some("compiled #Job returned an invalid Result carrier".to_string()),
                ));
            };
            if ok {
                let child = job_child_descriptor(rt, &descriptor, descriptor.ok, "result ok")
                    .map_err(|error| job_error(payload, "encode", Some(error)))?;
                let bytes = job_encode_payload(rt, value as i64, &child)
                    .map_err(|error| job_error(payload, "encode", Some(error)))?;
                return Ok(jet_codegen::Comptime::ServicesLite::JetJobResult {
                    type_id: child.name,
                    bytes,
                    publish: false,
                });
            }
            let child = job_child_descriptor(rt, &descriptor, descriptor.err, "result err")
                .map_err(|error| job_error(payload, "callback_failed", Some(error)))?;
            let detail = crate::Receipt::encode_jit_value(rt, value as i64, &child)
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|error| error);
            return Err(job_error(payload, "callback_failed", Some(detail)));
        }
        let bytes = job_encode_payload(rt, raw, &descriptor)
            .map_err(|error| job_error(payload, "encode", Some(error)))?;
        Ok(jet_codegen::Comptime::ServicesLite::JetJobResult {
            type_id: descriptor.name,
            bytes,
            publish: false,
        })
    }

    fn dispatch_job(
        job_type: &str,
        payload: &jet_codegen::Comptime::ServicesLite::JetJobPayload,
    ) -> Result<jet_codegen::Comptime::ServicesLite::JetJobResult, jet_codegen::Comptime::ServicesLite::JetJobError> {
        let no_runtime = || {
            job_error(
                payload,
                "missing_invocation_adapter",
                Some(format!("JIT has no active resident runtime for #Job `{job_type}`")),
            )
        };
        let (adapter, arg) = super::Concurrency::with_runtime_result(no_runtime(), |rt| {
            let adapter = *rt.job_adapters.get(job_type).ok_or_else(|| {
                job_error(
                    payload,
                    "missing_invocation_adapter",
                    Some(format!("JIT has no checked typed queue adapter for #Job `{job_type}`")),
                )
            })?;
            let descriptor = job_descriptor(rt, adapter.input_type, job_type)
                .map_err(|error| job_error(payload, "decode", Some(error)))?;
            if payload.type_id != descriptor.name && payload.type_id != descriptor.canonical {
                return Err(job_error(
                    payload,
                    "payload_type_mismatch",
                    Some(format!(
                        "queued #Job `{job_type}` expects payload type `{}`",
                        descriptor.name
                    )),
                ));
            }
            let value = CborKernel::decode(&payload.bytes, &CborKernel::Options::safe(), true)
                .map_err(|error| job_error(payload, "decode", Some(format!("{error:?}"))))?;
            let value = job_decode_value(rt, &value, &descriptor, 0)
                .map_err(|error| job_error(payload, "decode", Some(error)))?;
            let raw = job_marshal_value(rt, &value, &descriptor, 0)
                .map_err(|error| job_error(payload, "decode", Some(error)))?;
            let arg = job_raw(adapter.input_abi, raw)
                .map_err(|error| job_error(payload, "decode", Some(error)))?;
            Ok((adapter, arg))
        })?;
        let result = invoke_job_callback(&adapter, arg)
            .map_err(|error| job_error(payload, "callback_failed", Some(error)))?;
        super::Concurrency::with_runtime_result(no_runtime(), |rt| {
            job_result(rt, payload, adapter, result)
        })
    }
    pub(super) fn tick_worker_queue(
        endpoint: &service_prelude::JetServiceEndpoint,
    ) -> Result<(), String> {
        jet_codegen::Comptime::ServicesLite::jet_job_service_queue_tick_dispatch(
            endpoint,
            16,
            |job_type, payload| dispatch_job(job_type, payload),
        )
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
    }

    pub(super) fn call(
        rt: &mut JitRuntime,
        module: i64,
        method_handle: i64,
        argc: i64,
        raw: [i64; 7],
    ) -> i64 {
        let Some(method) = rt.heap.clone_string(method_handle) else {
            return 0;
        };
        let Some(count) = usize::try_from(argc)
            .ok()
            .filter(|count| *count <= raw.len())
        else {
            return 0;
        };
        let value = call_runtime(rt, module, &method, &raw[..count]);
        if module == SYNC_MODULE && method == "map_get" {
            marshal_option(rt, value)
        } else if module == SERVICES_MODULE && method == "runtime" {
            marshal_scalar(rt, &value)
        } else {
            marshal_result(rt, value)
        }
    }

    pub(super) fn call_bool(
        rt: &mut JitRuntime,
        module: i64,
        method_handle: i64,
        argc: i64,
        raw: [i64; 7],
    ) -> i8 {
        call(rt, module, method_handle, argc, raw) as i8
    }

    pub(super) fn show(rt: &mut JitRuntime, handle: i64) -> i64 {
        let Some(value) = service_value(rt, handle) else {
            rt.set_trap("the JIT received an invalid service value handle");
            return 0;
        };
        let Some(rendered) = service_prelude::service_display_runtime(&value) else {
            rt.set_trap("the JIT received an unsupported service display value");
            return 0;
        };
        rt.heap.alloc_string(rendered)
    }

    // Only unpack the scalar enum ABI here; the shared Prelude owns rendering.
    pub(super) fn show_enum(rt: &mut JitRuntime, type_name: &str, raw: i64) -> i64 {
        let variant = match (type_name, raw) {
            ("ServiceRestart", 0) => "OneForOne",
            ("ServiceRestart", 1) => "OneForAll",
            ("ServiceRestart", 2) => "RestForOne",
            ("ServiceDelivery", 0) => "AtMostOnce",
            ("ServiceDelivery", 1) => "DurableAtLeastOnce",
            _ => {
                rt.set_trap("the JIT received an invalid service enum value");
                return 0;
            }
        };
        let value = MirRuntimeValue::Enum {
            type_name: type_name.to_string(),
            variant: variant.to_string(),
            args: Vec::new(),
        };
        let Some(rendered) = service_prelude::service_display_runtime(&value) else {
            rt.set_trap("the JIT received an unsupported service enum display value");
            return 0;
        };
        rt.heap.alloc_string(rendered)
    }
}

fn invoke_jit_callable_zero(handler: &JitCallableSlot) {
    // Zero-input callbacks are invoked only from host code, after the runtime
    // borrow has ended.  Captured closures prepend their checked environment.
    unsafe {
        if handler.has_env {
            let function: unsafe extern "C" fn(i64) =
                std::mem::transmute(handler.fn_ptr as usize);
            function(handler.env);
        } else {
            let function: unsafe extern "C" fn() =
                std::mem::transmute(handler.fn_ptr as usize);
            function();
        }
    }
}


fn jet_jit_service_call(
    module: i64,
    method: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
) -> i64 {
    let raw = [a0, a1, a2, a3, a4, a5, a6];
    let is_start = Concurrency::with_runtime_mut(|rt| {
        module == 0 && rt.heap.clone_string(method).as_deref() == Some("start")
    });
    if !is_start {
        return Concurrency::with_runtime_mut(|rt| {
            service_adapter::call(rt, module, method, argc, raw)
        });
    }
    if argc != 1 {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("JIT service.start requires one checked ServiceTree receiver");
        });
        return 0;
    }
    service_adapter::start(a0)
}

fn jet_jit_service_call_bool(
    module: i64,
    method: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        service_adapter::call_bool(rt, module, method, argc, [a0, a1, a2, a3, a4, a5, a6])
    })
}

fn jet_jit_service_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| service_adapter::show(rt, handle))
}

fn jet_jit_service_restart_show(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| service_adapter::show_enum(rt, "ServiceRestart", value))
}

fn jet_jit_service_delivery_show(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| service_adapter::show_enum(rt, "ServiceDelivery", value))
}

pub(crate) fn alloc_io_error_result(
    rt: &mut JitRuntime,
    variant: i64,
    operation: i64,
    resource: Option<&str>,
    cause: &str,
) -> i64 {
    let context = rt.heap.alloc_record(4);
    let _ = rt.heap.record_set_int(context, 0, operation);
    let resource = resource
        .map(|value| rt.heap.alloc_string(value.to_string()).wrapping_add(1))
        .unwrap_or(0);
    let _ = rt.heap.record_set_int(context, 1, resource);
    let _ = rt.heap.record_set_int(context, 2, 0);
    let cause = rt.heap.alloc_string(cause.to_string()).wrapping_add(1);
    let _ = rt.heap.record_set_int(context, 3, cause);
    alloc_jit_result(rt, false, (context as u64).wrapping_shl(8) | variant as u64)
}

pub(crate) fn result_err_terminal(error: crate::IO::term_prelude::JetTermSecretError) -> i64 {
    let projection = crate::IO::term_prelude::jet_term_secret_error_projection(&error);
    let cause = error.message();
    let variant_name = match projection.kind {
        crate::IO::term_prelude::JetTermSecretErrorKind::InvalidInput => "InvalidInput",
        crate::IO::term_prelude::JetTermSecretErrorKind::Other => "Other",
    };
    let operation_name = match projection.operation {
        crate::IO::term_prelude::JetTermSecretErrorOperation::Read => "Read",
        crate::IO::term_prelude::JetTermSecretErrorOperation::Flush => "Flush",
    };
    let resource = Some(projection.resource);
    let variant = jet_foundation::Syntax::IO_ERROR_VARIANTS
        .iter()
        .position(|name| *name == variant_name)
        .expect("Prelude IOError variants must be registered") as i64;
    let operation = jet_foundation::Syntax::IO_OPERATION_VARIANTS
        .iter()
        .position(|name| *name == operation_name)
        .expect("Prelude IOOperation variants must be registered") as i64;
    Concurrency::with_runtime_mut(|rt| {
        alloc_io_error_result(rt, variant, operation, resource, &cause)
    })
}
pub(crate) fn jit_result(rt: &JitRuntime, handle: i64) -> Option<JitResultValue> {
    usize::try_from(handle)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| rt.results.get(index).copied())
}

pub(crate) fn jit_result_parts(rt: &JitRuntime, handle: i64) -> Option<(bool, u64)> {
    jit_result(rt, handle).map(|result| (result.ok, result.bits))
}

pub(crate) fn jit_result_i64(rt: &JitRuntime, handle: i64) -> Option<i64> {
    jit_result(rt, handle).map(|result| result.bits as i64)
}

pub(crate) fn jit_result_is_ok(rt: &JitRuntime, handle: i64) -> Option<bool> {
    jit_result(rt, handle).map(|result| result.ok)
}

fn jet_jit_result_new_i64(ok: i8, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u64))
}

fn duration_range_error(rt: &mut JitRuntime, reason: &str) -> i64 {
    let error = rt.heap.alloc_record(1);
    let reason = rt.heap.alloc_string(reason);
    let _ = rt.heap.record_set_string(error, 0, reason);
    alloc_jit_result(rt, false, error as u64)
}

fn duration_from_int_result(rt: &mut JitRuntime, value: i64, scale: i64) -> i64 {
    match duration_kernel::jet_duration_kernel_from_int(value, scale) {
        Some(value) => alloc_jit_result(rt, true, value as u64),
        None => duration_range_error(
            rt,
            duration_kernel::jet_duration_kernel_int_error_reason(),
        ),
    }
}

fn duration_from_float_result(rt: &mut JitRuntime, value: f64, scale: i64) -> i64 {
    match duration_kernel::jet_duration_kernel_from_float(value, scale) {
        Some(value) => alloc_jit_result(rt, true, value as u64),
        None => duration_range_error(
            rt,
            duration_kernel::jet_duration_kernel_float_error_reason(),
        ),
    }
}

fn jet_jit_duration_from_int(value: i64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| duration_from_int_result(rt, value, scale))
}

fn jet_jit_duration_from_float(value: f64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| duration_from_float_result(rt, value, scale))
}

fn duration_unit_scale(unit: i64) -> Option<i64> {
    // DurationUnit discriminants are declared in Prelude CommonTypes.
    match unit {
        0 => Some(1),                 // Nanoseconds
        1 => Some(1_000),             // Microseconds
        2 => Some(1_000_000),         // Milliseconds
        3 => Some(1_000_000_000),     // Seconds
        4 => Some(60_000_000_000),    // Minutes
        5 => Some(3_600_000_000_000), // Hours
        _ => None,
    }
}

fn jet_jit_duration_from_int_unit(value: i64, unit: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(value) = rt.heap.int_to_i64(value) else {
            return duration_range_error(
                rt,
                duration_kernel::jet_duration_kernel_int_error_reason(),
            );
        };
        let Some(scale) = duration_unit_scale(unit) else {
            return duration_range_error(
                rt,
                duration_kernel::jet_duration_kernel_int_error_reason(),
            );
        };
        duration_from_int_result(rt, value, scale)
    })
}

fn jet_jit_duration_from_float_unit(value: f64, unit: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(scale) = duration_unit_scale(unit) else {
            return duration_range_error(
                rt,
                duration_kernel::jet_duration_kernel_float_error_reason(),
            );
        };
        duration_from_float_result(rt, value, scale)
    })
}

fn jet_jit_duration_in(value: i64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = rt.heap.int_from_i64(duration_kernel::jet_duration_kernel_in(value, scale));
        alloc_jit_result(
            rt,
            true,
            value as u64,
        )
    })
}

fn jet_jit_duration_in_unit(value: i64, unit: i64) -> i64 {
    let scale = duration_unit_scale(unit).unwrap_or(1);
    jet_jit_duration_in(value, scale)
}

fn jet_jit_duration_is_zero(value: i64) -> i8 {
    i8::from(duration_kernel::jet_duration_kernel_is_zero(value))
}

fn jet_jit_duration_total_seconds(value: i64) -> i64 {
    duration_kernel::jet_duration_kernel_total_seconds(value)
}

fn jet_jit_duration_seconds_value(value: i64) -> f64 {
    duration_kernel::jet_duration_kernel_seconds_value(value)
}

fn jet_jit_duration_add(left: i64, right: i64) -> i64 {
    duration_kernel::jet_duration_kernel_add(left, right)
}

fn jet_jit_duration_sub(left: i64, right: i64) -> i64 {
    duration_kernel::jet_duration_kernel_sub(left, right)
}

fn jet_jit_duration_difference(a: i64, b: i64) -> i64 {
    duration_kernel::jet_duration_kernel_difference(a, b)
}

fn jet_jit_duration_abs(value: i64) -> i64 {
    duration_kernel::jet_duration_kernel_abs(value)
}

fn jet_jit_duration_negated(value: i64) -> i64 {
    duration_kernel::jet_duration_kernel_negated(value)
}

fn jet_jit_duration_sign(value: i64) -> i64 {
    duration_kernel::jet_duration_kernel_sign(value)
}

fn jet_jit_duration_total_in(value: i64, unit: i64) -> f64 {
    let unit = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(unit).unwrap_or_default());
    duration_kernel::jet_duration_kernel_total_in(value, &unit)
}

fn jet_jit_duration_round(value: i64, unit: i64, increment: i64, mode: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let unit = rt.heap.clone_string(unit).unwrap_or_default();
        let mode = rt.heap.clone_string(mode).unwrap_or_default();
        duration_kernel::jet_duration_kernel_round(value, &unit, increment, &mode).unwrap_or(value)
    })
}

fn jet_jit_duration_scale(value: i64, factor: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match duration_kernel::jet_duration_kernel_scale(value, factor) {
            Some(value) => value,
            None => {
                rt.set_arithmetic_stop(
                    0,
                    duration_kernel::jet_duration_kernel_scale_error_reason(),
                );
                0
            }
        }
    })
}

fn jet_jit_duration_divide(value: i64, factor: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match duration_kernel::jet_duration_kernel_divide(value, factor) {
            Some(value) => value,
            None => {
                rt.set_arithmetic_stop(
                    0,
                    duration_kernel::jet_duration_kernel_scale_error_reason(),
                );
                0
            }
        }
    })
}

/// Marshalling only: the nanosecond carrier in, a resident string handle out.
/// The rendering itself stays in the shared Prelude kernel AOT's
/// `impl JetShow for Duration` calls.
fn jet_jit_duration_show(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .alloc_string(duration_kernel::jet_duration_kernel_show(value))
    })
}

fn jet_jit_result_new_f64(ok: i8, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value.to_bits()))
}

fn jit_callable_index(handle: i64) -> Option<usize> {
    handle
        .checked_neg()?
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
}

fn jit_callable_slot(rt: &JitRuntime, handle: i64) -> Option<JitCallableSlot> {
    jit_callable_index(handle).and_then(|index| rt.jit_callables.get(index).copied())
}

/// Read one checked resident function-value slot for a host adapter. The
/// callable ABI remains opaque to Prelude code; resident adapters may inspect
/// the already-validated pointer/environment pair when a shared Prelude
/// operation returns a new function value.
pub(crate) fn jit_callable_parts(rt: &JitRuntime, handle: i64) -> Option<JitCallableSlot> {
    jit_callable_slot(rt, handle)
}

pub(crate) fn register_jit_atexit(handler: i64) -> bool {
    Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = jit_callable_slot(rt, handler) else {
            rt.set_trap("invalid resident atexit callback");
            return false;
        };
        jet_foundation::Outcome::jet_runtime_register_atexit(&mut rt.atexit_handlers, slot);
        true
    })
}

/// Invoke the callbacks in registration order. The generated function value
/// ABI is either `extern "C" fn()` or `extern "C" fn(i64)` when it carries
/// the resident environment handle.
pub(crate) fn run_jit_atexit_handlers(rt: &mut JitRuntime) {
    jet_foundation::Outcome::jet_runtime_drain_atexit(&mut rt.atexit_handlers, |handler| {
        // SAFETY: `fn_ptr`, `env`, and `has_env` are written together by the
        // checked JIT callable binder. The callback signature is the zero-arg
        // `atexit` signature, with the environment word prepended only for a
        // captured closure.
        unsafe {
            if handler.has_env {
                let callback: extern "C" fn(i64) = std::mem::transmute(handler.fn_ptr as usize);
                callback(handler.env);
            } else {
                let callback: extern "C" fn() = std::mem::transmute(handler.fn_ptr as usize);
                callback();
            }
        }
    });
}

/// I2: a broken callable invariant is Jet's own defect, so it must never be
/// reported as the running program's runtime stop. `set_trap` renders
/// `Stop [E3001]` and exits 70 (`Outcome.rs::jet_render_runtime_stop_from_row`),
/// a breach report that blames the program for a compiler bug. The branded ICE
/// rail is the right one: `set_host_fault` records `ExitCodes::ICE` (101) and
/// `resident.rs::take_host_fault_outcome` hands the run to
/// `Diagnostics::render_ice_report` ("internal compiler error: … This is a bug
/// in jet, NOT in your program"). Deliberately not a deopt: re-running a
/// miscompiled callable on another tier can succeed and bury the miscompile,
/// which is precisely how this class of defect stays invisible.
///
/// Both trap channels are set, because generated code polls a different one per
/// tier: `jet_jit_is_trapped` reads `JitRuntime::trapped` on the resident tier
/// but the task-local slot inside a scheduler task, the same routing
/// `JitRuntime::store_trap` performs. Setting only the runtime flag would let an
/// in-task null callable sail past `emit_trap_check` into `call 0` anyway.
///
/// A status return, never a panic: cranelift-jit registers no `eh_frame` for
/// JIT'd code, so a panic here would have to unwind an FDE-less frame.
fn callable_defect(rt: &mut JitRuntime, what: &str) {
    rt.set_host_fault(what);
    if Concurrency::in_scheduler_task() {
        Concurrency::set_task_trap(what);
    }
}

/// The one place a `JitCallableSlot` is created, so the one place the
/// callable invariant is enforced: a slot always names a Cranelift function.
///
/// A zero `fn_ptr` is not a function address, it is a word that reached here
/// from something that was never a callable. Binding it anyway hands JIT'd
/// code a `call 0` — an I1 violation that dies as SIGSEGV with the faulting
/// address inside the JIT code page and no diagnostic.
fn bind_jit_callable(rt: &mut JitRuntime, fn_ptr: i64, env: i64, has_env: bool) -> i64 {
    if fn_ptr == 0 {
        callable_defect(rt, "resident callable value has no function address");
        return 0;
    }
    let index = rt.jit_callables.len();
    if index >= i64::MAX as usize - 1 {
        rt.set_trap("too many resident callable values");
        return 0;
    }
    rt.jit_callables.push(JitCallableSlot {
        fn_ptr,
        env,
        history_captures_owned: None,
        has_env,
        raw_unary: None,
        raw_pair: None,
        raw_many: None,
    });
    -(index as i64) - 1
}

fn bind_jit_callable_history_capture_mode(
    rt: &mut JitRuntime,
    handle: i64,
    owned: i8,
) -> bool {
    let Some(index) = jit_callable_index(handle) else {
        callable_defect(rt, "history callable provenance received an invalid handle");
        return false;
    };
    let Some(slot) = rt.jit_callables.get_mut(index) else {
        callable_defect(rt, "history callable provenance received an unknown handle");
        return false;
    };
    slot.history_captures_owned = Some(owned != 0);
    true
}

fn jet_jit_callable_bind_history_capture_mode(handle: i64, owned: i8) -> i8 {
    with_runtime_result(0, |rt| {
        i8::from(bind_jit_callable_history_capture_mode(rt, handle, owned))
    })
}
/// Bind a resident adapter that implements a function-value ABI thunk. The
/// thunk owns only marshalling; the operation it invokes remains in its shared
/// Prelude seam.
pub(crate) fn bind_jit_callable_handle(
    rt: &mut JitRuntime,
    fn_ptr: i64,
    env: i64,
    has_env: bool,
) -> i64 {
    bind_jit_callable(rt, fn_ptr, env, has_env)
}

fn jet_jit_callable_bind(fn_ptr: i64, env: i64, has_env: i8) -> i64 {
    with_runtime_result(0, |rt| bind_jit_callable(rt, fn_ptr, env, has_env != 0))
}
/// Attach the checked universal View callback thunk to an existing callable
/// slot.  The compiler proves the thunk's source signature before it reaches
/// this boundary; this host only enforces the one-pointer invariant and turns
/// the address into a typed function pointer once.
fn bind_jit_callable_raw(
    rt: &mut JitRuntime,
    handle: i64,
    unary_ptr: i64,
    pair_ptr: i64,
) -> bool {
    if (unary_ptr == 0) == (pair_ptr == 0) {
        callable_defect(
            rt,
            "universal callable binding requires exactly one nonzero thunk",
        );
        return false;
    }
    let Some(index) = jit_callable_index(handle) else {
        callable_defect(rt, "universal callable binding received an invalid handle");
        return false;
    };
    let Some(slot) = rt.jit_callables.get_mut(index) else {
        callable_defect(rt, "universal callable binding received an unknown handle");
        return false;
    };
    if slot.raw_unary.is_some() || slot.raw_pair.is_some() || slot.raw_many.is_some() {
        callable_defect(rt, "universal callable slot already has a thunk");
        return false;
    }
    if unary_ptr != 0 {
        // SAFETY: MIR lowering emits the thunk with the exact env-first unary
        // ABI before calling this binder.
        slot.raw_unary = Some(unsafe {
            std::mem::transmute::<usize, unsafe extern "C" fn(i64, i64) -> i64>(
                unary_ptr as usize,
            )
        });
    } else {
        // SAFETY: MIR lowering emits the thunk with the exact env-first pair
        // ABI before calling this binder.
        slot.raw_pair = Some(unsafe {
            std::mem::transmute::<usize, unsafe extern "C" fn(i64, i64, i64) -> i64>(
                pair_ptr as usize,
            )
        });
    }
    true
}

fn bind_jit_callable_raw_many(rt: &mut JitRuntime, handle: i64, thunk_ptr: i64) -> bool {
    if thunk_ptr == 0 {
        callable_defect(
            rt,
            "universal callable many binding requires a nonzero thunk",
        );
        return false;
    }
    let Some(index) = jit_callable_index(handle) else {
        callable_defect(rt, "universal callable binding received an invalid handle");
        return false;
    };
    let Some(slot) = rt.jit_callables.get_mut(index) else {
        callable_defect(rt, "universal callable binding received an unknown handle");
        return false;
    };
    if slot.raw_unary.is_some() || slot.raw_pair.is_some() || slot.raw_many.is_some() {
        callable_defect(rt, "universal callable slot already has a thunk");
        return false;
    }
    // SAFETY: MIR lowering emits the thunk with the exact env-first
    // `(i64, pointer-to-i64-array) -> i64` ABI before calling this binder.
    slot.raw_many = Some(unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn(i64, *const i64) -> i64>(
            thunk_ptr as usize,
        )
    });
    true
}

pub(crate) fn invoke_universal_unary(slot: JitCallableSlot, value: i64) -> Option<i64> {
    match (slot.raw_unary, slot.raw_pair, slot.raw_many) {
        (Some(callback), None, None) => {
            // SAFETY: `bind_jit_callable_raw` installs only the exact thunk ABI.
            Some(unsafe { callback(slot.env, value) })
        }
        _ => None,
    }
}

pub(crate) fn invoke_universal_pair(
    slot: JitCallableSlot,
    left: i64,
    right: i64,
) -> Option<i64> {
    match (slot.raw_unary, slot.raw_pair, slot.raw_many) {
        (None, Some(callback), None) => {
            // SAFETY: `bind_jit_callable_raw` installs only the exact thunk ABI.
            Some(unsafe { callback(slot.env, left, right) })
        }
        _ => None,
    }
}

pub(crate) fn invoke_universal_many(slot: JitCallableSlot, values: &[i64]) -> Option<i64> {
    match (slot.raw_unary, slot.raw_pair, slot.raw_many) {
        (None, None, Some(callback)) => {
            // SAFETY: `bind_jit_callable_raw_many` installs only the exact
            // env-first pointer thunk ABI, and `values` remains borrowed for
            // the duration of this synchronous callback.
            Some(unsafe { callback(slot.env, values.as_ptr()) })
        }
        _ => None,
    }
}

fn jet_jit_callable_bind_raw(handle: i64, unary_ptr: i64, pair_ptr: i64) -> i8 {
    with_runtime_result(0, |rt| {
        i8::from(bind_jit_callable_raw(rt, handle, unary_ptr, pair_ptr))
    })
}

fn jet_jit_callable_bind_raw_many(handle: i64, thunk_ptr: i64) -> i8 {
    with_runtime_result(0, |rt| {
        i8::from(bind_jit_callable_raw_many(rt, handle, thunk_ptr))
    })
}

/// Accept a callable value on its way into a call, and nothing else.
///
/// Every function value the lowering can produce is already a bound handle:
/// each `TExprKind::FnValue` arm ends in `callable_bind`
/// (`lower_ctx.rs:18769`, `:18834`, `:18995`, `:19054`, `:26956`), resident
/// adapters go through `bind_jit_callable_handle`, and a stored function value
/// keeps that handle. So an unrecognised word here is never a code address —
/// it is a word from something that was never a callable, and binding it would
/// mint a slot whose `fn_ptr` is that word. `jet_jit_callable_fn` then hands it
/// back and JIT'd code executes `call <arbitrary integer>`: an I1 violation
/// that dies as SIGSEGV inside the JIT code page with no diagnostic.
///
/// The old fallback bound any non-handle word as a code address, so it accepted
/// whole classes of word that cannot be one, and a zero-only guard closes just
/// the first of them:
///
/// - `0` — the observed crash: a result-arena Option handle decoded through the
///   packed carrier yields `Some(0)`.
/// - `1`, `2`, `3`, … — the *same* mis-decode one allocation later
///   (`arena_handle - 1`), plus list indexes, packed-carrier words, `Bool`
///   `0`/`1`, and packed enum tags. Page zero is never mapped.
/// - Heap handles — `JitHeap` string/list/map handles are small positive ints
///   too (the faulting frame held `0x11`, the `"hi"` String), indistinguishable
///   from a "code address" under the old rule.
/// - Odd or under-aligned words — a Cranelift function entry is at least
///   4-byte aligned (16 on x86-64), so an odd word is never an entry point.
/// - Non-canonical words — a big `Int`, a `Float` bit pattern, or a hash lands
///   outside the `0..=0x0000_7FFF_FFFF_FFFF` user range.
/// - Negative words that merely *look* like handles — `jit_callable_slot`
///   rejects `-(len + 1)` and below, and the fallback then bound them as
///   kernel-space addresses.
///
/// Enumerating impossible words can never be complete: a live data pointer is a
/// mapped address and still the wrong answer. So the rule is inverted to a
/// whitelist — only an already-bound handle passes, everything else is a defect.
fn jet_jit_callable_normalize(value: i64) -> i64 {
    with_runtime_result(0, |rt| {
        if jit_callable_slot(rt, value).is_some() {
            value
        } else {
            callable_defect(rt, "resident callable value was never bound");
            0
        }
    })
}

/// JIT representation of the canonical C callback boundary. The callable
/// handle remains the only valid function carrier; callback ABI validation is
/// performed against the MIR callback row before this host is called.
fn jet_jit_ffi_callback_boundary(value: i64) -> i64 {
    with_runtime_result(0, |rt| {
        if jit_callable_slot(rt, value).is_some() {
            value
        } else {
            callable_defect(rt, "MIR C callback boundary received an invalid callable");
            0
        }
    })
}

/// A handle that names no slot is a defect too: handles are minted only by
/// `bind_jit_callable` and never leave the run that made them.
fn jit_callable_or_trap(rt: &mut JitRuntime, handle: i64) -> Option<JitCallableSlot> {
    let slot = jit_callable_slot(rt, handle);
    if slot.is_none() {
        callable_defect(rt, "invalid resident callable value");
    }
    slot
}

/// The word generated code puts straight into `call_indirect`
/// (`lower_ctx.rs:9874`, `:9895`), with no null check of its own. Returning `0`
/// without recording a trap is what let the faulting run reach `call rax` with
/// `rax == 0`: the `emit_trap_check` at `lower_ctx.rs:9857` OR's
/// `is_trapped`/`pending_exit_status`, both clear, so the branch fell through.
/// `bind_jit_callable` now refuses a zero `fn_ptr`, so this is the floor under
/// that invariant rather than the load-bearing check — but the floor is what
/// makes the generated check fire if a future writer mints a slot another way.
fn jet_jit_callable_fn(handle: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let Some(slot) = jit_callable_or_trap(rt, handle) else {
            return 0;
        };
        if slot.fn_ptr == 0 {
            callable_defect(rt, "resident callable slot has no function address");
            return 0;
        }
        slot.fn_ptr
    })
}

fn jet_jit_callable_env(handle: i64) -> i64 {
    with_runtime_result(0, |rt| {
        jit_callable_or_trap(rt, handle).map_or(0, |slot| slot.env)
    })
}

fn jet_jit_callable_has_env(handle: i64) -> i8 {
    with_runtime_result(0, |rt| {
        jit_callable_or_trap(rt, handle).map_or(0, |slot| i8::from(slot.has_env))
    })
}

/// The JIT-side callable ABI is deliberately opaque to the Prelude. The
/// factory evaluates the function-value expression, and the adapter invokes
/// that callable with two packed payload words. The `JetOptionPacked` values
/// are only the JIT ABI carrier; the shared `jet_option_lift2` operation owns
/// presence, lazy factory creation, invocation, and result selection.
type OptionLift2Factory = unsafe extern "C" fn(i64) -> i64;
type OptionLift2Adapter = unsafe extern "C" fn(i64, i64, i64) -> i64;

fn jet_jit_option_lift2(
    a_present: i8,
    a_value: i64,
    b_present: i8,
    b_value: i64,
    factory: i64,
    env: i64,
    adapter: i64,
) -> i64 {
    jet_codegen::option_lift2::jet_option_lift2(
        jet_codegen::option_lift2::JetOptionPacked {
            present: a_present != 0,
            value: a_value,
        },
        jet_codegen::option_lift2::JetOptionPacked {
            present: b_present != 0,
            value: b_value,
        },
        || jet_codegen::option_lift2::jet_option_pack_i64(false, 0),
        |value| jet_codegen::option_lift2::jet_option_pack_i64(true, value),
        || {
            let factory: OptionLift2Factory = unsafe { std::mem::transmute(factory as usize) };
            let adapter: OptionLift2Adapter = unsafe { std::mem::transmute(adapter as usize) };
            let callable = unsafe { factory(env) };
            move |left, right| unsafe { adapter(callable, left, right) }
        },
    )
}

fn jet_jit_unit_convert_exact(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] =
            [scale_num, scale_den, offset_num, offset_den].map(|id| rt.heap.get_string(id))
        else {
            rt.set_host_fault("exact unit conversion has an invalid coefficient handle");
            return 0;
        };
        let converted = jet_foundation::jet_unit_conversion_exact(
            value, scale_num, scale_den, offset_num, offset_den,
        );
        alloc_jit_result(rt, converted.is_some(), converted.map_or(0, f64::to_bits))
    })
}

fn jet_jit_unit_convert_rounded(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
    mode: i64,
    digits: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mode = match mode {
            0 => jet_foundation::UnitRoundingMode::TowardZero,
            1 => jet_foundation::UnitRoundingMode::Floor,
            2 => jet_foundation::UnitRoundingMode::Ceiling,
            3 => jet_foundation::UnitRoundingMode::NearestEven,
            _ => {
                rt.set_host_fault("rounded unit conversion has an invalid mode tag");
                return 0;
            }
        };
        let [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] =
            [scale_num, scale_den, offset_num, offset_den].map(|id| rt.heap.get_string(id))
        else {
            rt.set_host_fault("rounded unit conversion has an invalid coefficient handle");
            return 0;
        };
        let converted = jet_foundation::jet_unit_conversion_rounded(
            value, scale_num, scale_den, offset_num, offset_den, mode, digits,
        );
        match converted {
            Ok(converted) => alloc_jit_result(rt, true, converted.to_bits()),
            Err(message) => {
                let error = rt.heap.alloc_string(message);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_unit_convert_exact_measurement(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
    relative_uncertainty: f64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] =
            [scale_num, scale_den, offset_num, offset_den].map(|id| rt.heap.get_string(id))
        else {
            rt.set_host_fault("exact measured unit conversion has an invalid coefficient handle");
            return 0;
        };
        match jet_foundation::jet_unit_conversion_exact(
            value,
            scale_num,
            scale_den,
            offset_num,
            offset_den,
        ) {
            Some(value) => {
                let measurement = alloc_measurement(
                    rt,
                    measurement_kernel::jet_measurement_kernel_from_relative(
                        value,
                        relative_uncertainty,
                    ),
                );
                alloc_jit_result(rt, true, measurement as u64)
            }
            None => alloc_jit_result(rt, false, 0),
        }
    })
}

fn jet_jit_unit_convert_rounded_measurement(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
    mode: i64,
    digits: i64,
    relative_uncertainty: f64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mode = match mode {
            0 => jet_foundation::UnitRoundingMode::TowardZero,
            1 => jet_foundation::UnitRoundingMode::Floor,
            2 => jet_foundation::UnitRoundingMode::Ceiling,
            3 => jet_foundation::UnitRoundingMode::NearestEven,
            _ => {
                rt.set_host_fault("rounded measured unit conversion has an invalid mode tag");
                return 0;
            }
        };
        let [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] =
            [scale_num, scale_den, offset_num, offset_den].map(|id| rt.heap.get_string(id))
        else {
            rt.set_host_fault(
                "rounded measured unit conversion has an invalid coefficient handle",
            );
            return 0;
        };
        match jet_foundation::jet_unit_conversion_rounded(
            value,
            scale_num,
            scale_den,
            offset_num,
            offset_den,
            mode,
            digits,
        ) {
            Ok(value) => {
                let measurement = alloc_measurement(
                    rt,
                    measurement_kernel::jet_measurement_kernel_from_relative(
                        value,
                        relative_uncertainty,
                    ),
                );
                alloc_jit_result(rt, true, measurement as u64)
            }
            Err(message) => {
                let error = rt.heap.alloc_string(message);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn jet_jit_unit_convert_implicit(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        let ratios = [scale_num, scale_den, offset_num, offset_den]
            .map(|id| rt.heap.get_string(id).map(str::to_owned));
        let converted = match &ratios {
            [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] => {
                jet_foundation::jet_unit_conversion_exact(
                    value, scale_num, scale_den, offset_num, offset_den,
                )
            }
            _ => None,
        };
        match converted {
            Some(converted) => converted,
            None => {
                rt.set_trap("unit conversion would round");
                0.0
            }
        }
    })
}

fn jet_jit_result_new_i8(ok: i8, value: i8) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u8 as u64))
}

fn jet_jit_result_new_i32(ok: i8, value: i32) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u32 as u64))
}

fn jet_jit_result_is_ok(handle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(jit_result(rt, handle).is_some_and(|r| r.ok)))
}

fn jet_jit_result_get_i64(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| jit_result(rt, handle).map_or(0, |r| r.bits as i64))
}

fn jet_jit_result_get_f64(handle: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| f64::from_bits(jit_result(rt, handle).map_or(0, |r| r.bits)))
}

fn jet_jit_result_get_i8(handle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| jit_result(rt, handle).map_or(0, |r| r.bits as i8))
}

fn jet_jit_result_get_i32(handle: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| jit_result(rt, handle).map_or(0, |r| r.bits as i32))
}

fn jet_jit_perf_fidelity() -> f64 {
    f32::from_bits(perf_fidelity_bits()) as f64
}

fn jet_jit_perf_default_fidelity() -> f64 {
    f32::from_bits(JIT_PERF_DEFAULT_FIDELITY_BITS) as f64
}

fn jet_jit_perf_override_fidelity(value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            let message = format!(
                "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
                value
            );
            let string = rt.heap.alloc_string(message);
            return alloc_jit_result(rt, false, string as u64);
        }
        set_perf_fidelity_bits((value as f32).to_bits());
        alloc_jit_result(rt, true, 0)
    })
}

fn jet_jit_perf_reset_fidelity() {
    set_perf_fidelity_bits(JIT_PERF_DEFAULT_FIDELITY_BITS);
}

pub(crate) fn declare_host_fns_for_module<M: Module>(module: &mut M) -> Result<HostFns, String> {
    let coll = Collections::declare_collections_host_fns(module)?;
    let compute = Compute::declare_compute_host_fns(module)?;
    let memory = Memory::declare_memory_host_fns(module)?;
    let cell = LocalCell::declare_host_fns(module)?;
    let conc = Concurrency::declare_concurrency_host_fns(module)?;
    let core = CoreHost::declare_core_host_fns(module)?;
    let encoding = Encoding::declare_encoding_host_fns(module)?;
    let stream = crate::enc_stream::declare_stream_host_fns(module)?;
    let fmt = Fmt::declare_fmt_host_fns(module)?;
    let compress = Compress::declare_compress_host_fns(module)?;
    let archive = Archive::declare_archive_host_fns(module)?;
    let process = Process::declare_process_host_fns(module)?;
    let num = Numeric::declare_numeric_host_fns(module)?;
    let solver = Solver::declare_solver_host_fns(module)?;
    let random = Random::declare_random_host_fns(module)?;
    let text = crate::Text::declare_text_host_fns(module)?;
    let sketch = crate::Sketch::declare_sketch_host_fns(module)?;
    let args = crate::Args::declare_args_host_fns(module)?;
    let db = crate::DB::declare_db_host_fns(module)?;
    let crypto = Crypto::declare_crypto_host_fns(module)?;
    let net = Net::declare_net_host_fns(module)?;
    let net_http = crate::net_http_rt::declare_net_http_host_fns(module)?;
    let game = crate::Game::declare_game_host_fns(module)?;
    let plugin = crate::Plugin::declare_plugin_host_fns(module)?;
    let raylib = crate::Raylib::declare_raylib_host_fns(module)?;
    let layout = crate::Layout::declare_layout_host_fns(module)?;
    let reactive = crate::Reactive::declare_reactive_host_fns(module)?;
    let ui = crate::Ui::declare_ui_host_fns(module)?;
    let web = crate::Web::declare_web_host_fns(module)?;
    let parse = crate::Parse::declare(module)?;
    let data = crate::Data::declare(module)?;
    let time = crate::Time::declare_time_host_fns(module)?;
    let io = crate::IO::declare_io_host_fns(module)?;
    let watcher = crate::Watcher::declare_watcher_host_fns(module)?;
    let math = crate::Math::declare_math_host_fns(module)?;
    let math_extra = crate::MathExtra::declare_math_extra_host_fns(module)?;
    let ffi = crate::Ffi::declare_ffi_host_fns(module)?;
    declare_host_fns(
        module, coll, compute, memory, cell, conc, core, encoding, stream, fmt, compress,
        archive, process, num, solver, random, text, sketch, args, db, crypto, net, net_http,
        game, plugin, raylib, layout, reactive, ui, web, parse, data, time, io, watcher, math,
        math_extra, ffi,
    )
}

pub(crate) fn new_jit_module() -> Result<(JITModule, HostFns), String> {
    // Keep the resident JIT unoptimized until #2919's cross-tier corpus gate
    // proves that enabling speed preserves the canonical MIR contract.
    let mut builder = JITBuilder::with_flags(
        &[
            ("opt_level", "none"),
            ("use_colocated_libcalls", "false"),
            ("is_pic", "true"),
        ],
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| e.to_string())?;
    register_host_symbols(&mut builder);
    Collections::register_collections_symbols(&mut builder);
    Compute::register_compute_symbols(&mut builder);
    Memory::register_memory_symbols(&mut builder);
    LocalCell::register_symbols(&mut builder);
    Concurrency::register_concurrency_symbols(&mut builder);
    CoreHost::register_core_host_symbols(&mut builder);
    Encoding::register_encoding_symbols(&mut builder);
    crate::enc_stream::register_stream_symbols(&mut builder);
    Fmt::register_fmt_symbols(&mut builder);
    Compress::register_compress_symbols(&mut builder);
    Archive::register_archive_symbols(&mut builder);
    Process::register_process_symbols(&mut builder);
    Numeric::register_numeric_symbols(&mut builder);
    Solver::register_solver_symbols(&mut builder);
    Random::register_random_symbols(&mut builder);
    crate::Text::register_text_symbols(&mut builder);
    crate::Sketch::register_sketch_symbols(&mut builder);
    crate::Args::register_args_symbols(&mut builder);
    crate::DB::register_db_symbols(&mut builder);
    Crypto::register_crypto_symbols(&mut builder);
    Net::register_net_symbols(&mut builder);
    crate::net_http_rt::register_net_http_symbols(&mut builder);
    crate::Game::register_game_symbols(&mut builder);
    crate::Plugin::register_plugin_symbols(&mut builder);
    crate::Raylib::register_raylib_symbols(&mut builder);
    crate::Layout::register_layout_symbols(&mut builder);
    crate::Reactive::register_reactive_symbols(&mut builder);
    crate::Ui::register_ui_symbols(&mut builder);
    crate::Web::register_web_symbols(&mut builder);
    crate::Parse::register_symbols(&mut builder);
    crate::Data::register_symbols(&mut builder);
    crate::Time::register_time_symbols(&mut builder);
    crate::IO::register_io_symbols(&mut builder);
    crate::Watcher::register_watcher_symbols(&mut builder);
    crate::Net::register_net_symbols(&mut builder);
    crate::Math::register_math_host_symbols(&mut builder);
    crate::MathExtra::register_math_extra_symbols(&mut builder);
    crate::Ffi::register_ffi_host_symbols(&mut builder);
    let mut module = JITModule::new(builder);
    let host = declare_host_fns_for_module(&mut module)?;
    Ok((module, host))
}

fn jet_jit_reflect_of_finish(type_name: i64, path: i64, display: i64, fields: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let type_name = rt.heap.clone_string(type_name).unwrap_or_default();
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let display = rt.heap.clone_string(display).unwrap_or_default();
        let field_len = rt.heap.list_len(fields).unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..field_len {
            let fh = rt.heap.list_get_int(fields, i).unwrap_or(0);
            let idx = (fh as usize).wrapping_sub(1);
            if let Some(slot) = rt.reflect_values.get(idx) {
                if let Some(name) = &slot.field_name {
                    out.push((name.clone(), fh));
                }
            }
        }
        rt.reflect_values.push(ReflectSlot {
            field_name: None,
            type_name,
            path,
            display,
            fields: out,
            value: None,
        });
        rt.reflect_values.len() as i64
    })
}

fn jet_jit_reflect_field_new(name: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let value_idx = (value as usize).wrapping_sub(1);
        let Some(value_slot) = rt.reflect_values.get(value_idx).cloned() else {
            return 0;
        };
        rt.reflect_values.push(ReflectSlot {
            field_name: Some(name),
            type_name: value_slot.type_name,
            path: value_slot.path,
            display: value_slot.display,
            fields: value_slot.fields,
            value: Some(value),
        });
        rt.reflect_values.len() as i64
    })
}

fn jet_jit_reflect_type_name(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .map(|s| s.type_name.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_reflect_display(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .map(|s| s.display.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_reflect_path(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .map(|s| s.path.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_reflect_fields(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let fields = rt
            .reflect_values
            .get(idx)
            .map(|s| s.fields.clone())
            .unwrap_or_default();
        let mut ids = Vec::new();
        for (name, value) in fields {
            let value_idx = (value as usize).wrapping_sub(1);
            let Some(value_slot) = rt.reflect_values.get(value_idx).cloned() else {
                continue;
            };
            let value = value_slot.value.unwrap_or(value);
            let value_idx = (value as usize).wrapping_sub(1);
            let Some(value_slot) = rt.reflect_values.get(value_idx).cloned() else {
                continue;
            };
            rt.reflect_values.push(ReflectSlot {
                field_name: Some(name),
                type_name: value_slot.type_name,
                path: value_slot.path,
                display: value_slot.display,
                fields: value_slot.fields,
                value: Some(value),
            });
            ids.push(rt.reflect_values.len() as i64);
        }
        rt.heap.alloc_int_list(ids)
    })
}

fn jet_jit_reflect_field_name(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .and_then(|slot| slot.field_name.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

fn jet_jit_reflect_field_value(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        rt.reflect_values
            .get(idx)
            .and_then(|slot| slot.value)
            .unwrap_or(0)
    })
}

/// The checked surface of `core.testing.{temp_dir,golden,fixture}` is
/// `String`/`Bool`/`String`; the shared Prelude reports the I/O failure as a
/// typed `TestEvidenceError`. That failure has no value on the checked
/// surface, so the resident tier stops the run with the evidence message
/// rather than inventing a silent `false`/empty carrier.
fn testing_evidence_or_trap<T: Default>(
    rt: &mut JitRuntime,
    result: Result<T, crate::testing_shared::TestEvidenceError>,
) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            rt.set_trap(&error.to_string());
            T::default()
        }
    }
}

fn jet_jit_testing_temp_dir(prefix: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let prefix = rt.heap.clone_string(prefix).unwrap_or_else(|| "jet".into());
        let path = testing_evidence_or_trap(
            rt,
            crate::testing_shared::jet_testing_temp_dir_path(&prefix),
        );
        rt.heap.alloc_string(path)
    })
}

fn jet_jit_testing_snap(name: i64, actual: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let actual = rt.heap.clone_string(actual).unwrap_or_default();
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = std::path::Path::new("__snapshots__").join(format!("{safe}.snap"));
        let update = std::env::var("JET_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");
        if update || !path.is_file() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            return i8::from(std::fs::write(&path, actual).is_ok());
        }
        i8::from(
            std::fs::read_to_string(path)
                .map(|s| s == actual)
                .unwrap_or(false),
        )
    })
}

fn jet_jit_testing_golden(path: i64, actual: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let actual = rt.heap.clone_string(actual).unwrap_or_default();
        i8::from(testing_evidence_or_trap(
            rt,
            crate::testing_shared::jet_testing_golden(&path, &actual),
        ))
    })
}

fn jet_jit_testing_fixture(path: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let contents =
            testing_evidence_or_trap(rt, crate::testing_shared::jet_testing_fixture(&path));
        rt.heap.alloc_string(contents)
    })
}
fn jet_jit_testing_corpus(path: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let mut paths = std::fs::read_dir(path)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()))
            .collect::<Vec<_>>();
        paths.sort();
        let values = rt.heap.alloc_empty_list();
        for path in paths.into_iter().filter(|path| path.is_file()) {
            if let Ok(text) = std::fs::read_to_string(path) {
                let value = rt.heap.alloc_string(text);
                rt.heap
                    .list_push_int(values, value)
                    .expect("jit testing corpus: bad list handle");
            }
        }
        values
    })
}

/// D-CMD-OVERRIDE1=C: resident handles marshal the same Prelude-owned suite
/// snapshot as AOT. Discovery and filtering stay in the command callback.
fn jet_jit_testing_test_suite_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let suite = jet_codegen::command_suite::jet_test_suite_new();
        let handle = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(handle, 0, suite.iteration);
        let _ = rt.heap.record_set_int(handle, 1, suite.result);
        handle
    })
}

fn jet_jit_testing_test_suite_run(handle: i64) -> i64 {
    let (iteration, result) = Concurrency::with_runtime_mut(|rt| {
        (
            rt.heap.record_get_int(handle, 0).unwrap_or(0),
            rt.heap.record_get_int(handle, 1).unwrap_or(0),
        )
    });
    let mut suite = jet_codegen::command_suite::JetTestSuite {
        iteration,
        result,
        runner: None,
    };
    let status = jet_codegen::command_suite::jet_test_suite_run(&mut suite);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(handle, 0, suite.iteration);
        let _ = rt.heap.record_set_int(handle, 1, suite.result);
    });
    status
}

/// Resident `TestComparison` carrier.  Field order is the checked Core plain
/// type (`core_types.rs` `TestComparison`), the same shape AOT's
/// `jet_std::JetTestComparison` carries.
struct JitTestComparison {
    status: &'static str,
    relation: String,
    seed: Option<i64>,
    inputs: i64,
    reference: Vec<i64>,
    candidate: Vec<i64>,
    first_difference: i64,
    reason: String,
}

fn testing_comparison_relation(name: &str) -> ObservationRelation {
    match name {
        "" | "typed_equality" => ObservationRelation::TypedEquality,
        "ordered_effects" => ObservationRelation::OrderedEffects,
        "typed_failure" => ObservationRelation::TypedFailure,
        other => ObservationRelation::Custom(other.to_string()),
    }
}

fn testing_comparison_carrier(
    record: ComparisonRecord,
    inputs: i64,
    seed: Option<i64>,
    reference: Vec<i64>,
    candidate: Vec<i64>,
) -> JitTestComparison {
    let first_difference = record
        .first_difference
        .map(|index| index as i64)
        .unwrap_or(-1);
    JitTestComparison {
        status: record.status.as_str(),
        relation: record.relation.as_str().to_string(),
        seed,
        inputs,
        reference,
        candidate,
        first_difference,
        reason: record
            .reason
            .unwrap_or_else(|| "comparison has no reason".to_string()),
    }
}

fn alloc_test_comparison(rt: &mut JitRuntime, comparison: JitTestComparison) -> i64 {
    let handle = rt.heap.alloc_record(13);
    let status = rt.heap.alloc_string(comparison.status);
    let relation = rt.heap.alloc_string(comparison.relation);
    let source = rt.heap.alloc_string("core.testing");
    let tool = rt.heap.alloc_string("jet");
    let target = rt.heap.alloc_string("resident");
    let seed = match comparison.seed {
        Some(seed) => alloc_jit_result(rt, true, seed as u64),
        None => alloc_jit_result(rt, false, 0),
    };
    let case_ids = rt.heap.alloc_empty_list();
    for index in 0..comparison.reference.len() {
        let id = rt.heap.alloc_string(format!("case-{index}"));
        let _ = rt.heap.list_push_int(case_ids, id);
    }
    let reference = rt.heap.alloc_empty_list();
    for value in comparison.reference {
        let _ = rt.heap.list_push_int(reference, value);
    }
    let candidate = rt.heap.alloc_empty_list();
    for value in comparison.candidate {
        let _ = rt.heap.list_push_int(candidate, value);
    }
    let reason = rt.heap.alloc_string(comparison.reason);
    let _ = rt.heap.record_set_string(handle, 0, status);
    let _ = rt.heap.record_set_string(handle, 1, relation);
    let _ = rt.heap.record_set_string(handle, 2, source);
    let _ = rt.heap.record_set_string(handle, 3, tool);
    let _ = rt.heap.record_set_string(handle, 4, target);
    let _ = rt.heap.record_set_int(handle, 5, seed);
    let _ = rt.heap.record_set_int(handle, 6, case_ids);
    let _ = rt.heap.record_set_int(handle, 7, comparison.inputs);
    let _ = rt.heap.record_set_int(handle, 8, reference);
    let _ = rt.heap.record_set_int(handle, 9, candidate);
    let _ = rt.heap.record_set_int(handle, 10, comparison.first_difference);
    let _ = rt.heap.record_set_string(handle, 11, reason);
    // Matching a finite corpus is evidence, not a universal proof.
    let _ = rt.heap.record_set_bool(handle, 12, false);
    handle
}

/// Marshal a terminal Foundation outcome.  A missing callback, invalid
/// carrier, or trapped callback is not a matched empty corpus.
fn testing_compare_terminal(
    relation_name: String,
    status: ComparisonStatus,
    what: &str,
    defect: bool,
    inputs: i64,
    seed: Option<i64>,
    reference: Vec<i64>,
    candidate: Vec<i64>,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if defect {
            callable_defect(rt, what);
        }
        let relation = testing_comparison_relation(&relation_name);
        let record = ComparisonRecord::terminal(relation, status, what);
        alloc_test_comparison(
            rt,
            testing_comparison_carrier(record, inputs, seed, reference, candidate),
        )
    })
}

fn jet_jit_testing_compare(cases: i64, reference: i64, candidate: i64, relation: i64) -> i64 {
    let relation_name = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(relation))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "typed_equality".to_string());
    let relation = testing_comparison_relation(&relation_name);
    // The resident carrier has only a typed return value.  Effect and typed
    // failure relations must use the recorded-observation adapter instead of
    // silently treating unobserved metadata as equal.
    if matches!(
        relation,
        ObservationRelation::OrderedEffects | ObservationRelation::TypedFailure
    ) {
        return testing_compare_terminal(
            relation_name,
            ComparisonStatus::Unsupported,
            "core.testing.compare relation requires recorded observations",
            false,
            cases,
            None,
            Vec::new(),
            Vec::new(),
        );
    }
    // Read the corpus and both slots under one borrow; the borrow ends before
    // any generated code runs, so a callback may reenter every host freely.
    let prepared = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(cases)?;
        let inputs = (0..len)
            .map(|index| rt.heap.list_get_int(cases, index))
            .collect::<Option<Vec<_>>>()?;
        Some((inputs, jit_callable_slot(rt, reference)?, jit_callable_slot(rt, candidate)?))
    });
    let Some((inputs, reference_slot, candidate_slot)) = prepared else {
        return testing_compare_terminal(
            relation_name,
            ComparisonStatus::Unavailable,
            "core.testing.compare received an invalid corpus or callable handle",
            true,
            cases,
            None,
            Vec::new(),
            Vec::new(),
        );
    };
    if reference_slot.raw_unary.is_none() || candidate_slot.raw_unary.is_none() {
        return testing_compare_terminal(
            relation_name,
            ComparisonStatus::Unavailable,
            "core.testing.compare callable has no universal thunk",
            true,
            cases,
            None,
            Vec::new(),
            Vec::new(),
        );
    }
    let seed = std::env::var("JET_PROP_SEED")
        .ok()
        .and_then(|value| value.parse::<i64>().ok());
    let seed_u64 = seed.and_then(|value| u64::try_from(value).ok());
    let mut samples = Vec::with_capacity(inputs.len());
    let mut reference_values = Vec::with_capacity(inputs.len());
    let mut candidate_values = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().copied().enumerate() {
        let Some(tree) = crate::Encoding::read_datatree(input) else {
            return testing_compare_terminal(
                relation_name,
                ComparisonStatus::Unavailable,
                "core.testing.compare corpus element is not a DataTree",
                true,
                cases,
                seed,
                reference_values,
                candidate_values,
            );
        };
        // Each side receives its own copy; the Core adapter never shares the
        // input object between implementations.
        let reference_input = crate::Encoding::alloc_datatree(&tree);
        let candidate_input = crate::Encoding::alloc_datatree(&tree);
        let observed = invoke_universal_unary(reference_slot, reference_input)
            .filter(|_| jet_jit_is_trapped() == 0)
            .and_then(|reference_value| {
                invoke_universal_unary(candidate_slot, candidate_input)
                    .filter(|_| jet_jit_is_trapped() == 0)
                    .map(|candidate_value| (reference_value, candidate_value))
            });
        let Some((reference_value, candidate_value)) = observed else {
            // A stop raised inside a callback is already recorded on the
            // runtime; generated code leaves at its next trap check.
            return testing_compare_terminal(
                relation_name,
                ComparisonStatus::Unavailable,
                "core.testing.compare callback stopped before observing a value",
                false,
                cases,
                seed,
                reference_values,
                candidate_values,
            );
        };
        let (Some(reference_tree), Some(candidate_tree)) = (
            crate::Encoding::read_datatree(reference_value),
            crate::Encoding::read_datatree(candidate_value),
        ) else {
            return testing_compare_terminal(
                relation_name,
                ComparisonStatus::Unavailable,
                "core.testing.compare callback returned a value that is not a DataTree",
                true,
                cases,
                seed,
                reference_values,
                candidate_values,
            );
        };
        let identity = ComparisonIdentity {
            case_id: format!("case-{index}"),
            input_id: format!("input-{index}"),
            seed: seed_u64,
            source: "core.testing".to_string(),
            tool: "jet".to_string(),
            target: "resident".to_string(),
        };
        samples.push(ComparisonSample::new(
            identity,
            ComparisonObservation::value(crate::Encoding::json_rt::render_datatree_json(
                &reference_tree,
                false,
                0,
            )),
            ComparisonObservation::value(crate::Encoding::json_rt::render_datatree_json(
                &candidate_tree,
                false,
                0,
            )),
        ));
        reference_values.push(reference_value);
        candidate_values.push(candidate_value);
    }
    let record = compare_samples(relation, samples);
    let carrier = testing_comparison_carrier(
        record,
        cases,
        seed,
        reference_values,
        candidate_values,
    );
    Concurrency::with_runtime_mut(|rt| alloc_test_comparison(rt, carrier))
}

fn history_rng_pointer(
    rt: &JitRuntime,
    handle: i64,
) -> Option<*mut jet_foundation::TestingHistory::HistoryRng> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    rt.history_rngs
        .get(index)
        .copied()
        .map(|pointer| pointer as *mut jet_foundation::TestingHistory::HistoryRng)
}

/// Bridge the callback's opaque HistoryRng carrier to the Foundation runner's
/// borrowed state. The handle is live only during one synchronous callback.
fn jet_testing_history_rng_next_u64(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(pointer) = history_rng_pointer(rt, handle) else {
            rt.set_host_fault("core.testing.histories received an invalid HistoryRng");
            return 0;
        };
        // SAFETY: explicit strategy generation installs this pointer immediately
        // before invoking the callback and removes it after the call returns.
        let value = unsafe { (*pointer).next_u64() };
        rt.heap.int_from_u64(value)
    })
}

fn jet_testing_history_rng_below(handle: i64, bound: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(bound) = rt.heap.int_to_i64(bound) else {
            rt.set_host_fault("core.testing.histories HistoryRng bound is not an Int");
            return 0;
        };
        let Ok(bound) = u64::try_from(bound) else {
            rt.set_host_fault("core.testing.histories HistoryRng bound is negative");
            return 0;
        };
        let Some(pointer) = history_rng_pointer(rt, handle) else {
            rt.set_host_fault("core.testing.histories received an invalid HistoryRng");
            return 0;
        };
        // SAFETY: see `jet_testing_history_rng_next_u64`.
        let value = unsafe { (*pointer).below(bound) };
        rt.heap.int_from_u64(value)
    })
}

fn testing_history_observation(value: i64) -> Option<ComparisonObservation> {
    let tree = crate::Encoding::read_datatree(value)?;
    Some(
        ComparisonObservation::failure(
            crate::Encoding::json_rt::render_datatree_json(&tree, false, 0),
            "history_state",
        )
        .with_mutation("history lifecycle")
        .with_cleanup(["history handles released"]),
    )
}

fn testing_history_terminal(
    relation: ObservationRelation,
    status: ComparisonStatus,
    reason: &str,
) -> i64 {
    let message = format!(
        "core.testing.histories {} ({}): {}",
        status.as_str(),
        relation.as_str(),
        reason
    );
    crate::Marshal::result_err_msg(&message)
}
fn testing_history_comparison_terminal(
    relation: ObservationRelation,
    status: ComparisonStatus,
    reason: &str,
    seed: Option<i64>,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let inputs = rt.heap.alloc_empty_list();
        let comparison = alloc_test_comparison(
            rt,
            testing_comparison_carrier(
                ComparisonRecord::terminal(relation, status, reason),
                inputs,
                seed,
                Vec::new(),
                Vec::new(),
            ),
        );
        crate::Marshal::result_ok(comparison as u64)
    })
}
fn history_schema_field(
    rt: &JitRuntime,
    type_id: u64,
) -> Option<jet_foundation::TestingHistory::HistorySchemaField> {
    use jet_foundation::TestingHistory::HistorySchemaField;

    let descriptor = rt.runtime_type_descriptor(type_id)?;
    match descriptor.kind {
        RuntimeValueKind::Int => {
            let (lo, hi) = descriptor.integer_range.unwrap_or_else(|| {
                descriptor
                    .integer_width
                    .map_or((-8, 8), |width| {
                        if width.signed {
                            (-8, 8)
                        } else {
                            (0, 16)
                        }
                    })
            });
            let (lo, hi) = (i64::try_from(lo).ok()?, i64::try_from(hi).ok()?);
            Some(HistorySchemaField::Integer { lo, hi })
        }
        RuntimeValueKind::Bool => Some(HistorySchemaField::Boolean),
        RuntimeValueKind::String => Some(HistorySchemaField::Text),
        RuntimeValueKind::Char => Some(HistorySchemaField::Char),
        _ => None,
    }
}

fn history_schema_for_command(
    rt: &JitRuntime,
    command_type: &str,
) -> Result<
    Option<(
        RuntimeTypeDescriptor,
        jet_foundation::TestingHistory::HistorySchemaStrategy,
    )>,
    String,
> {
    use jet_foundation::TestingHistory::{
        HistorySchemaStrategy, HistorySchemaVariant,
    };

    let Some(descriptor) = rt.runtime_type_descriptor_by_name(command_type).cloned() else {
        return Ok(None);
    };
    if descriptor.kind != RuntimeValueKind::Enum || descriptor.variants.is_empty() {
        return Ok(None);
    }
    let mut variants = Vec::with_capacity(descriptor.variants.len());
    for variant in &descriptor.variants {
        let Some(fields) = variant
            .fields
            .iter()
            .map(|field| history_schema_field(rt, field.type_id))
            .collect::<Option<Vec<_>>>()
        else {
            return Err(
                jet_foundation::TestingHistory::history_unsupported_variant_reason(
                    command_type,
                    &variant.name,
                ),
            );
        };
        let Some(variant) = HistorySchemaVariant::new(
            variant.wire_name.clone(),
            variant.name.clone(),
            fields,
        ) else {
            return Err(
                jet_foundation::TestingHistory::history_unsupported_variant_reason(
                    command_type,
                    &variant.name,
                ),
            );
        };
        variants.push(variant);
    }
    let Some(strategy) = HistorySchemaStrategy::with_variants(command_type.to_string(), variants)
    else {
        return Err(jet_foundation::TestingHistory::history_unsupported_type_reason(
            command_type,
        ));
    };
    Ok(Some((descriptor, strategy)))
}

fn marshal_history_enum_input(
    rt: &mut JitRuntime,
    case: &jet_foundation::TestingHistory::HistoryCase,
    descriptor: &RuntimeTypeDescriptor,
) -> Result<i64, &'static str> {
    let list = rt.heap.alloc_empty_list();
    for operation in &case.operations {
        let variant = descriptor
            .variants
            .iter()
            .find(|variant| variant.wire_name == operation.name)
            .ok_or("core.testing.histories case contains an unknown command variant")?;
        if variant.fields.len() != operation.arguments.len() {
            return Err("core.testing.histories command payload arity does not match its schema");
        }
        let record = rt.heap.alloc_record(variant.fields.len() + 1);
        rt.heap
            .record_set_int(record, 0, variant.discriminant)
            .ok_or("core.testing.histories command discriminant could not be stored")?;
        for (index, (field, value)) in variant
            .fields
            .iter()
            .zip(&operation.arguments)
            .enumerate()
        {
            let child = rt
                .runtime_type_descriptor(field.type_id)
                .ok_or("core.testing.histories command payload type is unavailable")?;
            let field_index = i64::try_from(index + 1)
                .map_err(|_| "core.testing.histories command payload index overflowed")?;
            let written = match (&child.kind, value) {
                (RuntimeValueKind::Int, jet_foundation::TestingHistory::HistoryValue::Integer(value)) => {
                    if let Some((lo, hi)) = child.integer_range {
                        let value = i128::from(*value);
                        if value < lo || value > hi {
                            return Err("core.testing.histories command integer is outside its checked range");
                        }
                    }
                    rt.heap.record_set_int(record, field_index, *value)
                }
                (RuntimeValueKind::Bool, jet_foundation::TestingHistory::HistoryValue::Boolean(value)) => {
                    rt.heap.record_set_bool(record, field_index, *value)
                }
                (RuntimeValueKind::String, jet_foundation::TestingHistory::HistoryValue::Text(value)) => {
                    let handle = rt.heap.alloc_string(value.clone());
                    rt.heap.record_set_string(record, field_index, handle)
                }
                (RuntimeValueKind::Char, jet_foundation::TestingHistory::HistoryValue::Text(value)) => {
                    let mut chars = value.chars();
                    let Some(character) = chars.next() else {
                        return Err("core.testing.histories command char payload is empty");
                    };
                    if chars.next().is_some() {
                        return Err("core.testing.histories command char payload has multiple scalars");
                    }
                    rt.heap.record_set_char(record, field_index, character)
                }
                _ => return Err("core.testing.histories command payload does not match its checked type"),
            };
            written.ok_or("core.testing.histories command payload could not be stored")?;
        }
        rt.heap
            .list_push_int(list, record)
            .ok_or("core.testing.histories command list rejected a value")?;
    }
    Ok(list)
}

fn testing_history_input(
    case: &jet_foundation::TestingHistory::HistoryCase,
    descriptor: Option<&RuntimeTypeDescriptor>,
) -> Result<i64, &'static str> {
    if let Some(descriptor) = descriptor {
        return Concurrency::with_runtime_result(
            "no active resident runtime",
            |rt| marshal_history_enum_input(rt, case, descriptor),
        );
    }
    let tree = crate::Encoding::json_rt::parse_datatree(&case.json())
        .map_err(|_| "core.testing.histories case could not be decoded")?;
    let operations = match tree {
        crate::Encoding::json_rt::DataTree::Object(fields) => fields
            .into_iter()
            .find(|(name, _)| name == "operations")
            .map(|(_, value)| value)
            .ok_or("core.testing.histories case has no operations")?,
        _ => return Err("core.testing.histories case has an invalid JSON shape"),
    };
    Ok(crate::Encoding::alloc_datatree(&operations))
}
#[derive(Clone, Debug)]
struct JitHistoryCommand {
    raw: i64,
}

#[derive(Clone)]
struct JitHistoryStrategy {
    command_type: String,
    generate: JitCallableSlot,
    rebuild: JitCallableSlot,
    valid: JitCallableSlot,
    bounds: jet_foundation::TestingHistory::HistoryBounds,
    distributions: Vec<jet_foundation::TestingHistory::HistoryDistribution>,
    command_descriptor: Option<RuntimeTypeDescriptor>,
}

impl jet_foundation::TestingHistory::HistoryCommand for JitHistoryCommand {
    type Strategy = JitHistoryStrategy;

    fn history_strategy() -> Self::Strategy {
        JitHistoryStrategy {
            command_type: "JitHistoryCommand".to_string(),
            generate: empty_jit_callable_slot(),
            rebuild: empty_jit_callable_slot(),
            valid: empty_jit_callable_slot(),
            bounds: jet_foundation::TestingHistory::HistoryBounds::default(),
            distributions: Vec::new(),
            command_descriptor: None,
        }
    }

    fn history_command_type() -> &'static str {
        "JitHistoryCommand"
    }
}

impl jet_foundation::TestingHistory::HistoryStrategyBehavior<JitHistoryCommand>
    for JitHistoryStrategy
{
    fn command_type(&self) -> &str {
        &self.command_type
    }

    fn bounds(&self) -> jet_foundation::TestingHistory::HistoryBounds {
        self.bounds
    }

    fn distributions(&self) -> Vec<jet_foundation::TestingHistory::HistoryDistribution> {
        self.distributions.clone()
    }

    fn generate(
        &self,
        rng: &mut jet_foundation::TestingHistory::HistoryRng,
        case_index: jet_foundation::TestingHistory::Count,
        max_steps: jet_foundation::TestingHistory::Count,
    ) -> Option<
        jet_foundation::TestingHistory::TypedHistoryCase<JitHistoryCommand>,
    > {
        let (rng_handle, case_index, max_steps) = Concurrency::with_runtime_mut(|rt| {
            rt.history_rngs.push(rng as *mut _ as usize);
            let handle = i64::try_from(rt.history_rngs.len()).ok()?;
            Some((
                handle,
                rt.heap.int_from_u64(case_index as u64),
                rt.heap.int_from_u64(max_steps as u64),
            ))
        })?;
        let result = invoke_universal_many(
            self.generate,
            &[rng_handle, case_index, max_steps],
        );
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.history_rngs.pop();
        });
        let result = result.filter(|_| jet_jit_is_trapped() == 0)?;
        let (_, payload) = Concurrency::with_runtime_mut(|rt| jit_result_parts(rt, result))?;
        history_typed_case_from_runtime(
            payload as i64,
            self.command_descriptor.as_ref(),
        )
        .ok()
    }

    fn rebuild(
        &self,
        case: &jet_foundation::TestingHistory::HistoryCase,
    ) -> Option<
        jet_foundation::TestingHistory::TypedHistoryCase<JitHistoryCommand>,
    > {
        let input = Concurrency::with_runtime_string(|rt| marshal_history_case(rt, case)).ok()?;
        let result = invoke_universal_unary(self.rebuild, input)
            .filter(|_| jet_jit_is_trapped() == 0)?;
        let (_, payload) = Concurrency::with_runtime_mut(|rt| jit_result_parts(rt, result))?;
        history_typed_case_from_runtime(
            payload as i64,
            self.command_descriptor.as_ref(),
        )
        .ok()
    }

    fn valid(
        &self,
        case: &jet_foundation::TestingHistory::TypedHistoryCase<JitHistoryCommand>,
    ) -> bool {
        let Ok(input) = Concurrency::with_runtime_string(|rt| {
            marshal_typed_history_case(rt, case, self.command_descriptor.as_ref())
        }) else {
            return false;
        };
        invoke_universal_unary(self.valid, input)
            .filter(|_| jet_jit_is_trapped() == 0)
            .is_some_and(|value| value != 0)
    }

    fn commands_to_data_tree(
        &self,
        commands: &[JitHistoryCommand],
    ) -> Option<crate::DataTree::DataTree> {
        Concurrency::with_runtime_mut(|rt| {
            let mut values = Vec::with_capacity(commands.len());
            for command in commands {
                let value = match self.command_descriptor.as_ref() {
                    Some(descriptor) => {
                        crate::Receipt::encode_jit_value(rt, command.raw, descriptor).ok()?
                    }
                    None => crate::Encoding::read_datatree(command.raw)?,
                };
                values.push(value);
            }
            Some(crate::DataTree::DataTree::Array(values))
        })
    }
}

fn empty_jit_callable_slot() -> JitCallableSlot {
    JitCallableSlot {
        history_captures_owned: None,
        fn_ptr: 0,
        env: 0,
        has_env: false,
        raw_unary: None,
        raw_pair: None,
        raw_many: None,
    }
}

fn history_count_value(
    rt: &mut JitRuntime,
    value: usize,
) -> i64 {
    rt.heap.int_from_u64(value as u64)
}

fn history_count_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<usize, String> {
    let value = rt
        .heap
        .int_to_i64(raw)
        .or_else(|| (raw >= 0).then_some(raw))
        .ok_or_else(|| format!("core.testing.histories {label} is not an Int"))?;
    usize::try_from(value)
        .map_err(|_| format!("core.testing.histories {label} is negative"))
}

fn history_u64_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<u64, String> {
    let value = rt
        .heap
        .int_to_i64(raw)
        .or_else(|| (raw >= 0).then_some(raw))
        .ok_or_else(|| format!("core.testing.histories {label} is not an Int"))?;
    u64::try_from(value)
        .map_err(|_| format!("core.testing.histories {label} is negative"))
}

fn history_string_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<String, String> {
    rt.heap
        .clone_string(raw)
        .ok_or_else(|| format!("core.testing.histories {label} is not a String"))
}

fn history_id_to_runtime(
    rt: &mut JitRuntime,
    id: u32,
) -> Result<i64, String> {
    let record = rt.heap.alloc_record(1);
    let value = history_count_value(rt, id as usize);
    rt.heap
        .record_set_int(record, 0, value)
        .ok_or_else(|| "core.testing.histories could not store an ID".to_string())?;
    Ok(record)
}

fn history_id_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<u32, String> {
    let value = rt
        .heap
        .record_get_int(raw, 0)
        .ok_or_else(|| format!("core.testing.histories {label} has an invalid ID"))?;
    let value = history_count_from_runtime(rt, value, label)?;
    u32::try_from(value)
        .map_err(|_| format!("core.testing.histories {label} is out of range"))
}

fn history_optional_id_to_runtime(
    rt: &mut JitRuntime,
    value: Option<u32>,
) -> Result<i64, String> {
    let payload = match value {
        Some(value) => history_id_to_runtime(rt, value)? as u64,
        None => 0,
    };
    Ok(alloc_jit_result(rt, value.is_some(), payload))
}

fn history_optional_id_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<Option<u32>, String> {
    let (present, payload) = jit_result_parts(rt, raw)
        .ok_or_else(|| format!("core.testing.histories {label} is not an Option"))?;
    if !present {
        return Ok(None);
    }
    history_id_from_runtime(rt, payload as i64, label).map(Some)
}

fn history_int_to_runtime(
    rt: &mut JitRuntime,
    value: i64,
) -> i64 {
    rt.heap.int_from_i64(value)
}

fn history_value_to_runtime(
    rt: &mut JitRuntime,
    value: &jet_foundation::TestingHistory::HistoryValue,
) -> Result<i64, String> {
    use jet_foundation::TestingHistory::HistoryValue;
    let (discriminant, payload) = match value {
        HistoryValue::Integer(value) => (0, Some((history_int_to_runtime(rt, *value), 0))),
        HistoryValue::Boolean(value) => (1, Some((i64::from(*value), 1))),
        HistoryValue::Text(value) => (2, Some((rt.heap.alloc_string(value.clone()), 2))),
        HistoryValue::Handle(value) => (3, Some((history_id_to_runtime(rt, value.value)?, 3))),
        HistoryValue::Redacted(value) => (4, Some((rt.heap.alloc_string(value.clone()), 2))),
    };
    let record = rt.heap.alloc_record(2);
    rt.heap
        .record_set_int(record, 0, discriminant)
        .ok_or_else(|| "core.testing.histories could not store a value tag".to_string())?;
    let (payload, kind) = payload.expect("history value payload is always present");
    let written = match kind {
        0 | 3 => rt.heap.record_set_int(record, 1, payload),
        1 => rt.heap.record_set_bool(record, 1, payload != 0),
        2 => rt.heap.record_set_string(record, 1, payload),
        _ => None,
    };
    written
        .ok_or_else(|| "core.testing.histories could not store a value payload".to_string())?;
    Ok(record)
}

fn history_value_from_runtime(
    rt: &JitRuntime,
    raw: i64,
) -> Result<jet_foundation::TestingHistory::HistoryValue, String> {
    use jet_foundation::TestingHistory::HistoryValue;
    let tag = rt
        .heap
        .record_get_int(raw, 0)
        .ok_or_else(|| "core.testing.histories value has no tag".to_string())?;
    match tag {
        0 => {
            let payload = rt
                .heap
                .record_get_int(raw, 1)
                .ok_or_else(|| "core.testing.histories Integer payload is invalid".to_string())?;
            Ok(HistoryValue::Integer(history_int_from_runtime(
                rt,
                payload,
                "history value",
            )?))
        }
        1 => Ok(HistoryValue::Boolean(
            rt.heap
                .record_get_bool(raw, 1)
                .ok_or_else(|| "core.testing.histories Boolean payload is invalid".to_string())?,
        )),
        2 => Ok(HistoryValue::Text(
            rt.heap
                .record_clone_string(raw, 1)
                .ok_or_else(|| "core.testing.histories Text payload is invalid".to_string())?,
        )),
        3 => {
            let payload = rt
                .heap
                .record_get_int(raw, 1)
                .ok_or_else(|| "core.testing.histories Handle payload is invalid".to_string())?;
            Ok(HistoryValue::Handle(
                jet_foundation::TestingHistory::HandleId {
                    value: history_id_from_runtime(rt, payload, "Handle payload")?,
                },
            ))
        }
        4 => Ok(HistoryValue::Redacted(
            rt.heap
                .record_clone_string(raw, 1)
                .ok_or_else(|| "core.testing.histories Redacted payload is invalid".to_string())?,
        )),
        _ => Err("core.testing.histories value has an unknown tag".to_string()),
    }
}

fn history_int_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<i64, String> {
    rt.heap
        .int_to_i64(raw)
        .or_else(|| (raw >= 0).then_some(raw))
        .ok_or_else(|| format!("core.testing.histories {label} is not an Int"))
}

fn history_precondition_to_runtime(
    rt: &mut JitRuntime,
    value: &jet_foundation::TestingHistory::HistoryPrecondition,
) -> Result<i64, String> {
    use jet_foundation::TestingHistory::HistoryPrecondition;
    let (tag, payload) = match value {
        HistoryPrecondition::HandleLive(handle) => {
            (0, vec![history_id_to_runtime(rt, handle.value)?])
        }
        HistoryPrecondition::HandleState { handle, state } => (
            1,
            vec![
                history_id_to_runtime(rt, handle.value)?,
                rt.heap.alloc_string(state.clone()),
            ],
        ),
        HistoryPrecondition::TaskCompleted(task) => {
            (2, vec![history_id_to_runtime(rt, task.value)?])
        }
        HistoryPrecondition::EventAvailable(event) => {
            (3, vec![history_id_to_runtime(rt, event.value)?])
        }
    };
    let record = rt.heap.alloc_record(payload.len() + 1);
    rt.heap
        .record_set_int(record, 0, tag)
        .ok_or_else(|| "core.testing.histories could not store precondition tag".to_string())?;
    for (index, value) in payload.into_iter().enumerate() {
        let index = i64::try_from(index + 1)
            .map_err(|_| "core.testing.histories precondition index overflowed".to_string())?;
        rt.heap
            .record_set_int(record, index, value)
            .ok_or_else(|| "core.testing.histories could not store precondition".to_string())?;
    }
    Ok(record)
}

fn history_precondition_from_runtime(
    rt: &JitRuntime,
    raw: i64,
) -> Result<jet_foundation::TestingHistory::HistoryPrecondition, String> {
    use jet_foundation::TestingHistory::{
        EventId, HandleId, HistoryPrecondition, TaskId,
    };
    let tag = rt
        .heap
        .record_get_int(raw, 0)
        .ok_or_else(|| "core.testing.histories precondition has no tag".to_string())?;
    let value = |index: i64| {
        rt.heap
            .record_get_int(raw, index)
            .ok_or_else(|| "core.testing.histories precondition payload is invalid".to_string())
    };
    match tag {
        0 => Ok(HistoryPrecondition::HandleLive(HandleId {
            value: history_id_from_runtime(rt, value(1)?, "HandleLive")?,
        })),
        1 => Ok(HistoryPrecondition::HandleState {
            handle: HandleId {
                value: history_id_from_runtime(rt, value(1)?, "HandleState")?,
            },
            state: rt
                .heap
                .record_clone_string(raw, 2)
                .ok_or_else(|| "core.testing.histories HandleState has no state".to_string())?,
        }),
        2 => Ok(HistoryPrecondition::TaskCompleted(TaskId {
            value: history_id_from_runtime(rt, value(1)?, "TaskCompleted")?,
        })),
        3 => Ok(HistoryPrecondition::EventAvailable(EventId {
            value: history_id_from_runtime(rt, value(1)?, "EventAvailable")?,
        })),
        _ => Err("core.testing.histories precondition has an unknown tag".to_string()),
    }
}

fn history_list_of_ints(
    rt: &mut JitRuntime,
    values: impl IntoIterator<Item = i64>,
) -> Result<i64, String> {
    let list = rt.heap.alloc_empty_list();
    for value in values {
        rt.heap
            .list_push_int(list, value)
            .ok_or_else(|| "core.testing.histories list rejected an integer".to_string())?;
    }
    Ok(list)
}

fn history_list_of_ids(
    rt: &mut JitRuntime,
    values: &[u32],
) -> Result<i64, String> {
    let values = values
        .iter()
        .map(|value| history_id_to_runtime(rt, *value))
        .collect::<Result<Vec<_>, _>>()?;
    history_list_of_ints(rt, values)
}

fn history_list_of_values(
    rt: &mut JitRuntime,
    values: &[jet_foundation::TestingHistory::HistoryValue],
) -> Result<i64, String> {
    let values = values
        .iter()
        .map(|value| history_value_to_runtime(rt, value))
        .collect::<Result<Vec<_>, _>>()?;
    history_list_of_ints(rt, values)
}

fn history_list_of_preconditions(
    rt: &mut JitRuntime,
    values: &[jet_foundation::TestingHistory::HistoryPrecondition],
) -> Result<i64, String> {
    let values = values
        .iter()
        .map(|value| history_precondition_to_runtime(rt, value))
        .collect::<Result<Vec<_>, _>>()?;
    history_list_of_ints(rt, values)
}

fn marshal_history_operation(
    rt: &mut JitRuntime,
    operation: &jet_foundation::TestingHistory::HistoryOperation,
) -> Result<i64, String> {
    let record = rt.heap.alloc_record(9);
    let index = history_count_value(rt, operation.index as usize);
    rt.heap
        .record_set_int(record, 0, index)
        .ok_or_else(|| "core.testing.histories could not store operation index".to_string())?;
    let name = rt.heap.alloc_string(operation.name.clone());
    rt.heap
        .record_set_string(record, 1, name)
        .ok_or_else(|| "core.testing.histories could not store operation name".to_string())?;
    let arguments = history_list_of_values(rt, &operation.arguments)?;
    let creates_values = operation
        .creates
        .iter()
        .map(|value| value.value)
        .collect::<Vec<_>>();
    let creates = history_list_of_ids(rt, &creates_values)?;
    let consumes_values = operation
        .consumes
        .iter()
        .map(|value| value.value)
        .collect::<Vec<_>>();
    let consumes = history_list_of_ids(rt, &consumes_values)?;
    let preconditions = history_list_of_preconditions(rt, &operation.preconditions)?;
    let depends_values = operation
        .depends_on
        .iter()
        .map(|value| history_count_value(rt, *value as usize))
        .collect::<Vec<_>>();
    let depends_on = history_list_of_ints(rt, depends_values)?;
    rt.heap
        .record_set_int(record, 2, arguments)
        .and_then(|_| rt.heap.record_set_int(record, 3, creates))
        .and_then(|_| rt.heap.record_set_int(record, 4, consumes))
        .and_then(|_| rt.heap.record_set_int(record, 5, preconditions))
        .and_then(|_| rt.heap.record_set_int(record, 6, depends_on))
        .ok_or_else(|| "core.testing.histories could not store operation lists".to_string())?;
    let task = history_optional_id_to_runtime(rt, operation.task.map(|value| value.value))?;
    let event = history_optional_id_to_runtime(rt, operation.event.map(|value| value.value))?;
    rt.heap
        .record_set_int(record, 7, task)
        .and_then(|_| rt.heap.record_set_int(record, 8, event))
        .ok_or_else(|| "core.testing.histories could not store operation schedule".to_string())?;
    Ok(record)
}

fn history_list_values_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    decoder: impl Fn(&JitRuntime, i64) -> Result<jet_foundation::TestingHistory::HistoryValue, String>,
) -> Result<Vec<jet_foundation::TestingHistory::HistoryValue>, String> {
    let length = rt
        .heap
        .list_len(raw)
        .ok_or_else(|| "core.testing.histories list is invalid".to_string())?;
    let mut values = Vec::with_capacity(
        usize::try_from(length)
            .map_err(|_| "core.testing.histories list length is invalid".to_string())?,
    );
    for index in 0..length {
        let value = rt
            .heap
            .list_get_int(raw, index)
            .ok_or_else(|| "core.testing.histories list element is invalid".to_string())?;
        values.push(decoder(rt, value)?);
    }
    Ok(values)
}

fn history_ids_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    label: &str,
) -> Result<Vec<u32>, String> {
    let length = rt
        .heap
        .list_len(raw)
        .ok_or_else(|| format!("core.testing.histories {label} list is invalid"))?;
    let mut values = Vec::with_capacity(
        usize::try_from(length)
            .map_err(|_| format!("core.testing.histories {label} list length is invalid"))?,
    );
    for index in 0..length {
        let value = rt
            .heap
            .list_get_int(raw, index)
            .ok_or_else(|| format!("core.testing.histories {label} list element is invalid"))?;
        values.push(history_id_from_runtime(rt, value, label)?);
    }
    Ok(values)
}

fn history_preconditions_from_runtime(
    rt: &JitRuntime,
    raw: i64,
) -> Result<Vec<jet_foundation::TestingHistory::HistoryPrecondition>, String> {
    let length = rt
        .heap
        .list_len(raw)
        .ok_or_else(|| "core.testing.histories precondition list is invalid".to_string())?;
    let mut values = Vec::with_capacity(
        usize::try_from(length)
            .map_err(|_| "core.testing.histories precondition list length is invalid".to_string())?,
    );
    for index in 0..length {
        let value = rt
            .heap
            .list_get_int(raw, index)
            .ok_or_else(|| "core.testing.histories precondition list element is invalid".to_string())?;
        values.push(history_precondition_from_runtime(rt, value)?);
    }
    Ok(values)
}

fn marshal_history_schedule(
    rt: &mut JitRuntime,
    choice: &jet_foundation::TestingHistory::HistoryScheduleChoice,
) -> Result<i64, String> {
    let record = rt.heap.alloc_record(4);
    let operation = history_count_value(rt, choice.operation as usize);
    rt.heap
        .record_set_int(record, 0, operation)
        .ok_or_else(|| "core.testing.histories could not store schedule operation".to_string())?;
    let task = history_optional_id_to_runtime(rt, choice.task.map(|value| value.value))?;
    let event = history_optional_id_to_runtime(rt, choice.event.map(|value| value.value))?;
    rt.heap
        .record_set_int(record, 1, task)
        .and_then(|_| rt.heap.record_set_int(record, 2, event))
        .ok_or_else(|| "core.testing.histories could not store schedule IDs".to_string())?;
    let choice_text = rt.heap.alloc_string(choice.choice.clone());
    rt.heap
        .record_set_string(record, 3, choice_text)
        .ok_or_else(|| "core.testing.histories could not store schedule choice".to_string())?;
    Ok(record)
}

fn marshal_history_case(
    rt: &mut JitRuntime,
    case: &jet_foundation::TestingHistory::HistoryCase,
) -> Result<i64, String> {
    let record = rt.heap.alloc_record(4);
    let case_id = rt.heap.alloc_string(case.case_id.clone());
    rt.heap
        .record_set_string(record, 0, case_id)
        .ok_or_else(|| "core.testing.histories could not store case ID".to_string())?;
    let seed = rt.heap.int_from_u64(case.seed);
    rt.heap
        .record_set_int(record, 1, seed)
        .ok_or_else(|| "core.testing.histories could not store case seed".to_string())?;
    let operation_values = case
        .operations
        .iter()
        .map(|operation| marshal_history_operation(rt, operation))
        .collect::<Result<Vec<_>, _>>()?;
    let operations = history_list_of_ints(rt, operation_values)?;
    let schedule_values = case
        .schedule
        .iter()
        .map(|choice| marshal_history_schedule(rt, choice))
        .collect::<Result<Vec<_>, _>>()?;
    let schedule = history_list_of_ints(rt, schedule_values)?;
    rt.heap
        .record_set_int(record, 2, operations)
        .and_then(|_| rt.heap.record_set_int(record, 3, schedule))
        .ok_or_else(|| "core.testing.histories could not store case lists".to_string())?;
    Ok(record)
}

fn history_operation_from_runtime(
    rt: &JitRuntime,
    raw: i64,
) -> Result<jet_foundation::TestingHistory::HistoryOperation, String> {
    use jet_foundation::TestingHistory::HistoryOperation;
    let index = history_count_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 0)
            .ok_or_else(|| "core.testing.histories operation has no index".to_string())?,
        "operation index",
    )?;
    let name = rt
        .heap
        .record_clone_string(raw, 1)
        .ok_or_else(|| "core.testing.histories operation has no name".to_string())?;
    let arguments = history_list_values_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 2)
            .ok_or_else(|| "core.testing.histories operation has no arguments".to_string())?,
        history_value_from_runtime,
    )?;
    let creates_raw = rt
        .heap
        .record_get_int(raw, 3)
        .ok_or_else(|| "core.testing.histories operation has no creates".to_string())?;
    let consumes_raw = rt
        .heap
        .record_get_int(raw, 4)
        .ok_or_else(|| "core.testing.histories operation has no consumes".to_string())?;
    let preconditions_raw = rt
        .heap
        .record_get_int(raw, 5)
        .ok_or_else(|| "core.testing.histories operation has no preconditions".to_string())?;
    let depends_raw = rt
        .heap
        .record_get_int(raw, 6)
        .ok_or_else(|| "core.testing.histories operation has no dependencies".to_string())?;
    let creates = history_ids_from_runtime(rt, creates_raw, "creates")?
        .into_iter()
        .map(|value| jet_foundation::TestingHistory::HandleId { value })
        .collect();
    let consumes = history_ids_from_runtime(rt, consumes_raw, "consumes")?
        .into_iter()
        .map(|value| jet_foundation::TestingHistory::HandleId { value })
        .collect();
    let preconditions = history_preconditions_from_runtime(rt, preconditions_raw)?;
    let depends_on = history_ids_from_runtime(rt, depends_raw, "depends_on")?;
    let task = history_optional_id_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 7)
            .ok_or_else(|| "core.testing.histories operation has no task".to_string())?,
        "task",
    )?
    .map(|value| jet_foundation::TestingHistory::TaskId { value });
    let event = history_optional_id_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 8)
            .ok_or_else(|| "core.testing.histories operation has no event".to_string())?,
        "event",
    )?
    .map(|value| jet_foundation::TestingHistory::EventId { value });
    Ok(HistoryOperation {
        index: u32::try_from(index)
            .map_err(|_| "core.testing.histories operation index is out of range".to_string())?,
        name,
        arguments,
        creates,
        consumes,
        preconditions,
        depends_on: depends_on
            .into_iter()
            .map(|value| u32::try_from(value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "core.testing.histories dependency is out of range".to_string())?,
        task,
        event,
    })
}

fn history_schedule_from_runtime(
    rt: &JitRuntime,
    raw: i64,
) -> Result<jet_foundation::TestingHistory::HistoryScheduleChoice, String> {
    let operation = history_count_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 0)
            .ok_or_else(|| "core.testing.histories schedule has no operation".to_string())?,
        "schedule operation",
    )?;
    let task = history_optional_id_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 1)
            .ok_or_else(|| "core.testing.histories schedule has no task".to_string())?,
        "schedule task",
    )?
    .map(|value| jet_foundation::TestingHistory::TaskId { value });
    let event = history_optional_id_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 2)
            .ok_or_else(|| "core.testing.histories schedule has no event".to_string())?,
        "schedule event",
    )?
    .map(|value| jet_foundation::TestingHistory::EventId { value });
    let choice = rt
        .heap
        .record_clone_string(raw, 3)
        .ok_or_else(|| "core.testing.histories schedule has no choice".to_string())?;
    Ok(jet_foundation::TestingHistory::HistoryScheduleChoice {
        operation: u32::try_from(operation)
            .map_err(|_| "core.testing.histories schedule operation is out of range".to_string())?,
        task,
        event,
        choice,
    })
}

fn history_case_from_runtime(
    rt: &JitRuntime,
    raw: i64,
) -> Result<jet_foundation::TestingHistory::HistoryCase, String> {
    let case_id = rt
        .heap
        .record_clone_string(raw, 0)
        .ok_or_else(|| "core.testing.histories case has no ID".to_string())?;
    let seed = history_u64_from_runtime(
        rt,
        rt.heap
            .record_get_int(raw, 1)
            .ok_or_else(|| "core.testing.histories case has no seed".to_string())?,
        "case seed",
    )?;
    let operations_raw = rt
        .heap
        .record_get_int(raw, 2)
        .ok_or_else(|| "core.testing.histories case has no operations".to_string())?;
    let schedule_raw = rt
        .heap
        .record_get_int(raw, 3)
        .ok_or_else(|| "core.testing.histories case has no schedule".to_string())?;
    let operations_len = rt
        .heap
        .list_len(operations_raw)
        .ok_or_else(|| "core.testing.histories operation list is invalid".to_string())?;
    let mut operations = Vec::with_capacity(
        usize::try_from(operations_len)
            .map_err(|_| "core.testing.histories operation list length is invalid".to_string())?,
    );
    for index in 0..operations_len {
        operations.push(history_operation_from_runtime(
            rt,
            rt.heap
                .list_get_int(operations_raw, index)
                .ok_or_else(|| "core.testing.histories operation list element is invalid".to_string())?,
        )?);
    }
    let schedule_len = rt
        .heap
        .list_len(schedule_raw)
        .ok_or_else(|| "core.testing.histories schedule list is invalid".to_string())?;
    let mut schedule = Vec::with_capacity(
        usize::try_from(schedule_len)
            .map_err(|_| "core.testing.histories schedule list length is invalid".to_string())?,
    );
    for index in 0..schedule_len {
        schedule.push(history_schedule_from_runtime(
            rt,
            rt.heap
                .list_get_int(schedule_raw, index)
                .ok_or_else(|| "core.testing.histories schedule list element is invalid".to_string())?,
        )?);
    }
    Ok(jet_foundation::TestingHistory::HistoryCase {
        case_id,
        seed,
        operations,
        schedule,
    })
}

fn history_command_uses_float(descriptor: Option<&RuntimeTypeDescriptor>) -> Option<bool> {
    descriptor.map(|descriptor| {
        matches!(
            descriptor.abi,
            RuntimeValueAbi::Float | RuntimeValueAbi::Float32
        )
    })
}

fn history_commands_to_runtime(
    rt: &mut JitRuntime,
    commands: &[JitHistoryCommand],
    descriptor: Option<&RuntimeTypeDescriptor>,
) -> Result<i64, String> {
    let list = rt.heap.alloc_empty_list();
    for command in commands {
        let written = if history_command_uses_float(descriptor) == Some(true) {
            let value = descriptor
                .map(|descriptor| match descriptor.abi {
                    RuntimeValueAbi::Float32 => f32::from_bits(command.raw as u32) as f64,
                    _ => f64::from_bits(command.raw as u64),
                })
                .unwrap_or_default();
            rt.heap.list_push_float(list, value)
        } else {
            rt.heap.list_push_int(list, command.raw)
        };
        written
            .ok_or_else(|| "core.testing.histories command list rejected a value".to_string())?;
    }
    Ok(list)
}

fn history_commands_from_runtime(
    rt: &JitRuntime,
    raw: i64,
    descriptor: Option<&RuntimeTypeDescriptor>,
) -> Result<Vec<JitHistoryCommand>, String> {
    let length = rt
        .heap
        .list_len(raw)
        .ok_or_else(|| "core.testing.histories command list is invalid".to_string())?;
    let mut commands = Vec::with_capacity(
        usize::try_from(length)
            .map_err(|_| "core.testing.histories command list length is invalid".to_string())?,
    );
    for index in 0..length {
        let raw = if history_command_uses_float(descriptor) == Some(true) {
            let value = rt
                .heap
                .list_get_float(raw, index)
                .ok_or_else(|| "core.testing.histories command list element is invalid".to_string())?;
            descriptor
                .map(|descriptor| match descriptor.abi {
                    RuntimeValueAbi::Float32 => (value as f32).to_bits() as i64,
                    _ => value.to_bits() as i64,
                })
                .unwrap_or_else(|| value.to_bits() as i64)
        } else {
            rt.heap
                .list_get_int(raw, index)
                .ok_or_else(|| "core.testing.histories command list element is invalid".to_string())?
        };
        commands.push(JitHistoryCommand { raw });
    }
    Ok(commands)
}


fn marshal_typed_history_case(
    rt: &mut JitRuntime,
    case: &jet_foundation::TestingHistory::TypedHistoryCase<JitHistoryCommand>,
    descriptor: Option<&RuntimeTypeDescriptor>,
) -> Result<i64, String> {
    let record = rt.heap.alloc_record(2);
    let metadata = marshal_history_case(rt, &case.case)?;
    let commands = history_commands_to_runtime(rt, &case.commands, descriptor)?;
    rt.heap
        .record_set_int(record, 0, metadata)
        .and_then(|_| rt.heap.record_set_int(record, 1, commands))
        .ok_or_else(|| "core.testing.histories could not store typed case".to_string())?;
    Ok(record)
}

fn history_typed_case_from_runtime(
    raw: i64,
    descriptor: Option<&RuntimeTypeDescriptor>,
) -> Result<
    jet_foundation::TestingHistory::TypedHistoryCase<JitHistoryCommand>,
    String,
> {
    let decoded = Concurrency::with_runtime_string(|rt| {
        let metadata = rt
            .heap
            .record_get_int(raw, 0)
            .ok_or_else(|| "core.testing.histories typed case has no metadata".to_string())?;
        let commands = rt
            .heap
            .record_get_int(raw, 1)
            .ok_or_else(|| "core.testing.histories typed case has no commands".to_string())?;
        Ok::<_, String>((
            history_case_from_runtime(rt, metadata)?,
            history_commands_from_runtime(rt, commands, descriptor)?,
        ))
    })?;
    jet_foundation::TestingHistory::TypedHistoryCase::new(decoded.0, decoded.1)
        .ok_or_else(|| "core.testing.histories typed case command count does not match metadata".to_string())
}

fn decode_jit_history_strategy(
    strategy: i64,
    command_type: String,
) -> Result<Option<JitHistoryStrategy>, String> {
    let Some((present, payload)) =
        Concurrency::with_runtime_mut(|rt| jit_result_parts(rt, strategy))
    else {
        return Err("core.testing.histories strategy is not an Option".to_string());
    };
    if !present {
        return Ok(None);
    }
    Concurrency::with_runtime_string(|rt| {
        let generate_raw = rt
            .heap
            .record_get_int(payload as i64, 0)
            .ok_or_else(|| "core.testing.histories strategy has no generate callback".to_string())?;
        let rebuild_raw = rt
            .heap
            .record_get_int(payload as i64, 1)
            .ok_or_else(|| "core.testing.histories strategy has no rebuild callback".to_string())?;
        let valid_raw = rt
            .heap
            .record_get_int(payload as i64, 2)
            .ok_or_else(|| "core.testing.histories strategy has no valid callback".to_string())?;
        let bounds_raw = rt
            .heap
            .record_get_int(payload as i64, 3)
            .ok_or_else(|| "core.testing.histories strategy has no bounds".to_string())?;
        let distributions_raw = rt
            .heap
            .record_get_int(payload as i64, 4)
            .ok_or_else(|| "core.testing.histories strategy has no distributions".to_string())?;
        let generate = jit_callable_slot(rt, generate_raw)
            .ok_or_else(|| "core.testing.histories strategy generate callback is invalid".to_string())?;
        let rebuild = jit_callable_slot(rt, rebuild_raw)
            .ok_or_else(|| "core.testing.histories strategy rebuild callback is invalid".to_string())?;
        let valid = jit_callable_slot(rt, valid_raw)
            .ok_or_else(|| "core.testing.histories strategy valid callback is invalid".to_string())?;
        if generate.raw_many.is_none() || rebuild.raw_unary.is_none() || valid.raw_unary.is_none() {
            return Err("core.testing.histories strategy callback has no universal thunk".to_string());
        }
        let bounds = jet_foundation::TestingHistory::HistoryBounds {
            max_steps: history_count_from_runtime(
                rt,
                rt.heap
                    .record_get_int(bounds_raw, 0)
                    .ok_or_else(|| "core.testing.histories strategy bounds are incomplete".to_string())?,
                "max_steps",
            )?,
            max_resources: history_count_from_runtime(
                rt,
                rt.heap
                    .record_get_int(bounds_raw, 1)
                    .ok_or_else(|| "core.testing.histories strategy bounds are incomplete".to_string())?,
                "max_resources",
            )?,
            max_shrink_attempts: history_count_from_runtime(
                rt,
                rt.heap
                    .record_get_int(bounds_raw, 2)
                    .ok_or_else(|| "core.testing.histories strategy bounds are incomplete".to_string())?,
                "max_shrink_attempts",
            )?,
            max_discarded_cases: history_count_from_runtime(
                rt,
                rt.heap
                    .record_get_int(bounds_raw, 3)
                    .ok_or_else(|| "core.testing.histories strategy bounds are incomplete".to_string())?,
                "max_discarded_cases",
            )?,
        };
        let length = rt
            .heap
            .list_len(distributions_raw)
            .ok_or_else(|| "core.testing.histories strategy distributions are invalid".to_string())?;
        let mut distributions = Vec::with_capacity(
            usize::try_from(length)
                .map_err(|_| "core.testing.histories distribution length is invalid".to_string())?,
        );
        for index in 0..length {
            let row = rt
                .heap
                .list_get_int(distributions_raw, index)
                .ok_or_else(|| "core.testing.histories distribution row is invalid".to_string())?;
            let operation = rt
                .heap
                .record_clone_string(row, 0)
                .ok_or_else(|| "core.testing.histories distribution has no operation".to_string())?;
            let weight = history_count_from_runtime(
                rt,
                rt.heap
                    .record_get_int(row, 1)
                    .ok_or_else(|| "core.testing.histories distribution has no weight".to_string())?,
                "distribution weight",
            )?;
            distributions.push(jet_foundation::TestingHistory::HistoryDistribution {
                operation,
                weight,
            });
        }
        let command_descriptor = if command_type == "DataTree"
            || command_type.starts_with("DataTree|")
        {
            None
        } else {
            Some(
                rt.runtime_type_descriptor_by_name(&command_type)
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "core.testing.histories command type `{command_type}` has no checked runtime descriptor"
                        )
                    })?,
            )
        };
        Ok(Some(JitHistoryStrategy {
            command_type,
            generate,
            rebuild,
            valid,
            bounds,
            distributions,
            command_descriptor,
        }))
    })
}

fn testing_history_execute(
    model_input: i64,
    actual_input: i64,
    model: JitCallableSlot,
    actual: JitCallableSlot,
    observe: JitCallableSlot,
) -> Result<
    (
        i64,
        i64,
        ComparisonObservation,
        ComparisonObservation,
    ),
    &'static str,
> {
    let model_value = invoke_universal_unary(model, model_input)
        .filter(|_| jet_jit_is_trapped() == 0)
        .ok_or("core.testing.histories model callback stopped before observing a value")?;
    let actual_value = invoke_universal_unary(actual, actual_input)
        .filter(|_| jet_jit_is_trapped() == 0)
        .ok_or("core.testing.histories actual callback stopped before observing a value")?;
    let model_tree = crate::Encoding::read_datatree(model_value)
        .ok_or("core.testing.histories model callback returned a non-DataTree")?;
    let actual_tree = crate::Encoding::read_datatree(actual_value)
        .ok_or("core.testing.histories actual callback returned a non-DataTree")?;
    let model_observed = invoke_universal_unary(
        observe,
        crate::Encoding::alloc_datatree(&model_tree),
    )
    .filter(|_| jet_jit_is_trapped() == 0)
    .ok_or("core.testing.histories model observer stopped before observing a value")?;
    let actual_observed = invoke_universal_unary(
        observe,
        crate::Encoding::alloc_datatree(&actual_tree),
    )
    .filter(|_| jet_jit_is_trapped() == 0)
    .ok_or("core.testing.histories actual observer stopped before observing a value")?;
    let reference = testing_history_observation(model_observed)
        .ok_or("core.testing.histories model observer returned a non-DataTree")?;
    let candidate = testing_history_observation(actual_observed)
        .ok_or("core.testing.histories actual observer returned a non-DataTree")?;
    Ok((model_value, actual_value, reference, candidate))
}

fn testing_history_observe_case(
    case: &jet_foundation::TestingHistory::HistoryCase,
    descriptor: Option<&RuntimeTypeDescriptor>,
    relation: &ObservationRelation,
    model: JitCallableSlot,
    actual: JitCallableSlot,
    observe: JitCallableSlot,
    inputs: &mut Vec<i64>,
    reference_values: &mut Vec<i64>,
    candidate_values: &mut Vec<i64>,
) -> ComparisonRecord {
    let terminal = |status, reason: &str| {
        ComparisonRecord::terminal(relation.clone(), status, reason)
    };
    let model_input = match testing_history_input(case, descriptor) {
        Ok(input) => input,
        Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
    };
    let actual_input = match testing_history_input(case, descriptor) {
        Ok(input) => input,
        Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
    };
    let (model_value, actual_value, reference, candidate) =
        match testing_history_execute(model_input, actual_input, model, actual, observe) {
            Ok(values) => values,
            Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
        };
    let replay_model_input = match testing_history_input(case, descriptor) {
        Ok(input) => input,
        Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
    };
    let replay_actual_input = match testing_history_input(case, descriptor) {
        Ok(input) => input,
        Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
    };
    let (_model_replay_value, _actual_replay_value, reference_replay, candidate_replay) =
        match testing_history_execute(
            replay_model_input,
            replay_actual_input,
            model,
            actual,
            observe,
        ) {
            Ok(values) => values,
            Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
        };
    let Some(identity) = history_comparison_identity(
        case.case_id.clone(),
        case.input_id(),
        case.seed,
        &[
            ("model", HistoryCallbackIdentity::Slot(model)),
            ("actual", HistoryCallbackIdentity::Slot(actual)),
            ("observe", HistoryCallbackIdentity::Slot(observe)),
            (
                "strategy.derived",
                HistoryCallbackIdentity::Fingerprint(history_strategy_schema_fingerprint(descriptor)),
            ),
        ],
    ) else {
        return terminal(
            ComparisonStatus::Unavailable,
            "core.testing.histories checked provenance is unavailable",
        );
    };
    let sample = ComparisonSample::new(identity, reference, candidate)
        .with_replays(reference_replay, candidate_replay);
    let evidence_input = if descriptor.is_some() {
        match testing_history_input(case, None) {
            Ok(input) => input,
            Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
        }
    } else {
        model_input
    };
    inputs.push(evidence_input);
    reference_values.push(model_value);
    candidate_values.push(actual_value);
    compare_samples(relation.clone(), [sample])
}

fn testing_history_execute_typed(
    commands: &[JitHistoryCommand],
    descriptor: Option<&RuntimeTypeDescriptor>,
    model: JitCallableSlot,
    actual: JitCallableSlot,
    observe: JitCallableSlot,
) -> Result<(i64, i64, ComparisonObservation, ComparisonObservation), &'static str> {
    let (model_input, actual_input) = Concurrency::with_runtime_result("no active resident runtime", |rt| {
        Ok::<_, &'static str>((
            history_commands_to_runtime(rt, commands, descriptor)
                .map_err(|_| "core.testing.histories command list could not be allocated")?,
            history_commands_to_runtime(rt, commands, descriptor)
                .map_err(|_| "core.testing.histories command list could not be allocated")?,
        ))
    })?;
    testing_history_execute(model_input, actual_input, model, actual, observe)
}

fn testing_history_observe_typed_case(
    typed: &jet_foundation::TestingHistory::TypedHistoryCase<JitHistoryCommand>,
    strategy: &JitHistoryStrategy,
    relation: &ObservationRelation,
    model: JitCallableSlot,
    actual: JitCallableSlot,
    observe: JitCallableSlot,
    inputs: &mut Vec<i64>,
    reference_values: &mut Vec<i64>,
    candidate_values: &mut Vec<i64>,
) -> ComparisonRecord {
    let terminal = |status, reason: &str| {
        ComparisonRecord::terminal(relation.clone(), status, reason)
    };
    let evidence_tree =
        <JitHistoryStrategy as jet_foundation::TestingHistory::HistoryStrategyBehavior<
            JitHistoryCommand,
        >>::commands_to_data_tree(strategy, &typed.commands);
    let Some(evidence_tree) = evidence_tree else {
        return terminal(
            ComparisonStatus::Unavailable,
            "core.testing.histories strategy has no lossless command codec",
        );
    };
    let (model_value, actual_value, reference, candidate) =
        match testing_history_execute_typed(&typed.commands, strategy.command_descriptor.as_ref(), model, actual, observe) {
            Ok(values) => values,
            Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
        };
    let (reference_replay, candidate_replay) =
        match testing_history_execute_typed(&typed.commands, strategy.command_descriptor.as_ref(), model, actual, observe) {
            Ok((_, _, reference, candidate)) => (reference, candidate),
            Err(reason) => return terminal(ComparisonStatus::Unavailable, reason),
        };
    let Some(identity) = history_comparison_identity(
        typed.case.case_id.clone(),
        typed.case.input_id(),
        typed.case.seed,
        &[
            ("model", HistoryCallbackIdentity::Slot(model)),
            ("actual", HistoryCallbackIdentity::Slot(actual)),
            ("observe", HistoryCallbackIdentity::Slot(observe)),
            (
                "strategy.generate",
                HistoryCallbackIdentity::Slot(strategy.generate),
            ),
            (
                "strategy.rebuild",
                HistoryCallbackIdentity::Slot(strategy.rebuild),
            ),
            (
                "strategy.valid",
                HistoryCallbackIdentity::Slot(strategy.valid),
            ),
        ],
    ) else {
        return terminal(
            ComparisonStatus::Unavailable,
            "core.testing.histories checked provenance is unavailable",
        );
    };
    let sample = ComparisonSample::new(identity, reference, candidate)
        .with_replays(reference_replay, candidate_replay);
    inputs.push(crate::Encoding::alloc_datatree(&evidence_tree));
    reference_values.push(model_value);
    candidate_values.push(actual_value);
    compare_samples(relation.clone(), [sample])
}

fn run_jit_history_campaign<Command, Strategy, F>(
    config: jet_foundation::TestingHistory::HistoryConfig,
    strategy: Strategy,
    relation: ObservationRelation,
    model: JitCallableSlot,
    actual: JitCallableSlot,
    observe: JitCallableSlot,
    mut observe_case: F,
) -> i64
where
    Command: jet_foundation::TestingHistory::HistoryCommand,
    Strategy: jet_foundation::TestingHistory::HistoryStrategyBehavior<Command>,
    F: FnMut(
        &jet_foundation::TestingHistory::TypedHistoryCase<Command>,
        &ObservationRelation,
        &mut Vec<i64>,
        &mut Vec<i64>,
        &mut Vec<i64>,
    ) -> ComparisonRecord,
{
    let seed = config.seed;
    let run_relation = relation.clone();
    let runner = match jet_foundation::TestingHistory::HistoryRunner::new(config) {
        Ok(runner) => runner,
        Err(error) => {
            return testing_history_terminal(
                relation,
                ComparisonStatus::Unavailable,
                &error.to_string(),
            )
        }
    };
    let mut inputs = Vec::new();
    let mut reference_values = Vec::new();
    let mut candidate_values = Vec::new();
    let mut samples = Vec::new();
    let mut terminal = None;
    let run = runner.run_strategy::<Command, _, _>(&strategy, |typed| {
        let record = observe_case(
            typed,
            &run_relation,
            &mut inputs,
            &mut reference_values,
            &mut candidate_values,
        );
        if matches!(
            &record.status,
            ComparisonStatus::Unavailable
                | ComparisonStatus::Timeout
                | ComparisonStatus::Cancelled
                | ComparisonStatus::Unsupported
        ) {
            terminal = Some(record.clone());
        }
        if matches!(
            &record.status,
            ComparisonStatus::Matched | ComparisonStatus::Mismatch
        ) {
            samples.extend(record.samples.clone());
        }
        record
    });
    let record = match run {
        Ok(run) => {
            if let Some(artifact) = run.failure {
                let artifact_json = artifact.json();
                let mut record = artifact.comparison;
                record.reason = Some(format!("history-artifact {artifact_json}"));
                record
            } else if let Some(record) = terminal {
                record
            } else {
                let mut record = compare_samples_with_discarded(
                    run_relation.clone(),
                    samples,
                    run.discarded_cases,
                );
                if run.explored_cases == 0 {
                    record.status = ComparisonStatus::Unavailable;
                    record.reason =
                        Some("no history case produced an observation".to_string());
                }
                record
            }
        }
        Err(error) => ComparisonRecord::terminal(
            run_relation,
            if matches!(
                &error,
                jet_foundation::TestingHistory::HistoryError::InvalidOracle(_)
            ) {
                ComparisonStatus::InvalidOracle
            } else {
                ComparisonStatus::Unavailable
            },
            error.to_string(),
        ),
    };
    let is_matched = matches!(&record.status, ComparisonStatus::Matched);
    let status = record.status.as_str().to_string();
    let reason = record
        .reason
        .clone()
        .unwrap_or_else(|| "history comparison has no reason".to_string());
    let comparison = Concurrency::with_runtime_mut(|rt| {
        let input_list = rt.heap.alloc_empty_list();
        for input in inputs {
            let _ = rt.heap.list_push_int(input_list, input);
        }
        alloc_test_comparison(
            rt,
            testing_comparison_carrier(
                record,
                input_list,
                Some(seed as i64),
                reference_values,
                candidate_values,
            ),
        )
    });
    if is_matched {
        crate::Marshal::result_ok(comparison as u64)
    } else {
        crate::Marshal::result_err_msg(&format!(
            "core.testing.histories {status}: {reason}"
        ))
    }
}


fn jet_jit_testing_histories(
    seed: i64,
    cases: i64,
    strategy: i64,
    model: i64,
    actual: i64,
    observe: i64,
    command_type: i64,
) -> i64 {
    let relation = ObservationRelation::TypedEquality;
    let command_type = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(command_type))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "HistoryCommand".to_string());
    let Ok(seed) = u64::try_from(seed) else {
        return testing_history_terminal(
            relation,
            ComparisonStatus::Unavailable,
            "core.testing.histories seed must be non-negative",
        );
    };
    let Ok(cases) = usize::try_from(cases) else {
        return testing_history_terminal(
            relation,
            ComparisonStatus::Unavailable,
            "core.testing.histories case bound is invalid",
        );
    };
    let slots = Concurrency::with_runtime_mut(|rt| {
        Some((
            jit_callable_slot(rt, model)?,
            jit_callable_slot(rt, actual)?,
            jit_callable_slot(rt, observe)?,
        ))
    });
    let Some((model, actual, observe)) = slots else {
        return testing_history_terminal(
            relation,
            ComparisonStatus::Unavailable,
            "core.testing.histories received an invalid callback handle",
        );
    };
    if model.raw_unary.is_none() || actual.raw_unary.is_none() || observe.raw_unary.is_none() {
        return testing_history_terminal(
            relation,
            ComparisonStatus::Unavailable,
            "core.testing.histories callback has no universal thunk",
        );
    }
    let explicit = if strategy == 0 {
        Ok(None)
    } else {
        decode_jit_history_strategy(strategy, command_type.clone())
    };
    let Ok(explicit) = explicit else {
        return testing_history_terminal(
            relation,
            ComparisonStatus::Unavailable,
            "core.testing.histories explicit strategy is invalid",
        );
    };
    let oracle = jet_foundation::TestingHistory::OracleDeclaration::independent(
        "testing.histories.model",
    );
    if let Some(strategy) = explicit {
        let bounds = strategy.bounds();
        let config = jet_foundation::TestingHistory::HistoryConfig::new(
            seed,
            cases,
            bounds,
            "core.testing.histories",
            jet_foundation::TestingHistory::HISTORY_ENGINE,
            "resident",
            relation.clone(),
            oracle,
        )
        .with_command_type(strategy.command_type());
        let config = strategy
            .distributions()
            .into_iter()
            .fold(config, |config, distribution| config.with_distribution(distribution));
        let strategy_for_observe = strategy.clone();
        return run_jit_history_campaign::<JitHistoryCommand, _, _>(
            config,
            strategy,
            relation,
            model,
            actual,
            observe,
            move |typed, relation, inputs, reference_values, candidate_values| {
                testing_history_observe_typed_case(
                    typed,
                    &strategy_for_observe,
                    relation,
                    model,
                    actual,
                    observe,
                    inputs,
                    reference_values,
                    candidate_values,
                )
            },
        );
    }
    let (command_descriptor, strategy) =
        if command_type == "DataTree" || command_type.starts_with("DataTree|") {
            (
                None,
                jet_foundation::TestingHistory::HistorySchemaStrategy::new(command_type.clone()),
            )
        } else {
            let schema = Concurrency::with_runtime_string(|rt| {
                history_schema_for_command(rt, &command_type)
            });
            match schema {
                Ok(Some((descriptor, strategy))) => (Some(descriptor), strategy),
                Ok(None) | Err(_) => {
                    return testing_history_comparison_terminal(
                        relation,
                        ComparisonStatus::Unsupported,
                        &jet_foundation::TestingHistory::history_unsupported_type_reason(
                            &command_type,
                        ),
                        Some(seed as i64),
                    );
                }
            }
        };
    let bounds = jet_foundation::TestingHistory::HistoryBounds {
        max_steps: 8,
        max_resources: 32,
        max_shrink_attempts: 10_000,
        max_discarded_cases: 100,
    };
    let config = jet_foundation::TestingHistory::HistoryConfig::new(
        seed,
        cases,
        bounds,
        "core.testing.histories",
        jet_foundation::TestingHistory::HISTORY_ENGINE,
        "resident",
        relation.clone(),
        oracle,
    )
    .with_command_type(strategy.command_type());
    let config = strategy
        .distributions()
        .into_iter()
        .fold(config, |config, distribution| config.with_distribution(distribution));
    run_jit_history_campaign::<
        jet_foundation::TestingHistory::HistorySchemaCommand,
        _,
        _,
    >(
        config,
        strategy,
        relation,
        model,
        actual,
        observe,
        move |typed, relation, inputs, reference_values, candidate_values| {
            testing_history_observe_case(
                &typed.case,
                command_descriptor.as_ref(),
                relation,
                model,
                actual,
                observe,
                inputs,
                reference_values,
                candidate_values,
            )
        },
    )
}

fn jet_jit_testing_assert_equal(comparison: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let status = rt
            .heap
            .record_get_string(comparison, 0)
            .and_then(|value| rt.heap.clone_string(value));
        let samples = rt.heap.record_get_int(comparison, 7).unwrap_or(0);
        let first_difference = rt.heap.record_get_int(comparison, 10).unwrap_or(0);
        i8::from(
            status.as_deref() == Some("matched")
                && rt.heap.list_len(samples).unwrap_or(0) > 0
                && first_difference < 0,
        )
    })
}

fn jet_jit_testing_status(comparison: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let status = rt
            .heap
            .record_get_string(comparison, 0)
            .and_then(|value| rt.heap.clone_string(value))
            .unwrap_or_else(|| "unavailable".to_string());
        rt.heap.alloc_string(status)
    })
}

fn jet_hardware_setup(
    profile_id: i64,
    setup_kind: i64,
    item: i64,
    width_or_vector: i64,
    ownership_or_handler: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(host) = rt.hardware_host.as_mut() else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
            hardware_bridge::jet_hardware_setup(
                profile_id,
                setup_kind,
                item,
                width_or_vector,
                ownership_or_handler,
            )
        })
    })
}

fn jet_hardware_register_read(profile_id: i64, block: i64, register: i64, width: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(host) = rt.hardware_host.as_mut() else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
            hardware_bridge::jet_hardware_register_read(profile_id, block, register, width)
        })
    })
}

fn jet_hardware_register_write(
    profile_id: i64,
    block: i64,
    register: i64,
    width: i64,
    value: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(host) = rt.hardware_host.as_mut() else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
            hardware_bridge::jet_hardware_register_write(profile_id, block, register, width, value)
        })
    })
}

fn jet_hardware_dma_start(
    profile_id: i64,
    channel: i64,
    address: i64,
    bytes: i64,
) -> i64 {
    if jet_foundation::ResourceSchedule::current_frame_completion_state().is_some() {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault(
                "checked DMA start requires the marshaled transfer completion binding",
            );
            JET_HARDWARE_UNAVAILABLE
        });
    }
    Concurrency::with_runtime_mut(|rt| {
        let Some(host) = rt.hardware_host.as_mut() else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
            hardware_bridge::jet_hardware_dma_start(profile_id, channel, address, bytes)
        })
    })
}

fn jet_hardware_dma_wait(profile_id: i64, channel: i64, transfer: i64) -> i64 {
    if jet_foundation::ResourceSchedule::current_frame_completion_state().is_some() {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault(
                "checked DMA wait requires the marshaled transfer completion binding",
            );
            JET_HARDWARE_UNAVAILABLE
        });
    }
    Concurrency::with_runtime_mut(|rt| {
        let Some(host) = rt.hardware_host.as_mut() else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
            hardware_bridge::jet_hardware_dma_wait(profile_id, channel, transfer)
        })
    })
}

fn jet_jit_dma_start_marshaled(
    profile_id: i64,
    channel: i64,
    source: i64,
    type_key: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(profile) = runtime.heap.clone_string(profile_id) else {
            runtime.set_host_fault("MIR DMA profile handle is invalid");
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(channel_name) = runtime.heap.clone_string(channel) else {
            runtime.set_host_fault("MIR DMA channel handle is invalid");
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(type_key) = runtime.heap.clone_string(type_key) else {
            runtime.set_host_fault("MIR DMA carrier type handle is invalid");
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(buffer_ty) = runtime.dma_types.get(&type_key).cloned() else {
            runtime.set_host_fault("MIR DMA carrier type facts are missing");
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Ok(mut pinned) = encode_dma_payload(runtime, source, buffer_ty) else {
            runtime.set_host_fault("MIR DMA carrier could not be encoded");
            return JET_HARDWARE_UNAVAILABLE;
        };
        pinned.completion_handle =
            match jet_foundation::ResourceSchedule::current_frame_completion_handle(
                "jet_dma_transfer",
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    runtime.set_host_fault(&error);
                    return JET_HARDWARE_UNAVAILABLE;
                }
            };
        let Ok(address) = i64::try_from(pinned.bytes.as_mut_ptr() as usize) else {
            runtime.set_host_fault("MIR DMA carrier address exceeds the host ABI");
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Ok(bytes) = i64::try_from(pinned.bytes.len()) else {
            runtime.set_host_fault("MIR DMA carrier byte length exceeds the host ABI");
            return JET_HARDWARE_UNAVAILABLE;
        };
        let token = runtime
            .hardware_host
            .as_mut()
            .map(|host| {
                host.register_string_handle(profile_id, &profile);
                host.register_string_handle(channel, &channel_name);
                hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
                    hardware_bridge::jet_hardware_dma_start(profile_id, channel, address, bytes)
                })
            })
            .unwrap_or(JET_HARDWARE_UNAVAILABLE);
        if token <= 0 {
            if token == JET_HARDWARE_UNAVAILABLE {
                runtime.set_host_fault("MIR DMA start is unavailable");
            }
            return token;
        }
        if runtime.dma_transfers.insert(token, pinned).is_some() {
            runtime.dma_transfers.remove(&token);
            runtime.set_host_fault("MIR DMA host returned a duplicate transfer token");
            return JET_HARDWARE_UNAVAILABLE;
        }
        token
    })
}

fn jet_jit_dma_wait_marshaled(profile_id: i64, channel: i64, token: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let status = runtime
            .hardware_host
            .as_mut()
            .map(|host| {
                hardware_bridge::jet_hardware_with_erased_host(host.as_mut(), || {
                    hardware_bridge::jet_hardware_dma_wait(profile_id, channel, token)
                })
            })
            .unwrap_or(JET_HARDWARE_UNAVAILABLE);
        if status != 0 {
            runtime.set_host_fault("MIR DMA wait is unavailable or failed");
            return 0;
        }
        let Some(pinned) = runtime.dma_transfers.remove(&token) else {
            runtime.set_host_fault("MIR DMA wait returned an unknown transfer token");
            return 0;
        };
        if let Some(handle) = pinned.completion_handle.as_ref() {
            if let Err(error) = handle.signal() {
                runtime.set_host_fault(&error);
                return 0;
            }
        } else if jet_foundation::ResourceSchedule::current_frame_completion_state().is_some() {
            runtime.set_host_fault(
                "checked DMA completion is missing its submission binding",
            );
            return 0;
        }
        if let Err(message) = decode_dma_payload(runtime, &pinned) {
            runtime.set_host_fault(&message);
            return 0;
        }
        pinned.source
    })
}

fn jet_hardware_interrupt_poll() -> i64 {
    if !hardware_bridge::jet_hardware_has_pending() {
        return 0;
    }
    let callbacks = Concurrency::with_runtime_mut(|rt| {
        let registry = rt.hardware_interrupt_registry();
        hardware_bridge::jet_hardware_dispatch_pending(&registry)
    });
    hardware_bridge::jet_hardware_invoke_pending(callbacks) as i64
}
fn model_error_result(runtime: &mut JitRuntime, message: impl Into<String>) -> i64 {
    let message = runtime.heap.alloc_string(message.into());
    alloc_jit_result(runtime, false, message as u64)
}

/// Open one checked model output for the resident JIT.  The model package
/// adapter only projects loader facts; execution itself stays in `jet_rt`.
fn jet_jit_model_open(output: i64, trait_name: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let output = runtime.heap.get_string(output).unwrap_or("").to_owned();
        let trait_name = runtime.heap.get_string(trait_name).unwrap_or("").to_owned();
        let trait_leaf = trait_name
            .rsplit("::")
            .next()
            .unwrap_or(&trait_name)
            .rsplit('.')
            .next()
            .unwrap_or(&trait_name)
            .to_owned();
        let Some(fact) = runtime
            .model_outputs
            .iter()
            .find(|fact| fact.output == output && fact.signature_name.as_deref() == Some(trait_leaf.as_str()))
            .cloned()
        else {
            return model_error_result(
                runtime,
                format!("model output `{output}` has no checked `{trait_name}` binding"),
            );
        };
        let control = match jet_scheduler_current_task_control() {
            Some(control) => control,
            None => return model_error_result(runtime, "model execution requires an active scheduler task"),
        };
        let cancellation =
            jet_rt::model::provider::CancellationToken::from_cancel_flag(control.cancelled.clone());
        let _cancel_bridge = {
            let cancellation = cancellation.clone();
            control.register_cancel_callback(std::sync::Arc::new(move || cancellation.cancel()))
        };
        let package = match jet_pkg_model::ModelPackage::load(&fact.package_root, &fact.output) {
            Ok(package) => package,
            Err(error) => return model_error_result(runtime, error.to_string()),
        };
        let graph = match package.artifacts.first() {
            Some(artifact) => match package.read_artifact(&fact.package_root, artifact) {
                Ok(bytes) => bytes,
                Err(error) => return model_error_result(runtime, error.to_string()),
            },
            None => return model_error_result(runtime, "model package has no graph artifact"),
        };
        let policy = match jet_rt::model::provider::OnnxRuntimePolicy::cpu_for_graph(&graph) {
            Ok(policy) => policy,
            Err(error) => return model_error_result(runtime, error.to_string()),
        };
        let runtime_path = match std::env::var_os("JET_ONNX_RUNTIME_LIBRARY") {
            Some(path) => path,
            None => {
                return model_error_result(
                    runtime,
                    "JET_ONNX_RUNTIME_LIBRARY is required for model execution",
                )
            }
        };
        let pin = match jet_rt::model::provider::RuntimePin::official_linux_x64(runtime_path) {
            Ok(pin) => pin,
            Err(error) => return model_error_result(runtime, error.to_string()),
        };
        let provider = match jet_rt::model::provider::OnnxRuntimeProvider::native(pin, policy) {
            Ok(provider) => provider,
            Err(error) => return model_error_result(runtime, error.to_string()),
        };
        let session = match jet_rt::model::provider::run_ready(package.open_with(
            jet_rt::model::ModelSource::Directory(&fact.package_root),
            &provider,
            &cancellation,
        )) {
            Ok(session) => session,
            Err(error) => return model_error_result(runtime, error.to_string()),
        };
        runtime.model_sessions.push(session);
        let handle = runtime.model_sessions.len() as i64;
        alloc_jit_result(runtime, true, handle as u64)
    })
}

/// Execute a checked embedding method through a resident model session.
fn jet_jit_model_embed(session: i64, documents: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(length) = runtime.heap.list_len(documents) else {
            return model_error_result(runtime, "model embed expects a string list");
        };
        let mut source = Vec::with_capacity(length as usize);
        for index in 0..length {
            let Some(document) = runtime.heap.list_get_string(documents, index) else {
                return model_error_result(runtime, "model embed received a non-string document");
            };
            source.push(document);
        }
        let control = match jet_scheduler_current_task_control() {
            Some(control) => control,
            None => return model_error_result(runtime, "model execution requires an active scheduler task"),
        };
        let cancellation =
            jet_rt::model::provider::CancellationToken::from_cancel_flag(control.cancelled.clone());
        let _cancel_bridge = {
            let cancellation = cancellation.clone();
            control.register_cancel_callback(std::sync::Arc::new(move || cancellation.cancel()))
        };
        let Some(slot) = session
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| runtime.model_sessions.get_mut(index))
        else {
            return model_error_result(runtime, "model embed received an unknown session handle");
        };
        let batch = match jet_rt::model::provider::run_ready(slot.embed_documents(&source, &cancellation)) {
            Ok(batch) => batch,
            Err(error) => return model_error_result(runtime, error.to_string()),
        };
        let values = runtime.heap.alloc_empty_list();
        for row in batch.values() {
            let row_handle = runtime.heap.alloc_empty_list();
            for value in row {
                let _ = runtime.heap.list_push_float(row_handle, f64::from(*value));
            }
            if let Some(entries) = runtime.heap.list_values_mut(values) {
                entries.push(JetVal::RecordRef(row_handle));
            } else {
                return model_error_result(runtime, "model output list carrier is invalid");
            }
        }
        let space = runtime.heap.alloc_record_cells(vec![
            JetVal::String(batch.space().model_digest().to_owned()),
            JetVal::Int(batch.space().dimension() as i64),
            JetVal::String(batch.space().metric().to_owned()),
            JetVal::String(batch.space().normalization().to_owned()),
        ]);
        let result = runtime.heap.alloc_record_cells(vec![
            JetVal::RecordRef(values),
            JetVal::RecordRef(space),
        ]);
        alloc_jit_result(runtime, true, result as u64)
    })
}


fn jet_hardware_replay_interrupt(vector: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.trigger_hardware_interrupt(vector))
}

// #1633: one host_fns! listing for the top-level table + delegate composition.
host_fns! {
    struct HostFns;
    register: register_host_symbols;
    declare: declare_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig_bin_i64 = Signature::new(cc);
        sig_bin_i64.params.push(AbiParam::new(types::I64));
        sig_bin_i64.params.push(AbiParam::new(types::I64));
        sig_bin_i64.params.push(AbiParam::new(types::I32));
        sig_bin_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_pow_f64 = Signature::new(cc);
        sig_pow_f64.params.push(AbiParam::new(types::F64));
        sig_pow_f64.params.push(AbiParam::new(types::F64));
        sig_pow_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_intn_binop = Signature::new(cc);
        for _ in 0..7 {
            sig_intn_binop.params.push(AbiParam::new(types::I64));
        }
        sig_intn_binop.params.push(AbiParam::new(types::I32));
        sig_intn_binop.returns.push(AbiParam::new(types::I64));

        let mut sig_i64 = Signature::new(cc);
        sig_i64.params.push(AbiParam::new(types::I64));
        let mut sig_clock_clone = sig_i64.clone();
        sig_clock_clone.returns.push(AbiParam::new(types::I64));
        let mut sig_typed_clone = sig_clock_clone.clone();
        sig_typed_clone.params.push(AbiParam::new(types::I64));

        let mut sig_model_open = Signature::new(cc);
        sig_model_open.params.extend([AbiParam::new(types::I64); 2]);
        sig_model_open.returns.push(AbiParam::new(types::I64));
        let sig_model_embed = sig_model_open.clone();
        let mut sig_reflect_finish = Signature::new(cc);
        for _ in 0..4 {
            sig_reflect_finish.params.push(AbiParam::new(types::I64));
        }
        sig_reflect_finish.returns.push(AbiParam::new(types::I64));
        let mut sig_reflect_field_new = Signature::new(cc);
        sig_reflect_field_new
            .params
            .extend([AbiParam::new(types::I64); 2]);
        sig_reflect_field_new
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_rich_panic = Signature::new(cc);
        for _ in 0..8 {
            sig_rich_panic.params.push(AbiParam::new(types::I64));
        }
        sig_rich_panic.returns.push(AbiParam::new(types::I64));
        let mut sig_require = Signature::new(cc);
        sig_require
            .params
            .extend([AbiParam::new(types::I64); 9]);
        sig_require.returns.push(AbiParam::new(types::I64));
        let mut sig_require_eq = Signature::new(cc);
        sig_require_eq
            .params
            .extend([AbiParam::new(types::I64); 10]);
        sig_require_eq.returns.push(AbiParam::new(types::I64));
        let sig_test_require_eq = sig_require_eq.clone();
        let sig_test_require = sig_require.clone();
        let mut sig_f64 = Signature::new(cc);
        sig_f64.params.push(AbiParam::new(types::F64));
        let mut sig_i8 = Signature::new(cc);
        sig_i8.params.push(AbiParam::new(types::I8));
        let mut sig_i32 = Signature::new(cc);
        sig_i32.params.push(AbiParam::new(types::I32));
        let mut sig_debug_i64 = sig_i64.clone();
        sig_debug_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_debug_f64 = sig_f64.clone();
        sig_debug_f64.returns.push(AbiParam::new(types::I64));
        let mut sig_debug_f32 = Signature::new(cc);
        sig_debug_f32.params.push(AbiParam::new(types::F32));
        sig_debug_f32.returns.push(AbiParam::new(types::I64));
        let mut sig_debug_bool = sig_i8.clone();
        sig_debug_bool.returns.push(AbiParam::new(types::I64));
        let mut sig_debug_char = sig_i32.clone();
        sig_debug_char.returns.push(AbiParam::new(types::I64));
        let mut sig_debug_string = sig_i64.clone();
        sig_debug_string.returns.push(AbiParam::new(types::I64));
        let mut sig_debug_local_append = Signature::new(cc);
        sig_debug_local_append
            .params
            .extend([AbiParam::new(types::I64); 3]);
        sig_debug_local_append.params.push(AbiParam::new(types::I8));
        sig_debug_local_append.returns.push(AbiParam::new(types::I64));
        let mut sig_todo_stop = Signature::new(cc);
        sig_todo_stop.params.push(AbiParam::new(types::I64));
        sig_todo_stop.params.push(AbiParam::new(types::I64));
        sig_todo_stop.returns.push(AbiParam::new(types::I64));
        let mut sig_stack_enter = Signature::new(cc);
        sig_stack_enter
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_stack_enter.returns.push(AbiParam::new(types::I64));
        let mut sig_contract_check = Signature::new(cc);
        sig_contract_check.params.push(AbiParam::new(types::I8));
        sig_contract_check.returns.push(AbiParam::new(types::I8));
        let mut sig_contract_fail = Signature::new(cc);
        for _ in 0..4 {
            sig_contract_fail.params.push(AbiParam::new(types::I64));
        }
        sig_contract_fail.returns.push(AbiParam::new(types::I64));
        let mut sig_slice_range = Signature::new(cc);
        sig_slice_range
            .params
            .extend([AbiParam::new(types::I64); 3]);
        sig_slice_range.params.push(AbiParam::new(types::I32));
        sig_slice_range.returns.push(AbiParam::new(types::I64));
        let mut sig_slice_vec = Signature::new(cc);
        sig_slice_vec
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_slice_vec.params.push(AbiParam::new(types::I32));
        sig_slice_vec.returns.push(AbiParam::new(types::I64));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.params.push(AbiParam::new(types::F64));
        let mut sig_i8 = Signature::new(cc);
        sig_i8.params.push(AbiParam::new(types::I8));
        let mut sig_i32 = Signature::new(cc);
        sig_i32.params.push(AbiParam::new(types::I32));
        let mut sig_str_push_lit = Signature::new(cc);
        sig_str_push_lit.params.push(AbiParam::new(types::I64));
        sig_str_push_lit.params.push(AbiParam::new(types::I64));
        let mut sig_str_push_i64 = Signature::new(cc);
        sig_str_push_i64.params.push(AbiParam::new(types::I64));
        sig_str_push_i64.params.push(AbiParam::new(types::I64));
        let mut sig_str_push_f64 = Signature::new(cc);
        sig_str_push_f64.params.push(AbiParam::new(types::I64));
        sig_str_push_f64.params.push(AbiParam::new(types::F64));
        let mut sig_str_push_bool = Signature::new(cc);
        sig_str_push_bool.params.push(AbiParam::new(types::I64));
        sig_str_push_bool.params.push(AbiParam::new(types::I8));
        let mut sig_str_push_char = Signature::new(cc);
        sig_str_push_char.params.push(AbiParam::new(types::I64));
        sig_str_push_char.params.push(AbiParam::new(types::I32));
        let mut sig_str_eq = Signature::new(cc);
        sig_str_eq.params.push(AbiParam::new(types::I64));
        sig_str_eq.params.push(AbiParam::new(types::I64));
        sig_str_eq.returns.push(AbiParam::new(types::I8));
        let mut sig_typed_eq = sig_str_eq.clone();
        sig_typed_eq.params.push(AbiParam::new(types::I64));
        let mut sig_str_unary_i64 = Signature::new(cc);
        sig_str_unary_i64.params.push(AbiParam::new(types::I64));
        sig_str_unary_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_str_unary_i8 = Signature::new(cc);
        sig_str_unary_i8.params.push(AbiParam::new(types::I64));
        sig_str_unary_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_testing_compare = Signature::new(cc);
        sig_testing_compare
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_testing_compare.returns.push(AbiParam::new(types::I64));
        let mut sig_testing_histories = Signature::new(cc);
        sig_testing_histories
            .params
            .extend([AbiParam::new(types::I64); 7]);
        sig_testing_histories.returns.push(AbiParam::new(types::I64));
        let sig_testing_assert_equal = sig_str_unary_i8.clone();
        let sig_testing_status = sig_str_unary_i64.clone();
        let mut sig_str_replace = Signature::new(cc);
        sig_str_replace.params.push(AbiParam::new(types::I64));
        sig_str_replace.params.push(AbiParam::new(types::I64));
        sig_str_replace.params.push(AbiParam::new(types::I64));
        sig_str_replace.returns.push(AbiParam::new(types::I64));
        let mut sig_str_binary_i64 = Signature::new(cc);
        sig_str_binary_i64.params.push(AbiParam::new(types::I64));
        sig_str_binary_i64.params.push(AbiParam::new(types::I64));
        sig_str_binary_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_str_slice_range = Signature::new(cc);
        sig_str_slice_range.params.push(AbiParam::new(types::I64));
        sig_str_slice_range.params.push(AbiParam::new(types::I64));
        sig_str_slice_range.params.push(AbiParam::new(types::I64));
        sig_str_slice_range.params.push(AbiParam::new(types::I8));
        sig_str_slice_range.params.push(AbiParam::new(types::I32));
        sig_str_slice_range.returns.push(AbiParam::new(types::I64));
        let mut sig_f64_i64 = Signature::new(cc);
        sig_f64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64_i64_i8 = Signature::new(cc);
        sig_f64_i64_i8.params.push(AbiParam::new(types::F64));
        sig_f64_i64_i8.params.push(AbiParam::new(types::I64));
        sig_f64_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_observe_live_value_update = Signature::new(cc);
        sig_observe_live_value_update
            .params
            .extend([AbiParam::new(types::I64); 3]);
        let mut sig_err_apply_conversion = Signature::new(cc);
        sig_err_apply_conversion
            .params
            .extend([AbiParam::new(types::I64); 3]);
        let mut sig_err_add_context = Signature::new(cc);
        sig_err_add_context
            .params
            .extend([AbiParam::new(types::I64); 4]);
        let mut sig_err_with_context_frame = Signature::new(cc);
        sig_err_with_context_frame.params.extend([AbiParam::new(types::I64); 5]);
        sig_err_with_context_frame.returns.push(AbiParam::new(types::I64));
        let mut sig_trace_err = Signature::new(cc);
        sig_trace_err.params.push(AbiParam::new(types::I64));
        sig_trace_err.params.push(AbiParam::new(types::I64));
        sig_trace_err.params.push(AbiParam::new(types::I64));
        let mut sig_trace_err_note = Signature::new(cc);
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        let sig_trace_reset = Signature::new(cc);
        let mut sig_f64_i64_i64 = Signature::new(cc);
        sig_f64_i64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_f64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_str_begin = Signature::new(cc);
        sig_str_begin.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_new = Signature::new(cc);
        sig_struct_new.params.push(AbiParam::new(types::I64));
        sig_struct_new.returns.push(AbiParam::new(types::I64));
        let mut sig_trait_object_tag = Signature::new(cc);
        sig_trait_object_tag
            .params
            .extend([AbiParam::new(types::I64); 2]);
        sig_trait_object_tag.returns.push(AbiParam::new(types::I64));
        let mut sig_trait_object_type = Signature::new(cc);
        sig_trait_object_type.params.push(AbiParam::new(types::I64));
        sig_trait_object_type.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_assign = Signature::new(cc);
        sig_struct_assign.params.push(AbiParam::new(types::I64));
        sig_struct_assign.params.push(AbiParam::new(types::I64));
        let mut sig_struct_get_i64 = Signature::new(cc);
        sig_struct_get_i64.params.push(AbiParam::new(types::I64));
        sig_struct_get_i64.params.push(AbiParam::new(types::I64));
        sig_struct_get_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_get_f64 = Signature::new(cc);
        sig_struct_get_f64.params.push(AbiParam::new(types::I64));
        sig_struct_get_f64.params.push(AbiParam::new(types::I64));
        sig_struct_get_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_struct_get_i8 = Signature::new(cc);
        sig_struct_get_i8.params.push(AbiParam::new(types::I64));
        sig_struct_get_i8.params.push(AbiParam::new(types::I64));
        sig_struct_get_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_struct_get_i32 = Signature::new(cc);
        sig_struct_get_i32.params.push(AbiParam::new(types::I64));
        sig_struct_get_i32.params.push(AbiParam::new(types::I64));
        sig_struct_get_i32.returns.push(AbiParam::new(types::I32));
        let mut sig_struct_field_address = Signature::new(cc);
        sig_struct_field_address
            .params
            .extend([AbiParam::new(types::I64); 3]);
        sig_struct_field_address.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_set_i64 = Signature::new(cc);
        sig_struct_set_i64.params.push(AbiParam::new(types::I64));
        sig_struct_set_i64.params.push(AbiParam::new(types::I64));
        sig_struct_set_i64.params.push(AbiParam::new(types::I64));
        let sig_struct_set_record = sig_struct_set_i64.clone();
        let mut sig_receipt_attach = Signature::new(cc);
        sig_receipt_attach
            .params
            .extend([AbiParam::new(types::I64); 4]);
        let mut sig_callable_bind_raw = Signature::new(cc);
        sig_callable_bind_raw
            .params
            .extend([AbiParam::new(types::I64); 3]);
        sig_callable_bind_raw.returns.push(AbiParam::new(types::I8));
        let mut sig_callable_bind_raw_many = Signature::new(cc);
        sig_callable_bind_raw_many
            .params
            .extend([AbiParam::new(types::I64); 2]);
        sig_callable_bind_raw_many.returns.push(AbiParam::new(types::I8));
        let mut sig_struct_set_f64 = Signature::new(cc);
        sig_struct_set_f64.params.push(AbiParam::new(types::I64));
        sig_struct_set_f64.params.push(AbiParam::new(types::I64));
        sig_struct_set_f64.params.push(AbiParam::new(types::F64));
        let mut sig_struct_set_i8 = Signature::new(cc);
        sig_struct_set_i8.params.push(AbiParam::new(types::I64));
        sig_struct_set_i8.params.push(AbiParam::new(types::I64));
        sig_struct_set_i8.params.push(AbiParam::new(types::I8));
        let mut sig_struct_set_i32 = Signature::new(cc);
        sig_struct_set_i32.params.push(AbiParam::new(types::I64));
        sig_struct_set_i32.params.push(AbiParam::new(types::I64));
        sig_struct_set_i32.params.push(AbiParam::new(types::I32));
        let mut sig_memo_probe = Signature::new(cc);
        sig_memo_probe.params.push(AbiParam::new(types::I64));
        sig_memo_probe.params.push(AbiParam::new(types::I64));
        sig_memo_probe.returns.push(AbiParam::new(types::I8));
        let mut sig_i64_i64 = Signature::new(cc);
        sig_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64.params.push(AbiParam::new(types::I64));
        let mut sig_persist_read_f64 = Signature::new(cc);
        sig_persist_read_f64.params.push(AbiParam::new(types::I64));
        sig_persist_read_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_persist_write_f64 = Signature::new(cc);
        sig_persist_write_f64.params.push(AbiParam::new(types::I64));
        sig_persist_write_f64.params.push(AbiParam::new(types::F64));
        let mut sig_persist_write_i8 = Signature::new(cc);
        sig_persist_write_i8.params.push(AbiParam::new(types::I64));
        sig_persist_write_i8.params.push(AbiParam::new(types::I8));
        let mut sig_persist_read_i32 = Signature::new(cc);
        sig_persist_read_i32.params.push(AbiParam::new(types::I64));
        sig_persist_read_i32.returns.push(AbiParam::new(types::I32));
        let mut sig_persist_write_i32 = Signature::new(cc);
        sig_persist_write_i32.params.push(AbiParam::new(types::I64));
        sig_persist_write_i32.params.push(AbiParam::new(types::I32));
        let mut sig_measurement_new = Signature::new(cc);
        sig_measurement_new.params.push(AbiParam::new(types::F64));
        sig_measurement_new.params.push(AbiParam::new(types::F64));
        sig_measurement_new.returns.push(AbiParam::new(types::I64));
        let mut sig_measurement_arithmetic = Signature::new(cc);
        sig_measurement_arithmetic.params.push(AbiParam::new(types::I64));
        sig_measurement_arithmetic.params.push(AbiParam::new(types::I64));
        sig_measurement_arithmetic.params.push(AbiParam::new(types::I64));
        sig_measurement_arithmetic.returns.push(AbiParam::new(types::I64));
        let mut sig_measurement_get = Signature::new(cc);
        sig_measurement_get.params.push(AbiParam::new(types::I64));
        sig_measurement_get.params.push(AbiParam::new(types::I64));
        sig_measurement_get.returns.push(AbiParam::new(types::F64));
        let mut sig_measurement_value = Signature::new(cc);
        sig_measurement_value.params.push(AbiParam::new(types::I64));
        sig_measurement_value.returns.push(AbiParam::new(types::F64));
        let mut sig_measurement_unary = Signature::new(cc);
        sig_measurement_unary.params.push(AbiParam::new(types::I64));
        sig_measurement_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_measurement_binary = Signature::new(cc);
        sig_measurement_binary
            .params
            .extend([AbiParam::new(types::I64); 2]);
        sig_measurement_binary.returns.push(AbiParam::new(types::I64));

        let mut sig_is_trapped = Signature::new(cc);
        sig_is_trapped.returns.push(AbiParam::new(types::I64));
        let mut sig_numeric_checked_widen = Signature::new(cc);
        sig_numeric_checked_widen
            .params
            .extend([AbiParam::new(types::I64); 5]);
        sig_numeric_checked_widen
            .returns
            .push(AbiParam::new(types::F64));
        let mut sig_numeric_int_checked_widen = Signature::new(cc);
        sig_numeric_int_checked_widen
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_numeric_int_checked_widen
            .returns
            .push(AbiParam::new(types::F64));
        let mut sig_numeric_checked_int = Signature::new(cc);
        sig_numeric_checked_int.params.extend([AbiParam::new(types::I64); 4]);
        sig_numeric_checked_int.returns.push(AbiParam::new(types::I64));
        let mut sig_result_new_i64 = Signature::new(cc);
        sig_result_new_i64.params.push(AbiParam::new(types::I8));
        sig_result_new_i64.params.push(AbiParam::new(types::I64));
        sig_result_new_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_result_new_f64 = Signature::new(cc);
        sig_result_new_f64.params.push(AbiParam::new(types::I8));
        sig_result_new_f64.params.push(AbiParam::new(types::F64));
        sig_result_new_f64.returns.push(AbiParam::new(types::I64));
        let mut sig_option_lift2 = Signature::new(cc);
        sig_option_lift2.params.push(AbiParam::new(types::I8));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I8));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.returns.push(AbiParam::new(types::I64));
        let mut sig_callable_bind = Signature::new(cc);
        sig_callable_bind.params.push(AbiParam::new(types::I64));
        sig_callable_bind.params.push(AbiParam::new(types::I64));
        sig_callable_bind.params.push(AbiParam::new(types::I8));
        sig_callable_bind.returns.push(AbiParam::new(types::I64));
        let mut sig_callable_bind_history_capture_mode = Signature::new(cc);
        sig_callable_bind_history_capture_mode
            .params
            .push(AbiParam::new(types::I64));
        sig_callable_bind_history_capture_mode
            .params
            .push(AbiParam::new(types::I8));
        sig_callable_bind_history_capture_mode
            .returns
            .push(AbiParam::new(types::I8));
        let mut sig_callable_word = Signature::new(cc);
        sig_callable_word.params.push(AbiParam::new(types::I64));
        sig_callable_word.returns.push(AbiParam::new(types::I64));
        let mut sig_callable_flag = Signature::new(cc);
        sig_callable_flag.params.push(AbiParam::new(types::I64));
        sig_callable_flag.returns.push(AbiParam::new(types::I8));
        let mut sig_result_new_i8 = Signature::new(cc);
        sig_result_new_i8.params.push(AbiParam::new(types::I8));
        sig_result_new_i8.params.push(AbiParam::new(types::I8));
        sig_result_new_i8.returns.push(AbiParam::new(types::I64));
        let mut sig_result_new_i32 = Signature::new(cc);
        sig_result_new_i32.params.push(AbiParam::new(types::I8));
        sig_result_new_i32.params.push(AbiParam::new(types::I32));
        sig_result_new_i32.returns.push(AbiParam::new(types::I64));
        let mut sig_unit_convert_exact = Signature::new(cc);
        sig_unit_convert_exact.params.push(AbiParam::new(types::F64));
        sig_unit_convert_exact.params.extend([AbiParam::new(types::I64); 4]);
        sig_unit_convert_exact.returns.push(AbiParam::new(types::I64));
        let mut sig_unit_convert_rounded = Signature::new(cc);
        sig_unit_convert_rounded.params.push(AbiParam::new(types::F64));
        sig_unit_convert_rounded.params.extend([AbiParam::new(types::I64); 6]);
        sig_unit_convert_rounded.returns.push(AbiParam::new(types::I64));
        let mut sig_unit_convert_exact_measurement = Signature::new(cc);
        sig_unit_convert_exact_measurement
            .params
            .push(AbiParam::new(types::F64));
        sig_unit_convert_exact_measurement
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_unit_convert_exact_measurement
            .params
            .push(AbiParam::new(types::F64));
        sig_unit_convert_exact_measurement
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_unit_convert_rounded_measurement = Signature::new(cc);
        sig_unit_convert_rounded_measurement
            .params
            .push(AbiParam::new(types::F64));
        sig_unit_convert_rounded_measurement
            .params
            .extend([AbiParam::new(types::I64); 6]);
        sig_unit_convert_rounded_measurement
            .params
            .push(AbiParam::new(types::F64));
        sig_unit_convert_rounded_measurement
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_unit_convert_implicit = Signature::new(cc);
        sig_unit_convert_implicit.params.push(AbiParam::new(types::F64));
        sig_unit_convert_implicit.params.extend([AbiParam::new(types::I64); 4]);
        sig_unit_convert_implicit.returns.push(AbiParam::new(types::F64));
        let mut sig_result_query_i8 = Signature::new(cc);
        sig_result_query_i8.params.push(AbiParam::new(types::I64));
        sig_result_query_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_result_query_i64 = Signature::new(cc);
        sig_result_query_i64.params.push(AbiParam::new(types::I64));
        sig_result_query_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_result_query_f64 = Signature::new(cc);
        sig_result_query_f64.params.push(AbiParam::new(types::I64));
        sig_result_query_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_result_query_i32 = Signature::new(cc);
        sig_result_query_i32.params.push(AbiParam::new(types::I64));
        sig_result_query_i32.returns.push(AbiParam::new(types::I32));
        let mut sig_duration_float = Signature::new(cc);
        sig_duration_float.params.push(AbiParam::new(types::F64));
        sig_duration_float.params.push(AbiParam::new(types::I64));
        sig_duration_float.returns.push(AbiParam::new(types::I64));
        let mut sig_duration_int = Signature::new(cc);
        sig_duration_int.params.push(AbiParam::new(types::I64));
        sig_duration_int.params.push(AbiParam::new(types::I64));
        sig_duration_int.returns.push(AbiParam::new(types::I64));
        let mut sig_duration_total_in = Signature::new(cc);
        sig_duration_total_in.params.push(AbiParam::new(types::I64));
        sig_duration_total_in.params.push(AbiParam::new(types::I64));
        sig_duration_total_in.returns.push(AbiParam::new(types::F64));
        let mut sig_duration_round = Signature::new(cc);
        sig_duration_round
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_duration_round.returns.push(AbiParam::new(types::I64));
        let mut sig_noarg_f64 = Signature::new(cc);
        sig_noarg_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_perf_override = Signature::new(cc);
        sig_perf_override.params.push(AbiParam::new(types::F64));
        sig_perf_override.returns.push(AbiParam::new(types::I64));
        let mut sig_service_call = Signature::new(cc);
        sig_service_call
            .params
            .extend([AbiParam::new(types::I64); 10]);
        sig_service_call.returns.push(AbiParam::new(types::I64));
        let mut sig_service_call_bool = Signature::new(cc);
        sig_service_call_bool
            .params
            .extend([AbiParam::new(types::I64); 10]);
        sig_service_call_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_deopt = Signature::new(cc);
        // fn_idx, argc, a0..a7
        for _ in 0..10 {
            sig_deopt.params.push(AbiParam::new(types::I64));
        }
        sig_deopt.returns.push(AbiParam::new(types::I64));
        let sig_noarg = Signature::new(cc);
        let mut sig_noarg_i64 = Signature::new(cc);
        sig_noarg_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_hardware_setup = Signature::new(cc);
        sig_hardware_setup
            .params
            .extend([AbiParam::new(types::I64); 5]);
        sig_hardware_setup.returns.push(AbiParam::new(types::I64));
        let mut sig_hardware_register_read = Signature::new(cc);
        sig_hardware_register_read
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_hardware_register_read
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_hardware_register_write = Signature::new(cc);
        sig_hardware_register_write
            .params
            .extend([AbiParam::new(types::I64); 5]);
        sig_hardware_register_write
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_hardware_dma_start = Signature::new(cc);
        sig_hardware_dma_start
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_hardware_dma_start
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_hardware_dma_wait = Signature::new(cc);
        sig_hardware_dma_wait
            .params
            .extend([AbiParam::new(types::I64); 3]);
        sig_hardware_dma_wait
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_hardware_interrupt_poll = Signature::new(cc);
        sig_hardware_interrupt_poll
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_hardware_replay_interrupt = Signature::new(cc);
        sig_hardware_replay_interrupt
            .params
            .push(AbiParam::new(types::I64));
        sig_hardware_replay_interrupt
            .returns
            .push(AbiParam::new(types::I64));
    }
    #extra {
        coll: Collections::CollectionsHostFns,
        compute: Compute::ComputeHostFns,
        memory: Memory::MemoryHostFns,
        cell: LocalCell::CellHostFns,
        conc: Concurrency::ConcurrencyHostFns,
        core: CoreHost::CoreHostFns,
        encoding: Encoding::EncodingHostFns,
        stream: crate::enc_stream::StreamHostFns,
        fmt: Fmt::FmtHostFns,
        compress: Compress::CompressHostFns,
        archive: Archive::ArchiveHostFns,
        process: Process::ProcessHostFns,
        num: Numeric::NumericHostFns,
        solver: Solver::SolverHostFns,
        random: Random::RandomHostFns,
        text: crate::Text::TextHostFns,
        sketch: crate::Sketch::SketchHostFns,
        args: crate::Args::ArgsHostFns,
        db: crate::DB::DBHostFns,
        crypto: Crypto::CryptoHostFns,
        net: Net::NetHostFns,
        net_http: crate::net_http_rt::NetHttpHostFns,
        game: crate::Game::GameHostFns,
        plugin: crate::Plugin::PluginHostFns,
        raylib: crate::Raylib::RaylibHostFns,
        layout: crate::Layout::LayoutHostFns,
        reactive: crate::Reactive::ReactiveHostFns,
        ui: crate::Ui::UiHostFns,
        web: crate::Web::WebHostFns,
        parse: crate::Parse::HostFns,
        data: crate::Data::DataHostFns,
        time: crate::Time::TimeHostFns,
        io: crate::IO::IOHostFns,
        watcher: crate::Watcher::WatcherHostFns,
        math: crate::Math::MathHostFns,
        math_extra: crate::MathExtra::MathExtraHostFns,
        ffi: crate::Ffi::FfiHostFns,
    }
    add_i64: "jet_jit_add_i64" => jet_jit_add_i64: sig_bin_i64;
    sub_i64: "jet_jit_sub_i64" => jet_jit_sub_i64: sig_bin_i64;
    mul_i64: "jet_jit_mul_i64" => jet_jit_mul_i64: sig_bin_i64;
    div_i64: "jet_jit_div_i64" => jet_jit_div_i64: sig_bin_i64;
    rem_i64: "jet_jit_rem_i64" => jet_jit_rem_i64: sig_bin_i64;
    pow_i64: "jet_jit_pow_i64" => jet_jit_pow_i64: sig_bin_i64;
    floordiv_i64: "jet_jit_floordiv_i64" => jet_jit_floordiv_i64: sig_bin_i64;
    mod_i64: "jet_jit_mod_i64" => jet_jit_mod_i64: sig_bin_i64;
    floordiv_f64: "jet_jit_floordiv_f64" => jet_jit_floordiv_f64: sig_pow_f64;
    pow_f64: "jet_jit_pow_f64" => jet_jit_pow_f64: sig_pow_f64;
    intn_binop: "jet_jit_intn_binop" => jet_jit_intn_binop: sig_intn_binop;
    intn_to_string: "jet_jit_intn_to_string" => jet_jit_intn_to_string: sig_i64_i64_i64;
    model_open: "jet_jit_model_open" => jet_jit_model_open: sig_model_open;
    model_embed: "jet_jit_model_embed" => jet_jit_model_embed: sig_model_embed;
    print_i64: "jet_jit_print_i64" => jet_jit_print_i64: sig_i64;
    print_f64: "jet_jit_print_f64" => jet_jit_print_f64: sig_f64;
    print_bool: "jet_jit_print_bool" => jet_jit_print_bool: sig_i8;
    print_char: "jet_jit_print_char" => jet_jit_print_char: sig_i32;
    print_str: "jet_jit_print_str" => jet_jit_print_str: sig_i64;
    term_write_stdout_line: "jet_term_write_stdout_line" => jet_jit_term_write_stdout_line: sig_i64_i64;
    persist_read_i64: "jet_jit_persist_read_i64" => jet_jit_persist_read_i64: sig_str_unary_i64;
    persist_write_i64: "jet_jit_persist_write_i64" => jet_jit_persist_write_i64: sig_i64_i64;
    persist_read_f64: "jet_jit_persist_read_f64" => jet_jit_persist_read_f64: sig_persist_read_f64;
    persist_write_f64: "jet_jit_persist_write_f64" => jet_jit_persist_write_f64: sig_persist_write_f64;
    persist_read_bool: "jet_jit_persist_read_bool" => jet_jit_persist_read_bool: sig_str_unary_i8;
    persist_write_bool: "jet_jit_persist_write_bool" => jet_jit_persist_write_bool: sig_persist_write_i8;
    jit_dma_start_marshaled: "jet_jit_dma_start_marshaled" => jet_jit_dma_start_marshaled: sig_hardware_dma_start;
    jit_dma_wait_marshaled: "jet_jit_dma_wait_marshaled" => jet_jit_dma_wait_marshaled: sig_hardware_dma_wait;
    persist_read_char: "jet_jit_persist_read_char" => jet_jit_persist_read_char: sig_persist_read_i32;
    persist_write_char: "jet_jit_persist_write_char" => jet_jit_persist_write_char: sig_persist_write_i32;
    persist_read_str: "jet_jit_persist_read_str" => jet_jit_persist_read_str: sig_str_unary_i64;
    persist_write_str: "jet_jit_persist_write_str" => jet_jit_persist_write_str: sig_i64_i64;
    persist_runtime_set: "jet_persist_runtime_set" => jet_persist_runtime_set: sig_i64_i64_i64_i64;
    persist_runtime_get: "jet_persist_runtime_get" => jet_persist_runtime_get: sig_i64_i64_i64;
    observe_live_value_update: "jet_jit_observe_live_value_update" => jet_jit_observe_live_value_update: sig_observe_live_value_update;
    eprint_str: "jet_jit_eprint_str" => jet_jit_eprint_str: sig_i64;
    str_begin: "jet_jit_str_begin" => jet_jit_str_begin: sig_str_begin;
    display_nominal: "jet_jit_display_nominal" => jet_jit_display_nominal: sig_str_binary_i64;
    debug_nominal: "jet_jit_debug_nominal" => jet_jit_debug_nominal: sig_str_binary_i64;
    str_push_lit: "jet_jit_str_push_lit" => jet_jit_str_push_lit: sig_str_push_lit;
    str_push_i64: "jet_jit_str_push_i64" => jet_jit_str_push_i64: sig_str_push_i64;
    str_push_f64: "jet_jit_str_push_f64" => jet_jit_str_push_f64: sig_str_push_f64;
    str_push_compact_f64: "jet_jit_str_push_compact_f64" => jet_jit_str_push_compact_f64: sig_str_push_f64;
    str_push_bool: "jet_jit_str_push_bool" => jet_jit_str_push_bool: sig_str_push_bool;
    str_push_char: "jet_jit_str_push_char" => jet_jit_str_push_char: sig_str_push_char;
    str_push_str: "jet_jit_str_push_str" => jet_jit_str_push_str: sig_str_push_lit;
    str_eq: "jet_jit_str_eq" => jet_jit_str_eq: sig_str_eq;
    clock_clone: "jet_jit_clock_clone" => jet_jit_clock_clone: sig_clock_clone;
    typed_clone: "jet_jit_typed_clone" => jet_jit_typed_clone: sig_typed_clone;
    typed_eq: "jet_jit_typed_eq" => jet_jit_typed_eq: sig_typed_eq;
    str_order: "jet_jit_str_order" => jet_jit_str_order: sig_str_binary_i64;
    pattern_text_match: "jet_text_pattern_match" => jet_jit_pattern_text_match: sig_i64_i64_i64;
    checked_string_starts_with: "jet_string_starts_with" => jet_jit_str_starts_with: sig_str_eq;
    string_repeat: "jet_string_repeat" => jet_jit_str_repeat: sig_i64_i64_i64;

    pattern_binary_match: "jet_binary_pattern_match" => jet_jit_pattern_binary_match: sig_i64_i64_i64;
    str_contains: "jet_jit_str_contains" => jet_jit_str_contains: sig_str_eq;
    checked_string_contains: "jet_string_contains" => jet_jit_str_contains: sig_str_eq;
    checked_char_len: "jet_char_len" => jet_jit_str_len: sig_str_unary_i64;
    checked_string_count_bytes: "jet_string_count_bytes" => jet_jit_str_byte_len: sig_str_unary_i64;
    checked_string_is_empty: "jet_string_is_empty" => jet_jit_str_is_empty: sig_str_unary_i8;
    str_starts_with: "jet_jit_str_starts_with" => jet_jit_str_starts_with: sig_str_eq;
    str_ends_with: "jet_jit_str_ends_with" => jet_jit_str_ends_with: sig_str_eq;
    str_clone: "jet_jit_str_clone" => jet_jit_str_clone: sig_str_unary_i64;
    str_len: "jet_jit_str_len" => jet_jit_str_len: sig_str_unary_i64;
    str_byte_len: "jet_jit_str_byte_len" => jet_jit_str_byte_len: sig_str_unary_i64;
    str_is_ascii: "jet_jit_str_is_ascii" => jet_jit_str_is_ascii: sig_str_unary_i8;
    str_trim: "jet_jit_str_trim" => jet_jit_str_trim: sig_str_unary_i64;
    str_to_upper: "jet_jit_str_to_upper" => jet_jit_str_to_upper: sig_str_unary_i64;
    str_to_lower: "jet_jit_str_to_lower" => jet_jit_str_to_lower: sig_str_unary_i64;
    str_to_ascii_upper: "jet_jit_str_to_ascii_upper" => jet_jit_str_to_ascii_upper: sig_str_unary_i64;
    str_to_ascii_lower: "jet_jit_str_to_ascii_lower" => jet_jit_str_to_ascii_lower: sig_str_unary_i64;
    str_replace: "jet_jit_str_replace" => jet_jit_str_replace: sig_str_replace;
    str_lines: "jet_jit_str_lines" => jet_jit_str_lines: sig_str_unary_i64;
    str_split: "jet_jit_str_split" => jet_jit_str_split: sig_str_binary_i64;
    str_rsplit: "jet_jit_str_rsplit" => jet_jit_str_rsplit: sig_str_binary_i64;
    str_chars: "jet_jit_str_chars" => jet_jit_str_chars: sig_str_unary_i64;
    str_bytes: "jet_string_bytes" => jet_jit_str_bytes: sig_str_unary_i64;
    str_from_bytes: "jet_jit_str_from_bytes" => jet_jit_str_from_bytes: sig_str_unary_i64;
    str_from_bytes_lossy: "jet_jit_str_from_bytes_lossy" => jet_jit_str_from_bytes_lossy: sig_str_unary_i64;
    str_scalar_strings: "jet_jit_str_scalar_strings" => jet_jit_str_scalar_strings: sig_str_unary_i64;
    str_after: "jet_jit_str_after" => jet_jit_str_after: sig_str_binary_i64;
    str_before: "jet_jit_str_before" => jet_jit_str_before: sig_str_binary_i64;
    str_trim_view: "jet_jit_str_trim_view" => jet_jit_str_trim_view: sig_str_unary_i64;
    str_after_view: "jet_jit_str_after_view" => jet_jit_str_after_view: sig_str_binary_i64;
    str_before_view: "jet_jit_str_before_view" => jet_jit_str_before_view: sig_str_binary_i64;
    str_slice: "jet_jit_str_slice" => jet_jit_str_slice: sig_str_replace;
    checked_string_slice_builtin: "jet_string_slice_builtin" => jet_jit_str_slice: sig_str_replace;
    str_slice_range: "jet_jit_str_slice_range" => jet_jit_str_slice_range: sig_str_slice_range;
    checked_list_join: "jet_list_join" => jet_jit_list_join: sig_i64_i64_i64_i64;
    checked_unicode_trim_view: "jet_unicode_trim_view" => jet_jit_str_trim_view: sig_str_unary_i64;
    checked_string_slice: "jet_string_slice" => jet_jit_str_slice_direct: sig_slice_vec;
    checked_string_after: "jet_string_after" => jet_jit_str_after: sig_str_binary_i64;
    checked_string_before: "jet_string_before" => jet_jit_str_before: sig_str_binary_i64;
    parse_float: "jet_std::jet_float_parse" => jet_jit_parse_f64: sig_str_unary_i64;
    checked_string_after_view: "jet_string_after_view" => jet_jit_str_after_view: sig_str_binary_i64;
    checked_string_before_view: "jet_string_before_view" => jet_jit_str_before_view: sig_str_binary_i64;
    checked_string_from_bytes: "jet_string_from_bytes" => jet_jit_str_from_bytes: sig_str_unary_i64;
    checked_string_from_bytes_lossy: "jet_string_from_bytes_lossy" => jet_jit_str_from_bytes_lossy: sig_str_unary_i64;
    checked_slice_range: "jet_slice_range" => jet_jit_slice_range_value: sig_slice_range;
    checked_slice_vec_range: "jet_slice_vec_range" => crate::Collections::jet_jit_slice_vec_range: sig_slice_range;
    checked_slice_vec: "jet_slice_vec" => crate::Collections::jet_jit_slice_vec: sig_slice_vec;
    parse_i64: "jet_std::jet_int_parse" => jet_jit_parse_i64: sig_str_unary_i64;
    parse_f64: "jet_jit_parse_f64" => jet_jit_parse_f64: sig_str_unary_i64;
    numeric_try_i64: "jet_numeric_try_from_fixed" => jet_jit_numeric_try_i64: sig_i64_i64_i64_i64;
    numeric_try_int: "jet_std::jet_int_try_from_checked" => jet_jit_numeric_try_int: sig_i64_i64_i64;
    numeric_checked_int: "jet_std::jet_int_checked_fixed" => jet_jit_numeric_checked_int: sig_numeric_checked_int;
    numeric_float_to_int: "jet_numeric_float_to_int" => jet_jit_numeric_float_to_int: sig_f64_i64_i64;
    numeric_float_narrow: "jet_numeric_float_narrow" => jet_jit_numeric_float_narrow: sig_f64_i64;
    numeric_checked_widen: "jet_numeric_checked_widen_at" => jet_jit_numeric_checked_widen: sig_numeric_checked_widen;
    numeric_int_checked_widen: "jet_std::jet_int_checked_widen" => jet_jit_numeric_int_checked_widen: sig_numeric_int_checked_widen;
    distinct_range: "jet_jit_distinct_range" => jet_jit_distinct_range: sig_i64_i64_i64_i64;
    distinct_range_result: "jet_jit_distinct_range_result" => jet_jit_distinct_range_result: sig_i64_i64_i64_i64;
    inline_range: "jet_jit_inline_range" => jet_jit_inline_range: sig_i64_i64_i64_i64;
    inline_range_result: "jet_inline_range_from_int" => jet_jit_inline_range_result: sig_i64_i64_i64_i64;
    numeric_bit_count: "jet_numeric_bit_count" => jet_jit_numeric_bit_count: sig_i64_i64_i64_i64;
    numeric_int_bit_count: "jet_numeric_int_bit_count" => jet_jit_numeric_int_bit_count: sig_i64_i64_i64_i64;
    struct_new: "jet_jit_struct_new" => jet_jit_struct_new: sig_struct_new;
    trait_object_tag: "jet_jit_trait_object_tag" => jet_jit_trait_object_tag: sig_trait_object_tag;
    trait_object_type: "jet_jit_trait_object_type" => jet_jit_trait_object_type: sig_trait_object_type;
    struct_assign: "jet_jit_struct_assign" => jet_jit_struct_assign: sig_struct_assign;
    struct_get_i64: "jet_jit_struct_get_i64" => jet_jit_struct_get_i64: sig_struct_get_i64;
    struct_get_f64: "jet_jit_struct_get_f64" => jet_jit_struct_get_f64: sig_struct_get_f64;
    struct_get_bool: "jet_jit_struct_get_bool" => jet_jit_struct_get_bool: sig_struct_get_i8;
    struct_get_char: "jet_jit_struct_get_char" => jet_jit_struct_get_char: sig_struct_get_i32;
    struct_field_address: "jet_jit_struct_field_address" => jet_jit_struct_field_address: sig_struct_field_address;
    pattern_capture_char: "jet_jit_pattern_capture_char" => jet_jit_pattern_capture_char: sig_struct_get_i32;
    struct_get_str: "jet_jit_struct_get_str" => jet_jit_struct_get_str: sig_struct_get_i64;
    struct_set_i64: "jet_jit_struct_set_i64" => jet_jit_struct_set_i64: sig_struct_set_i64;
    struct_set_record: "jet_jit_struct_set_record" => jet_jit_struct_set_record: sig_struct_set_record;
    struct_set_f64: "jet_jit_struct_set_f64" => jet_jit_struct_set_f64: sig_struct_set_f64;
    struct_set_bool: "jet_jit_struct_set_bool" => jet_jit_struct_set_bool: sig_struct_set_i8;
    struct_set_char: "jet_jit_struct_set_char" => jet_jit_struct_set_char: sig_struct_set_i32;
    struct_set_str: "jet_jit_struct_set_str" => jet_jit_struct_set_str: sig_struct_set_i64;
    hardware_interrupt_poll: "jet_hardware_interrupt_poll" => jet_hardware_interrupt_poll: sig_hardware_interrupt_poll;
    hardware_replay_interrupt: "jet_hardware_replay_interrupt" => jet_hardware_replay_interrupt: sig_hardware_replay_interrupt;
    hardware_setup: "jet_hardware_setup" => jet_hardware_setup: sig_hardware_setup;
    hardware_register_read: "jet_hardware_register_read" => jet_hardware_register_read: sig_hardware_register_read;
    hardware_register_write: "jet_hardware_register_write" => jet_hardware_register_write: sig_hardware_register_write;
    hardware_dma_start: "jet_hardware_dma_start" => jet_hardware_dma_start: sig_hardware_dma_start;
    hardware_dma_wait: "jet_hardware_dma_wait" => jet_hardware_dma_wait: sig_hardware_dma_wait;
    receipt_attach: "jet_receipt_attach" => jet_jit_receipt_attach: sig_receipt_attach;
    memo_probe: "jet_jit_memo_probe" => jet_jit_memo_probe: sig_memo_probe;
    memo_get: "jet_jit_memo_get" => jet_jit_memo_get: sig_struct_get_i64;
    memo_put: "jet_jit_memo_put" => jet_jit_memo_put: sig_struct_set_i64;
    memo_clear: "jet_jit_memo_clear" => jet_jit_memo_clear: sig_i64;
    memo_clear_slot: "jet_jit_memo_clear_slot" => jet_jit_memo_clear_slot: sig_i64_i64;
    memo_stats: "jet_jit_memo_stats" => jet_jit_memo_stats: sig_struct_get_i64;
    err_new: "jet_jit_err_new" => jet_jit_err_new: sig_i64_i64_i64_i64;
    err_from_message: "jet_err_from_message" => jet_jit_err_from_message: sig_str_unary_i64;
    err_with_context_frame: "jet_err_with_context_frame" => jet_jit_err_with_context_frame: sig_err_with_context_frame;
    entry_error_exit: "jet_entry_error_exit_jet" => jet_jit_entry_error_exit: sig_i64;
    err_apply_conversion: "jet_jit_err_apply_conversion" => jet_jit_err_apply_conversion: sig_err_apply_conversion;
    err_add_context: "jet_jit_err_add_context" => jet_jit_err_add_context: sig_err_add_context;
    err_message: "jet_jit_err_message" => jet_jit_err_message: sig_str_unary_i64;
    err_code: "jet_jit_err_code" => jet_jit_err_code: sig_str_unary_i64;
    err_cause: "jet_jit_err_cause" => jet_jit_err_cause: sig_str_unary_i64;
    measurement_new: "jet_jit_measurement_new" => jet_jit_measurement_new: sig_measurement_new;
    measurement_new_aot: "jet_std::JetMeasurement::new" => jet_jit_measurement_new: sig_measurement_new;
    measurement_arithmetic: "jet_jit_measurement_arithmetic" => jet_jit_measurement_arithmetic: sig_measurement_arithmetic;
    measurement_get: "jet_jit_measurement_get" => jet_jit_measurement_get: sig_measurement_get;
    measurement_show: "jet_jit_measurement_show" => jet_jit_measurement_show: sig_str_unary_i64;
    measurement_value: "jet_std::JetMeasurement::value" => jet_jit_measurement_value: sig_measurement_value;
    measurement_uncertainty: "jet_std::JetMeasurement::uncertainty" => jet_jit_measurement_uncertainty: sig_measurement_value;
    measurement_add: "jet_std::JetMeasurement::add" => jet_jit_measurement_add: sig_measurement_binary;
    measurement_sub: "jet_std::JetMeasurement::sub" => jet_jit_measurement_sub: sig_measurement_binary;
    measurement_mul: "jet_std::JetMeasurement::mul" => jet_jit_measurement_mul: sig_measurement_binary;
    measurement_div: "jet_std::JetMeasurement::div" => jet_jit_measurement_div: sig_measurement_binary;
    measurement_sqrt: "jet_std::JetMeasurement::sqrt" => jet_jit_measurement_sqrt: sig_measurement_unary;
    result_new_i64: "jet_jit_result_new_i64" => jet_jit_result_new_i64: sig_result_new_i64;
    result_new_f64: "jet_jit_result_new_f64" => jet_jit_result_new_f64: sig_result_new_f64;
    result_new_i8: "jet_jit_result_new_i8" => jet_jit_result_new_i8: sig_result_new_i8;
    result_new_i32: "jet_jit_result_new_i32" => jet_jit_result_new_i32: sig_result_new_i32;
    option_lift2: "jet_jit_option_lift2" => jet_jit_option_lift2: sig_option_lift2;
    callable_bind: "jet_jit_callable_bind" => jet_jit_callable_bind: sig_callable_bind;
    callable_bind_history_capture_mode: "jet_jit_callable_bind_history_capture_mode" => jet_jit_callable_bind_history_capture_mode: sig_callable_bind_history_capture_mode;
    callable_normalize: "jet_jit_callable_normalize" => jet_jit_callable_normalize: sig_callable_word;
    callable_bind_raw: "jet_jit_callable_bind_raw" => jet_jit_callable_bind_raw: sig_callable_bind_raw;
    callable_bind_raw_many: "jet_jit_callable_bind_raw_many" => jet_jit_callable_bind_raw_many: sig_callable_bind_raw_many;
    ffi_callback_boundary: "jet_ffi_callback_boundary" => jet_jit_ffi_callback_boundary: sig_callable_word;
    callable_fn: "jet_jit_callable_fn" => jet_jit_callable_fn: sig_callable_word;
    callable_env: "jet_jit_callable_env" => jet_jit_callable_env: sig_callable_word;
    callable_has_env: "jet_jit_callable_has_env" => jet_jit_callable_has_env: sig_callable_flag;
    unit_convert_exact: "jet_unit_conversion_exact" => jet_jit_unit_convert_exact: sig_unit_convert_exact;
    unit_convert_rounded: "jet_unit_conversion_rounded" => jet_jit_unit_convert_rounded: sig_unit_convert_rounded;
    unit_convert_exact_measurement: "jet_std::jet_unit_conversion_exact_measurement" => jet_jit_unit_convert_exact_measurement: sig_unit_convert_exact_measurement;
    unit_convert_rounded_measurement: "jet_std::jet_unit_conversion_rounded_measurement" => jet_jit_unit_convert_rounded_measurement: sig_unit_convert_rounded_measurement;
    unit_convert_implicit: "jet_jit_unit_convert_implicit" => jet_jit_unit_convert_implicit: sig_unit_convert_implicit;
    result_is_ok: "jet_jit_result_is_ok" => jet_jit_result_is_ok: sig_result_query_i8;
    result_get_i64: "jet_jit_result_get_i64" => jet_jit_result_get_i64: sig_result_query_i64;
    result_get_f64: "jet_jit_result_get_f64" => jet_jit_result_get_f64: sig_result_query_f64;
    result_get_i8: "jet_jit_result_get_i8" => jet_jit_result_get_i8: sig_result_query_i8;
    result_get_i32: "jet_jit_result_get_i32" => jet_jit_result_get_i32: sig_result_query_i32;
    require: "jet_require" => jet_jit_require: sig_require;
    panic_rich: "jet_panic_rich" => jet_jit_rich_panic: sig_rich_panic;
    test_require: "jet_test_require" => jet_jit_test_require: sig_test_require;
    require_eq: "jet_require_eq" => jet_jit_require_eq: sig_require_eq;
    test_require_eq: "jet_test_require_eq" => jet_jit_test_require_eq: sig_test_require_eq;
    debug_i64: "jet_jit_debug_i64" => jet_jit_debug_i64: sig_debug_i64;
    debug_f64: "jet_jit_debug_f64" => jet_jit_debug_f64: sig_debug_f64;
    debug_f32: "jet_jit_debug_f32" => jet_jit_debug_f32: sig_debug_f32;
    debug_bool: "jet_jit_debug_bool" => jet_jit_debug_bool: sig_debug_bool;
    debug_char: "jet_jit_debug_char" => jet_jit_debug_char: sig_debug_char;
    debug_string: "jet_jit_debug_string" => jet_jit_debug_string: sig_debug_string;
    debug_local_append: "jet_jit_debug_local_append" => jet_jit_debug_local_append: sig_debug_local_append;
    trap_panic: "jet_jit_trap_panic" => jet_jit_trap_panic: sig_i64;
    todo_stop: "jet_jit_todo_stop" => jet_jit_todo_stop: sig_todo_stop;
    contract_check: "jet_jit_contract_check" => jet_jit_contract_check: sig_contract_check;
    contract_fail: "jet_jit_contract_fail" => jet_jit_contract_fail: sig_contract_fail;
    trace_err: "jet_jit_trace_err" => jet_jit_trace_err: sig_trace_err;
    trace_err_note: "jet_journey_frame_text" => jet_jit_trace_err_note: sig_trace_err_note;
    trace_reset: "jet_journey_reset" => jet_jit_trace_reset: sig_trace_reset;
    duration_from_int: "jet_jit_duration_from_int" => jet_jit_duration_from_int: sig_duration_int;
    duration_from_float: "jet_jit_duration_from_float" => jet_jit_duration_from_float: sig_duration_float;
    duration_from_int_exact: "jet_duration_from_int" => jet_jit_duration_from_int_unit: sig_duration_int;
    duration_from_float_exact: "jet_duration_from_float" => jet_jit_duration_from_float_unit: sig_duration_float;
    duration_in: "jet_jit_duration_in" => jet_jit_duration_in: sig_duration_int;
    duration_in_unit: "jet_jit_duration_in_unit" => jet_jit_duration_in_unit: sig_duration_int;
    duration_in_exact: "jet_duration_in" => jet_jit_duration_in_unit: sig_duration_int;
    duration_is_zero: "jet_jit_duration_is_zero" => jet_jit_duration_is_zero: sig_result_query_i8;
    duration_is_zero_exact: "jet_duration_is_zero" => jet_jit_duration_is_zero: sig_result_query_i8;
    duration_total_seconds: "jet_jit_duration_total_seconds" => jet_jit_duration_total_seconds: sig_result_query_i64;
    duration_total_seconds_exact: "jet_duration_total_seconds" => jet_jit_duration_total_seconds: sig_result_query_i64;
    duration_seconds_value: "jet_jit_duration_seconds_value" => jet_jit_duration_seconds_value: sig_result_query_f64;
    duration_seconds_value_exact: "jet_duration_seconds_value" => jet_jit_duration_seconds_value: sig_result_query_f64;
    duration_add: "jet_jit_duration_add" => jet_jit_duration_add: sig_duration_int;
    duration_sub: "jet_jit_duration_sub" => jet_jit_duration_sub: sig_duration_int;
    duration_difference: "jet_jit_duration_difference" => jet_jit_duration_difference: sig_duration_int;
    duration_difference_exact: "jet_duration_difference" => jet_jit_duration_difference: sig_duration_int;
    duration_abs: "jet_jit_duration_abs" => jet_jit_duration_abs: sig_result_query_i64;
    duration_abs_exact: "jet_duration_abs" => jet_jit_duration_abs: sig_result_query_i64;
    duration_negated: "jet_jit_duration_negated" => jet_jit_duration_negated: sig_result_query_i64;
    duration_negated_exact: "jet_duration_negated" => jet_jit_duration_negated: sig_result_query_i64;
    duration_sign: "jet_jit_duration_sign" => jet_jit_duration_sign: sig_result_query_i64;
    duration_sign_exact: "jet_duration_sign" => jet_jit_duration_sign: sig_result_query_i64;
    duration_total_in: "jet_jit_duration_total_in" => jet_jit_duration_total_in: sig_duration_total_in;
    duration_total_in_exact: "jet_duration_total_in" => jet_jit_duration_total_in: sig_duration_total_in;
    duration_round: "jet_jit_duration_round" => jet_jit_duration_round: sig_duration_round;
    duration_round_exact: "jet_duration_round" => jet_jit_duration_round: sig_duration_round;
    duration_scale: "jet_jit_duration_scale" => jet_jit_duration_scale: sig_duration_int;
    duration_scale_exact: "jet_duration_scale" => jet_jit_duration_scale: sig_duration_int;
    duration_divide: "jet_jit_duration_divide" => jet_jit_duration_divide: sig_duration_int;
    duration_divide_exact: "jet_duration_divide" => jet_jit_duration_divide: sig_duration_int;
    duration_show: "jet_jit_duration_show" => jet_jit_duration_show: sig_str_unary_i64;
    perf_fidelity: "jet_jit_perf_fidelity" => jet_jit_perf_fidelity: sig_noarg_f64;
    perf_default_fidelity: "jet_jit_perf_default_fidelity" => jet_jit_perf_default_fidelity: sig_noarg_f64;
    perf_override_fidelity: "jet_jit_perf_override_fidelity" => jet_jit_perf_override_fidelity: sig_perf_override;
    perf_reset_fidelity: "jet_jit_perf_reset_fidelity" => jet_jit_perf_reset_fidelity: sig_noarg;
    service_call: "jet_jit_service_call" => jet_jit_service_call: sig_service_call;
    service_call_bool: "jet_jit_service_call_bool" => jet_jit_service_call_bool: sig_service_call_bool;
    service_show: "jet_jit_service_show" => jet_jit_service_show: sig_str_unary_i64;
    service_restart_show: "jet_jit_service_restart_show" => jet_jit_service_restart_show: sig_str_unary_i64;
    service_delivery_show: "jet_jit_service_delivery_show" => jet_jit_service_delivery_show: sig_str_unary_i64;
    is_trapped: "jet_jit_is_trapped" => jet_jit_is_trapped: sig_is_trapped;
    stack_enter: "jet_jit_stack_enter" => jet_jit_stack_enter: sig_stack_enter;
    stack_leave: "jet_jit_stack_leave" => jet_jit_stack_leave: sig_noarg;
    deopt_call: "jet_deopt_call" => super::deopt::jet_deopt_call: sig_deopt;
    reflect_of_finish: "jet_jit_reflect_of_finish" => jet_jit_reflect_of_finish: sig_reflect_finish;
    reflect_field_new: "jet_jit_reflect_field_new" => jet_jit_reflect_field_new: sig_reflect_field_new;
    reflect_type_name: "jet_jit_reflect_type_name" => jet_jit_reflect_type_name: sig_str_unary_i64;
    reflect_path: "jet_jit_reflect_path" => jet_jit_reflect_path: sig_str_unary_i64;
    reflect_display: "jet_jit_reflect_display" => jet_jit_reflect_display: sig_str_unary_i64;
    reflect_fields: "jet_jit_reflect_fields" => jet_jit_reflect_fields: sig_str_unary_i64;
    reflect_field_name: "jet_jit_reflect_field_name" => jet_jit_reflect_field_name: sig_str_unary_i64;
    reflect_field_value: "jet_jit_reflect_field_value" => jet_jit_reflect_field_value: sig_str_unary_i64;
    testing_temp_dir: "jet_jit_testing_temp_dir" => jet_jit_testing_temp_dir: sig_str_unary_i64;
    testing_snap: "jet_jit_testing_snap" => jet_jit_testing_snap: sig_str_eq;
    testing_golden: "jet_jit_testing_golden" => jet_jit_testing_golden: sig_str_eq;
    testing_histories: "jet_jit_testing_histories" => jet_jit_testing_histories: sig_testing_histories;
    history_rng_next_u64: "jet_testing_history_rng_next_u64" => jet_testing_history_rng_next_u64: sig_str_unary_i64;
    history_rng_below: "jet_testing_history_rng_below" => jet_testing_history_rng_below: sig_i64_i64_i64;
    testing_fixture: "jet_jit_testing_fixture" => jet_jit_testing_fixture: sig_str_unary_i64;
    testing_corpus: "jet_jit_testing_corpus" => jet_jit_testing_corpus: sig_str_unary_i64;
    testing_test_suite_new: "jet_jit_testing_test_suite_new" => jet_jit_testing_test_suite_new: sig_str_begin;
    testing_test_suite_run: "jet_jit_testing_test_suite_run" => jet_jit_testing_test_suite_run: sig_str_unary_i64;
    testing_compare: "jet_jit_testing_compare" => jet_jit_testing_compare: sig_testing_compare;
    testing_assert_equal: "jet_jit_testing_assert_equal" => jet_jit_testing_assert_equal: sig_testing_assert_equal;
    testing_status: "jet_jit_testing_status" => jet_jit_testing_status: sig_testing_status;
    cli_main: "jet_jit_cli_main" => crate::CLI::jet_jit_cli_main: sig_noarg_i64;
}

/// `JitZipValueKind::code` and `from_code` are two hand-written tables over one
/// enum, so they can drift the way every other pair in this crate has. A
/// missing `from_code` row makes `jet_jit_list_unzip` ICE on a legal column; a
/// SHIFTED row makes it read a `String` field as an `Int` — the silent wrong
/// answer the unzip host already shipped once.
#[cfg(test)]
mod zip_value_kind_tests {
    use super::JitZipValueKind;

    /// No `_` arm: a new kind must fail to compile here rather than quietly
    /// miss the wire encoding.
    fn kind_name(kind: JitZipValueKind) -> &'static str {
        match kind {
            JitZipValueKind::Int => "Int",
            JitZipValueKind::Float => "Float",
            JitZipValueKind::Bool => "Bool",
            JitZipValueKind::Char => "Char",
            JitZipValueKind::String => "String",
            JitZipValueKind::Opaque => "Opaque",
        }
    }

    const EVERY_KIND: [JitZipValueKind; 6] = [
        JitZipValueKind::Int,
        JitZipValueKind::Float,
        JitZipValueKind::Bool,
        JitZipValueKind::Char,
        JitZipValueKind::String,
        JitZipValueKind::Opaque,
    ];

    #[test]
    fn every_zip_value_kind_round_trips_through_its_wire_code() {
        for kind in EVERY_KIND {
            assert_eq!(
                JitZipValueKind::from_code(kind.code()),
                Some(kind),
                "`{}` does not survive its own wire code {}",
                kind_name(kind),
                kind.code()
            );
        }
        let codes: Vec<i64> = EVERY_KIND.iter().map(|kind| kind.code()).collect();
        for (index, code) in codes.iter().enumerate() {
            assert!(
                !codes[..index].contains(code),
                "`{}` reuses wire code {code}, so one column kind decodes as another",
                kind_name(EVERY_KIND[index])
            );
        }
        assert_eq!(
            JitZipValueKind::from_code(EVERY_KIND.len() as i64),
            None,
            "from_code accepts a code past the last kind, so a stale immediate \
             would decode as a real column kind instead of stopping"
        );
    }
}

#[cfg(test)]
mod host_fns_tests {
    use super::{new_jit_module, Concurrency, JitRuntime, ReleaseDevtoolsPolicy};
    use crate::host_fns_audit;
    use crate::resident::fresh_runtime;

    /// #1633 criterion #3: every `host_fns!`-declared symbol (across every
    /// `host_fns!`-migrated module, `@shared` entries included) must have a
    /// matching `builder.symbol` registration.
    ///
    /// `JITModule::new` does NOT prove this: cranelift-jit 0.112.3's
    /// `declare_function` for `Linkage::Import` does
    /// `lookup_symbol(name).unwrap_or(null)` and installs a null PLT entry on
    /// on a miss, returning `Ok` regardless. A prior version of this test only
    /// asserted `new_jit_module()` is `Ok`, which stayed green even with a
    /// missing registration (e.g. deleting `Reactive`'s
    /// `event_scope: "jet_jit_event_scope" => jet_jit_event_scope`
    /// registration while `Watcher`'s `@shared event_scope_cancel` import
    /// kept expecting it) — the JIT would then call a null pointer at run
    /// time. `host_fns!` now records every symbol it declares and registers
    /// into two process-wide sets (`host_fns_audit`); this test compares
    /// them directly instead of trusting `new_jit_module`'s `Ok`.
    #[test]
    fn all_host_symbols_declared_match_all_registered() {
        let (_module, _host) = new_jit_module()
            .expect("every declared JIT host FuncId must resolve to a registered symbol");
        let (registered, declared) = host_fns_audit::take_snapshot();
        let declared_not_registered: Vec<_> = declared.difference(&registered).collect();
        assert!(
            declared_not_registered.is_empty(),
            "declared JIT host symbols with no matching registration (would resolve to a \
             null PLT entry at run time, not a build error): {declared_not_registered:?}"
        );
        let registered_not_declared: Vec<_> = registered.difference(&declared).collect();
        assert!(
            registered_not_declared.is_empty(),
            "registered JIT host symbols that no `host_fns!` table declares/imports \
             (dead registration, not the single listing #1633 requires): {registered_not_declared:?}"
        );
    }

    /// Card 1984: lowering reaches a Core row's resident host only through
    /// `CoreCallRecord::jit_symbol_candidates`, so an adapter exported under
    /// any other name is unreachable — `lower_recorded_core_call` misses it,
    /// falls through, and the whole function silently deopts to the
    /// interpreter. No output check can see that. `core.files.create_dir_all`
    #[test]
    fn core_rows_project_onto_a_registered_resident_host() {
        let (_module, host) = new_jit_module().expect("resident host module");
        for (module, member) in [
            ("core.files", "create_dir"),
            ("core.files", "create_dir_all"),
            ("core.log", "field"),
            ("core.log", "int"),
            ("core.log", "bool"),
        ] {
            let row = jet_foundation::Syntax::core_call(module, member)
                .unwrap_or_else(|| panic!("{module}.{member} has no Core row"));
            let candidates = row.jit_symbol_candidates();
            assert!(
                candidates
                    .iter()
                    .any(|symbol| host.lookup(symbol).is_some()),
                "{module}.{member} projects onto {candidates:?}; the resident host \
                 registers none of them, so every call to it deopts silently"
            );
        }
    }

    #[test]
    fn arbitrary_helper_panic_is_ice_not_runtime_stop() {
        let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
        let payload = std::panic::catch_unwind(|| std::panic::panic_any("helper fault"))
            .expect_err("test seam must provide an arbitrary helper panic");
        let message = payload
            .downcast_ref::<&'static str>()
            .copied()
            .unwrap_or("unknown helper fault");

        runtime.set_host_fault(message);

        assert_eq!(runtime.exit_code, Some(101));
        assert_eq!(runtime.trapped.as_deref(), Some("helper fault"));
        assert!(!runtime.stderr.contains("E3001"));
        assert!(!runtime.stderr.contains("E3010"));
        assert!(!runtime.stderr.contains("E3011"));
        assert!(!runtime.stderr.contains("E3012"));
    }

    #[test]
    fn scheduler_host_fault_marker_is_not_user_visible() {
        let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
        runtime.set_host_fault("__jet_host_fault__: helper fault");

        assert_eq!(runtime.exit_code, Some(101));
        assert_eq!(runtime.trapped.as_deref(), Some("helper fault"));
    }

    #[test]
    fn child_runtime_stop_stays_out_of_process_stop_state() {
        std::thread::spawn(|| {
            let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
            runtime.source_file = "child.jet".to_string();
            runtime.source_text = "divide()".to_string();
            runtime.current_function = "run".to_string();
            runtime.current_line = 1;
            let runtime_ptr = &mut runtime as *mut JitRuntime;
            Concurrency::set_active_runtime(Some(runtime_ptr));
            let outcome = jet_codegen::scheduler::jet_scheduler_wait_without_unwind(|| {
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_runtime_stop("E3010", 1, "division by zero");
                });
            });
            Concurrency::set_active_runtime(None);
            Concurrency::clear_http_shared_runtime();

            assert!(matches!(
                outcome,
                jet_codegen::scheduler::JetSchedulerWait::Ready(())
            ));
            assert!(runtime.stderr.is_empty());
            assert_eq!(runtime.exit_code, None);
            assert_eq!(runtime.trapped, None);
            assert!(!runtime.host_fault);
        })
        .join()
        .expect("child failure probe");
    }

    #[test]
    fn child_ffi_failure_stays_out_of_process_stop_state() {
        std::thread::spawn(|| {
            let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
            runtime.source_file = "ffi.jet".to_string();
            runtime.source_text = "foreign_call()".to_string();
            runtime.current_function = "run".to_string();
            runtime.current_line = 1;
            let runtime_ptr = &mut runtime as *mut JitRuntime;
            Concurrency::set_active_runtime(Some(runtime_ptr));
            let outcome = jet_codegen::scheduler::jet_scheduler_wait_without_unwind(|| {
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_ffi_runtime_stop("panic: a foreign function panicked");
                });
            });
            Concurrency::set_active_runtime(None);
            Concurrency::clear_http_shared_runtime();

            assert!(matches!(
                outcome,
                jet_codegen::scheduler::JetSchedulerWait::Ready(())
            ));
            assert!(runtime.stderr.is_empty());
            assert_eq!(runtime.exit_code, None);
            assert_eq!(runtime.trapped, None);
            assert!(!runtime.host_fault);
        })
        .join()
        .expect("child failure probe");
    }
    #[test]
    fn trap_poll_flag_publishes_payload_and_clears_on_take() {
        let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
        assert!(!runtime.trap_pending());

        runtime.set_trap_message("ordered payload".to_string());
        assert!(runtime.trap_pending());
        assert_eq!(runtime.trapped.as_deref(), Some("ordered payload"));

        assert_eq!(runtime.take_trap().as_deref(), Some("ordered payload"));
        assert!(!runtime.trap_pending());
        assert!(runtime.trapped.is_none());
    }
}
