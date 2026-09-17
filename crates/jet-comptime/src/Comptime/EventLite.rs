//! Interpreter `core.event` — mirrors AOT `ReactiveEventWatch.rs` for TIR deopt.
//! parity: guard tests/event_hooks.rs::decision_hook_outcomes_transform_and_short_circuit
//!
//! Handles are `CtValue` structs with an `id` field. Handler callables stay as
//! `CtValue` (eval `__JetTirCallable`); async handlers are queued and invoked
//! by the task join transition, never as a synchronous emit substitute.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Comptime::Diagnostics::unsupported;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::CtValue;

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
    hard_cancellers: Vec<Rc<dyn Fn()>>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum AsyncOverflow {
    Block,
    DropNewest,
    DropOldest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AsyncFailurePolicy {
    StopFirst,
    Collect,
    Log,
    Ignore,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AsyncPhase {
    Queued,
    Blocked,
    Running,
    Terminal,
}

struct AsyncTaskState {
    id: u64,
    event_id: i64,
    payload: CtValue,
    phase: AsyncPhase,
    accepted: bool,
    report: Option<CtValue>,
    joined: bool,
}

struct AsyncEventState {
    listeners: Vec<Listener>,
    capacity: usize,
    overflow: AsyncOverflow,
    failure_policy: AsyncFailurePolicy,
    queued: VecDeque<i64>,
    blocked: VecDeque<i64>,
    running: Option<i64>,
    closed: bool,
    cancelled: bool,
}

struct TraceState {
    delivered: i64,
    queued: i64,
    dropped: i64,
    summary: String,
}

struct ReportState {
    accepted: bool,
    state: String,
    handlers: i64,
    failures: Vec<CtValue>,
    trace: i64,
}

thread_local! {
    static SCOPES: RefCell<Vec<Option<ScopeState>>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<Option<EventState>>> = const { RefCell::new(Vec::new()) };
    static HOOKS: RefCell<Vec<Option<HookState>>> = const { RefCell::new(Vec::new()) };
    static DECISION_HOOKS: RefCell<Vec<Option<DecisionHookState>>> =
        const { RefCell::new(Vec::new()) };
    static ASYNC_EVENTS: RefCell<Vec<Option<AsyncEventState>>> =
        const { RefCell::new(Vec::new()) };
    static ASYNC_TASKS: RefCell<Vec<Option<AsyncTaskState>>> =
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
    ASYNC_TASKS.with(|s| s.borrow_mut().clear());
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

fn push_async_task(state: AsyncTaskState) -> i64 {
    ASYNC_TASKS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        (v.len() - 1) as i64
    })
}

fn task_handle_value(task_id: i64) -> CtValue {
    CtValue::Struct {
        type_name: "__JetTirTask".to_string(),
        fields: vec![("event_task".to_string(), CtValue::Int(task_id))],
    }
}

fn task_id(recv: &CtValue) -> Option<i64> {
    match recv {
        CtValue::Struct { type_name, fields } if type_name == "__JetTirTask" => {
            fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                ("event_task", CtValue::Int(id)) if *id >= 0 => Some(*id),
                _ => None,
            })
        }
        _ => None,
    }
}

fn is_event_task(recv: &CtValue) -> bool {
    task_id(recv).is_some()
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
        CtValue::Struct { type_name, fields } if type_name == want => {
            fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
                ("id", CtValue::Int(i)) => Some(*i),
                _ => None,
            })
        }
        _ => None,
    }
}
fn scope_track_hard_cancel<F: Fn() + 'static>(scope_id: i64, cancel: F) {
    let cancel: Rc<dyn Fn()> = Rc::new(cancel);
    let accepted = SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = scope_id.saturating_sub(1) as usize;
        let Some(Some(scope)) = v.get_mut(idx) else {
            return false;
        };
        if scope.cancelled {
            return false;
        }
        scope.hard_cancellers.push(cancel.clone());
        true
    });
    if !accepted {
        cancel();
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
    let (subs, hard_cancellers) = SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = scope_id.saturating_sub(1) as usize;
        let Some(Some(scope)) = v.get_mut(idx) else {
            return (Vec::new(), Vec::new());
        };
        if scope.cancelled {
            return (Vec::new(), Vec::new());
        }
        scope.cancelled = true;
        (
            std::mem::take(&mut scope.subs),
            std::mem::take(&mut scope.hard_cancellers),
        )
    });
    for sub in subs {
        sub.unsubscribe();
    }
    for cancel in hard_cancellers {
        cancel();
    }
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

