//! D-REACT1 / D-EVENT1 / D-PENDING1: resident-JIT reactive + event host.
//! Signal/derived/effect use the canonical observer algorithm via `include!`
//! of the reactive core extracted from ReactiveEventWatch.rs (build.rs).
//! Opaque i64 handles + JIT fn-ptr adapters — no third reactive graph.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_codegen::scheduler::{
    jet_ctx_deadline_ms, jet_ctx_push_deadline, jet_scheduler_blocking_wait_enter,
    jet_scheduler_blocking_wait_leave, jet_scheduler_is_cancel_unwind,
    jet_scheduler_is_deadline_unwind, jet_scheduler_propagate_deadline,
    jet_scheduler_shielded, jet_scheduler_spawn_blocking_with_control_at, jet_scheduler_yield,
    jet_std_time_now, jet_task_join_deadline_check, JetSchedulerJoin, JetTaskControl,
    JetTaskFailure, JetTypedDeadlineBoundary, ParkSlot,
};
use jet_codegen::task_group::jet_task_deadline_if_expired;
use std::sync::Arc;
use jet_foundation::Devtools::jet_devtools_publish_event;

fn jet_web_runtime_devtools_enabled() -> bool {
    crate::Concurrency::with_runtime_mut(|runtime| runtime.web_runtime_devtools_enabled())
}

mod loadable_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Loadable.rs");
}

include!("../../jet-foundation/src/LiveLifecycle.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs");

/// Canonical reactive core (JetSignal / JetDerived / jet_reactive_effect*).
#[allow(dead_code, unused_imports)]
pub(crate) mod reactive_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/reactive_rt.rs"));
}

use reactive_rt as jet_std;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebQuery.rs");

#[derive(Clone, Copy)]
pub(crate) struct JitCb {
    pub(crate) fn_ptr: u64,
    pub(crate) caps: [i64; 4],
    pub(crate) n_caps: u8,
}

impl JitCb {
    pub(crate) fn invoke_void(self) {
        // SAFETY: fn_ptr is a JIT-compiled spawn body with matching capture arity.
        unsafe {
            match self.n_caps {
                0 => {
                    let f: unsafe extern "C" fn() = std::mem::transmute(self.fn_ptr);
                    f();
                }
                1 => {
                    let f: unsafe extern "C" fn(i64) = std::mem::transmute(self.fn_ptr);
                    f(self.caps[0]);
                }
                2 => {
                    let f: unsafe extern "C" fn(i64, i64) = std::mem::transmute(self.fn_ptr);
                    f(self.caps[0], self.caps[1]);
                }
                3 => {
                    let f: unsafe extern "C" fn(i64, i64, i64) = std::mem::transmute(self.fn_ptr);
                    f(self.caps[0], self.caps[1], self.caps[2]);
                }
                _ => {
                    let f: unsafe extern "C" fn(i64, i64, i64, i64) =
                        std::mem::transmute(self.fn_ptr);
                    f(self.caps[0], self.caps[1], self.caps[2], self.caps[3]);
                }
            }
        }
    }

    fn invoke_i64(self) -> i64 {
        unsafe {
            match self.n_caps {
                0 => {
                    let f: unsafe extern "C" fn() -> i64 = std::mem::transmute(self.fn_ptr);
                    f()
                }
                1 => {
                    let f: unsafe extern "C" fn(i64) -> i64 = std::mem::transmute(self.fn_ptr);
                    f(self.caps[0])
                }
                2 => {
                    let f: unsafe extern "C" fn(i64, i64) -> i64 = std::mem::transmute(self.fn_ptr);
                    f(self.caps[0], self.caps[1])
                }
                3 => {
                    let f: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                        std::mem::transmute(self.fn_ptr);
                    f(self.caps[0], self.caps[1], self.caps[2])
                }
                _ => {
                    let f: unsafe extern "C" fn(i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(self.fn_ptr);
                    f(self.caps[0], self.caps[1], self.caps[2], self.caps[3])
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
enum AsyncEventCallback {
    Universal { handle: i64, epoch: usize },
}

fn async_event_callback(
    rt: &mut crate::runtime_host::JitRuntime,
    fn_ptr: i64,
) -> Option<AsyncEventCallback> {
    if fn_ptr >= 0 {
        rt.set_host_fault("JIT async Event listener requires a universal callback thunk");
        return None;
    }
    let slot = crate::runtime_host::jit_callable_parts(rt, fn_ptr)?;
    if slot.raw_unary.is_none() || slot.raw_pair.is_some() || slot.raw_many.is_some() {
        rt.set_host_fault("JIT async Event listener has no unary universal callback thunk");
        return None;
    }
    let epoch = Concurrency::http_runtime_epoch();
    Some(AsyncEventCallback::Universal {
        handle: slot.handle,
        epoch,
    })
}

fn invoke_async_event_callback(
    callback: AsyncEventCallback,
    payload: i64,
) -> Result<(), String> {
    let AsyncEventCallback::Universal { handle, epoch } = callback;
    let Some(result) = Concurrency::with_http_jet_runtime(|| {
        if Concurrency::http_runtime_epoch() != epoch {
            return None;
        }
        let slot = Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::jit_callable_parts(rt, handle)
        });
        slot.and_then(|slot| crate::runtime_host::invoke_universal_unary(slot, payload))
    }) else {
        return Err("async Event listener callback runtime snapshot expired".to_string());
    };
    if result == 0 {
        // Unit-returning handlers have no Result carrier; their universal thunk
        // uses zero as the ordinary infallible return value. Explicit
        // `Result<(), E>` handlers return a nonzero heap result handle below.
        return Ok(());
    }
    let outcome = Concurrency::with_http_jet_runtime(|| {
        if Concurrency::http_runtime_epoch() != epoch {
            return Err("async Event listener callback result runtime expired".to_string());
        }
        Concurrency::with_runtime_string(|rt| {
            let Some((ok, bits)) = crate::runtime_host::jit_result_parts(rt, result) else {
                return Err("async Event listener callback returned an invalid Result".to_string());
            };
            if ok {
                Ok(())
            } else {
                Err(rt
                    .heap
                    .clone_string(bits as i64)
                    .unwrap_or_else(|| "async Event listener failed".to_string()))
            }
        })
    });
    outcome
}
fn event_callback(
    rt: &mut crate::runtime_host::JitRuntime,
    fn_ptr: i64,
) -> Option<crate::runtime_host::JitCallableSlot> {
    if fn_ptr >= 0 {
        rt.set_host_fault("JIT Event listener requires a universal callback thunk");
        return None;
    }
    let slot = crate::runtime_host::jit_callable_parts(rt, fn_ptr)?;
    if slot.raw_unary.is_none() || slot.raw_pair.is_some() || slot.raw_many.is_some() {
        rt.set_host_fault("JIT Event listener has no unary universal callback thunk");
        return None;
    }
    Some(slot)
}

fn invoke_unary_callback(
    callback: crate::runtime_host::JitCallableSlot,
    payload: i64,
) -> Option<i64> {
    Concurrency::with_http_jet_runtime(|| {
        crate::runtime_host::invoke_universal_unary(callback, payload)
    })
}

fn invoke_event_callback(
    callback: crate::runtime_host::JitCallableSlot,
    payload: i64,
) {
    if invoke_unary_callback(callback, payload).is_none() {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("JIT Event listener callback invocation failed");
        });
    }
}

fn invoke_text_callback(
    callback: crate::runtime_host::JitCallableSlot,
    payload: i64,
) -> String {
    let Some(value) = invoke_unary_callback(callback, payload) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("JIT Hook listener callback invocation failed");
        });
        return String::new();
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.heap.clone_string(value).unwrap_or_else(|| {
            rt.set_host_fault("JIT Hook listener callback returned non-text");
            String::new()
        })
    })
}

