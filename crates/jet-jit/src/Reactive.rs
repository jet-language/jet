//! D-REACT1 / D-EVENT1 / D-PENDING1: resident-JIT reactive + event host.
//! Signal/derived/effect use the canonical observer algorithm via `include!`
//! of the reactive core extracted from ReactiveEventWatch.rs (build.rs).
//! Opaque i64 handles + JIT fn-ptr adapters — no third reactive graph.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::sync::Arc;

/// Canonical reactive core (JetSignal / JetDerived / jet_reactive_effect*).
#[allow(dead_code, unused_imports)]
pub(crate) mod reactive_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/reactive_rt.rs"));
}

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

#[derive(Default)]
pub(crate) struct AsyncEventSlot {
    pub(crate) listeners: Vec<JitCb>,
    pub(crate) closed: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DispatchReportSlot {
    pub(crate) delivered: bool,
    pub(crate) handlers: i64,
}

#[derive(Default)]
pub(crate) struct ReactiveState {
    pub(crate) signals: Vec<reactive_rt::JetSignal<i64>>,
    pub(crate) deriveds: Vec<reactive_rt::JetDerived<i64>>,
    pub(crate) effects: Vec<reactive_rt::JetReactiveEffect>,
    pub(crate) event_scopes: Vec<reactive_rt::JetEventScope>,
    pub(crate) events: Vec<reactive_rt::JetEvent<i64>>,
    pub(crate) subscriptions: Vec<reactive_rt::JetSubscription>,
    pub(crate) hooks: Vec<reactive_rt::JetHook<i64, String>>,
    pub(crate) decision_hooks: Vec<reactive_rt::JetDecisionHook<i64, String>>,
    pub(crate) event_traces: Vec<reactive_rt::JetEventTrace>,
    pub(crate) async_events: Vec<AsyncEventSlot>,
    pub(crate) dispatch_reports: Vec<DispatchReportSlot>,
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::runtime_host::JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

extern "C" fn jet_jit_reactive_signal(init: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .signals
            .push(reactive_rt::JetSignal::new(init));
        rt.reactive.signals.len() as i64
    })
}

extern "C" fn jet_jit_reactive_get(handle: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .signals
            .get(handle.saturating_sub(1) as usize)
            .expect("jit reactive get: bad signal")
            .get()
    })
}

extern "C" fn jet_jit_reactive_set(handle: i64, value: i64) {
    with_rt(|rt| {
        rt.reactive
            .signals
            .get(handle.saturating_sub(1) as usize)
            .expect("jit reactive set: bad signal")
            .set(value);
    });
}

extern "C" fn jet_jit_reactive_derived(
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let derived = reactive_rt::JetDerived::new(move || cb.invoke_i64());
        rt.reactive.deriveds.push(derived);
        rt.reactive.deriveds.len() as i64
    })
}

extern "C" fn jet_jit_reactive_derived_get(handle: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .deriveds
            .get(handle.saturating_sub(1) as usize)
            .expect("jit reactive derived get: bad handle")
            .get()
    })
}