fn event_add(event_id: i64, scope_id: i64, priority: i64, once: bool, handler: CtValue) -> CtValue {
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

fn hook_add(hook_id: i64, scope_id: i64, priority: i64, once: bool, handler: CtValue) -> CtValue {
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

fn async_event_add(
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
    let mut found = false;
    ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        if let Some(Some(event)) = v.get_mut(idx) {
            found = true;
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
                priority,
                once,
                sub: sub.clone(),
                handler,
            });
        }
    });
    if found {
        let cancel_event = event_id;
        scope_track_hard_cancel(scope_id, move || cancel_async_event(cancel_event));
    } else {
        sub.unsubscribe();
    }
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
    make_trace_state(delivered, 0, 0, format!("event delivered={delivered} queued=0 dropped=0"))
}

fn make_trace_state(delivered: i64, queued: i64, dropped: i64, summary: String) -> CtValue {
    handle_value(
        "EventTrace",
        push_trace(TraceState {
            delivered,
            queued,
            dropped,
            summary,
        }),
    )
}

fn make_dispatch_report(
    accepted: bool,
    state: &str,
    handlers: i64,
    failures: Vec<CtValue>,
    delivered: i64,
    queued: i64,
    dropped: i64,
) -> CtValue {
    let trace = make_trace_state(
        delivered,
        queued,
        dropped,
        format!("event state={state} delivered={delivered} queued={queued} dropped={dropped}"),
    );
    let trace_id = handle_id(&trace, "EventTrace").unwrap_or(0);
    handle_value(
        "DispatchReport",
        push_report(ReportState {
            accepted,
            state: state.to_string(),
            handlers,
            failures,
            trace: trace_id,
        }),
    )
}

fn policy_parts(policy: &CtValue) -> Option<(i64, AsyncOverflow)> {
    let CtValue::Struct { type_name, fields } = policy else {
        return None;
    };
    if type_name != "AsyncPolicy" && !type_name.ends_with("AsyncPolicy") {
        return None;
    }
    let capacity = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("capacity", CtValue::Int(value)) => Some(*value),
        _ => None,
    })?;
    let overflow = fields.iter().find_map(|(name, value)| {
        if name != "overflow" {
            return None;
        }
        match value {
            CtValue::Enum { variant, .. } => match variant.as_str() {
                "Block" => Some(AsyncOverflow::Block),
                "DropNewest" => Some(AsyncOverflow::DropNewest),
                "DropOldest" => Some(AsyncOverflow::DropOldest),
                _ => None,
            },
            _ => None,
        }
    })?;
    Some((capacity, overflow))
}

fn failure_policy_kind(value: &CtValue) -> Option<AsyncFailurePolicy> {
    match value {
        CtValue::Enum { variant, .. } => match variant.as_str() {
            "StopFirst" => Some(AsyncFailurePolicy::StopFirst),
            "Collect" => Some(AsyncFailurePolicy::Collect),
            "Log" => Some(AsyncFailurePolicy::Log),
            "Ignore" => Some(AsyncFailurePolicy::Ignore),
            _ => None,
        },
        _ => None,
    }
}