fn invoke_decision_callback(
    callback: crate::runtime_host::JitCallableSlot,
    payload: i64,
) -> reactive_rt::JetHookDecision<i64, String> {
    let Some(packed) = invoke_unary_callback(callback, payload) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("JIT DecisionHook listener callback invocation failed");
        });
        return reactive_rt::JetHookDecision::Continue;
    };
    let disc = packed & 0xff;
    let value = packed >> 8;
    match disc {
        1 => reactive_rt::JetHookDecision::Transform(value),
        2 => reactive_rt::JetHookDecision::Cancel,
        3 => {
            let message = Concurrency::with_runtime_mut(|rt| {
                rt.heap.clone_string(value).unwrap_or_else(|| {
                    rt.set_host_fault("JIT DecisionHook callback returned invalid failure text");
                    String::new()
                })
            });
            reactive_rt::JetHookDecision::Fail(message)
        }
        _ => reactive_rt::JetHookDecision::Continue,
    }
}

pub(crate) struct AsyncEventSlot {
    pub(crate) policy: reactive_rt::JetAsyncPolicy,
    pub(crate) failure_policy: reactive_rt::JetFailurePolicy,
    pub(crate) event: reactive_rt::JetAsyncEvent<i64, String>,
}

#[derive(Clone, Default)]
pub(crate) struct DispatchReportSlot {
    pub(crate) delivered: bool,
    pub(crate) accepted: bool,
    pub(crate) handlers: i64,
    pub(crate) state: Option<reactive_rt::JetDispatchState>,
    pub(crate) failures: Vec<reactive_rt::JetDispatchFailure<String>>,
    pub(crate) trace: Option<reactive_rt::JetEventTrace>,
}


trait TypedSignal: Send + Sync {
    fn get_word(&self) -> i64;
    fn set_word(&self, value: i64);
}

struct TypedSignalImpl<T, Enc, Dec> {
    signal: reactive_rt::JetSignal<T>,
    encode: Enc,
    decode: Dec,
}

impl<T, Enc, Dec> TypedSignal for TypedSignalImpl<T, Enc, Dec>
where
    T: Clone + Send + Sync + 'static,
    Enc: Fn(T) -> i64 + Send + Sync,
    Dec: Fn(i64) -> T + Send + Sync,
{
    fn get_word(&self) -> i64 {
        (self.encode)(self.signal.get())
    }

    fn set_word(&self, value: i64) {
        self.signal.set((self.decode)(value));
    }
}

pub(crate) enum SignalSlot {
    Word(reactive_rt::JetSignal<i64>),
    Typed(Box<dyn TypedSignal>),
}

impl SignalSlot {
    pub(crate) fn typed<T, Enc, Dec>(
        signal: reactive_rt::JetSignal<T>,
        encode: Enc,
        decode: Dec,
    ) -> Self
    where
        T: Clone + Send + Sync + 'static,
        Enc: Fn(T) -> i64 + Send + Sync + 'static,
        Dec: Fn(i64) -> T + Send + Sync + 'static,
    {
        Self::Typed(Box::new(TypedSignalImpl {
            signal,
            encode,
            decode,
        }))
    }

    pub(crate) fn get(&self) -> i64 {
        match self {
            Self::Word(signal) => signal.get(),
            Self::Typed(signal) => signal.get_word(),
        }
    }

    pub(crate) fn set(&self, value: i64) {
        match self {
            Self::Word(signal) => signal.set(value),
            Self::Typed(signal) => signal.set_word(value),
        }
    }
}

trait TypedDerived: Send + Sync {
    fn get_word(&self) -> i64;
}

struct TypedDerivedImpl<T, Enc> {
    derived: reactive_rt::JetDerived<T>,
    encode: Enc,
}

impl<T, Enc> TypedDerived for TypedDerivedImpl<T, Enc>
where
    T: Clone + Send + Sync + 'static,
    Enc: Fn(T) -> i64 + Send + Sync,
{
    fn get_word(&self) -> i64 {
        (self.encode)(self.derived.get())
    }
}

pub(crate) enum DerivedSlot {
    Word(reactive_rt::JetDerived<i64>),
    Typed(Box<dyn TypedDerived>),
}

impl DerivedSlot {
    pub(crate) fn typed<T, Enc>(derived: reactive_rt::JetDerived<T>, encode: Enc) -> Self
    where
        T: Clone + Send + Sync + 'static,
        Enc: Fn(T) -> i64 + Send + Sync + 'static,
    {
        Self::Typed(Box::new(TypedDerivedImpl { derived, encode }))
    }

