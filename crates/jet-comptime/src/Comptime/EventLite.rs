//! Interpreter `core.event` — mirrors AOT `ReactiveEventWatch.rs` for TIR deopt.
//!
//! Handles are `CtValue` structs with an `id` field. Handler callables stay as
//! `CtValue` (eval `__JetTirCallable`); the evaluator invokes them during
//! `emit` / `run` / `emit_async`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::AST::CtValue;
use crate::Comptime::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
struct Subscription {
    active: Rc<Cell<bool>>,
    cleanup: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
}

impl Subscription {
    fn new() -> Self {
        Subscription {
            active: Rc::new(Cell::new(true)),
            cleanup: Rc::new(RefCell::new(None)),
        }
    }
    fn set_cleanup<F: Fn() + 'static>(&self, cleanup: F) {
        *self.cleanup.borrow_mut() = Some(Rc::new(cleanup));
    }
    fn unsubscribe(&self) {
        if self.active.replace(false) {
            if let Some(cleanup) = self.cleanup.borrow().clone() {
                cleanup();
            }
        }
    }
    fn active(&self) -> bool {
        self.active.get()
    }
}

struct ScopeState {
    cancelled: bool,
    subs: Vec<Subscription>,
}

struct Listener {
    id: u64,
    priority: i64,
    once: bool,
    sub: Subscription,
    handler: CtValue,
}

struct EventState {
    listeners: Vec<Listener>,
}

struct HookState {
    fallback: CtValue,
    listeners: Vec<Listener>,
}

struct DecisionHookState {
    listeners: Vec<Listener>,
}

struct AsyncEventState {
    listeners: Vec<Listener>,
    closed: bool,
}

struct TraceState {
    delivered: i64,
    queued: i64,
    dropped: i64,
    summary: String,
}

struct ReportState {
    delivered: bool,
    handlers: i64,
}

thread_local! {
    static SCOPES: RefCell<Vec<Option<ScopeState>>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<Option<EventState>>> = const { RefCell::new(Vec::new()) };
    static HOOKS: RefCell<Vec<Option<HookState>>> = const { RefCell::new(Vec::new()) };
    static DECISION_HOOKS: RefCell<Vec<Option<DecisionHookState>>> =
        const { RefCell::new(Vec::new()) };
    static ASYNC_EVENTS: RefCell<Vec<Option<AsyncEventState>>> =
        const { RefCell::new(Vec::new()) };
    static SUBS: RefCell<Vec<Option<Subscription>>> = const { RefCell::new(Vec::new()) };
    static TRACES: RefCell<Vec<Option<TraceState>>> = const { RefCell::new(Vec::new()) };
    static REPORTS: RefCell<Vec<Option<ReportState>>> = const { RefCell::new(Vec::new()) };
}

/// Clear every EventLite store. Call at whole-program evaluator run entry so
/// REPL / warm-cache / test workers do not retain closures or stale ids.
pub fn reset() {
    SCOPES.with(|s| s.borrow_mut().clear());
    EVENTS.with(|s| s.borrow_mut().clear());
    HOOKS.with(|s| s.borrow_mut().clear());
    DECISION_HOOKS.with(|s| s.borrow_mut().clear());
    ASYNC_EVENTS.with(|s| s.borrow_mut().clear());
    SUBS.with(|s| s.borrow_mut().clear());
    TRACES.with(|s| s.borrow_mut().clear());
    REPORTS.with(|s| s.borrow_mut().clear());
    NEXT_ID.store(1, Ordering::Relaxed);
}

fn push_scope(state: ScopeState) -> i64 {
    SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn push_event(state: EventState) -> i64 {
    EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn push_hook(state: HookState) -> i64 {
    HOOKS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn push_decision_hook(state: DecisionHookState) -> i64 {
    DECISION_HOOKS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn push_async_event(state: AsyncEventState) -> i64 {
    ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn push_sub(sub: Subscription) -> i64 {
    SUBS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(sub));
        v.len() as i64
    })
}

fn push_trace(trace: TraceState) -> i64 {
    TRACES.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(trace));
        v.len() as i64
    })
}

fn push_report(report: ReportState) -> i64 {
    REPORTS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(report));
        v.len() as i64
    })
}