pub fn core_event_scope() -> CtValue {
    handle_value(
        "EventScope",
        push_scope(ScopeState {
            cancelled: false,
            subs: Vec::new(),
            hard_cancellers: Vec::new(),
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
    failure: &CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let Some((capacity, overflow)) = policy_parts(policy) else {
        let _ = span;
        return Ok(CtValue::failed(Box::new(CtValue::Enum {
            type_name: "EventConfigError".to_string(),
            variant: "InvalidCapacity".to_string(),
            args: vec![],
        })));
    };
    let Some(failure_policy) = failure_policy_kind(failure) else {
        let _ = span;
        return Ok(CtValue::failed(Box::new(CtValue::Enum {
            type_name: "EventConfigError".to_string(),
            variant: "InvalidCapacity".to_string(),
            args: vec![],
        })));
    };
    if capacity <= 0 {
        return Ok(CtValue::failed(Box::new(CtValue::Enum {
            type_name: "EventConfigError".to_string(),
            variant: "InvalidCapacity".to_string(),
            args: vec![],
        })));
    }
    Ok(CtValue::Present(Box::new(handle_value(
        "AsyncEvent",
        push_async_event(AsyncEventState {
            listeners: Vec::new(),
            capacity: capacity as usize,
            overflow,
            failure_policy,
            queued: VecDeque::new(),
            blocked: VecDeque::new(),
            running: None,
            closed: false,
            cancelled: false,
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
    if ty == "__JetTirTask" && !is_event_task(recv) {
        return None;
    }
    Some(eval_method_inner(ty, method, recv, args, span, invoke))
}
fn task_snapshot(
    task_id: i64,
) -> Option<(u64, i64, CtValue, AsyncPhase, bool, Option<CtValue>, bool)> {
    ASYNC_TASKS.with(|slot| {
        let v = slot.borrow();
        let task = v.get(task_id as usize)?.as_ref()?;
        Some((
            task.id,
            task.event_id,
            task.payload.clone(),
            task.phase,
            task.accepted,
            task.report.clone(),
            task.joined,
        ))
    })
}

fn set_task_phase(task_id: i64, phase: AsyncPhase, accepted: Option<bool>) {
    ASYNC_TASKS.with(|slot| {
        let mut v = slot.borrow_mut();
        if let Some(Some(task)) = v.get_mut(task_id as usize) {
            task.phase = phase;
            if let Some(accepted) = accepted {
                task.accepted = accepted;
            }
        }
    });
}

fn set_task_report(task_id: i64, report: CtValue) {
    ASYNC_TASKS.with(|slot| {
        let mut v = slot.borrow_mut();
        if let Some(Some(task)) = v.get_mut(task_id as usize) {
            task.phase = AsyncPhase::Terminal;
            task.report = Some(report);
        }
    });
}

fn task_report(
    task_id: i64,
    state: &str,
    handlers: i64,
    failures: Vec<CtValue>,
    delivered: i64,
    queued: i64,
    dropped: i64,
) {
    let accepted = task_snapshot(task_id).map(|(_, _, _, _, accepted, _, _)| accepted).unwrap_or(false);
    set_task_report(
        task_id,
        make_dispatch_report(accepted, state, handlers, failures, delivered, queued, dropped),
    );
}

fn cancel_async_event(event_id: i64) {
    let (queued, blocked, running, listeners) = ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            return (Vec::new(), Vec::new(), None, Vec::new());
        };
        event.cancelled = true;
        (
            event.queued.drain(..).collect::<Vec<_>>(),
            event.blocked.drain(..).collect::<Vec<_>>(),
            event.running,
            event
                .listeners
                .iter()
                .map(|listener| listener.sub.clone())
                .collect::<Vec<_>>(),
        )
    });
    for sub in listeners {
        sub.unsubscribe();
    }
    for task_id in queued {
        task_report(task_id, "Cancelled", 0, Vec::new(), 0, 1, 0);
    }
    for task_id in blocked {
        task_report(task_id, "Cancelled", 0, Vec::new(), 0, 0, 0);
    }
    if let Some(task_id) = running {
        set_task_phase(task_id, AsyncPhase::Running, Some(true));
    }
}

fn close_async_event(event_id: i64) {
    let blocked = ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            return Vec::new();
        };
        event.closed = true;
        event.blocked.drain(..).collect::<Vec<_>>()
    });
    for task_id in blocked {
        task_report(task_id, "Closed", 0, Vec::new(), 0, 0, 0);
    }
}

fn promote_blocked(event_id: i64) {
    ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            return;
        };
        if event.closed || event.cancelled {
            return;
        }
        while event.queued.len() < event.capacity {
            let Some(task_id) = event.blocked.pop_front() else {
                break;
            };
            set_task_phase(task_id, AsyncPhase::Queued, Some(true));
            event.queued.push_back(task_id);
        }
    });
}