    pub(crate) fn get(&self) -> i64 {
        match self {
            Self::Word(derived) => derived.get(),
            Self::Typed(derived) => derived.get_word(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ReactiveState {
    pub(crate) signals: Vec<SignalSlot>,
    pub(crate) deriveds: Vec<DerivedSlot>,
    pub(crate) effects: Vec<reactive_rt::JetReactiveEffect>,
    pub(crate) event_scopes: Vec<reactive_rt::JetEventScope>,
    pub(crate) event_policies: Vec<reactive_rt::JetEventPolicy>,
    pub(crate) events: Vec<reactive_rt::JetEvent<i64>>,
    pub(crate) subscriptions: Vec<reactive_rt::JetSubscription>,
    pub(crate) hooks: Vec<reactive_rt::JetHook<i64, String>>,
    pub(crate) decision_hooks: Vec<reactive_rt::JetDecisionHook<i64, String>>,
    pub(crate) event_traces: Vec<reactive_rt::JetEventTrace>,
    pub(crate) async_events: Vec<AsyncEventSlot>,
    pub(crate) dispatch_reports: Vec<DispatchReportSlot>,
    live_queries: Vec<Option<JetLiveQuery>>,
}

fn live_index(handle: i64) -> Option<usize> {
    usize::try_from(handle).ok()?.checked_sub(1)
}

fn live_query<'a>(
    rt: &'a crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<&'a JetLiveQuery> {
    live_index(handle)
        .and_then(|index| rt.reactive.live_queries.get(index))
        .and_then(Option::as_ref)
}

fn jet_jit_app_live(footprint: i64, initial: i64) -> i64 {
    with_rt(|rt| {
        let footprint = rt.heap.clone_string(footprint).unwrap_or_default();
        let initial = rt.heap.clone_string(initial).unwrap_or_default();
        rt.reactive
            .live_queries
            .push(Some(jet_app_live(footprint, initial)));
        rt.reactive.live_queries.len() as i64
    })
}

fn jet_jit_app_subscribe(source: i64) -> i64 {
    with_rt(|rt| {
        let source = rt.heap.clone_string(source).unwrap_or_default();
        rt.reactive
            .live_queries
            .push(Some(jet_app_subscribe(source)));
        rt.reactive.live_queries.len() as i64
    })
}

fn jet_jit_app_invalidate(footprint: i64) -> i64 {
    with_rt(|rt| {
        let footprint = rt.heap.clone_string(footprint).unwrap_or_default();
        jet_app_invalidate(footprint)
    })
}

fn jet_jit_app_transact_invalidate(write_set: i64) -> i64 {
    with_rt(|rt| {
        let write_set = rt.heap.clone_string(write_set).unwrap_or_default();
        jet_app_transact_invalidate(write_set)
    })
}

pub(crate) fn jet_jit_app_transact_invalidate_text(write_set: String) -> i64 {
    jet_app_transact_invalidate(write_set)
}

pub(crate) fn web_query_from_live(key: String, live_handle: i64) -> JetWebQuery {
    let fallback_key = key.clone();
    Concurrency::with_runtime_result((), |rt| {
        let Some(live) = live_query(rt, live_handle).cloned() else {
            return Ok(JetWebQuery::new(key, String::new()));
        };
        let initial = jet_app_live_get(&live);
        let mut query = JetWebQuery::new(key, initial);
        query.footprint = live.footprint.display();
        let bound = query.bind_live_state(&live);
        query.live = Some(bound);
        jet_web_query_register_panel_fact(&query);
        Ok(query)
    })
    .unwrap_or_else(|_| JetWebQuery::new(fallback_key, String::new()))
}

fn jet_jit_app_signal_push(query: i64, payload: i64) -> i64 {
    with_rt(|rt| {
        let Some(index) = live_index(query) else {
            return 0;
        };
        let Some(current) = rt
            .reactive
            .live_queries
            .get(index)
            .and_then(Option::as_ref)
            .cloned()
        else {
            return 0;
        };
        let payload = rt.heap.clone_string(payload).unwrap_or_default();
        let updated = jet_app_signal_push(&current, payload);
        let Some(slot) = rt.reactive.live_queries.get_mut(index) else {
            return 0;
        };
        *slot = Some(updated);
        query
    })
}

fn jet_jit_app_live_get(query: i64) -> i64 {
    with_rt(|rt| {
        let Some(query) = live_query(rt, query).cloned() else {
            return rt
                .heap
                .alloc_string("LiveError(live query is closed)".to_string());
        };
        rt.heap.alloc_string(jet_app_live_get(&query))
    })
}

fn jet_jit_app_live_show(query: i64) -> i64 {
    with_rt(|rt| {
        let Some(query) = live_query(rt, query).cloned() else {
            return rt
                .heap
                .alloc_string("LiveQueryError(reason=live query is closed)".to_string());
        };
        rt.heap.alloc_string(jet_app_live_show(&query))
    })
}

fn jet_jit_app_live_stats() -> i64 {
    with_rt(|rt| rt.heap.alloc_string(jet_app_live_stats()))
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::runtime_host::JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

fn with_async_runtime<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    Concurrency::with_http_jet_runtime(f)
}

fn jet_deadline_check(wait_kind: &str) {
    if jet_scheduler_shielded() {
        return;
    }
    let remaining = jet_ctx_deadline_ms().map(|deadline| {
        deadline.saturating_sub(jet_std_time_now())
    });
    if let Some(deadline) = jet_task_deadline_if_expired(remaining, wait_kind) {
        jet_scheduler_propagate_deadline(deadline.render());
    }
}

fn jet_jit_reactive_signal(init: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive.signals.push(SignalSlot::Word(reactive_rt::JetSignal::new(init)));
        rt.reactive.signals.len() as i64
    })
}

fn jet_jit_reactive_get(handle: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .signals
            .get(handle.saturating_sub(1) as usize)
            .expect("jit reactive get: bad signal")
            .get()
    })
}

fn jet_jit_reactive_set(handle: i64, value: i64) {
    with_rt(|rt| {
        rt.reactive
            .signals
            .get(handle.saturating_sub(1) as usize)
            .expect("jit reactive set: bad signal")
            .set(value);
    });
}

fn jet_jit_reactive_derived(fn_ptr: i64, n_caps: i64, c0: i64, c1: i64, c2: i64, c3: i64) -> i64 {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let derived = reactive_rt::JetDerived::new(move || cb.invoke_i64());
        rt.reactive.deriveds.push(DerivedSlot::Word(derived));
        rt.reactive.deriveds.len() as i64
    })
}

fn jet_jit_reactive_derived_get(handle: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .deriveds
            .get(handle.saturating_sub(1) as usize)
            .expect("jit reactive derived get: bad handle")
            .get()
    })
}

fn jet_jit_reactive_effect(fn_ptr: i64, n_caps: i64, c0: i64, c1: i64, c2: i64, c3: i64) -> i64 {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let effect = reactive_rt::jet_reactive_effect(move || cb.invoke_void());
        rt.reactive.effects.push(effect);
        rt.reactive.effects.len() as i64
    })
}

fn jet_jit_reactive_effect_rooted(fn_ptr: i64, n_caps: i64, c0: i64, c1: i64, c2: i64, c3: i64) {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    reactive_rt::jet_reactive_effect_rooted(move || cb.invoke_void());
}

fn jet_jit_loadable_idle() -> i64 {
    // disc=Idle(0), no payload — packed enum ABI
    i64::from(loadable_kernel::JET_LOADABLE_IDLE)
}
fn jet_jit_loadable_loading() -> i64 {
    i64::from(loadable_kernel::JET_LOADABLE_LOADING)
}
fn jet_jit_loadable_loaded(payload: i64) -> i64 {
    (payload << 8) | i64::from(loadable_kernel::JET_LOADABLE_LOADED)
}
fn jet_jit_loadable_failed(payload: i64) -> i64 {
    (payload << 8) | i64::from(loadable_kernel::JET_LOADABLE_FAILED)
}
fn jet_jit_loadable_is_idle(handle: i64) -> i8 {
    jet_jit_loadable_is(handle, i64::from(loadable_kernel::JET_LOADABLE_IDLE))
}

fn jet_jit_loadable_is_loading(handle: i64) -> i8 {
    jet_jit_loadable_is(handle, i64::from(loadable_kernel::JET_LOADABLE_LOADING))
}

fn jet_jit_loadable_is_loaded(handle: i64) -> i8 {
    jet_jit_loadable_is(handle, i64::from(loadable_kernel::JET_LOADABLE_LOADED))
}

fn jet_jit_loadable_is_failed(handle: i64) -> i8 {
    jet_jit_loadable_is(handle, i64::from(loadable_kernel::JET_LOADABLE_FAILED))
}

fn jet_jit_loadable_loaded_value(handle: i64) -> i64 {
    if loadable_kernel::jet_loadable_has_value((handle & 0xff) as u8) {
        (handle >> 8).wrapping_add(1)
    } else {
        0
    }
}

fn jet_jit_loadable_is(handle: i64, kind: i64) -> i8 {
    if loadable_kernel::jet_loadable_is_tag((handle & 0xff) as u8, kind as u8) {
        1
    } else {
        0
    }
}

fn jet_jit_loadable_payload(handle: i64) -> i64 {
    handle >> 8
}

fn jet_jit_loadable_or_else(handle: i64, default: i64) -> i64 {
    if loadable_kernel::jet_loadable_has_value((handle & 0xff) as u8) {
        handle >> 8
    } else {
        default
    }
}

// ── Events (thin adapters over canonical JetEvent*) ──────────────────────────

