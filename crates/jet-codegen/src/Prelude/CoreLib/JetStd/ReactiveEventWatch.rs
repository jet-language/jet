    // ── D-REACT1=B: opt-in reactive runtime (signals / derived / effects) ──────
    // Reactivity is a LIBRARY, not core semantics (option B): ordinary bindings are
    // unchanged; these types are the explicit, opt-in surface. Pure std — no external
    // crate (I6) and no raw-memory tier (interior mutability via Rc/RefCell). Dependency
    // tracking is explicit-by-read: a `.get()` evaluated while an observer (a derived
    // recompute or an effect run) is on the thread-local stack subscribes that
    // observer to the signal. A `.set(v)` re-runs every subscribed observer.
    use std::cell::RefCell;
    use std::rc::Rc;

    type Observer = Rc<dyn Fn()>;

    thread_local! {
        // The stack of observers currently (re)computing. The top is the active one.
        static JET_REACTIVE_OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) };
    }

    fn jet_reactive_active_observer() -> Option<Observer> {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow().last().cloned())
    }

    fn jet_reactive_run_observed(obs: &Observer, body: &dyn Fn()) {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow_mut().push(obs.clone()));
        body();
        JET_REACTIVE_OBSERVERS.with(|s| {
            s.borrow_mut().pop();
        });
    }

    struct SignalCell<T> {
        value: T,
        // Subscribers are re-run on set. Held as weak-free Rc closures; an effect or
        // derived keeps its own observer alive, so these stay valid for the run.
        subs: Vec<Observer>,
    }

    pub struct JetSignal<T> {
        cell: Rc<RefCell<SignalCell<T>>>,
    }

    impl<T> Clone for JetSignal<T> {
        fn clone(&self) -> Self {
            JetSignal {
                cell: self.cell.clone(),
            }
        }
    }

    impl<T: Clone> JetSignal<T> {
        pub fn new(initial: T) -> JetSignal<T> {
            JetSignal {
                cell: Rc::new(RefCell::new(SignalCell {
                    value: initial,
                    subs: Vec::new(),
                })),
            }
        }
        pub fn get(&self) -> T {
            if let Some(obs) = jet_reactive_active_observer() {
                let mut c = self.cell.borrow_mut();
                if !c.subs.iter().any(|s| Rc::ptr_eq(s, &obs)) {
                    c.subs.push(obs);
                }
            }
            self.cell.borrow().value.clone()
        }
        pub fn set(&self, value: T) {
            let subs = {
                let mut c = self.cell.borrow_mut();
                c.value = value;
                c.subs.clone()
            };
            for s in subs {
                s();
            }
        }
    }

    // A derived value is itself observable: it holds a current value plus its own
    // subscriber list, so effects (and other deriveds) that read it re-run when it
    // recomputes. The `_observer` it registers with its source signals recomputes the
    // value and then notifies the derived's own subscribers.
    pub struct JetDerived<T> {
        cell: Rc<RefCell<SignalCell<T>>>,
        _observer: Observer,
    }

    impl<T> Clone for JetDerived<T> {
        fn clone(&self) -> Self {
            JetDerived {
                cell: self.cell.clone(),
                _observer: self._observer.clone(),
            }
        }
    }

    impl<T: Clone + 'static> JetDerived<T> {
        pub fn new<F: Fn() -> T + 'static>(compute: F) -> JetDerived<T> {
            let compute = Rc::new(compute);
            let cell: Rc<RefCell<SignalCell<T>>> = Rc::new(RefCell::new(SignalCell {
                value: (compute)(),
                subs: Vec::new(),
            }));
            // The observer recomputes the value, then notifies the derived's own subs.
            let cell_for_obs = cell.clone();
            let compute_for_obs = compute.clone();
            let observer: Observer = Rc::new(move || {
                let v = (compute_for_obs)();
                let subs = {
                    let mut c = cell_for_obs.borrow_mut();
                    c.value = v;
                    c.subs.clone()
                };
                for s in subs {
                    s();
                }
            });
            // Run once under observation to record the source-signal dependency set.
            jet_reactive_run_observed(&observer, &{
                let cell = cell.clone();
                let compute = compute.clone();
                move || {
                    let v = (compute)();
                    cell.borrow_mut().value = v;
                }
            });
            JetDerived {
                cell,
                _observer: observer,
            }
        }
        pub fn get(&self) -> T {
            // Reading a derived inside an observer subscribes that observer to it.
            if let Some(obs) = jet_reactive_active_observer() {
                let mut c = self.cell.borrow_mut();
                if !c.subs.iter().any(|s| Rc::ptr_eq(s, &obs)) {
                    c.subs.push(obs);
                }
            }
            self.cell.borrow().value.clone()
        }
    }

    /// `reactive.effect(body)` — run `body` now, and again whenever a signal it read
    /// changes. The first run records the effect's dependencies; each subscribed
    /// signal then holds an `Rc` to the observer, keeping the effect alive for as long
    /// as a signal it reads is alive (a long-lived reactive sink). An effect that reads
    /// no signal simply runs once.
    pub fn jet_reactive_effect<F: Fn() + 'static>(body: F) {
        let observer: Observer = Rc::new(body);
        let run = observer.clone();
        jet_reactive_run_observed(&observer, &move || {
            run();
        });
    }

    /// D-REACTCORE1: `#Reactive` scope marker — alias for `jet_reactive_effect`.
    pub fn jet_reactive_scope<F: Fn() + 'static>(body: F) {
        jet_reactive_effect(body);
    }

    // D-EVENT1: first-party typed Event/Hook family. Values are ordinary Core
    // handles; the compiler knows their generic payload/result types.
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static JET_EVENT_NEXT_ID: AtomicU64 = AtomicU64::new(1);

    #[derive(Clone)]
    pub struct JetEventPolicy {
        async_buffer: Option<usize>,
        reentrancy: JetEventReentrancy,
    }

    #[derive(Clone)]
    enum JetEventReentrancy {
        // D-EVENT1's synchronous entrypoint dispatches nested emits immediately.
        AllowDepthFirst,
    }

    impl JetEventPolicy {
        pub fn sync() -> Self {
            JetEventPolicy {
                async_buffer: None,
                reentrancy: JetEventReentrancy::AllowDepthFirst,
            }
        }
        pub fn async_buffered(buffer: i64) -> Self {
            JetEventPolicy {
                async_buffer: Some(buffer.max(0) as usize),
                reentrancy: JetEventReentrancy::AllowDepthFirst,
            }
        }
    }

    #[derive(Clone)]
    pub struct JetEventTrace {
        delivered: i64,
        queued: i64,
        dropped: i64,
        summary: String,
    }

    impl JetEventTrace {
        pub fn delivered(&self) -> i64 {
            self.delivered
        }
        pub fn queued(&self) -> i64 {
            self.queued
        }
        pub fn dropped(&self) -> i64 {
            self.dropped
        }
        pub fn summary(&self) -> String {
            self.summary.clone()
        }
    }

    #[derive(Clone)]
    pub struct JetSubscription {
        active: Rc<Cell<bool>>,
        shared_active: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        cleanup: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    }

    impl JetSubscription {
        fn new() -> Self {
            JetSubscription {
                active: Rc::new(Cell::new(true)),
                shared_active: None,
                cleanup: Rc::new(RefCell::new(None)),
            }
        }
        fn shared(active: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
            JetSubscription {
                active: Rc::new(Cell::new(true)),
                shared_active: Some(active),
                cleanup: Rc::new(RefCell::new(None)),
            }
        }
        fn set_cleanup<F: Fn() + 'static>(&self, cleanup: F) {
            *self.cleanup.borrow_mut() = Some(Rc::new(cleanup));
        }
        pub fn unsubscribe(&self) {
            if self.active.replace(false) {
                if let Some(active) = &self.shared_active {
                    active.store(false, std::sync::atomic::Ordering::Release);
                }
                let cleanup = self.cleanup.borrow().clone();
                if let Some(cleanup) = cleanup {
                    cleanup();
                }
            }
        }
        pub fn active(&self) -> bool {
            self.active.get()
                && self.shared_active.as_ref().is_none_or(|active| {
                    active.load(std::sync::atomic::Ordering::Acquire)
                })
        }
    }

    #[derive(Clone)]
    pub struct JetEventScope {
        subs: Rc<RefCell<Vec<JetSubscription>>>,
        cancelled: Rc<Cell<bool>>,
    }

    impl JetEventScope {
        pub fn new() -> Self {
            JetEventScope {
                subs: Rc::new(RefCell::new(Vec::new())),
                cancelled: Rc::new(Cell::new(false)),
            }
        }
        pub fn track(&self, sub: JetSubscription) -> JetSubscription {
            if self.cancelled.get() {
                sub.unsubscribe();
                return sub;
            }
            let mut subs = self.subs.borrow_mut();
            subs.retain(|tracked| tracked.active());
            subs.push(sub.clone());
            sub
        }
        pub fn cancel(&self) {
            self.cancelled.set(true);
            let subs = std::mem::take(&mut *self.subs.borrow_mut());
            for sub in subs {
                sub.unsubscribe();
            }
        }
        pub fn active_count(&self) -> i64 {
            let mut subs = self.subs.borrow_mut();
            subs.retain(|sub| sub.active());
            subs.len() as i64
        }
    }

    impl Drop for JetEventScope {
        fn drop(&mut self) {
            if Rc::strong_count(&self.subs) == 1 {
                self.cancel();
            }
        }
    }

    struct JetListener<T> {
        id: u64,
        priority: i64,
        once: bool,
        sub: JetSubscription,
        handler: Rc<dyn Fn(T)>,
    }

    pub struct JetEvent<T: Clone + 'static> {
        policy: JetEventPolicy,
        listeners: Rc<RefCell<Vec<JetListener<T>>>>,
        queue: Rc<RefCell<Vec<T>>>,
        dropped: Rc<Cell<i64>>,
    }

    impl<T: Clone + 'static> Clone for JetEvent<T> {
        fn clone(&self) -> Self {
            JetEvent {
                policy: self.policy.clone(),
                listeners: self.listeners.clone(),
                queue: self.queue.clone(),
                dropped: self.dropped.clone(),
            }
        }
    }

    impl<T: Clone + 'static> JetEvent<T> {
        pub fn new() -> Self {
            Self::with_policy(JetEventPolicy::sync())
        }
        pub fn with_policy(policy: JetEventPolicy) -> Self {
            JetEvent {
                policy,
                listeners: Rc::new(RefCell::new(Vec::new())),
                queue: Rc::new(RefCell::new(Vec::new())),
                dropped: Rc::new(Cell::new(0)),
            }
        }
        pub fn on<F: Fn(T) + 'static>(&self, scope: &JetEventScope, handler: F) -> JetSubscription {
            self.on_priority(scope, 0, handler)
        }
        pub fn once<F: Fn(T) + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, 0, true, handler)
        }
        pub fn on_priority<F: Fn(T) + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, priority, false, handler)
        }
        fn add<F: Fn(T) + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            once: bool,
            handler: F,
        ) -> JetSubscription {
            let sub = JetSubscription::new();
            self.listeners.borrow_mut().push(JetListener {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                priority,
                once,
                sub: sub.clone(),
                handler: Rc::new(handler),
            });
            let listeners = Rc::downgrade(&self.listeners);
            let id = self.listeners.borrow().last().expect("event listener").id;
            sub.set_cleanup(move || {
                if let Some(listeners) = listeners.upgrade() {
                    listeners.borrow_mut().retain(|listener| listener.id != id);
                }
            });
            scope.track(sub)
        }
        pub fn emit(&self, payload: T) -> JetEventTrace {
            self.dispatch(payload, false)
        }
        pub fn emit_async(&self, payload: T) -> JetEventTrace {
            self.dispatch(payload, true)
        }
        fn dispatch(&self, payload: T, queued: bool) -> JetEventTrace {
            match self.policy.reentrancy {
                JetEventReentrancy::AllowDepthFirst => {}
            }
            let mut queued_count = 0;
            if queued || self.policy.async_buffer.is_some() {
                queued_count = 1;
                if let Some(limit) = self.policy.async_buffer {
                    let mut q = self.queue.borrow_mut();
                    if limit == 0 {
                        self.dropped.set(self.dropped.get() + 1);
                    } else {
                        if q.len() >= limit {
                            q.remove(0);
                            self.dropped.set(self.dropped.get() + 1);
                        }
                        q.push(payload.clone());
                    }
                    q.clear();
                }
            }
            let mut entries: Vec<(i64, u64, bool, JetSubscription, Rc<dyn Fn(T)>)> = self
                .listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .map(|l| (l.priority, l.id, l.once, l.sub.clone(), l.handler.clone()))
                .collect();
            entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut delivered = 0;
            for (_, _, once, sub, handler) in entries {
                if sub.active() {
                    // Consume once before invoking: a nested emit from this
                    // handler must not deliver it twice.
                    if once {
                        sub.unsubscribe();
                    }
                    handler(payload.clone());
                    delivered += 1;
                }
            }
            self.listeners.borrow_mut().retain(|l| l.sub.active());
            JetEventTrace {
                delivered,
                queued: queued_count,
                dropped: self.dropped.get(),
                summary: format!(
                    "event delivered={delivered} queued={queued_count} dropped={}",
                    self.dropped.get()
                ),
            }
        }
        pub fn listener_count(&self) -> i64 {
            self.listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .count() as i64
        }
        pub fn queued_count(&self) -> i64 {
            self.queue.borrow().len() as i64
        }
        pub fn trace(&self) -> String {
            format!(
                "listeners={} queued={} dropped={}",
                self.listener_count(),
                self.queued_count(),
                self.dropped.get()
            )
        }
    }

    // D-EVENT2=A: bounded scheduler-backed asynchronous dispatch. This is a
    // separate Send-safe handle; the synchronous Event keeps its Rc fast path.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum JetEventOverflow {
        Block,
        DropNewest,
        DropOldest,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum JetFailurePolicy {
        StopFirst,
        Collect,
        Log,
        Ignore,
    }

    #[derive(Clone, Copy)]
    pub struct JetAsyncPolicy {
        pub capacity: i64,
        pub overflow: JetEventOverflow,
    }

    impl JetAsyncPolicy {
        pub fn new(capacity: i64, overflow: JetEventOverflow) -> Self {
            JetAsyncPolicy { capacity, overflow }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum JetEventConfigError { InvalidCapacity }

    #[derive(Clone)]
    pub enum JetDispatchFailure<E: Clone + Send + 'static> {
        Handler(E),
        Panic(String),
    }

    pub trait JetIntoDispatchResult<E> {
        fn into_dispatch_result(self) -> Result<(), E>;
    }
    impl<E> JetIntoDispatchResult<E> for () {
        fn into_dispatch_result(self) -> Result<(), E> { Ok(()) }
    }
    impl<E> JetIntoDispatchResult<E> for Result<(), E> {
        fn into_dispatch_result(self) -> Result<(), E> { self }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum JetDispatchState {
        Delivered,
        HandlerFailed,
        DroppedNewest,
        DroppedOldest,
        Closed,
        Cancelled,
        DeadlineExceeded,
    }

    #[derive(Clone)]
    pub struct JetDispatchReport<E: Clone + Send + 'static> {
        accepted: bool,
        state: JetDispatchState,
        delivered_handlers: i64,
        failures: Vec<JetDispatchFailure<E>>,
        trace: Vec<String>,
    }

    impl<E: Clone + Send + 'static> JetDispatchReport<E> {
        fn terminal(accepted: bool, state: JetDispatchState) -> Self {
            JetDispatchReport {
                accepted,
                state,
                delivered_handlers: 0,
                failures: Vec::new(),
                trace: vec![format!("terminal:{:?}", state)],
            }
        }
        fn terminal_with_trace(
            accepted: bool,
            state: JetDispatchState,
            mut trace: Vec<String>,
        ) -> Self {
            trace.push(format!("terminal:{:?}", state));
            JetDispatchReport {
                accepted,
                state,
                delivered_handlers: 0,
                failures: Vec::new(),
                trace,
            }
        }
        pub fn accepted(&self) -> bool { self.accepted }
        pub fn delivered_handlers(&self) -> i64 { self.delivered_handlers }
        pub fn state(&self) -> JetDispatchState { self.state }
        pub fn failures(&self) -> Vec<JetDispatchFailure<E>> { self.failures.clone() }
        pub fn trace(&self) -> JetEventTrace {
            JetEventTrace {
                delivered: self.delivered_handlers,
                queued: self.trace.iter().filter(|entry| entry.as_str() == "queued").count() as i64,
                dropped: i64::from(matches!(self.state, JetDispatchState::DroppedNewest | JetDispatchState::DroppedOldest)),
                summary: self.trace.join(" -> "),
            }
        }
    }

    struct JetAsyncListener<T, E> {
        id: u64,
        priority: i64,
        once: bool,
        active: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handler: std::sync::Arc<dyn Fn(T) -> Result<(), E> + Send + Sync>,
    }

    struct JetAsyncEntry<T, E: Clone + Send + 'static> {
        id: u64,
        accepted: std::sync::atomic::AtomicBool,
        payload: std::sync::Mutex<Option<T>>,
        terminal: std::sync::Mutex<Option<JetDispatchReport<E>>>,
        trace: std::sync::Mutex<Vec<String>>,
        wake: std::sync::Arc<super::ParkSlot>,
    }

    struct JetAsyncState<T, E: Clone + Send + 'static> {
        listeners: Vec<JetAsyncListener<T, E>>,
        queued: std::collections::VecDeque<std::sync::Arc<JetAsyncEntry<T, E>>>,
        blocked: std::collections::VecDeque<std::sync::Arc<JetAsyncEntry<T, E>>>,
        running: usize,
        closed: bool,
        cancelled: bool,
    }

    pub struct JetAsyncEvent<T: Clone + Send + 'static, E: Clone + Send + 'static> {
        policy: JetAsyncPolicy,
        failure_policy: JetFailurePolicy,
        state: std::sync::Arc<std::sync::Mutex<JetAsyncState<T, E>>>,
        owner: Option<std::sync::Arc<()>>,
    }

    impl<T: Clone + Send + 'static, E: Clone + Send + 'static> Clone for JetAsyncEvent<T, E> {
        fn clone(&self) -> Self {
            JetAsyncEvent {
                policy: self.policy,
                failure_policy: self.failure_policy,
                state: self.state.clone(),
                owner: self.owner.clone(),
            }
        }
    }

    impl<T: Clone + Send + 'static, E: Clone + Send + 'static> Drop for JetAsyncEvent<T, E> {
        fn drop(&mut self) {
            let Some(owner) = &self.owner else { return; };
            if std::sync::Arc::strong_count(owner) != 1 { return; }
            let wakes = {
                let mut state = self.state.lock().unwrap();
                state.cancelled = true;
                state.queued.iter().chain(state.blocked.iter()).map(|e| e.wake.clone()).collect::<Vec<_>>()
            };
            for wake in wakes { super::jet_scheduler_wake(&wake); }
        }
    }

    impl<T: Clone + Send + 'static, E: Clone + Send + 'static> JetAsyncEvent<T, E> {
        pub fn new(policy: JetAsyncPolicy, failure_policy: JetFailurePolicy) -> Result<Self, JetEventConfigError> {
            if policy.capacity <= 0 { return Err(JetEventConfigError::InvalidCapacity); }
            Ok(JetAsyncEvent {
                policy,
                failure_policy,
                state: std::sync::Arc::new(std::sync::Mutex::new(JetAsyncState {
                    listeners: Vec::new(),
                    queued: std::collections::VecDeque::new(),
                    blocked: std::collections::VecDeque::new(),
                    running: 0,
                    closed: false,
                    cancelled: false,
                })),
                owner: Some(std::sync::Arc::new(())),
            })
        }

        pub fn on<F, R>(&self, scope: &JetEventScope, handler: F) -> JetSubscription
        where F: Fn(T) -> R + Send + Sync + 'static, R: JetIntoDispatchResult<E> {
            self.add(scope, 0, false, handler)
        }
        pub fn once<F, R>(&self, scope: &JetEventScope, handler: F) -> JetSubscription
        where F: Fn(T) -> R + Send + Sync + 'static, R: JetIntoDispatchResult<E> {
            self.add(scope, 0, true, handler)
        }
        pub fn on_priority<F, R>(&self, scope: &JetEventScope, priority: i64, handler: F) -> JetSubscription
        where F: Fn(T) -> R + Send + Sync + 'static, R: JetIntoDispatchResult<E> {
            self.add(scope, priority, false, handler)
        }
        fn add<F, R>(&self, scope: &JetEventScope, priority: i64, once: bool, handler: F) -> JetSubscription
        where F: Fn(T) -> R + Send + Sync + 'static, R: JetIntoDispatchResult<E> {
            let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let id = JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed);
            self.state.lock().unwrap().listeners.push(JetAsyncListener {
                id, priority, once, active: active.clone(), handler: std::sync::Arc::new(move |payload| handler(payload).into_dispatch_result()),
            });
            let sub = JetSubscription::shared(active);
            let state = std::sync::Arc::downgrade(&self.state);
            sub.set_cleanup(move || {
                if let Some(state) = state.upgrade() {
                    state.lock().unwrap().listeners.retain(|listener| listener.id != id);
                }
            });
            scope.track(sub)
        }

        pub fn emit_async(&self, payload: T) -> super::jet_std::JetTask<JetDispatchReport<E>> {
            let entry = std::sync::Arc::new(JetAsyncEntry {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                accepted: std::sync::atomic::AtomicBool::new(false),
                payload: std::sync::Mutex::new(Some(payload)),
                terminal: std::sync::Mutex::new(None),
                trace: std::sync::Mutex::new(Vec::new()),
                wake: super::ParkSlot::new(),
            });
            {
                let mut state = self.state.lock().unwrap();
                if state.closed {
                    *entry.terminal.lock().unwrap() = Some(JetDispatchReport::terminal(false, JetDispatchState::Closed));
                } else if state.queued.len() < self.policy.capacity as usize {
                    entry.accepted.store(true, std::sync::atomic::Ordering::Release);
                    entry.trace.lock().unwrap().push("queued".to_string());
                    state.queued.push_back(entry.clone());
                } else {
                    match self.policy.overflow {
                        JetEventOverflow::Block => {
                            entry.trace.lock().unwrap().push("pending".to_string());
                            state.blocked.push_back(entry.clone());
                        }
                        JetEventOverflow::DropNewest => {
                            *entry.terminal.lock().unwrap() = Some(JetDispatchReport::terminal(false, JetDispatchState::DroppedNewest));
                        }
                        JetEventOverflow::DropOldest => {
                            if let Some(oldest) = state.queued.pop_front() {
                                let trace = oldest.trace.lock().unwrap().clone();
                                *oldest.terminal.lock().unwrap() = Some(JetDispatchReport::terminal_with_trace(true, JetDispatchState::DroppedOldest, trace));
                                oldest.wake.wake();
                            }
                            entry.accepted.store(true, std::sync::atomic::Ordering::Release);
                            entry.trace.lock().unwrap().push("queued".to_string());
                            state.queued.push_back(entry.clone());
                        }
                    }
                }
                if state.queued.front().is_some_and(|front| front.id == entry.id) { entry.wake.wake(); }
            }
            let event = JetAsyncEvent {
                policy: self.policy,
                failure_policy: self.failure_policy,
                state: self.state.clone(),
                owner: None,
            };
            super::jet_std::JetTask::spawn(move || event.run_entry(entry))
        }

        fn run_entry(&self, entry: std::sync::Arc<JetAsyncEntry<T, E>>) -> JetDispatchReport<E> {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run_entry_inner(&entry)));
            match run {
                Ok(report) => report,
                Err(payload) if payload.is::<super::JetCancelUnwind>() => {
                    self.remove_entry(&entry);
                    JetDispatchReport::terminal_with_trace(
                        entry.accepted.load(std::sync::atomic::Ordering::Acquire),
                        JetDispatchState::Cancelled,
                        entry.trace.lock().unwrap().clone(),
                    )
                }
                Err(payload) if payload.is::<super::JetDeadlineUnwind>() => {
                    self.remove_entry(&entry);
                    JetDispatchReport::terminal_with_trace(
                        entry.accepted.load(std::sync::atomic::Ordering::Acquire),
                        JetDispatchState::DeadlineExceeded,
                        entry.trace.lock().unwrap().clone(),
                    )
                }
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }

        fn run_entry_inner(&self, entry: &std::sync::Arc<JetAsyncEntry<T, E>>) -> JetDispatchReport<E> {
            loop {
                if let Some(report) = entry.terminal.lock().unwrap().clone() { return report; }
                let listeners = {
                    let mut state = self.state.lock().unwrap();
                    if state.cancelled {
                        return JetDispatchReport::terminal_with_trace(
                            entry.accepted.load(std::sync::atomic::Ordering::Acquire),
                            JetDispatchState::Cancelled,
                            entry.trace.lock().unwrap().clone(),
                        );
                    }
                    if let Some(pos) = state.blocked.iter().position(|item| item.id == entry.id) {
                        if state.closed {
                            state.blocked.remove(pos);
                            return JetDispatchReport::terminal_with_trace(
                                false,
                                JetDispatchState::Closed,
                                entry.trace.lock().unwrap().clone(),
                            );
                        }
                        if state.queued.len() < self.policy.capacity as usize {
                            state.blocked.remove(pos);
                            entry.accepted.store(true, std::sync::atomic::Ordering::Release);
                            entry.trace.lock().unwrap().push("queued".to_string());
                            state.queued.push_back(entry.clone());
                        }
                    }
                    if state.queued.front().is_some_and(|front| front.id == entry.id) {
                        state.queued.pop_front();
                        state.running += 1;
                        entry.trace.lock().unwrap().push("running".to_string());
                        let mut listeners = state.listeners.iter()
                            .filter(|listener| listener.active.load(std::sync::atomic::Ordering::Acquire))
                            .map(|listener| (listener.priority, listener.id, listener.once, listener.active.clone(), listener.handler.clone()))
                            .collect::<Vec<_>>();
                        listeners.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                        for blocked in &state.blocked { blocked.wake.wake(); }
                        Some(listeners)
                    } else { None }
                };
                if let Some(listeners) = listeners {
                    let payload = entry.payload.lock().unwrap().take().expect("async event payload consumed once");
                    let mut report = JetDispatchReport {
                        accepted: true,
                        state: JetDispatchState::Delivered,
                        delivered_handlers: 0,
                        failures: Vec::new(),
                        trace: entry.trace.lock().unwrap().clone(),
                    };
                    for (handler_index, (_, _, once, active, handler)) in listeners.into_iter().enumerate() {
                        if !active.load(std::sync::atomic::Ordering::Acquire) { continue; }
                        if once { active.store(false, std::sync::atomic::Ordering::Release); }
                        report.delivered_handlers += 1;
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(payload.clone())));
                        match outcome {
                            Ok(Ok(())) => report.trace.push(format!("handler:{handler_index}:delivered")),
                            Ok(Err(error)) => {
                                match self.failure_policy {
                                    JetFailurePolicy::StopFirst => {
                                        report.state = JetDispatchState::HandlerFailed;
                                        report.failures.push(JetDispatchFailure::Handler(error));
                                        report.trace.push(format!("handler:{handler_index}:failed"));
                                        break;
                                    }
                                    JetFailurePolicy::Collect => {
                                        report.state = JetDispatchState::HandlerFailed;
                                        report.failures.push(JetDispatchFailure::Handler(error));
                                        report.trace.push(format!("handler:{handler_index}:failed"));
                                    }
                                    JetFailurePolicy::Log => {
                                        report.trace.push(format!("handler:{handler_index}:failed"));
                                        eprintln!("event handler failed");
                                    }
                                    JetFailurePolicy::Ignore => {}
                                }
                            }
                            Err(payload) => {
                                let message = payload.downcast_ref::<String>().cloned()
                                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                                    .unwrap_or_else(|| "event handler panicked".to_string());
                                report.state = JetDispatchState::HandlerFailed;
                                report.failures.push(JetDispatchFailure::Panic(message.clone()));
                                report.trace.push(format!("handler:{handler_index}:panic:{message}"));
                                break;
                            }
                        }
                    }
                    report.trace.push(format!("terminal:{:?}", report.state));
                    let wakes = {
                        let mut state = self.state.lock().unwrap();
                        state.running = state.running.saturating_sub(1);
                        state.listeners.retain(|listener| listener.active.load(std::sync::atomic::Ordering::Acquire));
                        state.queued.front().into_iter().chain(state.blocked.iter()).map(|item| item.wake.clone()).collect::<Vec<_>>()
                    };
                    for wake in wakes { wake.wake(); }
                    return report;
                }
                super::jet_scheduler_yield("async event dispatch", &entry.wake, None);
            }
        }

        fn remove_entry(&self, entry: &std::sync::Arc<JetAsyncEntry<T, E>>) {
            let wakes = {
                let mut state = self.state.lock().unwrap();
                state.queued.retain(|item| item.id != entry.id);
                state.blocked.retain(|item| item.id != entry.id);
                state.queued.front().into_iter().chain(state.blocked.iter()).map(|item| item.wake.clone()).collect::<Vec<_>>()
            };
            for wake in wakes { wake.wake(); }
        }

        pub fn close(&self) {
            let wakes = {
                let mut state = self.state.lock().unwrap();
                if state.closed { return; }
                state.closed = true;
                state.blocked.iter().map(|item| item.wake.clone()).collect::<Vec<_>>()
            };
            for wake in wakes { wake.wake(); }
        }
        pub fn queued_count(&self) -> i64 { self.state.lock().unwrap().queued.len() as i64 }
        pub fn running_count(&self) -> i64 { self.state.lock().unwrap().running as i64 }
        pub fn blocked_count(&self) -> i64 { self.state.lock().unwrap().blocked.len() as i64 }
        pub fn listener_count(&self) -> i64 { self.state.lock().unwrap().listeners.iter().filter(|l| l.active.load(std::sync::atomic::Ordering::Acquire)).count() as i64 }
    }

    struct JetHookListener<T, R> {
        id: u64,
        priority: i64,
        once: bool,
        sub: JetSubscription,
        handler: Rc<dyn Fn(T) -> R>,
    }

    pub struct JetHook<T: Clone + 'static, R: Clone + 'static> {
        fallback: R,
        listeners: Rc<RefCell<Vec<JetHookListener<T, R>>>>,
    }

    impl<T: Clone + 'static, R: Clone + 'static> Clone for JetHook<T, R> {
        fn clone(&self) -> Self {
            JetHook {
                fallback: self.fallback.clone(),
                listeners: self.listeners.clone(),
            }
        }
    }

    impl<T: Clone + 'static, R: Clone + 'static> JetHook<T, R> {
        pub fn new(fallback: R) -> Self {
            JetHook {
                fallback,
                listeners: Rc::new(RefCell::new(Vec::new())),
            }
        }
        pub fn on<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.on_priority(scope, 0, handler)
        }
        pub fn once<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, 0, true, handler)
        }
        pub fn on_priority<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, priority, false, handler)
        }
        fn add<F: Fn(T) -> R + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            once: bool,
            handler: F,
        ) -> JetSubscription {
            let sub = JetSubscription::new();
            self.listeners.borrow_mut().push(JetHookListener {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                priority,
                once,
                sub: sub.clone(),
                handler: Rc::new(handler),
            });
            let listeners = Rc::downgrade(&self.listeners);
            let id = self.listeners.borrow().last().expect("hook listener").id;
            sub.set_cleanup(move || {
                if let Some(listeners) = listeners.upgrade() {
                    listeners.borrow_mut().retain(|listener| listener.id != id);
                }
            });
            scope.track(sub)
        }
        pub fn run(&self, payload: T, fallback: R) -> R {
            let mut entries: Vec<(i64, u64, bool, JetSubscription, Rc<dyn Fn(T) -> R>)> = self
                .listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .map(|l| (l.priority, l.id, l.once, l.sub.clone(), l.handler.clone()))
                .collect();
            entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut result = if entries.is_empty() {
                fallback
            } else {
                self.fallback.clone()
            };
            for (_, _, once, sub, handler) in entries {
                if sub.active() {
                    if once {
                        sub.unsubscribe();
                    }
                    result = handler(payload.clone());
                }
            }
            self.listeners.borrow_mut().retain(|l| l.sub.active());
            result
        }
        pub fn listener_count(&self) -> i64 {
            self.listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .count() as i64
        }
        pub fn trace(&self) -> String {
            format!("hook listeners={}", self.listener_count())
        }
    }

    #[derive(Clone)]
    enum JetWatchTarget {
        Files { root: String },
        Process { pid: i64 },
        Port { host: String, port: i64 },
    }

    type JetWatchSnapshot = std::collections::BTreeMap<String, (u64, i64, bool)>;

    #[derive(Clone)]
    struct JetWatchState {
        target: JetWatchTarget,
        snapshot: JetWatchSnapshot,
        seen_ready: bool,
        active: bool,
    }

    #[derive(Clone)]
    pub struct WatchHandle {
        state: Rc<RefCell<JetWatchState>>,
        event: JetEvent<WatchEvent>,
    }

    #[derive(Clone)]
    pub struct WatchSet {
        handles: Rc<RefCell<Vec<WatchHandle>>>,
    }

    impl WatchHandle {
        pub fn files(path: String) -> Result<Self, IoError> {
            let snapshot = jet_watch_snapshot(&path)?;
            Ok(WatchHandle {
                state: Rc::new(RefCell::new(JetWatchState {
                    target: JetWatchTarget::Files { root: path },
                    snapshot,
                    seen_ready: false,
                    active: true,
                })),
                event: JetEvent::new(),
            })
        }

        pub fn process_pid(pid: i64) -> Self {
            WatchHandle {
                state: Rc::new(RefCell::new(JetWatchState {
                    target: JetWatchTarget::Process { pid },
                    snapshot: JetWatchSnapshot::new(),
                    seen_ready: jet_process_alive(pid),
                    active: true,
                })),
                event: JetEvent::new(),
            }
        }

        pub fn port(host: String, port: i64) -> Self {
            WatchHandle {
                state: Rc::new(RefCell::new(JetWatchState {
                    target: JetWatchTarget::Port { host, port },
                    snapshot: JetWatchSnapshot::new(),
                    seen_ready: false,
                    active: true,
                })),
                event: JetEvent::new(),
            }
        }

        pub fn poll(&self) -> Vec<WatchEvent> {
            let mut state = self.state.borrow_mut();
            if !state.active {
                return Vec::new();
            }
            let target = state.target.clone();
            let events = match target {
                JetWatchTarget::Files { root } => match jet_watch_snapshot(&root) {
                    Ok(next) => {
                        let events = jet_watch_diff(&state.snapshot, &next);
                        state.snapshot = next;
                        events
                    }
                    Err(e) => vec![WatchEvent {
                        domain: "file".to_string(),
                        kind: "Error".to_string(),
                        path: root.clone(),
                        detail: format!("{:?}", e),
                        pid: 0,
                        port: 0,
                    }],
                },
                JetWatchTarget::Process { pid } => {
                    let alive = jet_process_alive(pid);
                    if state.seen_ready && !alive {
                        state.seen_ready = false;
                        vec![WatchEvent {
                            domain: "process".to_string(),
                            kind: "Exited".to_string(),
                            path: String::new(),
                            detail: "process exited".to_string(),
                            pid,
                            port: 0,
                        }]
                    } else if !state.seen_ready && !alive {
                        vec![WatchEvent {
                            domain: "process".to_string(),
                            kind: "Exited".to_string(),
                            path: String::new(),
                            detail: "process is not running".to_string(),
                            pid,
                            port: 0,
                        }]
                    } else {
                        Vec::new()
                    }
                }
                JetWatchTarget::Port { host, port } => {
                    let ready = std::net::TcpStream::connect((host.as_str(), port as u16)).is_ok();
                    if ready && !state.seen_ready {
                        state.seen_ready = true;
                        vec![WatchEvent {
                            domain: "port".to_string(),
                            kind: "Ready".to_string(),
                            path: String::new(),
                            detail: format!("{}:{}", host, port),
                            pid: 0,
                            port,
                        }]
                    } else {
                        Vec::new()
                    }
                }
            };
            drop(state);
            for ev in events.iter().cloned() {
                self.event.emit(ev);
            }
            events
        }

        pub fn events(&self) -> Vec<WatchEvent> {
            self.poll()
        }

        pub fn on<F: Fn(WatchEvent) + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.event.on(scope, handler)
        }

        pub fn once<F: Fn(WatchEvent) + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.event.once(scope, handler)
        }

        pub fn cancel(&self) {
            self.state.borrow_mut().active = false;
        }

        pub fn active(&self) -> bool {
            self.state.borrow().active
        }

        pub fn summary(&self) -> String {
            match &self.state.borrow().target {
                JetWatchTarget::Files { root } => format!("watch file {}", root),
                JetWatchTarget::Process { pid } => format!("watch process {}", pid),
                JetWatchTarget::Port { host, port } => format!("watch port {}:{}", host, port),
            }
        }
    }

    impl WatchSet {
        pub fn new() -> Self {
            WatchSet {
                handles: Rc::new(RefCell::new(Vec::new())),
            }
        }
        pub fn add(&mut self, handle: WatchHandle) {
            self.handles.borrow_mut().push(handle);
        }
        pub fn poll(&self) -> Vec<WatchEvent> {
            let mut out = Vec::new();
            for handle in self.handles.borrow().iter() {
                out.extend(handle.poll());
            }
            out
        }
        pub fn events(&self) -> Vec<WatchEvent> {
            self.poll()
        }
        pub fn summary(&self) -> String {
            format!("watchset handles={}", self.handles.borrow().len())
        }
    }

    fn jet_watch_snapshot(root: &str) -> Result<JetWatchSnapshot, IoError> {
        let mut out = JetWatchSnapshot::new();
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(path) = stack.pop() {
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| io_error_at(IoOperation::Read, path.to_string_lossy().as_ref(), e))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let path_s = path.to_string_lossy().to_string();
            let is_dir = meta.is_dir();
            out.insert(path_s, (modified, meta.len() as i64, is_dir));
            if is_dir {
                for entry in std::fs::read_dir(&path)
                    .map_err(|e| io_error_at(IoOperation::Read, path.to_string_lossy().as_ref(), e))?
                {
                    let entry = entry.map_err(|e| io_error_at(IoOperation::Read, path.to_string_lossy().as_ref(), e))?;
                    stack.push(entry.path());
                }
            }
        }
        Ok(out)
    }

    fn jet_watch_diff(old: &JetWatchSnapshot, new: &JetWatchSnapshot) -> Vec<WatchEvent> {
        let mut out = Vec::new();
        for (path, facts) in new {
            match old.get(path) {
                None => out.push(jet_watch_event("Created", path, facts.2)),
                Some(prev) if prev != facts => out.push(jet_watch_event("Modified", path, facts.2)),
                _ => {}
            }
        }
        for (path, facts) in old {
            if !new.contains_key(path) {
                out.push(jet_watch_event("Removed", path, facts.2));
            }
        }
        out
    }

    fn jet_watch_event(kind: &str, path: &str, is_dir: bool) -> WatchEvent {
        WatchEvent {
            domain: "file".to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            detail: if is_dir { "dir" } else { "file" }.to_string(),
            pid: 0,
            port: 0,
        }
    }

    fn jet_process_alive(pid: i64) -> bool {
        if pid <= 0 {
            return false;
        }
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let current = std::process::id() as i64;
            pid == current
        }
    }

    impl super::JetShow for Closed {
        fn jet_show(&self) -> String {
            "Closed".to_string()
        }
    }

    // D-HONESTNUM1=A: Measurement<T> — a value paired with its standard uncertainty.
    // Arithmetic propagates uncertainty using the standard quadrature rules.
    // Only `JetMeasurement<f64>` (Float) is exposed to Jet programs.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct JetMeasurement<T: Copy> {
        value: T,
        uncertainty: T,
    }

    impl JetMeasurement<f64> {
        pub fn new(value: f64, uncertainty: f64) -> Self {
            JetMeasurement { value, uncertainty }
        }
        pub fn value(&self) -> f64 {
            self.value
        }
        pub fn uncertainty(&self) -> f64 {
            self.uncertainty
        }
        // Addition / subtraction: σ_z = sqrt(σ_a² + σ_b²)
        pub fn add(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value + other.value,
                uncertainty: (self.uncertainty * self.uncertainty
                    + other.uncertainty * other.uncertainty)
                    .sqrt(),
            }
        }
        pub fn sub(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value - other.value,
                uncertainty: (self.uncertainty * self.uncertainty
                    + other.uncertainty * other.uncertainty)
                    .sqrt(),
            }
        }
        // Multiplication: σ_z = sqrt((b·σ_a)² + (a·σ_b)²)
        pub fn mul(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value * other.value,
                uncertainty: ((other.value * self.uncertainty).powi(2)
                    + (self.value * other.uncertainty).powi(2))
                .sqrt(),
            }
        }
        // Division: σ_z = sqrt((σ_a/b)² + (a·σ_b/b²)²)
        pub fn div(&self, other: JetMeasurement<f64>) -> JetMeasurement<f64> {
            JetMeasurement {
                value: self.value / other.value,
                uncertainty: ((self.uncertainty / other.value).powi(2)
                    + (self.value * other.uncertainty / (other.value * other.value)).powi(2))
                .sqrt(),
            }
        }
    }

    impl super::JetShow for JetMeasurement<f64> {
        fn jet_show(&self) -> String {
            format!("{:?} \u{00b1} {:?}", self.value, self.uncertainty)
        }
    }