fn async_event_emit(event_id: i64, payload: CtValue) -> CtValue {
    let task_id = push_async_task(AsyncTaskState {
        id: next_id(),
        event_id,
        payload,
        phase: AsyncPhase::Terminal,
        accepted: false,
        report: None,
        joined: false,
    });
    let mut terminal = None;
    let mut displaced = None;
    ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            terminal = Some(("Closed", false));
            return;
        };
        if event.cancelled {
            terminal = Some(("Cancelled", false));
        } else if event.closed {
            terminal = Some(("Closed", false));
        } else if event.queued.len() < event.capacity {
            set_task_phase(task_id, AsyncPhase::Queued, Some(true));
            event.queued.push_back(task_id);
        } else {
            match event.overflow {
                AsyncOverflow::Block => {
                    set_task_phase(task_id, AsyncPhase::Blocked, Some(false));
                    event.blocked.push_back(task_id);
                }
                AsyncOverflow::DropNewest => {
                    terminal = Some(("DroppedNewest", false));
                }
                AsyncOverflow::DropOldest => {
                    displaced = event.queued.pop_front();
                    set_task_phase(task_id, AsyncPhase::Queued, Some(true));
                    event.queued.push_back(task_id);
                    if displaced.is_none() {
                        terminal = Some(("DroppedNewest", false));
                    }
                }
            }
        }
    });
    if let Some((state, accepted)) = terminal {
        ASYNC_TASKS.with(|slot| {
            if let Some(Some(task)) = slot.borrow_mut().get_mut(task_id as usize) {
                task.accepted = accepted;
            }
        });
        task_report(task_id, state, 0, Vec::new(), 0, 0, 1);
    }
    if let Some(displaced) = displaced {
        task_report(displaced, "DroppedOldest", 0, Vec::new(), 0, 1, 1);
    }
    task_handle_value(task_id)
}

fn handler_failure(value: CtValue) -> Option<CtValue> {
    match value {
        CtValue::Failed(crate::AST::CtReport::Told(value)) => Some(*value),
        CtValue::Failed(crate::AST::CtReport::Clean(_)) => Some(CtValue::Unit),
        _ => None,
    }
}

fn dispatch_failure(value: CtValue) -> CtValue {
    CtValue::Enum {
        type_name: "DispatchFailure".to_string(),
        variant: "Handler".to_string(),
        args: vec![(None, value)],
    }
}

fn prepare_next_task(target: i64) -> Result<Option<(i64, i64)>, Diagnostic> {
    let Some((_, event_id, _, phase, _, report, _)) = task_snapshot(target) else {
        return Err(unsupported("EventTask.join", Span::new(0, 0)));
    };
    if report.is_some() {
        return Ok(None);
    }
    if phase == AsyncPhase::Blocked {
        promote_blocked(event_id);
    }
    let choice = ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            return None;
        };
        if event.running.is_some() {
            return None;
        }
        let task_id = event.queued.pop_front()?;
        event.running = Some(task_id);
        set_task_phase(task_id, AsyncPhase::Running, Some(true));
        Some((task_id, event_id))
    });
    if choice.is_none() && phase == AsyncPhase::Blocked {
        let Some((_, _, _, _, _, report, _)) = task_snapshot(target) else {
            return Err(unsupported("EventTask.join", Span::new(0, 0)));
        };
        if report.is_none() {
            return Err(unsupported(
                "EventTask.join is waiting on a running dispatch",
                Span::new(0, 0),
            ));
        }
    }
    Ok(choice)
}