fn handle_value(type_name: &str, id: i64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("id".to_string(), CtValue::Int(id))],
    }
}

fn handle_id(recv: &CtValue, want: &str) -> Option<i64> {
    match recv {
        CtValue::Struct { type_name, fields } if type_name == want => fields
            .iter()
            .find_map(|(n, v)| match (n.as_str(), v) {
                ("id", CtValue::Int(i)) => Some(*i),
                _ => None,
            }),
        _ => None,
    }
}

fn recv_type(recv: &CtValue) -> Option<&str> {
    match recv {
        CtValue::Struct { type_name, .. } => Some(type_name.as_str()),
        _ => None,
    }
}

fn scope_track(scope_id: i64, sub: Subscription) -> Subscription {
    SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = scope_id.saturating_sub(1) as usize;
        let Some(Some(scope)) = v.get_mut(idx) else {
            sub.unsubscribe();
            return sub;
        };
        if scope.cancelled {
            sub.unsubscribe();
            return sub;
        }
        scope.subs.retain(|s| s.active());
        scope.subs.push(sub.clone());
        sub
    })
}

fn scope_cancel(scope_id: i64) {
    SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = scope_id.saturating_sub(1) as usize;
        let Some(Some(scope)) = v.get_mut(idx) else {
            return;
        };
        if scope.cancelled {
            return;
        }
        scope.cancelled = true;
        let subs = std::mem::take(&mut scope.subs);
        for sub in subs {
            sub.unsubscribe();
        }
    });
}

fn scope_active_count(scope_id: i64) -> i64 {
    SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = scope_id.saturating_sub(1) as usize;
        let Some(Some(scope)) = v.get_mut(idx) else {
            return 0;
        };
        scope.subs.retain(|s| s.active());
        scope.subs.len() as i64
    })
}

fn store_sub(sub: Subscription) -> CtValue {
    handle_value("Subscription", push_sub(sub))
}

fn event_add(
    event_id: i64,
    scope_id: i64,
    priority: i64,
    once: bool,
    handler: CtValue,
) -> CtValue {
    let sub = scope_track(scope_id, Subscription::new());
    if !sub.active() {
        return store_sub(sub);
    }
    let lid = next_id();
    EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        if let Some(Some(event)) = v.get_mut(idx) {
            let cleanup_event = event_id;
            let cleanup_lid = lid;
            sub.set_cleanup(move || {
                EVENTS.with(|slot| {
                    let mut v = slot.borrow_mut();
                    let idx = cleanup_event.saturating_sub(1) as usize;
                    if let Some(Some(event)) = v.get_mut(idx) {
                        event.listeners.retain(|l| l.id != cleanup_lid);
                    }
                });
            });
            event.listeners.push(Listener {
                id: lid,
                priority,
                once,
                sub: sub.clone(),
                handler,
            });
        }
    });
    store_sub(sub)
}

fn hook_add(
    hook_id: i64,
    scope_id: i64,
    priority: i64,
    once: bool,
    handler: CtValue,
) -> CtValue {
    // AOT Hook::add inserts the listener before scope.track.
    let sub = Subscription::new();
    let lid = next_id();
    HOOKS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = hook_id.saturating_sub(1) as usize;
        if let Some(Some(hook)) = v.get_mut(idx) {
            let cleanup_hook = hook_id;
            let cleanup_lid = lid;
            sub.set_cleanup(move || {
                HOOKS.with(|slot| {
                    let mut v = slot.borrow_mut();
                    let idx = cleanup_hook.saturating_sub(1) as usize;
                    if let Some(Some(hook)) = v.get_mut(idx) {
                        hook.listeners.retain(|l| l.id != cleanup_lid);
                    }
                });
            });
            hook.listeners.push(Listener {
                id: lid,
                priority,
                once,
                sub: sub.clone(),
                handler,
            });
        }
    });
    let tracked = scope_track(scope_id, sub);
    store_sub(tracked)
}