extern "C" fn jet_jit_reactive_effect(
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
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

extern "C" fn jet_jit_reactive_effect_rooted(
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    reactive_rt::jet_reactive_effect_rooted(move || cb.invoke_void());
}

extern "C" fn jet_jit_loadable_idle() -> i64 {
    // disc=Idle(0), no payload — packed enum ABI
    0
}
extern "C" fn jet_jit_loadable_loading() -> i64 {
    1
}
extern "C" fn jet_jit_loadable_loaded(payload: i64) -> i64 {
    (payload << 8) | 2
}
extern "C" fn jet_jit_loadable_failed(payload: i64) -> i64 {
    (payload << 8) | 3
}

extern "C" fn jet_jit_loadable_is(handle: i64, kind: i64) -> i8 {
    if (handle & 0xff) == kind {
        1
    } else {
        0
    }
}

extern "C" fn jet_jit_loadable_payload(handle: i64) -> i64 {
    handle >> 8
}

extern "C" fn jet_jit_loadable_or_else(handle: i64, default: i64) -> i64 {
    if (handle & 0xff) == 2 {
        handle >> 8
    } else {
        default
    }
}

// ── Events (thin adapters over canonical JetEvent*) ──────────────────────────

extern "C" fn jet_jit_event_scope() -> i64 {
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

extern "C" fn jet_jit_event_new() -> i64 {
    with_rt(|rt| {
        rt.reactive.events.push(reactive_rt::JetEvent::new());
        rt.reactive.events.len() as i64
    })
}

extern "C" fn jet_jit_event_on(
    event: i64,
    scope: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
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
        let sub = event.on(&scope, move |n| {
            let mut caps = cb.caps;
            let mut n_caps = cb.n_caps;
            if n_caps < 4 {
                caps[n_caps as usize] = n;
                n_caps += 1;
            }
            JitCb {
                fn_ptr: cb.fn_ptr,
                caps,
                n_caps,
            }
            .invoke_void();
        });
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

extern "C" fn jet_jit_event_once(
    event: i64,
    scope: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
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
        let sub = event.once(&scope, move |n| {
            let mut caps = cb.caps;
            let mut n_caps = cb.n_caps;
            if n_caps < 4 {
                caps[n_caps as usize] = n;
                n_caps += 1;
            }
            JitCb {
                fn_ptr: cb.fn_ptr,
                caps,
                n_caps,
            }
            .invoke_void();
        });
        rt.reactive.subscriptions.push(sub);
        rt.reactive.subscriptions.len() as i64
    })
}

extern "C" fn jet_jit_event_emit(event: i64, payload: i64) -> i64 {
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

extern "C" fn jet_jit_event_trace_summary(trace: i64) -> i64 {
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

extern "C" fn jet_jit_event_scope_active(scope: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .expect("jit event scope: bad handle")
            .active_count()
    })
}

extern "C" fn jet_jit_event_scope_cancel(scope: i64) {
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

extern "C" fn jet_jit_subscription_unsubscribe(sub: i64) {
    with_rt(|rt| {
        rt.reactive
            .subscriptions
            .get(sub.saturating_sub(1) as usize)
            .expect("jit subscription: bad handle")
            .unsubscribe();
    });
}

extern "C" fn jet_jit_hook_new(name: i64) -> i64 {
    with_rt(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        rt.reactive.hooks.push(reactive_rt::JetHook::new(name));
        rt.reactive.hooks.len() as i64
    })
}

extern "C" fn jet_jit_hook_on_priority(
    hook: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let Some(hook) = rt.reactive.hooks.get(hook.saturating_sub(1) as usize).cloned() else {
            return;
        };
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return;
        };
        hook.on_priority(&scope, priority, move |n| {
            let mut caps = cb.caps;
            let mut n_caps = cb.n_caps;
            if n_caps < 4 {
                caps[n_caps as usize] = n;
                n_caps += 1;
            }
            let sid = JitCb {
                fn_ptr: cb.fn_ptr,
                caps,
                n_caps,
            }
            .invoke_i64();
            Concurrency::with_runtime_mut(|rt| {
                rt.heap.clone_string(sid).unwrap_or_else(|| format!("seen {n}"))
            })
        });
    });
}