fn jet_jit_event_scope() -> i64 {
    // One EventScope handle for watcher (#1219) and UI/reactive (#1225).
    let wid = crate::Watcher::mirror_event_scope();
    with_rt(|rt| {
        while rt.reactive.event_scopes.len() < wid as usize {
            rt.reactive
                .event_scopes
                .push(reactive_rt::JetEventScope::new());
        }
        debug_assert_eq!(rt.reactive.event_scopes.len() as i64, wid);
        wid
    })
}

fn jet_jit_event_policy_sync() -> i64 {
    with_rt(|rt| {
        rt.reactive
            .event_policies
            .push(reactive_rt::JetEventPolicy::sync());
        rt.reactive.event_policies.len() as i64
    })
}

fn jet_jit_event_with_policy(policy: i64) -> i64 {
    with_rt(|rt| {
        let policy = rt
            .reactive
            .event_policies
            .get(policy.saturating_sub(1) as usize)
            .cloned()
            .expect("jit event with_policy: bad policy");
        rt.reactive
            .events
            .push(reactive_rt::JetEvent::with_policy(policy));
        rt.reactive.events.len() as i64
    })
}

fn jet_jit_event_new() -> i64 {
    with_rt(|rt| {
        rt.reactive.events.push(reactive_rt::JetEvent::new());
        rt.reactive.events.len() as i64
    })
}

fn jet_jit_event_on(event: i64, scope: i64, fn_ptr: i64) -> i64 {
    let Some(callback) = with_rt(|rt| event_callback(rt, fn_ptr)) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let Some(event) = rt
            .reactive
            .events
            .get(event.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let sub = event.on(&scope, move |payload| {
            invoke_event_callback(callback, payload);
        });
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

fn jet_jit_event_once(event: i64, scope: i64, fn_ptr: i64) -> i64 {
    let Some(callback) = with_rt(|rt| event_callback(rt, fn_ptr)) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let Some(event) = rt
            .reactive
            .events
            .get(event.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let sub = event.once(&scope, move |payload| {
            invoke_event_callback(callback, payload);
        });
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

fn jet_jit_event_on_priority(event: i64, scope: i64, priority: i64, fn_ptr: i64) -> i64 {
    let Some(callback) = with_rt(|rt| event_callback(rt, fn_ptr)) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let Some(event) = rt
            .reactive
            .events
            .get(event.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let sub = event.on_priority(&scope, priority, move |payload| {
            invoke_event_callback(callback, payload);
        });
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

fn jet_jit_event_emit(event: i64, payload: i64) -> i64 {
    with_rt(|rt| {
        let event = rt
            .reactive
            .events
            .get(event.saturating_sub(1) as usize)
            .expect("jit event emit: bad event")
            .clone();
        let trace = event.emit(payload);
        rt.reactive.event_traces.push(trace);
        rt.reactive.event_traces.len() as i64
    })
}

fn jet_jit_event_listener_count(event: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .events
            .get(event.saturating_sub(1) as usize)
            .map(|event| event.listener_count())
            .unwrap_or(0)
    })
}

fn jet_jit_hook_listener_count(hook: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .hooks
            .get(hook.saturating_sub(1) as usize)
            .map(|hook| hook.listener_count())
            .unwrap_or(0)
    })
}

fn jet_jit_decision_hook_listener_count(hook: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .decision_hooks
            .get(hook.saturating_sub(1) as usize)
            .map(|hook| hook.listener_count())
            .unwrap_or(0)
    })
}


fn jet_jit_event_trace_summary(trace: i64) -> i64 {
    with_rt(|rt| {
        let summary = rt
            .reactive
            .event_traces
            .get(trace.saturating_sub(1) as usize)
            .expect("jit event trace: bad handle")
            .summary();
        rt.heap.alloc_string(summary)
    })
}

fn jet_jit_event_trace_delivered(trace: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .event_traces
            .get(trace.saturating_sub(1) as usize)
            .map(|trace| trace.delivered())
            .unwrap_or(0)
    })
}

fn jet_jit_event_trace_queued(trace: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .event_traces
            .get(trace.saturating_sub(1) as usize)
            .map(|trace| trace.queued())
            .unwrap_or(0)
    })
}

fn jet_jit_event_trace_dropped(trace: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .event_traces
            .get(trace.saturating_sub(1) as usize)
            .map(|trace| trace.dropped())
            .unwrap_or(0)
    })
}

fn jet_jit_event_trace(event: i64) -> i64 {
    with_rt(|rt| {
        let trace = rt
            .reactive
            .events
            .get(event.saturating_sub(1) as usize)
            .expect("jit event trace: bad event")
            .trace();
        rt.heap.alloc_string(trace)
    })
}

fn jet_jit_event_scope_active(scope: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .expect("jit event scope: bad handle")
            .active_count()
    })
}

fn jet_jit_event_scope_cancel(scope: i64) {
    crate::Watcher::mirror_event_scope_cancel(scope);
    with_rt(|rt| {
        if let Some(s) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
        {
            s.cancel();
        }
    });
}

fn jet_jit_subscription_unsubscribe(sub: i64) {
    with_rt(|rt| {
        rt.reactive
            .subscriptions
            .get(sub.saturating_sub(1) as usize)
            .expect("jit subscription: bad handle")
            .unsubscribe();
    });
}