fn run_async_task(
    task_id: i64,
    event_id: i64,
    span: Span,
    invoke: &mut dyn FnMut(CtValue, Vec<CtValue>) -> Result<CtValue, Diagnostic>,
) -> Result<(), Diagnostic> {
    let Some((_, _, payload, _, _, _, _)) = task_snapshot(task_id) else {
        return Err(unsupported("EventTask.join", span));
    };
    let (failure_policy, listeners) = ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            return (
                AsyncFailurePolicy::StopFirst,
                (Vec::new(), Vec::new()),
            );
        };
        let dispatch = collect_dispatch(&event.listeners);
        let once = dispatch
            .iter()
            .filter(|(_, _, once, _, _)| *once)
            .map(|(_, _, _, sub, _)| sub.clone())
            .collect::<Vec<_>>();
        (event.failure_policy, (dispatch, once))
    });
    let (listeners, once) = listeners;
    for sub in once {
        sub.unsubscribe();
    }
    let mut failures = Vec::new();
    let mut delivered = 0;
    let mut callback_error = None;
    for (_, _, _, _, handler) in listeners {
        if ASYNC_EVENTS.with(|slot| {
            let v = slot.borrow();
            let idx = event_id.saturating_sub(1) as usize;
            v.get(idx)
                .and_then(|state| state.as_ref())
                .map(|event| event.cancelled)
                .unwrap_or(true)
        }) {
            break;
        }
        delivered += 1;
        match invoke(handler, vec![payload.clone()]) {
            Ok(value) => {
                if let Some(error) = handler_failure(value) {
                    failures.push(dispatch_failure(error));
                    if failure_policy == AsyncFailurePolicy::StopFirst {
                        break;
                    }
                }
            }
            Err(error) => {
                callback_error = Some(error);
                break;
            }
        }
    }
    let cancelled = ASYNC_EVENTS.with(|slot| {
        let mut v = slot.borrow_mut();
        let idx = event_id.saturating_sub(1) as usize;
        let Some(Some(event)) = v.get_mut(idx) else {
            return true;
        };
        event.running = None;
        event.cancelled
    });
    let state = if cancelled {
        "Cancelled"
    } else if !failures.is_empty()
        && matches!(
            failure_policy,
            AsyncFailurePolicy::StopFirst | AsyncFailurePolicy::Collect
        )
    {
        "HandlerFailed"
    } else {
        "Delivered"
    };
    let report_failures = if matches!(
        failure_policy,
        AsyncFailurePolicy::Log | AsyncFailurePolicy::Ignore
    ) {
        Vec::new()
    } else {
        failures
    };
    task_report(
        task_id,
        state,
        delivered,
        report_failures,
        delivered,
        1,
        0,
    );
    promote_blocked(event_id);
    if let Some(error) = callback_error {
        return Err(error);
    }
    Ok(())
}