extern "C" fn jet_jit_hook_run(hook: i64, payload: i64, fallback: i64) -> i64 {
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

extern "C" fn jet_jit_decision_hook_new(policy: i64) -> i64 {
    let _ = policy; // HookPolicy.FirstCancelElseTransform — default for example
    with_rt(|rt| {
        rt.reactive.decision_hooks.push(reactive_rt::JetDecisionHook::new(
            reactive_rt::JetHookPolicy::FirstCancelElseTransform,
        ));
        rt.reactive.decision_hooks.len() as i64
    })
}

extern "C" fn jet_jit_decision_hook_on(
    hook: i64,
    scope: i64,
    priority: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
    has_priority: i8,
) {
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        let Some(scope) = rt
            .reactive
            .event_scopes
            .get(scope.saturating_sub(1) as usize)
            .cloned()
        else {
            return;
        };
        let Some(hook) = rt
            .reactive
            .decision_hooks
            .get(hook.saturating_sub(1) as usize)
            .cloned()
        else {
            return;
        };
        let handler = move |n: i64| {
            let mut caps = cb.caps;
            let mut n_caps = cb.n_caps;
            if n_caps < 4 {
                caps[n_caps as usize] = n;
                n_caps += 1;
            }
            let packed = JitCb {
                fn_ptr: cb.fn_ptr,
                caps,
                n_caps,
            }
            .invoke_i64();
            let disc = packed & 0xff;
            let payload = packed >> 8;
            match disc {
                1 => reactive_rt::JetHookDecision::Transform(payload),
                2 => reactive_rt::JetHookDecision::Cancel,
                3 => {
                    let msg = Concurrency::with_runtime_mut(|rt| {
                        rt.heap.clone_string(payload).unwrap_or_default()
                    });
                    reactive_rt::JetHookDecision::Fail(msg)
                }
                _ => reactive_rt::JetHookDecision::Continue,
            }
        };
        if has_priority != 0 {
            hook.on_priority(&scope, priority, handler);
        } else {
            hook.on(&scope, handler);
        }
    });
}

extern "C" fn jet_jit_decision_hook_run(hook: i64, payload: i64) -> i64 {
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

extern "C" fn jet_jit_hook_decision_continue() -> i64 {
    0
}
extern "C" fn jet_jit_hook_decision_transform(v: i64) -> i64 {
    (v << 8) | 1
}
extern "C" fn jet_jit_hook_decision_cancel() -> i64 {
    2
}
extern "C" fn jet_jit_hook_decision_fail(msg: i64) -> i64 {
    (msg << 8) | 3
}

// ── Async event thin host (same golden as JetAsyncEvent sync path) ───────────

extern "C" fn jet_jit_async_event_new(_capacity: i64, _overflow: i64, _failure: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive.async_events.push(AsyncEventSlot::default());
        // High bit tags async handles so EventMethod.on can dispatch correctly.
        (rt.reactive.async_events.len() as i64) | (1 << 62)
    })
}

extern "C" fn jet_jit_async_event_on(
    event: i64,
    _scope: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    let idx = (event & !(1 << 62)).saturating_sub(1) as usize;
    let cb = JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    with_rt(|rt| {
        if let Some(slot) = rt.reactive.async_events.get_mut(idx) {
            slot.listeners.push(cb);
        }
    });
}

extern "C" fn jet_jit_async_event_emit(event: i64, payload: i64) -> i64 {
    let idx = (event & !(1 << 62)).saturating_sub(1) as usize;
    with_rt(|rt| {
        let Some(slot) = rt.reactive.async_events.get_mut(idx) else {
            return 0;
        };
        if slot.closed {
            rt.reactive.dispatch_reports.push(DispatchReportSlot {
                delivered: false,
                handlers: 0,
            });
            return rt.reactive.dispatch_reports.len() as i64;
        }
        let listeners = slot.listeners.clone();
        let handlers = listeners.len() as i64;
        for cb in listeners {
            let mut caps = cb.caps;
            let mut n_caps = cb.n_caps;
            if n_caps < 4 {
                caps[n_caps as usize] = payload;
                n_caps += 1;
            }
            JitCb {
                fn_ptr: cb.fn_ptr,
                caps,
                n_caps,
            }
            .invoke_void();
        }
        rt.reactive.dispatch_reports.push(DispatchReportSlot {
            delivered: true,
            handlers,
        });
        rt.reactive.dispatch_reports.len() as i64
    })
}