fn jet_jit_subscription_active(sub: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .subscriptions
            .get(sub.saturating_sub(1) as usize)
            .map(|sub| sub.active() as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_hook_new(name: i64) -> i64 {
    with_rt(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        rt.reactive.hooks.push(reactive_rt::JetHook::new(name));
        rt.reactive.hooks.len() as i64
    })
}

fn jet_jit_hook_subscribe(
    hook: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
    once: bool,
) -> i64 {
    let Some(callback) = with_rt(|rt| event_callback(rt, fn_ptr)) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(hook) = rt
            .reactive
            .hooks
            .get(hook.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let handler = move |payload| invoke_text_callback(callback, payload);
        let sub = if once {
            hook.once(&scope, handler)
        } else if priority == 0 {
            hook.on(&scope, handler)
        } else {
            hook.on_priority(&scope, priority, handler)
        };
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

fn jet_jit_hook_on(hook: i64, scope: i64, fn_ptr: i64) -> i64 {
    jet_jit_hook_subscribe(hook, scope, 0, fn_ptr, false)
}

fn jet_jit_hook_once(hook: i64, scope: i64, fn_ptr: i64) -> i64 {
    jet_jit_hook_subscribe(hook, scope, 0, fn_ptr, true)
}

fn jet_jit_hook_on_priority(hook: i64, scope: i64, priority: i64, fn_ptr: i64) -> i64 {
    jet_jit_hook_subscribe(hook, scope, priority, fn_ptr, false)
}

fn jet_jit_hook_run(hook: i64, payload: i64, fallback: i64) -> i64 {
    with_rt(|rt| {
        let fallback = rt.heap.clone_string(fallback).unwrap_or_default();
        let hook = rt
            .reactive
            .hooks
            .get(hook.saturating_sub(1) as usize)
            .expect("jit hook run: bad handle")
            .clone();
        let out = hook.run(payload, fallback);
        rt.heap.alloc_string(out)
    })
}

fn jet_jit_hook_trace(hook: i64) -> i64 {
    with_rt(|rt| {
        let trace = rt
            .reactive
            .hooks
            .get(hook.saturating_sub(1) as usize)
            .expect("jit hook trace: bad handle")
            .trace();
        rt.heap.alloc_string(trace)
    })
}

fn jet_jit_decision_hook_new(policy: i64) -> i64 {
    let _ = policy; // HookPolicy.FirstCancelElseTransform — default for example
    with_rt(|rt| {
        rt.reactive
            .decision_hooks
            .push(reactive_rt::JetDecisionHook::new(
                reactive_rt::JetHookPolicy::FirstCancelElseTransform,
            ));
        rt.reactive.decision_hooks.len() as i64
    })
}

fn jet_jit_decision_hook_subscribe(
    hook: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
    once: bool,
) -> i64 {
    let Some(callback) = with_rt(|rt| event_callback(rt, fn_ptr)) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let Some(hook) = rt
            .reactive
            .decision_hooks
            .get(hook.saturating_sub(1) as usize)
            .cloned()
        else {
            return 0;
        };
        let handler = move |payload| invoke_decision_callback(callback, payload);
        let sub = if once {
            hook.once(&scope, handler)
        } else if priority == 0 {
            hook.on(&scope, handler)
        } else {
            hook.on_priority(&scope, priority, handler)
        };
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

fn jet_jit_decision_hook_on(hook: i64, scope: i64, fn_ptr: i64) -> i64 {
    jet_jit_decision_hook_subscribe(hook, scope, 0, fn_ptr, false)
}

fn jet_jit_decision_hook_once(hook: i64, scope: i64, fn_ptr: i64) -> i64 {
    jet_jit_decision_hook_subscribe(hook, scope, 0, fn_ptr, true)
}

fn jet_jit_decision_hook_on_priority(
    hook: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
) -> i64 {
    jet_jit_decision_hook_subscribe(hook, scope, priority, fn_ptr, false)
}

fn jet_jit_decision_hook_run(hook: i64, payload: i64) -> i64 {
    with_rt(|rt| {
        let hook = rt
            .reactive
            .decision_hooks
            .get(hook.saturating_sub(1) as usize)
            .expect("jit decision hook run: bad handle")
            .clone();
        let outcome = hook.run(payload);
        match outcome {
            reactive_rt::JetHookOutcome::Continue(v) => (v << 8) | 0,
            reactive_rt::JetHookOutcome::Cancel => 1,
            reactive_rt::JetHookOutcome::Fail(e) => {
                let sid = rt.heap.alloc_string(e);
                (sid << 8) | 2
            }
        }
    })
}

fn jet_jit_hook_decision_continue() -> i64 {
    0
}
fn jet_jit_hook_decision_transform(v: i64) -> i64 {
    (v << 8) | 1
}
fn jet_jit_hook_decision_cancel() -> i64 {
    2
}
fn jet_jit_hook_decision_fail(msg: i64) -> i64 {
    (msg << 8) | 3
}

// ── Async event host: canonical scheduler-backed Event lifecycle ─────────────

fn enum_variant_handle(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
    enum_name: &str,
) -> Option<&'static str> {
    let discriminant = rt.heap.record_get_int(handle, 0)?;
    crate::types_meta::prelude_enum_variant_at(enum_name, discriminant)
}

fn decode_async_policy(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<reactive_rt::JetAsyncPolicy> {
    let capacity = rt.heap.record_get_int(handle, 0)?;
    let overflow_handle = rt
        .heap
        .record_get_record(handle, 1)
        .or_else(|| rt.heap.record_get_int(handle, 1))?;
    let overflow = match enum_variant_handle(rt, overflow_handle, "Overflow")? {
        "Block" => reactive_rt::JetEventOverflow::Block,
        "DropNewest" => reactive_rt::JetEventOverflow::DropNewest,
        "DropOldest" => reactive_rt::JetEventOverflow::DropOldest,
        _ => return None,
    };
    Some(reactive_rt::JetAsyncPolicy::new(capacity, overflow))
}

fn decode_failure_policy(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<reactive_rt::JetFailurePolicy> {
    match enum_variant_handle(rt, handle, "FailurePolicy")? {
        "StopFirst" => Some(reactive_rt::JetFailurePolicy::StopFirst),
        "Collect" => Some(reactive_rt::JetFailurePolicy::Collect),
        "Log" => Some(reactive_rt::JetFailurePolicy::Log),
        "Ignore" => Some(reactive_rt::JetFailurePolicy::Ignore),
        _ => None,
    }
}

fn async_config_error_result(
    rt: &mut crate::runtime_host::JitRuntime,
    error: reactive_rt::JetEventConfigError,
) -> i64 {
    let discriminant = match error {
        reactive_rt::JetEventConfigError::InvalidCapacity => {
            crate::types_meta::prelude_enum_variant_index("EventConfigError", "InvalidCapacity")
                .expect("Prelude EventConfigError variants must be registered")
        }
    };
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    crate::runtime_host::alloc_jit_result(rt, false, record as u64)
}

fn async_event_index(handle: i64) -> Option<usize> {
    if handle & (1 << 62) == 0 {
        return None;
    }
    usize::try_from((handle & !(1 << 62)).checked_sub(1)?).ok()
}

fn dispatch_state_index(state: reactive_rt::JetDispatchState) -> i64 {
    match state {
        reactive_rt::JetDispatchState::Delivered => 0,
        reactive_rt::JetDispatchState::HandlerFailed => 1,
        reactive_rt::JetDispatchState::DroppedNewest => 2,
        reactive_rt::JetDispatchState::DroppedOldest => 3,
        reactive_rt::JetDispatchState::Closed => 4,
        reactive_rt::JetDispatchState::Cancelled => 5,
        reactive_rt::JetDispatchState::DeadlineExceeded => 6,
    }
}

fn async_dispatch_report_slot(
    report: reactive_rt::JetDispatchReport<String>,
) -> DispatchReportSlot {
    let state = report.state();
    DispatchReportSlot {
        delivered: matches!(state, reactive_rt::JetDispatchState::Delivered),
        accepted: report.accepted(),
        handlers: report.delivered_handlers(),
        state: Some(state),
        failures: report.failures(),
        trace: Some(report.trace()),
    }
}

fn jet_jit_async_event_new(policy: i64, failure: i64) -> i64 {
    with_rt(|rt| {
        let Some(policy) = decode_async_policy(rt, policy) else {
            rt.set_host_fault("JIT async event constructor received an invalid AsyncPolicy");
            return 0;
        };
        let Some(failure_policy) = decode_failure_policy(rt, failure) else {
            rt.set_host_fault("JIT async event constructor received an invalid FailurePolicy");
            return 0;
        };
        let event = match reactive_rt::JetAsyncEvent::new(policy, failure_policy) {
            Ok(event) => event,
            Err(error) => return async_config_error_result(rt, error),
        };
        rt.reactive.async_events.push(AsyncEventSlot {
            policy,
            failure_policy,
            event,
        });
        // High bit tags async handles so EventMethod.on can dispatch correctly.
        let handle = (rt.reactive.async_events.len() as i64) | (1 << 62);
        crate::runtime_host::alloc_jit_result(rt, true, handle as u64)
    })
}

fn jet_jit_async_event_subscribe(
    event: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
    once: bool,
) -> i64 {
    let Some(index) = async_event_index(event) else {
        return 0;
    };
    let Some(callback) = with_rt(|rt| async_event_callback(rt, fn_ptr)) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(scope) = scope
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| rt.reactive.event_scopes.get(index))
            .cloned()
        else {
            rt.set_host_fault("JIT async Event listener received an invalid scope");
            return 0;
        };
        let Some(event) = rt
            .reactive
            .async_events
            .get(index)
            .map(|slot| slot.event.clone())
        else {
            rt.set_host_fault("JIT async Event listener received an invalid event");
            return 0;
        };
        let subscription = if once {
            event.once(&scope, move |payload| invoke_async_event_callback(callback, payload))
        } else if priority == 0 {
            event.on(&scope, move |payload| invoke_async_event_callback(callback, payload))
        } else {
            event.on_priority(&scope, priority, move |payload| {
                invoke_async_event_callback(callback, payload)
            })
        };
        rt.reactive.subscriptions.push(subscription);
        rt.reactive.subscriptions.len() as i64
    })
}

fn jet_jit_async_event_on(event: i64, scope: i64, fn_ptr: i64) -> i64 {
    jet_jit_async_event_subscribe(event, scope, 0, fn_ptr, false)
}

fn jet_jit_async_event_once(event: i64, scope: i64, fn_ptr: i64) -> i64 {
    jet_jit_async_event_subscribe(event, scope, 0, fn_ptr, true)
}

fn jet_jit_async_event_on_priority(
    event: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
) -> i64 {
    jet_jit_async_event_subscribe(event, scope, priority, fn_ptr, false)
}

fn jet_jit_async_event_emit(event: i64, payload: i64) -> i64 {
    let Some(index) = async_event_index(event) else {
        return 0;
    };
    let Some(event) = with_rt(|rt| {
        rt.reactive
            .async_events
            .get(index)
            .map(|slot| slot.event.clone())
    }) else {
        with_rt(|rt| rt.set_host_fault("JIT async Event emit received an invalid event"));
        return 0;
    };
    Concurrency::spawn_ffi_task_typed(move || {
        let task = event.emit_async(payload);
        let report = match task.join() {
            Ok(report) => async_dispatch_report_slot(report),
            Err(failure) => {
                let message = match failure {
                    JetTaskFailure::Cancelled => "async event dispatch cancelled".to_string(),
                    JetTaskFailure::DeadlineBlown => {
                        "async event dispatch deadline exceeded".to_string()
                    }
                    JetTaskFailure::Panicked(reason) => {
                        format!("async event dispatch failed: {reason}")
                    }
                };
                std::panic::resume_unwind(Box::new(message));
            }
        };
        with_rt(|rt| {
            rt.reactive.dispatch_reports.push(report);
            rt.reactive.dispatch_reports.len() as i64
        })
    })
}

fn jet_jit_async_event_close(event: i64) {
    let Some(index) = async_event_index(event) else {
        return;
    };
    with_rt(|rt| {
        if let Some(slot) = rt.reactive.async_events.get(index) {
            slot.event.close();
        }
    });
}
fn jet_jit_async_event_listener_count(event: i64) -> i64 {
    with_rt(|rt| {
        async_event_index(event)
            .and_then(|index| rt.reactive.async_events.get(index))
            .map(|slot| slot.event.listener_count())
            .unwrap_or(0)
    })
}

fn jet_jit_async_event_queued_count(event: i64) -> i64 {
    with_rt(|rt| {
        async_event_index(event)
            .and_then(|index| rt.reactive.async_events.get(index))
            .map(|slot| slot.event.queued_count())
            .unwrap_or(0)
    })
}

fn jet_jit_async_event_running_count(event: i64) -> i64 {
    with_rt(|rt| {
        async_event_index(event)
            .and_then(|index| rt.reactive.async_events.get(index))
            .map(|slot| slot.event.running_count())
            .unwrap_or(0)
    })
}

fn jet_jit_async_event_blocked_count(event: i64) -> i64 {
    with_rt(|rt| {
        async_event_index(event)
            .and_then(|index| rt.reactive.async_events.get(index))
            .map(|slot| slot.event.blocked_count())
            .unwrap_or(0)
    })
}



fn jet_jit_dispatch_report_state(report: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .dispatch_reports
            .get(report.saturating_sub(1) as usize)
            .and_then(|r| r.state)
            .map(dispatch_state_index)
            .unwrap_or(1)
    })
}