fn join_event_task(
    recv: &CtValue,
    span: Span,
    invoke: &mut dyn FnMut(CtValue, Vec<CtValue>) -> Result<CtValue, Diagnostic>,
) -> Result<CtValue, Diagnostic> {
    let task_id = task_id(recv).ok_or_else(|| unsupported("EventTask.join", span))?;
    let Some((_, _, _, _, _, report, joined)) = task_snapshot(task_id) else {
        return Err(unsupported("EventTask.join", span));
    };
    if joined {
        return Err(unsupported("EventTask.join called twice", span));
    }
    ASYNC_TASKS.with(|slot| {
        if let Some(Some(task)) = slot.borrow_mut().get_mut(task_id as usize) {
            task.joined = true;
        }
    });
    let report = if let Some(report) = report {
        report
    } else {
        loop {
            let Some((next, event_id)) = prepare_next_task(task_id)? else {
                let Some((_, _, _, _, _, report, _)) = task_snapshot(task_id) else {
                    return Err(unsupported("EventTask.join", span));
                };
                if let Some(report) = report {
                    break report;
                }
                continue;
            };
            run_async_task(next, event_id, span, invoke)?;
        }
    };
    Ok(CtValue::Present(Box::new(report)))
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
            let eid = handle_id(recv, "Event")
                .ok_or_else(|| unsupported("Event.listener_count", span))?;
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
        ("Hook", "listener_count") => {
            let hid =
                handle_id(recv, "Hook").ok_or_else(|| unsupported("Hook.listener_count", span))?;
            let n = HOOKS.with(|slot| {
                let v = slot.borrow();
                let idx = hid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|h| h.as_ref())
                    .map(|h| h.listeners.iter().filter(|l| l.sub.active()).count() as i64)
                    .unwrap_or(0)
            });
            Ok(CtValue::Int(n))
        }
        ("DecisionHook", "listener_count") => {
            let hid = handle_id(recv, "DecisionHook")
                .ok_or_else(|| unsupported("DecisionHook.listener_count", span))?;
            let n = DECISION_HOOKS.with(|slot| {
                let v = slot.borrow();
                let idx = hid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|h| h.as_ref())
                    .map(|h| h.listeners.iter().filter(|l| l.sub.active()).count() as i64)
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
            let id = handle_id(recv, "EventTrace")
                .ok_or_else(|| unsupported("EventTrace.summary", span))?;
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
            let hid =
                handle_id(recv, "Hook").ok_or_else(|| unsupported("Hook.on_priority", span))?;
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
                    Some(hook) => (hook.fallback.clone(), collect_dispatch(&hook.listeners)),
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
            let hid = handle_id(recv, "DecisionHook")
                .ok_or_else(|| unsupported("DecisionHook.on", span))?;
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
                    } if type_name == "HookDecision" || type_name.ends_with("HookDecision") => {
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
            Ok(async_event_add(eid, sid, 0, false, handler))
        }
        ("AsyncEvent", "once") => {
            let eid =
                handle_id(recv, "AsyncEvent").ok_or_else(|| unsupported("AsyncEvent.once", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("AsyncEvent.once scope", span))?;
            let handler = args
                .get(1)
                .cloned()
                .ok_or_else(|| unsupported("AsyncEvent.once handler", span))?;
            Ok(async_event_add(eid, sid, 0, true, handler))
        }
        ("AsyncEvent", "on_priority") => {
            let eid = handle_id(recv, "AsyncEvent")
                .ok_or_else(|| unsupported("AsyncEvent.on_priority", span))?;
            let sid = args
                .first()
                .and_then(|a| handle_id(a, "EventScope"))
                .ok_or_else(|| unsupported("AsyncEvent.on_priority scope", span))?;
            let priority = match args.get(1) {
                Some(CtValue::Int(value)) => *value,
                _ => return Err(unsupported("AsyncEvent.on_priority priority", span)),
            };
            let handler = args
                .get(2)
                .cloned()
                .ok_or_else(|| unsupported("AsyncEvent.on_priority handler", span))?;
            Ok(async_event_add(eid, sid, priority, false, handler))
        }
        ("AsyncEvent", "listener_count" | "queued_count" | "running_count" | "blocked_count") => {
            let eid = handle_id(recv, "AsyncEvent")
                .ok_or_else(|| unsupported("AsyncEvent.count", span))?;
            let n = ASYNC_EVENTS.with(|slot| {
                let v = slot.borrow();
                let idx = eid.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|e| e.as_ref())
                    .map(|event| match method {
                        "listener_count" => {
                            event.listeners.iter().filter(|l| l.sub.active()).count() as i64
                        }
                        "queued_count" => event.queued.len() as i64,
                        "running_count" => {
                            if event.running.is_some() { 1 } else { 0 }
                        }
                        "blocked_count" => event.blocked.len() as i64,
                        _ => 0,
                    })
                    .unwrap_or(0)
            });
            Ok(CtValue::Int(n))
        }
        ("AsyncEvent", "emit_async") => {
            let eid = handle_id(recv, "AsyncEvent")
                .ok_or_else(|| unsupported("AsyncEvent.emit_async", span))?;
            let payload = args
                .first()
                .cloned()
                .ok_or_else(|| unsupported("AsyncEvent.emit_async payload", span))?;
            Ok(async_event_emit(eid, payload))
        }
        ("AsyncEvent", "close") => {
            let eid = handle_id(recv, "AsyncEvent")
                .ok_or_else(|| unsupported("AsyncEvent.close", span))?;
            close_async_event(eid);
            Ok(CtValue::Unit)
        }
        ("__JetTirTask", "join") => join_event_task(recv, span, invoke),
        ("DispatchReport", "state") => {
            let id = handle_id(recv, "DispatchReport")
                .ok_or_else(|| unsupported("DispatchReport.state", span))?;
            let state = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.state.clone())
                    .unwrap_or_else(|| "Closed".to_string())
            });
            Ok(CtValue::Enum {
                type_name: "DispatchState".to_string(),
                variant: state,
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
            let accepted = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.accepted)
                    .unwrap_or(false)
            });
            Ok(CtValue::Bool(accepted))
        }
        ("DispatchReport", "failures") => {
            let id = handle_id(recv, "DispatchReport")
                .ok_or_else(|| unsupported("DispatchReport.failures", span))?;
            let failures = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.failures.clone())
                    .unwrap_or_default()
            });
            Ok(CtValue::List(failures))
        }
        ("DispatchReport", "trace") => {
            let id = handle_id(recv, "DispatchReport")
                .ok_or_else(|| unsupported("DispatchReport.trace", span))?;
            let trace = REPORTS.with(|slot| {
                let v = slot.borrow();
                let idx = id.saturating_sub(1) as usize;
                v.get(idx)
                    .and_then(|r| r.as_ref())
                    .map(|r| r.trace)
                    .unwrap_or(0)
            });
            Ok(handle_value("EventTrace", trace))
        }
        _ => Err(unsupported(&format!("event method `{ty}.{method}`"), span)),
    }
}