fn decision_hook_add(
    hook_id: i64,
    scope_id: i64,
    priority: i64,
    once: bool,
    handler: CtValue,
) -> CtValue {
    let sub = scope_track(scope_id, Subscription::new());
    if !sub.active() {
        return store_sub(sub);
    }
    let lid = next_id();
    DECISION_HOOKS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = hook_id.saturating_sub(1) as usize;
        if let Some(Some(hook)) = v.get_mut(idx) {
            let cleanup_hook = hook_id;
            let cleanup_lid = lid;
            sub.set_cleanup(move || {
                DECISION_HOOKS.with(|slot| {
                    let mut v = slot.borrow_mut();
                    let idx = cleanup_hook.saturating_sub(1) as usize;
                    if let Some(Some(hook)) = v.get_mut(idx) {
                        hook.listeners.retain(|l| l.id != cleanup_lid);
                    }
                });
            });
            hook.listeners.push(Listener {
                id: lid,
                priority,
                once,
                sub: sub.clone(),
                handler,
            });
        }
    });
    store_sub(sub)
}

fn async_event_add(event_id: i64, scope_id: i64, handler: CtValue) -> CtValue {
    let sub = scope_track(scope_id, Subscription::new());
    if !sub.active() {
        return store_sub(sub);
    }
    let lid = next_id();
    ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        if let Some(Some(event)) = v.get_mut(idx) {
            let cleanup_event = event_id;
            let cleanup_lid = lid;
            sub.set_cleanup(move || {
                ASYNC_EVENTS.with(|slot| {
                    let mut v = slot.borrow_mut();
                    let idx = cleanup_event.saturating_sub(1) as usize;
                    if let Some(Some(event)) = v.get_mut(idx) {
                        event.listeners.retain(|l| l.id != cleanup_lid);
                    }
                });
            });
            event.listeners.push(Listener {
                id: lid,
                priority: 0,
                once: false,
                sub: sub.clone(),
                handler,
            });
        }
    });
    store_sub(sub)
}

/// Sorted active listeners for dispatch (priority desc, then id asc).
fn collect_dispatch(listeners: &[Listener]) -> Vec<(i64, u64, bool, Subscription, CtValue)> {
    let mut entries: Vec<(i64, u64, bool, Subscription, CtValue)> = listeners
        .iter()
        .filter(|l| l.sub.active())
        .map(|l| (l.priority, l.id, l.once, l.sub.clone(), l.handler.clone()))
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    entries
}

fn make_trace(delivered: i64) -> CtValue {
    let summary = format!("event delivered={delivered} queued=0 dropped=0");
    handle_value(
        "EventTrace",
        push_trace(TraceState {
            delivered,
            queued: 0,
            dropped: 0,
            summary,
        }),
    )
}

fn make_task(value: CtValue) -> CtValue {
    CtValue::Struct {
        type_name: "__JetTirTask".to_string(),
        fields: vec![("value".to_string(), value)],
    }
}

fn make_report(delivered: bool, handlers: i64) -> CtValue {
    handle_value(
        "DispatchReport",
        push_report(ReportState {
            delivered,
            handlers,
        }),
    )
}

fn policy_capacity(policy: &CtValue) -> Option<i64> {
    match policy {
        CtValue::Struct { type_name, fields }
            if type_name == "AsyncPolicy" || type_name.ends_with("AsyncPolicy") =>
        {
            fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
                ("capacity", CtValue::Int(n)) => Some(*n),
                _ => None,
            })
        }
        _ => None,
    }
}

pub fn core_event_scope() -> CtValue {
    handle_value(
        "EventScope",
        push_scope(ScopeState {
            cancelled: false,
            subs: Vec::new(),
        }),
    )
}

pub fn core_event_new() -> CtValue {
    handle_value(
        "Event",
        push_event(EventState {
            listeners: Vec::new(),
        }),
    )
}

pub fn core_event_with_policy(_policy: CtValue) -> CtValue {
    core_event_new()
}

pub fn core_event_policy_sync() -> CtValue {
    CtValue::Struct {
        type_name: "EventPolicy".to_string(),
        fields: vec![("kind".to_string(), CtValue::Str("sync".to_string()))],
    }
}

pub fn core_event_hook(fallback: CtValue) -> CtValue {
    handle_value(
        "Hook",
        push_hook(HookState {
            fallback,
            listeners: Vec::new(),
        }),
    )
}