fn jet_jit_dispatch_report_accepted(report: i64) -> i64 {
    with_rt(|rt| {
        i64::from(
            rt.reactive
                .dispatch_reports
                .get(report.saturating_sub(1) as usize)
                .map(|r| r.accepted)
                .unwrap_or(false),
        )
    })
}

fn jet_jit_dispatch_report_handlers(report: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .dispatch_reports
            .get(report.saturating_sub(1) as usize)
            .map(|r| r.handlers)
            .unwrap_or(0)
    })
}

fn dispatch_failure_record(
    rt: &mut crate::runtime_host::JitRuntime,
    failure: &reactive_rt::JetDispatchFailure<String>,
) -> i64 {
    let (discriminant, message) = match failure {
        reactive_rt::JetDispatchFailure::Handler(message) => (0, message),
        reactive_rt::JetDispatchFailure::Panic(message) => (1, message),
    };
    let record = rt.heap.alloc_record(2);
    let message_handle = rt.heap.alloc_string(message.clone());
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    let _ = rt.heap.record_set_string(record, 1, message_handle);
    record
}

fn jet_jit_dispatch_report_failures(report: i64) -> i64 {
    with_rt(|rt| {
        let failures = rt
            .reactive
            .dispatch_reports
            .get(report.saturating_sub(1) as usize)
            .map(|r| r.failures.clone())
            .unwrap_or_default();
        let values = failures
            .iter()
            .map(|failure| jet_rt::JetVal::RecordRef(dispatch_failure_record(rt, failure)))
            .collect();
        rt.heap.alloc_list_values(values)
    })
}

fn jet_jit_dispatch_report_trace(report: i64) -> i64 {
    with_rt(|rt| {
        let Some(trace) = rt
            .reactive
            .dispatch_reports
            .get(report.saturating_sub(1) as usize)
            .and_then(|r| r.trace.clone())
        else {
            return 0;
        };
        rt.reactive.event_traces.push(trace);
        rt.reactive.event_traces.len() as i64
    })
}