extern "C" fn jet_jit_async_event_close(event: i64) {
    let idx = (event & !(1 << 62)).saturating_sub(1) as usize;
    with_rt(|rt| {
        if let Some(slot) = rt.reactive.async_events.get_mut(idx) {
            slot.closed = true;
        }
    });
}

extern "C" fn jet_jit_async_event_join(task: i64) -> i64 {
    task
}

extern "C" fn jet_jit_dispatch_report_state(report: i64) -> i64 {
    // 0 = Delivered for `state() == .Delivered` comparison in examples.
    with_rt(|rt| {
        let ok = rt
            .reactive
            .dispatch_reports
            .get(report.saturating_sub(1) as usize)
            .map(|r| r.delivered)
            .unwrap_or(false);
        if ok {
            0
        } else {
            1
        }
    })
}

extern "C" fn jet_jit_dispatch_report_handlers(report: i64) -> i64 {
    with_rt(|rt| {
        rt.reactive
            .dispatch_reports
            .get(report.saturating_sub(1) as usize)
            .map(|r| r.handlers)
            .unwrap_or(0)
    })
}

pub(crate) struct ReactiveHostFns {
    pub(crate) signal: FuncId,
    pub(crate) get: FuncId,
    pub(crate) set: FuncId,
    pub(crate) derived: FuncId,
    pub(crate) derived_get: FuncId,
    pub(crate) effect: FuncId,
    pub(crate) effect_rooted: FuncId,
    pub(crate) loadable_idle: FuncId,
    pub(crate) loadable_loading: FuncId,
    pub(crate) loadable_loaded: FuncId,
    pub(crate) loadable_failed: FuncId,
    pub(crate) loadable_is: FuncId,
    pub(crate) loadable_payload: FuncId,
    pub(crate) loadable_or_else: FuncId,
    pub(crate) event_scope: FuncId,
    pub(crate) event_new: FuncId,
    pub(crate) event_on: FuncId,
    pub(crate) event_once: FuncId,
    pub(crate) event_emit: FuncId,
    pub(crate) event_trace_summary: FuncId,
    pub(crate) event_scope_active: FuncId,
    pub(crate) event_scope_cancel: FuncId,
    pub(crate) subscription_unsubscribe: FuncId,
    pub(crate) hook_new: FuncId,
    pub(crate) hook_on_priority: FuncId,
    pub(crate) hook_run: FuncId,
    pub(crate) decision_hook_new: FuncId,
    pub(crate) decision_hook_on: FuncId,
    pub(crate) decision_hook_run: FuncId,
    pub(crate) hook_decision_continue: FuncId,
    pub(crate) hook_decision_transform: FuncId,
    pub(crate) hook_decision_cancel: FuncId,
    pub(crate) hook_decision_fail: FuncId,
    pub(crate) async_event_new: FuncId,
    pub(crate) async_event_on: FuncId,
    pub(crate) async_event_emit: FuncId,
    pub(crate) async_event_join: FuncId,
    pub(crate) async_event_close: FuncId,
    pub(crate) dispatch_report_state: FuncId,
    pub(crate) dispatch_report_handlers: FuncId,
}