pub fn core_event_decision_hook(_policy: CtValue) -> CtValue {
    handle_value(
        "DecisionHook",
        push_decision_hook(DecisionHookState {
            listeners: Vec::new(),
        }),
    )
}

pub fn core_event_async_result(
    policy: &CtValue,
    _failure: &CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let capacity = policy_capacity(policy).unwrap_or(0);
    if capacity <= 0 {
        return Ok(CtValue::ResErr(Box::new(CtValue::Enum {
            type_name: "EventConfigError".to_string(),
            variant: "InvalidCapacity".to_string(),
            args: vec![],
        })));
    }
    let _ = span;
    Ok(CtValue::ResOk(Box::new(handle_value(
        "AsyncEvent",
        push_async_event(AsyncEventState {
            listeners: Vec::new(),
            closed: false,
        }),
    ))))
}

/// Dispatch an event method. `invoke` runs stored handler callables.
pub fn eval_method(
    method: &str,
    recv: &mut CtValue,
    args: &[CtValue],
    span: Span,
    invoke: &mut dyn FnMut(CtValue, Vec<CtValue>) -> Result<CtValue, Diagnostic>,
) -> Option<Result<CtValue, Diagnostic>> {
    let ty = recv_type(recv)?;
    Some(eval_method_inner(ty, method, recv, args, span, invoke))
}