host_fns! {
    struct ReactiveHostFns;
    register: register_reactive_symbols;
    declare: declare_reactive_host_fns(module) {
        let cc = module.target_config().default_call_conv;

        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_i8 = Signature::new(cc);
        unary_i8.params.push(AbiParam::new(types::I64));
        unary_i8.returns.push(AbiParam::new(types::I8));
        let mut unary_void = Signature::new(cc);
        unary_void.params.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut binary_void = Signature::new(cc);
        binary_void.params.push(AbiParam::new(types::I64));
        binary_void.params.push(AbiParam::new(types::I64));
        let mut binary_i8 = Signature::new(cc);
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.returns.push(AbiParam::new(types::I8));
        let mut cb6 = Signature::new(cc);
        for _ in 0..6 {
            cb6.params.push(AbiParam::new(types::I64));
        }
        cb6.returns.push(AbiParam::new(types::I64));
        let mut cb6_void = Signature::new(cc);
        for _ in 0..6 {
            cb6_void.params.push(AbiParam::new(types::I64));
        }
        let mut listener = Signature::new(cc);
        for _ in 0..3 {
            listener.params.push(AbiParam::new(types::I64));
        }
        listener.returns.push(AbiParam::new(types::I64));
        let mut listener_priority = Signature::new(cc);
        for _ in 0..4 {
            listener_priority.params.push(AbiParam::new(types::I64));
        }
        listener_priority.returns.push(AbiParam::new(types::I64));
        let mut ternary = Signature::new(cc);
        ternary.params.push(AbiParam::new(types::I64));
        ternary.params.push(AbiParam::new(types::I64));
        ternary.params.push(AbiParam::new(types::I64));
        ternary.returns.push(AbiParam::new(types::I64));

    }
    signal: "jet_jit_reactive_signal" => jet_jit_reactive_signal: unary;
    signal_aot: "jet_std::JetSignal::new" => jet_jit_reactive_signal: unary;
    get: "jet_jit_reactive_get" => jet_jit_reactive_get: unary;
    get_aot: "jet_std::JetSignal::get" => jet_jit_reactive_get: unary;
    set: "jet_jit_reactive_set" => jet_jit_reactive_set: binary_void;
    set_aot: "jet_std::JetSignal::set" => jet_jit_reactive_set: binary_void;
    derived: "jet_jit_reactive_derived" => jet_jit_reactive_derived: cb6;
    derived_aot: "jet_std::JetDerived::new" => jet_jit_reactive_derived: cb6;
    derived_get: "jet_jit_reactive_derived_get" => jet_jit_reactive_derived_get: unary;
    derived_get_aot: "jet_std::JetDerived::get" => jet_jit_reactive_derived_get: unary;
    effect: "jet_jit_reactive_effect" => jet_jit_reactive_effect: cb6;
    effect_rooted: "jet_jit_reactive_effect_rooted" => jet_jit_reactive_effect_rooted: cb6_void;
    loadable_idle: "jet_jit_loadable_idle" => jet_jit_loadable_idle: nullary;
    loadable_loading: "jet_jit_loadable_loading" => jet_jit_loadable_loading: nullary;
    loadable_loaded: "jet_jit_loadable_loaded" => jet_jit_loadable_loaded: unary;
    loadable_failed: "jet_jit_loadable_failed" => jet_jit_loadable_failed: unary;
    loadable_idle_aot: "jet_loadable_idle" => jet_jit_loadable_idle: nullary;
    loadable_loading_aot: "jet_loadable_loading" => jet_jit_loadable_loading: nullary;
    loadable_loaded_aot: "jet_loadable_loaded" => jet_jit_loadable_loaded: unary;
    loadable_failed_aot: "jet_loadable_failed" => jet_jit_loadable_failed: unary;
    loadable_is_idle_method: "JetLoadable::is_idle" => jet_jit_loadable_is_idle: unary_i8;
    loadable_is_loading_method: "JetLoadable::is_loading" => jet_jit_loadable_is_loading: unary_i8;
    loadable_is_loaded_method: "JetLoadable::is_loaded" => jet_jit_loadable_is_loaded: unary_i8;
    loadable_is_failed_method: "JetLoadable::is_failed" => jet_jit_loadable_is_failed: unary_i8;
    loadable_loaded_method: "JetLoadable::loaded" => jet_jit_loadable_loaded_value: unary;
    loadable_or_else_method: "JetLoadable::or_else" => jet_jit_loadable_or_else: binary;
    loadable_is: "jet_jit_loadable_is" => jet_jit_loadable_is: binary_i8;
    loadable_payload: "jet_jit_loadable_payload" => jet_jit_loadable_payload: unary;
    loadable_or_else: "jet_jit_loadable_or_else" => jet_jit_loadable_or_else: binary;
    event_scope: "jet_jit_event_scope" => jet_jit_event_scope: nullary;
    event_scope_aot: "jet_std::JetEventScope::new" => jet_jit_event_scope: nullary;
    event_policy_sync: "jet_jit_event_policy_sync" => jet_jit_event_policy_sync: nullary;
    event_policy_sync_aot: "jet_std::JetEventPolicy::sync" => jet_jit_event_policy_sync: nullary;
    event_with_policy: "jet_jit_event_with_policy" => jet_jit_event_with_policy: unary;
    event_with_policy_aot: "jet_std::JetEvent::with_policy" => jet_jit_event_with_policy: unary;
    event_new: "jet_jit_event_new" => jet_jit_event_new: nullary;
    event_new_aot: "jet_std::JetEvent::new" => jet_jit_event_new: nullary;
    event_on: "jet_jit_event_on" => jet_jit_event_on: listener;
    event_on_aot: "jet_std::JetEvent::on" => jet_jit_event_on: listener;
    event_once: "jet_jit_event_once" => jet_jit_event_once: listener;
    event_once_aot: "jet_std::JetEvent::once" => jet_jit_event_once: listener;
    event_on_priority: "jet_jit_event_on_priority" => jet_jit_event_on_priority: listener_priority;
    event_on_priority_aot: "jet_std::JetEvent::on_priority" => jet_jit_event_on_priority: listener_priority;
    event_emit: "jet_jit_event_emit" => jet_jit_event_emit: binary;
    event_emit_aot: "jet_std::JetEvent::emit" => jet_jit_event_emit: binary;
    event_listener_count: "jet_jit_event_listener_count" => jet_jit_event_listener_count: unary;
    event_listener_count_aot: "jet_std::JetEvent::listener_count" => jet_jit_event_listener_count: unary;
    event_trace: "jet_jit_event_trace" => jet_jit_event_trace: unary;
    event_trace_aot: "jet_std::JetEvent::trace" => jet_jit_event_trace: unary;
    event_trace_summary: "jet_jit_event_trace_summary" => jet_jit_event_trace_summary: unary;
    event_trace_summary_aot: "jet_std::JetEventTrace::summary" => jet_jit_event_trace_summary: unary;
    event_trace_delivered: "jet_jit_event_trace_delivered" => jet_jit_event_trace_delivered: unary;
    event_trace_delivered_aot: "jet_std::JetEventTrace::delivered" => jet_jit_event_trace_delivered: unary;
    event_trace_queued: "jet_jit_event_trace_queued" => jet_jit_event_trace_queued: unary;
    event_trace_queued_aot: "jet_std::JetEventTrace::queued" => jet_jit_event_trace_queued: unary;
    event_trace_dropped: "jet_jit_event_trace_dropped" => jet_jit_event_trace_dropped: unary;
    event_trace_dropped_aot: "jet_std::JetEventTrace::dropped" => jet_jit_event_trace_dropped: unary;
    event_scope_active: "jet_jit_event_scope_active" => jet_jit_event_scope_active: unary;
    event_scope_active_aot: "jet_std::JetEventScope::active_count" => jet_jit_event_scope_active: unary;
    event_scope_cancel: "jet_jit_event_scope_cancel" => jet_jit_event_scope_cancel: unary_void;
    event_scope_cancel_aot: "jet_std::JetEventScope::cancel" => jet_jit_event_scope_cancel: unary_void;
    subscription_unsubscribe: "jet_jit_subscription_unsubscribe" => jet_jit_subscription_unsubscribe: unary_void;
    subscription_unsubscribe_aot: "jet_std::JetSubscription::unsubscribe" => jet_jit_subscription_unsubscribe: unary_void;
    subscription_active: "jet_jit_subscription_active" => jet_jit_subscription_active: unary;
    subscription_active_aot: "jet_std::JetSubscription::active" => jet_jit_subscription_active: unary;
    hook_new: "jet_jit_hook_new" => jet_jit_hook_new: unary;
    hook_new_aot: "jet_std::JetHook::new" => jet_jit_hook_new: unary;
    hook_on: "jet_jit_hook_on" => jet_jit_hook_on: listener;
    hook_on_aot: "jet_std::JetHook::on" => jet_jit_hook_on: listener;
    hook_once: "jet_jit_hook_once" => jet_jit_hook_once: listener;
    hook_once_aot: "jet_std::JetHook::once" => jet_jit_hook_once: listener;
    hook_on_priority: "jet_jit_hook_on_priority" => jet_jit_hook_on_priority: listener_priority;
    hook_on_priority_aot: "jet_std::JetHook::on_priority" => jet_jit_hook_on_priority: listener_priority;
    hook_run: "jet_jit_hook_run" => jet_jit_hook_run: ternary;
    hook_run_aot: "jet_std::JetHook::run" => jet_jit_hook_run: ternary;
    hook_listener_count: "jet_jit_hook_listener_count" => jet_jit_hook_listener_count: unary;
    hook_listener_count_aot: "jet_std::JetHook::listener_count" => jet_jit_hook_listener_count: unary;
    hook_trace: "jet_jit_hook_trace" => jet_jit_hook_trace: unary;
    hook_trace_aot: "jet_std::JetHook::trace" => jet_jit_hook_trace: unary;
    decision_hook_new: "jet_jit_decision_hook_new" => jet_jit_decision_hook_new: unary;
    decision_hook_new_aot: "jet_std::JetDecisionHook::new" => jet_jit_decision_hook_new: unary;
    decision_hook_on: "jet_jit_decision_hook_on" => jet_jit_decision_hook_on: listener;
    decision_hook_on_aot: "jet_std::JetDecisionHook::on" => jet_jit_decision_hook_on: listener;
    decision_hook_once: "jet_jit_decision_hook_once" => jet_jit_decision_hook_once: listener;
    decision_hook_once_aot: "jet_std::JetDecisionHook::once" => jet_jit_decision_hook_once: listener;
    decision_hook_on_priority: "jet_jit_decision_hook_on_priority" => jet_jit_decision_hook_on_priority: listener_priority;
    decision_hook_on_priority_aot: "jet_std::JetDecisionHook::on_priority" => jet_jit_decision_hook_on_priority: listener_priority;
    decision_hook_run: "jet_jit_decision_hook_run" => jet_jit_decision_hook_run: unary;
    decision_hook_run_aot: "jet_std::JetDecisionHook::run" => jet_jit_decision_hook_run: unary;
    decision_hook_listener_count: "jet_jit_decision_hook_listener_count" => jet_jit_decision_hook_listener_count: unary;
    decision_hook_listener_count_aot: "jet_std::JetDecisionHook::listener_count" => jet_jit_decision_hook_listener_count: unary;
    async_event_new: "jet_jit_async_event_new" => jet_jit_async_event_new: binary;
    async_event_new_aot: "jet_std::JetAsyncEvent::new" => jet_jit_async_event_new: binary;
    async_event_on: "jet_jit_async_event_on" => jet_jit_async_event_on: listener;
    async_event_on_aot: "jet_std::JetAsyncEvent::on" => jet_jit_async_event_on: listener;
    async_event_once: "jet_jit_async_event_once" => jet_jit_async_event_once: listener;
    async_event_once_aot: "jet_std::JetAsyncEvent::once" => jet_jit_async_event_once: listener;
    async_event_on_priority: "jet_jit_async_event_on_priority" => jet_jit_async_event_on_priority: listener_priority;
    async_event_on_priority_aot: "jet_std::JetAsyncEvent::on_priority" => jet_jit_async_event_on_priority: listener_priority;
    async_event_emit: "jet_jit_async_event_emit" => jet_jit_async_event_emit: binary;
    async_event_emit_aot: "jet_std::JetAsyncEvent::emit_async" => jet_jit_async_event_emit: binary;
    async_event_close: "jet_jit_async_event_close" => jet_jit_async_event_close: unary_void;
    async_event_close_aot: "jet_std::JetAsyncEvent::close" => jet_jit_async_event_close: unary_void;
    async_event_listener_count: "jet_jit_async_event_listener_count" => jet_jit_async_event_listener_count: unary;
    async_event_listener_count_aot: "jet_std::JetAsyncEvent::listener_count" => jet_jit_async_event_listener_count: unary;
    async_event_queued_count: "jet_jit_async_event_queued_count" => jet_jit_async_event_queued_count: unary;
    async_event_queued_count_aot: "jet_std::JetAsyncEvent::queued_count" => jet_jit_async_event_queued_count: unary;
    async_event_running_count: "jet_jit_async_event_running_count" => jet_jit_async_event_running_count: unary;
    async_event_running_count_aot: "jet_std::JetAsyncEvent::running_count" => jet_jit_async_event_running_count: unary;
    async_event_blocked_count: "jet_jit_async_event_blocked_count" => jet_jit_async_event_blocked_count: unary;
    async_event_blocked_count_aot: "jet_std::JetAsyncEvent::blocked_count" => jet_jit_async_event_blocked_count: unary;
    dispatch_report_accepted: "jet_jit_dispatch_report_accepted" => jet_jit_dispatch_report_accepted: unary;
    dispatch_report_accepted_aot: "jet_std::JetDispatchReport::accepted" => jet_jit_dispatch_report_accepted: unary;
    dispatch_report_state: "jet_jit_dispatch_report_state" => jet_jit_dispatch_report_state: unary;
    dispatch_report_state_aot: "jet_std::JetDispatchReport::state" => jet_jit_dispatch_report_state: unary;
    dispatch_report_handlers: "jet_jit_dispatch_report_handlers" => jet_jit_dispatch_report_handlers: unary;
    dispatch_report_handlers_aot: "jet_std::JetDispatchReport::delivered_handlers" => jet_jit_dispatch_report_handlers: unary;
    dispatch_report_failures: "jet_jit_dispatch_report_failures" => jet_jit_dispatch_report_failures: unary;
    dispatch_report_failures_aot: "jet_std::JetDispatchReport::failures" => jet_jit_dispatch_report_failures: unary;
    dispatch_report_trace: "jet_jit_dispatch_report_trace" => jet_jit_dispatch_report_trace: unary;
    dispatch_report_trace_aot: "jet_std::JetDispatchReport::trace" => jet_jit_dispatch_report_trace: unary;
    app_live: "jet_jit_app_live" => jet_jit_app_live: binary;
    app_subscribe: "jet_jit_app_subscribe" => jet_jit_app_subscribe: unary;
    app_invalidate: "jet_jit_app_invalidate" => jet_jit_app_invalidate: unary;
    app_transact_invalidate: "jet_jit_app_transact_invalidate" => jet_jit_app_transact_invalidate: unary;
    app_signal_push: "jet_jit_app_signal_push" => jet_jit_app_signal_push: binary;
    app_live_get: "jet_jit_app_live_get" => jet_jit_app_live_get: unary;
    app_live_show: "jet_jit_app_live_show" => jet_jit_app_live_show: unary;
    app_live_stats: "jet_jit_app_live_stats" => jet_jit_app_live_stats: nullary;
}

#[allow(dead_code)]
fn _arc_keepalive() {
    let _: Arc<()> = Arc::new(());
}