pub(crate) fn register_reactive_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_reactive_signal", jet_jit_reactive_signal as *const u8);
    builder.symbol("jet_jit_reactive_get", jet_jit_reactive_get as *const u8);
    builder.symbol("jet_jit_reactive_set", jet_jit_reactive_set as *const u8);
    builder.symbol("jet_jit_reactive_derived", jet_jit_reactive_derived as *const u8);
    builder.symbol(
        "jet_jit_reactive_derived_get",
        jet_jit_reactive_derived_get as *const u8,
    );
    builder.symbol("jet_jit_reactive_effect", jet_jit_reactive_effect as *const u8);
    builder.symbol(
        "jet_jit_reactive_effect_rooted",
        jet_jit_reactive_effect_rooted as *const u8,
    );
    builder.symbol("jet_jit_loadable_idle", jet_jit_loadable_idle as *const u8);
    builder.symbol("jet_jit_loadable_loading", jet_jit_loadable_loading as *const u8);
    builder.symbol("jet_jit_loadable_loaded", jet_jit_loadable_loaded as *const u8);
    builder.symbol("jet_jit_loadable_failed", jet_jit_loadable_failed as *const u8);
    builder.symbol("jet_jit_loadable_is", jet_jit_loadable_is as *const u8);
    builder.symbol("jet_jit_loadable_payload", jet_jit_loadable_payload as *const u8);
    builder.symbol("jet_jit_loadable_or_else", jet_jit_loadable_or_else as *const u8);
    builder.symbol("jet_jit_event_scope", jet_jit_event_scope as *const u8);
    builder.symbol("jet_jit_event_new", jet_jit_event_new as *const u8);
    builder.symbol("jet_jit_event_on", jet_jit_event_on as *const u8);
    builder.symbol("jet_jit_event_once", jet_jit_event_once as *const u8);
    builder.symbol("jet_jit_event_emit", jet_jit_event_emit as *const u8);
    builder.symbol(
        "jet_jit_event_trace_summary",
        jet_jit_event_trace_summary as *const u8,
    );
    builder.symbol("jet_jit_event_scope_active", jet_jit_event_scope_active as *const u8);
    builder.symbol("jet_jit_event_scope_cancel", jet_jit_event_scope_cancel as *const u8);
    builder.symbol(
        "jet_jit_subscription_unsubscribe",
        jet_jit_subscription_unsubscribe as *const u8,
    );
    builder.symbol("jet_jit_hook_new", jet_jit_hook_new as *const u8);
    builder.symbol("jet_jit_hook_on_priority", jet_jit_hook_on_priority as *const u8);
    builder.symbol("jet_jit_hook_run", jet_jit_hook_run as *const u8);
    builder.symbol("jet_jit_decision_hook_new", jet_jit_decision_hook_new as *const u8);
    builder.symbol("jet_jit_decision_hook_on", jet_jit_decision_hook_on as *const u8);
    builder.symbol("jet_jit_decision_hook_run", jet_jit_decision_hook_run as *const u8);
    builder.symbol(
        "jet_jit_hook_decision_continue",
        jet_jit_hook_decision_continue as *const u8,
    );
    builder.symbol(
        "jet_jit_hook_decision_transform",
        jet_jit_hook_decision_transform as *const u8,
    );
    builder.symbol(
        "jet_jit_hook_decision_cancel",
        jet_jit_hook_decision_cancel as *const u8,
    );
    builder.symbol("jet_jit_hook_decision_fail", jet_jit_hook_decision_fail as *const u8);
    builder.symbol("jet_jit_async_event_new", jet_jit_async_event_new as *const u8);
    builder.symbol("jet_jit_async_event_on", jet_jit_async_event_on as *const u8);
    builder.symbol("jet_jit_async_event_emit", jet_jit_async_event_emit as *const u8);
    builder.symbol("jet_jit_async_event_join", jet_jit_async_event_join as *const u8);
    builder.symbol("jet_jit_async_event_close", jet_jit_async_event_close as *const u8);
    builder.symbol(
        "jet_jit_dispatch_report_state",
        jet_jit_dispatch_report_state as *const u8,
    );
    builder.symbol(
        "jet_jit_dispatch_report_handlers",
        jet_jit_dispatch_report_handlers as *const u8,
    );
}