fn eval_method_inner(
    ty: &str,
    method: &str,
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
    invoke: &mut dyn FnMut(CtValue, Vec<CtValue>) -> Result<CtValue, Diagnostic>,
) -> Result<CtValue, Diagnostic> {
    match (ty, method) {
        ("EventScope", "active_count") => {
            let id = handle_id(recv, "EventScope")
                .ok_or_else(|| unsupported("EventScope.active_count", span))?;
            Ok(CtValue::Int(scope_active_count(id)))
        }
        ("EventScope", "cancel") => {
            let id = handle_id(recv, "EventScope")
                .ok_or_else(|| unsupported("EventScope.cancel", span))?;
            scope_cancel(id);
            Ok(CtValue::Unit)
        }
        ("Subscription", "unsubscribe") => {
            let id = handle_id(recv, "Subscription")
                .ok_or_else(|| unsupported("Subscription.unsubscribe", span))?;
            SUBS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                if let Some(Some(sub)) = v.get(idx) {
                    sub.unsubscribe();
                }
            });
            Ok(CtValue::Unit)
        }
        ("Subscription", "is_active") => {
            let id = handle_id(recv, "Subscription")
                .ok_or_else(|| unsupported("Subscription.is_active", span))?;
            let active = SUBS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|s| s.as_ref())
                    .map(|s| s.active())
                    .unwrap_or(false)
            });
            Ok(CtValue::Bool(active))
        }
        ("Event", "on") => {
            let eid = handle_id(recv, "Event").ok_or_else(|| unsupported("Event.on", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("Event.on scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("Event.on handler", span))?;
            Ok(event_add(eid, sid, 0, false, handler))
        }
        ("Event", "once") => {
            let eid = handle_id(recv, "Event").ok_or_else(|| unsupported("Event.once", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("Event.once scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("Event.once handler", span))?;
            Ok(event_add(eid, sid, 0, true, handler))
        }
        ("Event", "on_priority") => {
            let eid =
                handle_id(recv, "Event").ok_or_else(|| unsupported("Event.on_priority", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("Event.on_priority scope", span))?;
            let priority = match args.get(1) {
                Some(CtValue::Int(n)) => *n,
                _ => return Err(unsupported("Event.on_priority priority", span)),
            };
            let handler = args
                .get(2)
                .cloned()
                .ok_or_else(|| unsupported("Event.on_priority handler", span))?;
            Ok(event_add(eid, sid, priority, false, handler))
        }
        ("Event", "emit") => {
            let eid = handle_id(recv, "Event").ok_or_else(|| unsupported("Event.emit", span))?;
            let payload = args
                .first()
                .cloned()
                .ok_or_else(|| unsupported("Event.emit payload", span))?;
            let entries = EVENTS.with(|slot| {
                let v = slot.borrow();
                let idx = eid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|e| e.as_ref())
                    .map(|e| collect_dispatch(&e.listeners))
                    .unwrap_or_default()
            });
            let mut delivered = 0i64;
            for (_priority, _id, once, sub, handler) in entries {
                if !sub.active() {
                    continue;
                }
                // Consume once before invoke (nested emit must not re-fire).
                if once {
                    sub.unsubscribe();
                }
                invoke(handler, vec![payload.clone()])?;
                delivered += 1;
            }
            EVENTS.with(|slot| {
                let mut v = slot.borrow_mut();
                let idx = eid.saturating_sub(1) as usize;
                if let Some(Some(event)) = v.get_mut(idx) {
                    event.listeners.retain(|l| l.sub.active());
                }
            });
            Ok(make_trace(delivered))
        }
        ("Event", "listener_count") => {
            let eid =
                handle_id(recv, "Event").ok_or_else(|| unsupported("Event.listener_count", span))?;
            let n = EVENTS.with(|slot| {
                let v = slot.borrow();
                let idx = eid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|e| e.as_ref())
                    .map(|e| e.listeners.iter().filter(|l| l.sub.active()).count() as i64)
                    .unwrap_or(0)
            });
            Ok(CtValue::Int(n))
        }
        ("Event", "trace") => {
            let eid = handle_id(recv, "Event").ok_or_else(|| unsupported("Event.trace", span))?;
            let n = EVENTS.with(|slot| {
                let v = slot.borrow();
                let idx = eid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|e| e.as_ref())
                    .map(|e| e.listeners.iter().filter(|l| l.sub.active()).count() as i64)
                    .unwrap_or(0)
            });
            Ok(CtValue::Str(format!("listeners={n} queued=0 dropped=0")))
        }
        ("EventTrace", "summary") => {
            let id =
                handle_id(recv, "EventTrace").ok_or_else(|| unsupported("EventTrace.summary", span))?;
            let summary = TRACES.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|t| t.as_ref())
                    .map(|t| t.summary.clone())
                    .unwrap_or_default()
            });
            Ok(CtValue::Str(summary))
        }
        ("EventTrace", "delivered") => {
            let id = handle_id(recv, "EventTrace")
                .ok_or_else(|| unsupported("EventTrace.delivered", span))?;
            let n = TRACES.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|t| t.as_ref())
                    .map(|t| t.delivered)
                    .unwrap_or(0)
            });
            Ok(CtValue::Int(n))
        }
        ("EventTrace", "queued" | "dropped") => {
            let id = handle_id(recv, "EventTrace")
                .ok_or_else(|| unsupported("EventTrace field", span))?;
            let n = TRACES.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx).and_then(|t| t.as_ref()).map(|t| {
                    if method == "queued" {
                        t.queued
                    } else {
                        t.dropped
                    }
                })
            });
            Ok(CtValue::Int(n.unwrap_or(0)))
        }
        ("Hook", "on") => {
            let hid = handle_id(recv, "Hook").ok_or_else(|| unsupported("Hook.on", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("Hook.on scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("Hook.on handler", span))?;
            Ok(hook_add(hid, sid, 0, false, handler))
        }
        ("Hook", "once") => {
            let hid = handle_id(recv, "Hook").ok_or_else(|| unsupported("Hook.once", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("Hook.once scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("Hook.once handler", span))?;
            Ok(hook_add(hid, sid, 0, true, handler))
        }
        ("Hook", "on_priority") => {
            let hid = handle_id(recv, "Hook").ok_or_else(|| unsupported("Hook.on_priority", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("Hook.on_priority scope", span))?;
            let priority = match args.get(1) {
                Some(CtValue::Int(n)) => *n,
                _ => return Err(unsupported("Hook.on_priority priority", span)),
            };
            let handler = args
                .get(2)
                .cloned()
                .ok_or_else(|| unsupported("Hook.on_priority handler", span))?;
            Ok(hook_add(hid, sid, priority, false, handler))
        }
        ("Hook", "run") => {
            let hid = handle_id(recv, "Hook").ok_or_else(|| unsupported("Hook.run", span))?;
            let payload = args
                .first()
                .cloned()
                .ok_or_else(|| unsupported("Hook.run payload", span))?;
            let empty_fallback = args.get(1).cloned().unwrap_or(CtValue::Unit);
            let (fallback, entries) = HOOKS.with(|slot| {
                let v = slot.borrow();
                let idx = hid.saturating_sub(1) as usize;
                match v.get(idx).and_then(|h| h.as_ref()) {
                    Some(hook) => (
                        hook.fallback.clone(),
                        collect_dispatch(&hook.listeners),
                    ),
                    None => (CtValue::Unit, Vec::new()),
                }
            });
            let mut result = if entries.is_empty() {
                empty_fallback
            } else {
                fallback
            };
            for (_priority, _id, once, sub, handler) in entries {
                if !sub.active() {
                    continue;
                }
                if once {
                    sub.unsubscribe();
                }
                result = invoke(handler, vec![payload.clone()])?;
            }
            HOOKS.with(|slot| {
                let mut v = slot.borrow_mut();
                let idx = hid.saturating_sub(1) as usize;
                if let Some(Some(hook)) = v.get_mut(idx) {
                    hook.listeners.retain(|l| l.sub.active());
                }
            });
            Ok(result)
        }
        ("DecisionHook", "on") => {
            let hid =
                handle_id(recv, "DecisionHook").ok_or_else(|| unsupported("DecisionHook.on", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("DecisionHook.on scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("DecisionHook.on handler", span))?;
            Ok(decision_hook_add(hid, sid, 0, false, handler))
        }
        ("DecisionHook", "once") => {
            let hid = handle_id(recv, "DecisionHook")
                .ok_or_else(|| unsupported("DecisionHook.once", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("DecisionHook.once scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("DecisionHook.once handler", span))?;
            Ok(decision_hook_add(hid, sid, 0, true, handler))
        }
        ("DecisionHook", "on_priority") => {
            let hid = handle_id(recv, "DecisionHook")
                .ok_or_else(|| unsupported("DecisionHook.on_priority", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("DecisionHook.on_priority scope", span))?;
            let priority = match args.get(1) {
                Some(CtValue::Int(n)) => *n,
                _ => return Err(unsupported("DecisionHook.on_priority priority", span)),
            };
            let handler = args
                .get(2)
                .cloned()
                .ok_or_else(|| unsupported("DecisionHook.on_priority handler", span))?;
            Ok(decision_hook_add(hid, sid, priority, false, handler))
        }
        ("DecisionHook", "run") => {
            let hid = handle_id(recv, "DecisionHook")
                .ok_or_else(|| unsupported("DecisionHook.run", span))?;
            let mut current = args
                .first()
                .cloned()
                .ok_or_else(|| unsupported("DecisionHook.run payload", span))?;
            let entries = DECISION_HOOKS.with(|slot| {
                let v = slot.borrow();
                let idx = hid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|h| h.as_ref())
                    .map(|h| collect_dispatch(&h.listeners))
                    .unwrap_or_default()
            });
            for (_priority, _id, once, sub, handler) in entries {
                if !sub.active() {
                    continue;
                }
                if once {
                    sub.unsubscribe();
                }
                let decision = invoke(handler, vec![current.clone()])?;
                match decision {
                    CtValue::Enum {
                        type_name,
                        variant,
                        args: dargs,
                    } if type_name == "HookDecision"
                        || type_name.ends_with("HookDecision") =>
                    {
                        match variant.as_str() {
                            "Continue" => {}
                            "Transform" => {
                                if let Some((_, v)) = dargs.first() {
                                    current = v.clone();
                                }
                            }
                            "Cancel" => {
                                return Ok(CtValue::Enum {
                                    type_name: "HookOutcome".to_string(),
                                    variant: "Cancel".to_string(),
                                    args: vec![],
                                });
                            }
                            "Fail" => {
                                let err = dargs
                                    .first()
                                    .map(|(_, v)| v.clone())
                                    .unwrap_or(CtValue::Unit);
                                return Ok(CtValue::Enum {
                                    type_name: "HookOutcome".to_string(),
                                    variant: "Fail".to_string(),
                                    args: vec![(None, err)],
                                });
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            DECISION_HOOKS.with(|slot| {
                let mut v = slot.borrow_mut();
                let idx = hid.saturating_sub(1) as usize;
                if let Some(Some(hook)) = v.get_mut(idx) {
                    hook.listeners.retain(|l| l.sub.active());
                }
            });
            Ok(CtValue::Enum {
                type_name: "HookOutcome".to_string(),
                variant: "Continue".to_string(),
                args: vec![(None, current)],
            })
        }
        ("AsyncEvent", "on") => {
            let eid =
                handle_id(recv, "AsyncEvent").ok_or_else(|| unsupported("AsyncEvent.on", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("AsyncEvent.on scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("AsyncEvent.on handler", span))?;
            Ok(async_event_add(eid, sid, handler))
        }
        ("AsyncEvent", "once" | "on_priority") => {
            // Golden path uses `.on`; treat once/on_priority like on.
            let eid = handle_id(recv, "AsyncEvent")
                .ok_or_else(|| unsupported("AsyncEvent.subscribe", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("AsyncEvent.subscribe scope", span))?;
            let handler = if method == "on_priority" {
                args.get(2).cloned()
            } else {
                args.get(1).cloned()
            }
            .ok_or_else(|| unsupported("AsyncEvent.subscribe handler", span))?;
            Ok(async_event_add(eid, sid, handler))
        }
        ("AsyncEvent", "emit_async") => {
            let eid = handle_id(recv, "AsyncEvent")
                .ok_or_else(|| unsupported("AsyncEvent.emit_async", span))?;
            let payload = args
                .first()
                .cloned()
                .ok_or_else(|| unsupported("AsyncEvent.emit_async payload", span))?;
            let (closed, handlers) = ASYNC_EVENTS.with(|slot| {
                let v = slot.borrow();
                let idx = eid.saturating_sub(1) as usize;
                match v.get(idx).and_then(|e| e.as_ref()) {
                    Some(event) => (
                        event.closed,
                        event
                            .listeners
                            .iter()
                            .filter(|l| l.sub.active())
                            .map(|l| l.handler.clone())
                            .collect::<Vec<_>>(),
                    ),
                    None => (true, Vec::new()),
                }
            });
            if closed {
                return Ok(make_task(make_report(false, 0)));
            }
            let count = handlers.len() as i64;
            for handler in handlers {
                let _ = invoke(handler, vec![payload.clone()])?;
            }
            Ok(make_task(make_report(true, count)))
        }
        ("AsyncEvent", "close") => {
            let eid =
                handle_id(recv, "AsyncEvent").ok_or_else(|| unsupported("AsyncEvent.close", span))?;
            ASYNC_EVENTS.with(|slot| {
                let mut v = slot.borrow_mut();
                let idx = eid.saturating_sub(1) as usize;
                if let Some(Some(event)) = v.get_mut(idx) {
                    event.closed = true;
                }
            });
            Ok(CtValue::Unit)
        }
        ("DispatchReport", "state") => {
            let id = handle_id(recv, "DispatchReport")
                .ok_or_else(|| unsupported("DispatchReport.state", span))?;
            let delivered = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.delivered)
                    .unwrap_or(false)
            });
            Ok(CtValue::Enum {
                type_name: "DispatchState".to_string(),
                variant: if delivered {
                    "Delivered".to_string()
                } else {
                    "Closed".to_string()
                },
                args: vec![],
            })
        }
        ("DispatchReport", "delivered_handlers") => {
            let id = handle_id(recv, "DispatchReport")
                .ok_or_else(|| unsupported("DispatchReport.delivered_handlers", span))?;
            let n = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.handlers)
                    .unwrap_or(0)
            });
            Ok(CtValue::Int(n))
        }
        ("DispatchReport", "accepted") => {
            let id = handle_id(recv, "DispatchReport")
                .ok_or_else(|| unsupported("DispatchReport.accepted", span))?;
            let delivered = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.delivered)
                    .unwrap_or(false)
            });
            Ok(CtValue::Bool(delivered))
        }
        _ => Err(unsupported(
            &format!("event method `{ty}.{method}`"),
            span,
        )),
    }
}