pub(crate) fn declare_reactive_host_fns(
    module: &mut JITModule,
) -> Result<ReactiveHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    let mut nullary = Signature::new(cc);
    nullary.returns.push(AbiParam::new(types::I64));
    let mut unary = Signature::new(cc);
    unary.params.push(AbiParam::new(types::I64));
    unary.returns.push(AbiParam::new(types::I64));
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
    let mut event_on = Signature::new(cc);
    for _ in 0..8 {
        event_on.params.push(AbiParam::new(types::I64));
    }
    event_on.returns.push(AbiParam::new(types::I64));
    let mut hook_pri = Signature::new(cc);
    for _ in 0..9 {
        hook_pri.params.push(AbiParam::new(types::I64));
    }
    let mut decision_on = Signature::new(cc);
    for _ in 0..9 {
        decision_on.params.push(AbiParam::new(types::I64));
    }
    decision_on.params.push(AbiParam::new(types::I8));
    let mut ternary = Signature::new(cc);
    ternary.params.push(AbiParam::new(types::I64));
    ternary.params.push(AbiParam::new(types::I64));
    ternary.params.push(AbiParam::new(types::I64));
    ternary.returns.push(AbiParam::new(types::I64));

    Ok(ReactiveHostFns {
        signal: import("jet_jit_reactive_signal", &unary)?,
        get: import("jet_jit_reactive_get", &unary)?,
        set: import("jet_jit_reactive_set", &binary_void)?,
        derived: import("jet_jit_reactive_derived", &cb6)?,
        derived_get: import("jet_jit_reactive_derived_get", &unary)?,
        effect: import("jet_jit_reactive_effect", &cb6)?,
        effect_rooted: import("jet_jit_reactive_effect_rooted", &cb6_void)?,
        loadable_idle: import("jet_jit_loadable_idle", &nullary)?,
        loadable_loading: import("jet_jit_loadable_loading", &nullary)?,
        loadable_loaded: import("jet_jit_loadable_loaded", &unary)?,
        loadable_failed: import("jet_jit_loadable_failed", &unary)?,
        loadable_is: import("jet_jit_loadable_is", &binary_i8)?,
        loadable_payload: import("jet_jit_loadable_payload", &unary)?,
        loadable_or_else: import("jet_jit_loadable_or_else", &binary)?,
        event_scope: import("jet_jit_event_scope", &nullary)?,
        event_new: import("jet_jit_event_new", &nullary)?,
        event_on: import("jet_jit_event_on", &event_on)?,
        event_once: import("jet_jit_event_once", &event_on)?,
        event_emit: import("jet_jit_event_emit", &binary)?,
        event_trace_summary: import("jet_jit_event_trace_summary", &unary)?,
        event_scope_active: import("jet_jit_event_scope_active", &unary)?,
        event_scope_cancel: import("jet_jit_event_scope_cancel", &unary_void)?,
        subscription_unsubscribe: import("jet_jit_subscription_unsubscribe", &unary_void)?,
        hook_new: import("jet_jit_hook_new", &unary)?,
        hook_on_priority: import("jet_jit_hook_on_priority", &hook_pri)?,
        hook_run: import("jet_jit_hook_run", &ternary)?,
        decision_hook_new: import("jet_jit_decision_hook_new", &unary)?,
        decision_hook_on: import("jet_jit_decision_hook_on", &decision_on)?,
        decision_hook_run: import("jet_jit_decision_hook_run", &binary)?,
        hook_decision_continue: import("jet_jit_hook_decision_continue", &nullary)?,
        hook_decision_transform: import("jet_jit_hook_decision_transform", &unary)?,
        hook_decision_cancel: import("jet_jit_hook_decision_cancel", &nullary)?,
        hook_decision_fail: import("jet_jit_hook_decision_fail", &unary)?,
        async_event_new: import("jet_jit_async_event_new", &ternary)?,
        async_event_on: import("jet_jit_async_event_on", &event_on)?,
        async_event_emit: import("jet_jit_async_event_emit", &binary)?,
        async_event_join: import("jet_jit_async_event_join", &unary)?,
        async_event_close: import("jet_jit_async_event_close", &unary_void)?,
        dispatch_report_state: import("jet_jit_dispatch_report_state", &unary)?,
        dispatch_report_handlers: import("jet_jit_dispatch_report_handlers", &unary)?,
    })
}

#[allow(dead_code)]
fn _arc_keepalive() {
    let _: Arc<()> = Arc::new(());
}
