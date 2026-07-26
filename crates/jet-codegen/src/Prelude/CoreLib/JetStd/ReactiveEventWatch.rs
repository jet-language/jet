    // ── D-REACT1=B + D-DATARACE1=C: opt-in reactive runtime ───────────────────
    // Reactivity is a LIBRARY, not core semantics (option B): ordinary bindings are
    // unchanged; these types are the explicit, opt-in surface. Pure std (I6).
    //
    // D-DATARACE1=C: reactive boxes use lock-ordered Arc storage so a task/channel
    // crossing cannot data-race and cannot lean on rustc Send (I2/I3).
    // Every Signal/Derived/Computed uses the synchronized form; `#Local` rejects
    // crossings (E1102) and `#Shared`/boundary crossings emit upgrade-report lines.
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, RwLock, Weak};

    type Observer = Arc<ReactiveObserver>;
    type WeakObserver = Weak<ReactiveObserver>;
    type DependencyCleanup = Box<dyn Fn() + Send + Sync>;
    type ObserverBody = Arc<dyn Fn() + Send + Sync>;

    struct ReactiveObserver {
        id: u64,
        active: AtomicBool,
        running: AtomicBool,
        body: Mutex<Option<ObserverBody>>,
        dependencies: Mutex<Vec<DependencyCleanup>>,
    }

    static JET_REACTIVE_NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);

    impl ReactiveObserver {
        fn new(body: ObserverBody) -> Observer {
            let id = JET_REACTIVE_NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
            Arc::new(ReactiveObserver {
                id,
                active: AtomicBool::new(true),
                running: AtomicBool::new(false),
                body: Mutex::new(Some(body)),
                dependencies: Mutex::new(Vec::new()),
            })
        }

        fn clear_dependencies(&self) {
            let cleanups = {
                let mut deps = self.dependencies.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::take(&mut *deps)
            };
            for cleanup in cleanups {
                cleanup();
            }
        }

        fn run(self: &Observer) {
            if !self.active.load(Ordering::Acquire)
                || self
                    .running
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return;
            }
            self.clear_dependencies();
            JET_REACTIVE_OBSERVERS.with(|stack| stack.borrow_mut().push(self.clone()));
            let body = self
                .body
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(body) = body {
                body();
            }
            JET_REACTIVE_OBSERVERS.with(|stack| {
                stack.borrow_mut().pop();
            });
            self.running.store(false, Ordering::Release);
        }

        fn track(&self, cleanup: DependencyCleanup) {
            self.dependencies
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(cleanup);
        }

        fn dispose(&self) {
            if self
                .active
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.clear_dependencies();
                *self.body.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
    }

    impl Drop for ReactiveObserver {
        fn drop(&mut self) {
            let cleanups = self
                .dependencies
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .drain(..)
                .collect::<Vec<_>>();
            for cleanup in cleanups {
                cleanup();
            }
        }
    }

    #[derive(Clone)]
    pub struct JetReactiveEffect {
        observer: Observer,
        owners: Arc<()>,
    }

    impl JetReactiveEffect {
        pub fn unsubscribe(&self) {
            self.observer.dispose();
        }

        pub fn active(&self) -> bool {
            self.observer.active.load(Ordering::Acquire)
        }
    }

    impl Drop for JetReactiveEffect {
        fn drop(&mut self) {
            if Arc::strong_count(&self.owners) == 1 {
                self.unsubscribe();
            }
        }
    }

    thread_local! {
        // Per-thread observer stack (which observer is recomputing on THIS thread).
        static JET_REACTIVE_OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) };
        // Marker/UI fire-and-retain scopes have a runtime owner. Public
        // `reactive.effect` returns its own Effect handle instead.
        static JET_REACTIVE_ROOT_EFFECTS: RefCell<Vec<JetReactiveEffect>> = const { RefCell::new(Vec::new()) };
    }

    fn jet_reactive_active_observer() -> Option<Observer> {
        JET_REACTIVE_OBSERVERS.with(|s| s.borrow().last().cloned())
    }

    struct SignalCell<T> {
        value: T,
        // Sources never own observers. Effect/derived/component owners retain them;
        // weak subscriptions let owner Drop deterministically detach the graph.
        subs: Vec<(u64, WeakObserver)>,
    }

    pub struct JetSignal<T> {
        cell: Arc<RwLock<SignalCell<T>>>,
    }

    impl<T> Clone for JetSignal<T> {
        fn clone(&self) -> Self {
            JetSignal {
                cell: self.cell.clone(),
            }
        }
    }

    impl<T: Clone + Send + Sync + 'static> JetSignal<T> {
        pub fn new(initial: T) -> JetSignal<T> {
            JetSignal {
                cell: Arc::new(RwLock::new(SignalCell {
                    value: initial,
                    subs: Vec::new(),
                })),
            }
        }
        pub fn get(&self) -> T {
            if let Some(obs) = jet_reactive_active_observer() {
                let added = {
                    let mut c = self.cell.write().unwrap_or_else(|e| e.into_inner());
                    c.subs.retain(|(_, weak)| weak.strong_count() > 0);
                    if c.subs.iter().any(|(id, _)| *id == obs.id) {
                        false
                    } else {
                        c.subs.push((obs.id, Arc::downgrade(&obs)));
                        true
                    }
                };
                if added {
                    let weak_cell = Arc::downgrade(&self.cell);
                    let id = obs.id;
                    obs.track(Box::new(move || {
                        if let Some(cell) = weak_cell.upgrade() {
                            cell.write()
                                .unwrap_or_else(|e| e.into_inner())
                                .subs
                                .retain(|(sub_id, _)| *sub_id != id);
                        }
                    }));
                }
            }
            self.cell
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .value
                .clone()
        }
        pub fn set(&self, value: T) {
            let subs = {
                let mut c = self.cell.write().unwrap_or_else(|e| e.into_inner());
                c.value = value;
                c.subs.retain(|(_, weak)| weak.strong_count() > 0);
                c.subs
                    .iter()
                    .filter_map(|(_, weak)| weak.upgrade())
                    .collect::<Vec<_>>()
            };
            for s in subs {
                s.run();
            }
        }
    }

    // A derived value is itself observable: it holds a current value plus its own
    // subscriber list, so effects (and other deriveds) that read it re-run when it
    // recomputes. The `_observer` it registers with its source signals recomputes the
    // value and then notifies the derived's own subscribers.
    pub struct JetDerived<T> {
        cell: Arc<RwLock<SignalCell<T>>>,
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

    impl<T: Clone + Send + Sync + 'static> JetDerived<T> {
        pub fn new<F: Fn() -> T + Send + Sync + 'static>(compute: F) -> JetDerived<T> {
            let compute = Arc::new(compute);
            let cell: Arc<RwLock<SignalCell<T>>> = Arc::new(RwLock::new(SignalCell {
                value: (compute)(),
                subs: Vec::new(),
            }));
            // The observer recomputes the value, then notifies the derived's own subs.
            let cell_for_obs = cell.clone();
            let compute_for_obs = compute.clone();
            let observer = ReactiveObserver::new(Arc::new(move || {
                let v = (compute_for_obs)();
                let subs = {
                    let mut c = cell_for_obs.write().unwrap_or_else(|e| e.into_inner());
                    c.value = v;
                    c.subs.retain(|(_, weak)| weak.strong_count() > 0);
                    c.subs
                        .iter()
                        .filter_map(|(_, weak)| weak.upgrade())
                        .collect::<Vec<_>>()
                };
                for s in subs {
                    s.run();
                }
            }));
            // Run once under observation to record the source-signal dependency set.
            observer.run();
            JetDerived {
                cell,
                _observer: observer,
            }
        }
        pub fn get(&self) -> T {
            // Reading a derived inside an observer subscribes that observer to it.
            if let Some(obs) = jet_reactive_active_observer() {
                let added = {
                    let mut c = self.cell.write().unwrap_or_else(|e| e.into_inner());
                    c.subs.retain(|(_, weak)| weak.strong_count() > 0);
                    if c.subs.iter().any(|(id, _)| *id == obs.id) {
                        false
                    } else {
                        c.subs.push((obs.id, Arc::downgrade(&obs)));
                        true
                    }
                };
                if added {
                    let weak_cell = Arc::downgrade(&self.cell);
                    let id = obs.id;
                    obs.track(Box::new(move || {
                        if let Some(cell) = weak_cell.upgrade() {
                            cell.write()
                                .unwrap_or_else(|e| e.into_inner())
                                .subs
                                .retain(|(sub_id, _)| *sub_id != id);
                        }
                    }));
                }
            }
            self.cell
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .value
                .clone()
        }
    }

    /// `reactive.effect(body)` — run `body` now, then re-run when a signal it read
    /// changes. The returned owner detaches on `unsubscribe()` or final-handle drop.
    pub fn jet_reactive_effect<F: FnMut() + Send + 'static>(body: F) -> JetReactiveEffect {
        let body = Mutex::new(body);
        let observer = ReactiveObserver::new(Arc::new(move || {
            let mut body = body.lock().unwrap_or_else(|e| e.into_inner());
            (*body)();
        }));
        observer.run();
        JetReactiveEffect {
            observer,
            owners: Arc::new(()),
        }
    }

    pub fn jet_reactive_effect_rooted<F: FnMut() + Send + 'static>(body: F) {
        let effect = jet_reactive_effect(body);
        JET_REACTIVE_ROOT_EFFECTS.with(|effects| effects.borrow_mut().push(effect));
    }

    /// D-REACTCORE1: `#Reactive` scope marker with runtime-owned lifetime.
    pub fn jet_reactive_scope<F: FnMut() + Send + 'static>(body: F) {
        jet_reactive_effect_rooted(body);
    }

    // D-EVENT1: first-party typed Event/Hook family. Values are ordinary Core
    // handles; the compiler knows their generic payload/result types.
    use std::cell::Cell;
    use std::rc::Rc;
    // AtomicU64/Ordering already imported above for the reactive runtime.

    static JET_EVENT_NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn jet_event_observe(
        source: &'static str,
        event_id: u64,
        owner_id: u64,
        subscription_id: u64,
        dispatch_id: u64,
        lifecycle: &'static str,
        queued: i64,
        blocked: i64,
        running: i64,
        capacity: i64,
        overflow: &'static str,
        priority: i64,
        failure: &'static str,
        terminal: &'static str,
    ) {
        super::jet_observe_event(super::JetObserveEvent {
            sequence: 0,
            source,
            event_id,
            owner_id,
            subscription_id,
            dispatch_id,
            lifecycle,
            queued,
            blocked,
            running,
            capacity,
            overflow,
            priority,
            failure,
            terminal,
        });
    }

    #[derive(Clone)]
    pub struct JetEventPolicy {
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
                if self.shared_active.as_ref().is_some_and(|active| {
                    !active.swap(false, std::sync::atomic::Ordering::AcqRel)
                }) {
                    return;
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
        id: u64,
        subs: Rc<RefCell<Vec<JetSubscription>>>,
        cancelled: Rc<Cell<bool>>,
        hard_cancellers: Rc<RefCell<Vec<(u64, Rc<dyn Fn()>)>>>,
    }

    impl JetEventScope {
        pub fn new() -> Self {
            JetEventScope {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                subs: Rc::new(RefCell::new(Vec::new())),
                cancelled: Rc::new(Cell::new(false)),
                hard_cancellers: Rc::new(RefCell::new(Vec::new())),
            }
        }
        fn track_hard_cancel<F: Fn() + 'static>(&self, owner_id: u64, cancel: F) {
            if self.cancelled.get() {
                return;
            }
            let mut cancellers = self.hard_cancellers.borrow_mut();
            if !cancellers.iter().any(|(id, _)| *id == owner_id) {
                cancellers.push((owner_id, Rc::new(cancel)));
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
            if self.cancelled.replace(true) {
                return;
            }
            let subs = std::mem::take(&mut *self.subs.borrow_mut());
            for sub in subs {
                sub.unsubscribe();
            }
            let cancellers = std::mem::take(&mut *self.hard_cancellers.borrow_mut());
            for (_, cancel) in cancellers {
                cancel();
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
        owner_id: u64,
        priority: i64,
        once: bool,
        sub: JetSubscription,
        handler: Rc<dyn Fn(T)>,
    }

    pub struct JetEvent<T: Clone + 'static> {
        id: u64,
        policy: JetEventPolicy,
        listeners: Rc<RefCell<Vec<JetListener<T>>>>,
    }

    impl<T: Clone + 'static> Clone for JetEvent<T> {
        fn clone(&self) -> Self {
            JetEvent {
                id: self.id,
                policy: self.policy.clone(),
                listeners: self.listeners.clone(),
            }
        }
    }

    impl<T: Clone + 'static> JetEvent<T> {
        pub fn new() -> Self {
            Self::with_policy(JetEventPolicy::sync())
        }
        pub fn with_policy(policy: JetEventPolicy) -> Self {
            JetEvent {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                policy,
                listeners: Rc::new(RefCell::new(Vec::new())),
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
            let sub = scope.track(JetSubscription::new());
            if !sub.active() {
                return sub;
            }
            let id = JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed);
            self.listeners.borrow_mut().push(JetListener {
                id,
                owner_id: scope.id,
                priority,
                once,
                sub: sub.clone(),
                handler: Rc::new(handler),
            });
            let listeners = Rc::downgrade(&self.listeners);
            let event_id = self.id;
            let owner_id = scope.id;
            sub.set_cleanup(move || {
                if let Some(listeners) = listeners.upgrade() {
                    listeners.borrow_mut().retain(|listener| listener.id != id);
                }
                jet_event_observe(
                    "Event", event_id, owner_id, id, 0, "Removed",
                    0, 0, 0, 0, "None", priority, "None", "None",
                );
            });
            jet_event_observe(
                "Event", self.id, scope.id, id, 0, "Subscribed",
                0, 0, 0, 0, "None", priority, "None", "None",
            );
            sub
        }
        pub fn emit(&self, payload: T) -> JetEventTrace {
            self.dispatch(payload)
        }
        fn dispatch(&self, payload: T) -> JetEventTrace {
            match self.policy.reentrancy {
                JetEventReentrancy::AllowDepthFirst => {}
            }
            let dispatch_id = JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed);
            jet_event_observe(
                "Event", self.id, 0, 0, dispatch_id, "DispatchStarted",
                0, 0, 0, 0, "None", 0, "None", "None",
            );
            let mut entries: Vec<(i64, u64, u64, bool, JetSubscription, Rc<dyn Fn(T)>)> = self
                .listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .map(|l| (l.priority, l.id, l.owner_id, l.once, l.sub.clone(), l.handler.clone()))
                .collect();
            entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut delivered = 0;
            for (priority, id, owner_id, once, sub, handler) in entries {
                if sub.active() {
                    // Consume once before invoking: a nested emit from this
                    // handler must not deliver it twice.
                    if once {
                        sub.unsubscribe();
                    }
                    jet_event_observe(
                        "Event", self.id, owner_id, id, dispatch_id, "HandlerStarted",
                        0, 0, 0, 0, "None", priority, "None", "None",
                    );
                    handler(payload.clone());
                    delivered += 1;
                    jet_event_observe(
                        "Event", self.id, owner_id, id, dispatch_id, "HandlerDelivered",
                        0, 0, 0, 0, "None", priority, "None", "None",
                    );
                }
            }
            self.listeners.borrow_mut().retain(|l| l.sub.active());
            jet_event_observe(
                "Event", self.id, 0, 0, dispatch_id, "Terminal",
                0, 0, 0, 0, "None", 0, "None", "Delivered",
            );
            JetEventTrace {
                delivered,
                queued: 0,
                dropped: 0,
                summary: format!("event delivered={delivered} queued=0 dropped=0"),
            }
        }
        pub fn listener_count(&self) -> i64 {
            self.listeners
                .borrow()
                .iter()
                .filter(|l| l.sub.active())
                .count() as i64
        }
        pub fn trace(&self) -> String {
            format!("listeners={} queued=0 dropped=0", self.listener_count())
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

    impl JetEventOverflow {
        fn observation(self) -> &'static str {
            match self {
                JetEventOverflow::Block => "Block",
                JetEventOverflow::DropNewest => "DropNewest",
                JetEventOverflow::DropOldest => "DropOldest",
            }
        }
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

    impl JetDispatchState {
        fn observation(self) -> &'static str {
            match self {
                JetDispatchState::Delivered => "Delivered",
                JetDispatchState::HandlerFailed => "HandlerFailed",
                JetDispatchState::DroppedNewest => "DroppedNewest",
                JetDispatchState::DroppedOldest => "DroppedOldest",
                JetDispatchState::Closed => "Closed",
                JetDispatchState::Cancelled => "Cancelled",
                JetDispatchState::DeadlineExceeded => "DeadlineExceeded",
            }
        }
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
        owner_id: u64,
        priority: i64,
        once: bool,
        active: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handler: std::sync::Arc<dyn Fn(T) -> Result<(), E> + Send + Sync>,
    }

    struct JetAsyncEntry<T, E: Clone + Send + 'static> {
        id: u64,
        event_id: u64,
        capacity: i64,
        overflow: JetEventOverflow,
        phase: std::sync::atomic::AtomicU8,
        accepted: std::sync::atomic::AtomicBool,
        payload: std::sync::Mutex<Option<T>>,
        terminal: std::sync::Mutex<Option<JetDispatchReport<E>>>,
        trace: std::sync::Mutex<Vec<String>>,
        wake: std::sync::Arc<super::ParkSlot>,
        control: std::sync::Arc<super::JetTaskControl>,
    }

    const JET_EVENT_PENDING: u8 = 0;
    const JET_EVENT_QUEUED: u8 = 1;
    const JET_EVENT_RUNNING: u8 = 2;
    const JET_EVENT_TERMINAL: u8 = 3;

    struct JetAsyncState<T, E: Clone + Send + 'static> {
        listeners: Vec<JetAsyncListener<T, E>>,
        queued: std::collections::VecDeque<std::sync::Arc<JetAsyncEntry<T, E>>>,
        blocked: std::collections::VecDeque<std::sync::Arc<JetAsyncEntry<T, E>>>,
        running_entries: Vec<std::sync::Arc<JetAsyncEntry<T, E>>>,
        running: usize,
        closed: bool,
        cancelled: bool,
    }

    pub struct JetAsyncEvent<T: Clone + Send + 'static, E: Clone + Send + 'static> {
        policy: JetAsyncPolicy,
        failure_policy: JetFailurePolicy,
        state: std::sync::Arc<std::sync::Mutex<JetAsyncState<T, E>>>,
        owner: Option<std::sync::Arc<()>>,
        owner_id: u64,
    }

    impl<T: Clone + Send + 'static, E: Clone + Send + 'static> Clone for JetAsyncEvent<T, E> {
        fn clone(&self) -> Self {
            JetAsyncEvent {
                policy: self.policy,
                failure_policy: self.failure_policy,
                state: self.state.clone(),
                owner: self.owner.clone(),
                owner_id: self.owner_id,
            }
        }
    }

    impl<T: Clone + Send + 'static, E: Clone + Send + 'static> Drop for JetAsyncEvent<T, E> {
        fn drop(&mut self) {
            let Some(owner) = &self.owner else { return; };
            if std::sync::Arc::strong_count(owner) != 1 { return; }
            Self::hard_cancel_state(&self.state);
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
                    running_entries: Vec::new(),
                    running: 0,
                    closed: false,
                    cancelled: false,
                })),
                owner: Some(std::sync::Arc::new(())),
                owner_id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
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
            let sub = scope.track(JetSubscription::shared(active.clone()));
            if !sub.active() {
                return sub;
            }
            let id = JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed);
            self.state.lock().unwrap().listeners.push(JetAsyncListener {
                id, owner_id: scope.id, priority, once, active: active.clone(), handler: std::sync::Arc::new(move |payload| handler(payload).into_dispatch_result()),
            });
            let cancel_state = std::sync::Arc::downgrade(&self.state);
            scope.track_hard_cancel(self.owner_id, move || {
                if let Some(state) = cancel_state.upgrade() {
                    Self::hard_cancel_state(&state);
                }
            });
            let state = std::sync::Arc::downgrade(&self.state);
            let event_id = self.owner_id;
            let owner_id = scope.id;
            let capacity = self.policy.capacity;
            let overflow = self.policy.overflow.observation();
            sub.set_cleanup(move || {
                if let Some(state) = state.upgrade() {
                    state.lock().unwrap().listeners.retain(|listener| listener.id != id);
                }
                jet_event_observe(
                    "AsyncEvent", event_id, owner_id, id, 0, "Removed",
                    -1, -1, -1, capacity, overflow, priority, "None", "None",
                );
            });
            let (queued, blocked, running) = {
                let state = self.state.lock().unwrap();
                (state.queued.len(), state.blocked.len(), state.running)
            };
            jet_event_observe(
                "AsyncEvent", self.owner_id, scope.id, id, 0, "Subscribed",
                queued as i64, blocked as i64, running as i64, self.policy.capacity,
                self.policy.overflow.observation(), priority, "None", "None",
            );
            sub
        }

        pub fn emit_async(&self, payload: T) -> super::jet_std::JetTask<JetDispatchReport<E>> {
            let control = super::JetTaskControl::new();
            let entry = std::sync::Arc::new(JetAsyncEntry {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                event_id: self.owner_id,
                capacity: self.policy.capacity,
                overflow: self.policy.overflow,
                phase: std::sync::atomic::AtomicU8::new(JET_EVENT_PENDING),
                accepted: std::sync::atomic::AtomicBool::new(false),
                payload: std::sync::Mutex::new(Some(payload)),
                terminal: std::sync::Mutex::new(None),
                trace: std::sync::Mutex::new(Vec::new()),
                wake: super::ParkSlot::new(),
                control: control.clone(),
            });
            {
                let mut state = self.state.lock().unwrap();
                if state.closed {
                    Self::complete_entry(&entry, JET_EVENT_PENDING, false, JetDispatchState::Closed);
                } else if state.cancelled {
                    Self::complete_entry(&entry, JET_EVENT_PENDING, false, JetDispatchState::Cancelled);
                } else if state.queued.len() < self.policy.capacity as usize {
                    let moved = entry.phase.compare_exchange(
                        JET_EVENT_PENDING,
                        JET_EVENT_QUEUED,
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                    ).is_ok();
                    debug_assert!(moved, "fresh async event entry must become queued once");
                    entry.accepted.store(true, std::sync::atomic::Ordering::Release);
                    entry.trace.lock().unwrap().push("queued".to_string());
                    state.queued.push_back(entry.clone());
                    jet_event_observe(
                        "AsyncEvent", self.owner_id, 0, 0, entry.id, "Queued",
                        state.queued.len() as i64, state.blocked.len() as i64, state.running as i64,
                        self.policy.capacity, self.policy.overflow.observation(),
                        0, "None", "None",
                    );
                } else {
                    match self.policy.overflow {
                        JetEventOverflow::Block => {
                            entry.trace.lock().unwrap().push("pending".to_string());
                            state.blocked.push_back(entry.clone());
                            jet_event_observe(
                                "AsyncEvent", self.owner_id, 0, 0, entry.id, "Backpressured",
                                state.queued.len() as i64, state.blocked.len() as i64, state.running as i64,
                                self.policy.capacity, self.policy.overflow.observation(),
                                0, "None", "None",
                            );
                        }
                        JetEventOverflow::DropNewest => {
                            Self::complete_entry(&entry, JET_EVENT_PENDING, false, JetDispatchState::DroppedNewest);
                        }
                        JetEventOverflow::DropOldest => {
                            if let Some(oldest) = state.queued.pop_front() {
                                Self::complete_entry(&oldest, JET_EVENT_QUEUED, true, JetDispatchState::DroppedOldest);
                            }
                            let moved = entry.phase.compare_exchange(
                                JET_EVENT_PENDING,
                                JET_EVENT_QUEUED,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            ).is_ok();
                            debug_assert!(moved, "fresh replacement entry must become queued once");
                            entry.accepted.store(true, std::sync::atomic::Ordering::Release);
                            entry.trace.lock().unwrap().push("queued".to_string());
                            state.queued.push_back(entry.clone());
                            jet_event_observe(
                                "AsyncEvent", self.owner_id, 0, 0, entry.id, "Queued",
                                state.queued.len() as i64, state.blocked.len() as i64, state.running as i64,
                                self.policy.capacity, self.policy.overflow.observation(),
                                0, "None", "None",
                            );
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
                owner_id: self.owner_id,
            };
            super::jet_std::JetTask::spawn_typed_deadline(
                move || event.run_entry(entry),
                control,
            )
        }

        fn complete_entry(
            entry: &std::sync::Arc<JetAsyncEntry<T, E>>,
            expected: u8,
            accepted: bool,
            state: JetDispatchState,
        ) -> bool {
            let mut terminal = entry.terminal.lock().unwrap();
            // Terminal is absorbing. Catch paths load the current phase after
            // an unwind, so a concurrent cancel/close winner can already have
            // published its report before they acquire this lock. Never allow
            // TERMINAL -> TERMINAL to succeed and replace that winner.
            if expected == JET_EVENT_TERMINAL {
                return false;
            }
            if entry.phase.compare_exchange(
                expected,
                JET_EVENT_TERMINAL,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ).is_err() {
                return false;
            }
            entry.accepted.store(accepted, std::sync::atomic::Ordering::Release);
            let trace = entry.trace.lock().unwrap().clone();
            *terminal = Some(JetDispatchReport::terminal_with_trace(
                accepted,
                state,
                trace,
            ));
            // Payload ownership ends at the same winning terminal transition.
            // A competing lifecycle path cannot drop or deliver it again.
            entry.payload.lock().unwrap().take();
            jet_event_observe(
                "AsyncEvent", entry.event_id, 0, 0, entry.id, "Terminal",
                -1, -1, -1, entry.capacity, entry.overflow.observation(),
                0, "None", state.observation(),
            );
            entry.wake.wake();
            true
        }

        fn complete_report(
            entry: &std::sync::Arc<JetAsyncEntry<T, E>>,
            report: JetDispatchReport<E>,
        ) -> bool {
            let mut terminal = entry.terminal.lock().unwrap();
            if entry.phase.compare_exchange(
                JET_EVENT_RUNNING,
                JET_EVENT_TERMINAL,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            ).is_err() {
                return false;
            }
            let failure = if report.failures.iter().any(|failure| matches!(failure, JetDispatchFailure::Panic(_))) {
                "Panic"
            } else if report.failures.is_empty() {
                "None"
            } else {
                "Handler"
            };
            jet_event_observe(
                "AsyncEvent", entry.event_id, 0, 0, entry.id, "Terminal",
                -1, -1, -1, entry.capacity, entry.overflow.observation(),
                0, failure, report.state.observation(),
            );
            *terminal = Some(report);
            entry.payload.lock().unwrap().take();
            entry.wake.wake();
            true
        }

        fn hard_cancel_state(state: &std::sync::Arc<std::sync::Mutex<JetAsyncState<T, E>>>) {
            let (queued, blocked, running) = {
                let mut state = state.lock().unwrap();
                if state.cancelled {
                    return;
                }
                state.cancelled = true;
                (
                    state.queued.drain(..).collect::<Vec<_>>(),
                    state.blocked.drain(..).collect::<Vec<_>>(),
                    state.running_entries.clone(),
                )
            };
            for entry in queued {
                Self::complete_entry(&entry, JET_EVENT_QUEUED, true, JetDispatchState::Cancelled);
            }
            for entry in blocked {
                Self::complete_entry(&entry, JET_EVENT_PENDING, false, JetDispatchState::Cancelled);
            }
            for entry in running {
                if entry.phase.load(std::sync::atomic::Ordering::Acquire) == JET_EVENT_RUNNING {
                    entry.control.cancel();
                }
            }
        }

        fn run_entry(&self, entry: std::sync::Arc<JetAsyncEntry<T, E>>) -> JetDispatchReport<E> {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run_entry_inner(&entry)));
            match run {
                Ok(report) => report,
                Err(payload) if payload.is::<super::JetCancelUnwind>() => {
                    let phase = entry.phase.load(std::sync::atomic::Ordering::Acquire);
                    let accepted = entry.accepted.load(std::sync::atomic::Ordering::Acquire);
                    Self::complete_entry(&entry, phase, accepted, JetDispatchState::Cancelled);
                    self.remove_entry(&entry);
                    entry.terminal.lock().unwrap().clone().unwrap_or_else(|| {
                        JetDispatchReport::terminal_with_trace(
                            accepted,
                            JetDispatchState::Cancelled,
                            entry.trace.lock().unwrap().clone(),
                        )
                    })
                }
                Err(payload) if payload.is::<super::JetDeadlineUnwind>() => {
                    let phase = entry.phase.load(std::sync::atomic::Ordering::Acquire);
                    let accepted = entry.accepted.load(std::sync::atomic::Ordering::Acquire);
                    Self::complete_entry(&entry, phase, accepted, JetDispatchState::DeadlineExceeded);
                    self.remove_entry(&entry);
                    entry.terminal.lock().unwrap().clone().unwrap_or_else(|| {
                        JetDispatchReport::terminal_with_trace(
                            accepted,
                            JetDispatchState::DeadlineExceeded,
                            entry.trace.lock().unwrap().clone(),
                        )
                    })
                }
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }

        fn run_entry_inner(&self, entry: &std::sync::Arc<JetAsyncEntry<T, E>>) -> JetDispatchReport<E> {
            loop {
                if let Some(report) = entry.terminal.lock().unwrap().clone() { return report; }
                // Deadline competes with Queued -> Running through the same
                // phase CAS. If it already expired, no handler observes payload.
                super::jet_deadline_check("async event dispatch");
                let listeners = {
                    let mut state = self.state.lock().unwrap();
                    if state.cancelled {
                        let phase = entry.phase.load(std::sync::atomic::Ordering::Acquire);
                        let accepted = entry.accepted.load(std::sync::atomic::Ordering::Acquire);
                        Self::complete_entry(entry, phase, accepted, JetDispatchState::Cancelled);
                        return entry.terminal.lock().unwrap().clone().unwrap_or_else(|| {
                            JetDispatchReport::terminal_with_trace(
                                accepted,
                                JetDispatchState::Cancelled,
                                entry.trace.lock().unwrap().clone(),
                            )
                        });
                    }
                    if let Some(pos) = state.blocked.iter().position(|item| item.id == entry.id) {
                        if state.closed {
                            state.blocked.remove(pos);
                            Self::complete_entry(entry, JET_EVENT_PENDING, false, JetDispatchState::Closed);
                            return entry.terminal.lock().unwrap().clone().unwrap_or_else(|| {
                                JetDispatchReport::terminal_with_trace(
                                    false,
                                    JetDispatchState::Closed,
                                    entry.trace.lock().unwrap().clone(),
                                )
                            });
                        }
                        if state.queued.len() < self.policy.capacity as usize {
                            state.blocked.remove(pos);
                            if entry.phase.compare_exchange(
                                JET_EVENT_PENDING,
                                JET_EVENT_QUEUED,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            ).is_ok() {
                                entry.accepted.store(true, std::sync::atomic::Ordering::Release);
                                entry.trace.lock().unwrap().push("queued".to_string());
                                state.queued.push_back(entry.clone());
                                jet_event_observe(
                                    "AsyncEvent", self.owner_id, 0, 0, entry.id, "Queued",
                                    state.queued.len() as i64, state.blocked.len() as i64,
                                    state.running as i64, self.policy.capacity,
                                    self.policy.overflow.observation(), 0, "None", "None",
                                );
                            }
                        }
                    }
                    if state.running == 0
                        && state.queued.front().is_some_and(|front| front.id == entry.id)
                    {
                        state.queued.pop_front();
                        if entry.phase.compare_exchange(
                            JET_EVENT_QUEUED,
                            JET_EVENT_RUNNING,
                            std::sync::atomic::Ordering::AcqRel,
                            std::sync::atomic::Ordering::Acquire,
                        ).is_err() {
                            None
                        } else {
                        state.running += 1;
                        state.running_entries.push(entry.clone());
                        entry.trace.lock().unwrap().push("running".to_string());
                        jet_event_observe(
                            "AsyncEvent", self.owner_id, 0, 0, entry.id, "Running",
                            state.queued.len() as i64, state.blocked.len() as i64,
                            state.running as i64, self.policy.capacity,
                            self.policy.overflow.observation(), 0, "None", "None",
                        );
                        let mut listeners = state.listeners.iter()
                            .filter_map(|listener| {
                                if !listener.active.load(std::sync::atomic::Ordering::Acquire) {
                                    return None;
                                }
                                if listener.once && listener.active.compare_exchange(
                                    true,
                                    false,
                                    std::sync::atomic::Ordering::AcqRel,
                                    std::sync::atomic::Ordering::Acquire,
                                ).is_err() {
                                    return None;
                                }
                                if listener.once {
                                    jet_event_observe(
                                        "AsyncEvent", self.owner_id, listener.owner_id,
                                        listener.id, entry.id, "Removed", -1, -1, -1,
                                        self.policy.capacity, self.policy.overflow.observation(),
                                        listener.priority, "None", "None",
                                    );
                                }
                                Some((listener.priority, listener.id, listener.owner_id, listener.once, listener.active.clone(), listener.handler.clone()))
                            })
                            .collect::<Vec<_>>();
                        listeners.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                        for blocked in &state.blocked { blocked.wake.wake(); }
                        Some(listeners)
                        }
                    } else { None }
                };
                if let Some(listeners) = listeners {
                    let Some(payload) = entry.payload.lock().unwrap().take() else {
                        let accepted = entry.accepted.load(std::sync::atomic::Ordering::Acquire);
                        Self::complete_entry(entry, JET_EVENT_RUNNING, accepted, JetDispatchState::Cancelled);
                        self.remove_entry(entry);
                        return entry.terminal.lock().unwrap().clone().unwrap_or_else(|| {
                            JetDispatchReport::terminal_with_trace(
                                accepted,
                                JetDispatchState::Cancelled,
                                entry.trace.lock().unwrap().clone(),
                            )
                        });
                    };
                    let mut report = JetDispatchReport {
                        accepted: true,
                        state: JetDispatchState::Delivered,
                        delivered_handlers: 0,
                        failures: Vec::new(),
                        trace: entry.trace.lock().unwrap().clone(),
                    };
                    for (handler_index, (priority, subscription_id, owner_id, once, active, handler)) in listeners.into_iter().enumerate() {
                        // A once listener is reserved by clearing `active` while
                        // the async state lock is held. Its owning snapshot must
                        // still invoke it; later snapshots cannot reserve it.
                        if !once && !active.load(std::sync::atomic::Ordering::Acquire) { continue; }
                        report.delivered_handlers += 1;
                        jet_event_observe(
                            "AsyncEvent", self.owner_id, owner_id, subscription_id, entry.id,
                            "HandlerStarted", -1, -1, -1, self.policy.capacity,
                            self.policy.overflow.observation(), priority, "None", "None",
                        );
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(payload.clone())));
                        match outcome {
                            Ok(Ok(())) => {
                                report.trace.push(format!("handler:{handler_index}:delivered"));
                                jet_event_observe(
                                    "AsyncEvent", self.owner_id, owner_id, subscription_id,
                                    entry.id, "HandlerDelivered", -1, -1, -1,
                                    self.policy.capacity, self.policy.overflow.observation(),
                                    priority, "None", "None",
                                );
                            }
                            Ok(Err(error)) => {
                                jet_event_observe(
                                    "AsyncEvent", self.owner_id, owner_id, subscription_id,
                                    entry.id, "HandlerFailed", -1, -1, -1,
                                    self.policy.capacity, self.policy.overflow.observation(),
                                    priority, "Handler", "None",
                                );
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
                                if payload.is::<super::JetCancelUnwind>()
                                    || payload.is::<super::JetDeadlineUnwind>()
                                {
                                    std::panic::resume_unwind(payload);
                                }
                                let message = payload.downcast_ref::<String>().cloned()
                                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                                    .unwrap_or_else(|| "event handler panicked".to_string());
                                report.state = JetDispatchState::HandlerFailed;
                                jet_event_observe(
                                    "AsyncEvent", self.owner_id, owner_id, subscription_id,
                                    entry.id, "HandlerFailed", -1, -1, -1,
                                    self.policy.capacity, self.policy.overflow.observation(),
                                    priority, "Panic", "None",
                                );
                                report.failures.push(JetDispatchFailure::Panic(message.clone()));
                                report.trace.push(format!("handler:{handler_index}:panic:{message}"));
                                break;
                            }
                        }
                    }
                    report.trace.push(format!("terminal:{:?}", report.state));
                    Self::complete_report(entry, report.clone());
                    let wakes = {
                        let mut state = self.state.lock().unwrap();
                        let before = state.running_entries.len();
                        state.running_entries.retain(|item| item.id != entry.id);
                        if state.running_entries.len() != before {
                            state.running = state.running.saturating_sub(1);
                        }
                        state.listeners.retain(|listener| listener.active.load(std::sync::atomic::Ordering::Acquire));
                        state.queued.front().into_iter().chain(state.blocked.iter()).map(|item| item.wake.clone()).collect::<Vec<_>>()
                    };
                    for wake in wakes { wake.wake(); }
                    return entry.terminal.lock().unwrap().clone().unwrap_or(report);
                }
                super::jet_scheduler_yield("async event dispatch", &entry.wake, None);
            }
        }

        fn remove_entry(&self, entry: &std::sync::Arc<JetAsyncEntry<T, E>>) {
            let wakes = {
                let mut state = self.state.lock().unwrap();
                state.queued.retain(|item| item.id != entry.id);
                state.blocked.retain(|item| item.id != entry.id);
                let before = state.running_entries.len();
                state.running_entries.retain(|item| item.id != entry.id);
                if state.running_entries.len() != before {
                    state.running = state.running.saturating_sub(1);
                }
                state.queued.front().into_iter().chain(state.blocked.iter()).map(|item| item.wake.clone()).collect::<Vec<_>>()
            };
            for wake in wakes { wake.wake(); }
        }

        pub fn close(&self) {
            let blocked = {
                let mut state = self.state.lock().unwrap();
                if state.closed { return; }
                state.closed = true;
                state.blocked.drain(..).collect::<Vec<_>>()
            };
            for entry in blocked {
                Self::complete_entry(&entry, JET_EVENT_PENDING, false, JetDispatchState::Closed);
            }
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

    // D-EVENT-CONTINUE1=C: one typed hook mechanism for transform/cancel/fail.
    #[derive(Clone, Copy)]
    pub enum JetHookPolicy {
        FirstCancelElseTransform,
    }

    #[derive(Clone)]
    pub enum JetHookDecision<T, E> {
        Continue,
        Transform(T),
        Cancel,
        Fail(E),
    }

    #[derive(Clone)]
    pub enum JetHookOutcome<T, E> {
        Continue(T),
        Cancel,
        Fail(E),
    }

    struct JetDecisionHookListener<T, E> {
        id: u64,
        owner_id: u64,
        priority: i64,
        once: bool,
        sub: JetSubscription,
        handler: Rc<dyn Fn(T) -> JetHookDecision<T, E>>,
    }

    pub struct JetDecisionHook<T: Clone + 'static, E: Clone + 'static> {
        id: u64,
        policy: JetHookPolicy,
        listeners: Rc<RefCell<Vec<JetDecisionHookListener<T, E>>>>,
    }

    impl<T: Clone + 'static, E: Clone + 'static> Clone for JetDecisionHook<T, E> {
        fn clone(&self) -> Self {
            JetDecisionHook {
                id: self.id,
                policy: self.policy,
                listeners: self.listeners.clone(),
            }
        }
    }

    impl<T: Clone + 'static, E: Clone + 'static> JetDecisionHook<T, E> {
        pub fn new(policy: JetHookPolicy) -> Self {
            JetDecisionHook {
                id: JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed),
                policy,
                listeners: Rc::new(RefCell::new(Vec::new())),
            }
        }
        pub fn on<F: Fn(T) -> JetHookDecision<T, E> + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.on_priority(scope, 0, handler)
        }
        pub fn once<F: Fn(T) -> JetHookDecision<T, E> + 'static>(
            &self,
            scope: &JetEventScope,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, 0, true, handler)
        }
        pub fn on_priority<F: Fn(T) -> JetHookDecision<T, E> + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            handler: F,
        ) -> JetSubscription {
            self.add(scope, priority, false, handler)
        }
        fn add<F: Fn(T) -> JetHookDecision<T, E> + 'static>(
            &self,
            scope: &JetEventScope,
            priority: i64,
            once: bool,
            handler: F,
        ) -> JetSubscription {
            let sub = scope.track(JetSubscription::new());
            if !sub.active() {
                return sub;
            }
            let id = JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed);
            self.listeners.borrow_mut().push(JetDecisionHookListener {
                id,
                owner_id: scope.id,
                priority,
                once,
                sub: sub.clone(),
                handler: Rc::new(handler),
            });
            let listeners = Rc::downgrade(&self.listeners);
            let event_id = self.id;
            let owner_id = scope.id;
            sub.set_cleanup(move || {
                if let Some(listeners) = listeners.upgrade() {
                    listeners.borrow_mut().retain(|listener| listener.id != id);
                }
                jet_event_observe(
                    "DecisionHook", event_id, owner_id, id, 0, "Removed",
                    0, 0, 0, 0, "None", priority, "None", "None",
                );
            });
            jet_event_observe(
                "DecisionHook", self.id, scope.id, id, 0, "Subscribed",
                0, 0, 0, 0, "None", priority, "None", "None",
            );
            sub
        }
        pub fn run(&self, payload: T) -> JetHookOutcome<T, E> {
            match self.policy {
                JetHookPolicy::FirstCancelElseTransform => {}
            }
            let dispatch_id = JET_EVENT_NEXT_ID.fetch_add(1, Ordering::Relaxed);
            jet_event_observe(
                "DecisionHook", self.id, 0, 0, dispatch_id, "DispatchStarted",
                0, 0, 0, 0, "None", 0, "None", "None",
            );
            let mut entries: Vec<(
                i64,
                u64,
                u64,
                bool,
                JetSubscription,
                Rc<dyn Fn(T) -> JetHookDecision<T, E>>,
            )> = self.listeners.borrow().iter()
                .filter(|listener| listener.sub.active())
                .map(|listener| (
                    listener.priority,
                    listener.id,
                    listener.owner_id,
                    listener.once,
                    listener.sub.clone(),
                    listener.handler.clone(),
                ))
                .collect();
            entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut current = payload;
            for (priority, subscription_id, owner_id, once, sub, handler) in entries {
                if !sub.active() { continue; }
                if once { sub.unsubscribe(); }
                jet_event_observe(
                    "DecisionHook", self.id, owner_id, subscription_id, dispatch_id,
                    "HandlerStarted", 0, 0, 0, 0, "None", priority, "None", "None",
                );
                match handler(current.clone()) {
                    JetHookDecision::Continue => jet_event_observe(
                        "DecisionHook", self.id, owner_id, subscription_id, dispatch_id,
                        "HandlerContinue", 0, 0, 0, 0, "None", priority, "None", "None",
                    ),
                    JetHookDecision::Transform(value) => {
                        current = value;
                        jet_event_observe(
                            "DecisionHook", self.id, owner_id, subscription_id, dispatch_id,
                            "HandlerTransform", 0, 0, 0, 0, "None", priority, "None", "None",
                        );
                    }
                    JetHookDecision::Cancel => {
                        jet_event_observe(
                            "DecisionHook", self.id, owner_id, subscription_id, dispatch_id,
                            "HandlerCancel", 0, 0, 0, 0, "None", priority, "None", "None",
                        );
                        jet_event_observe(
                            "DecisionHook", self.id, 0, 0, dispatch_id, "Terminal",
                            0, 0, 0, 0, "None", 0, "None", "Cancel",
                        );
                        return JetHookOutcome::Cancel;
                    }
                    JetHookDecision::Fail(error) => {
                        jet_event_observe(
                            "DecisionHook", self.id, owner_id, subscription_id, dispatch_id,
                            "HandlerFail", 0, 0, 0, 0, "None", priority, "Handler", "None",
                        );
                        jet_event_observe(
                            "DecisionHook", self.id, 0, 0, dispatch_id, "Terminal",
                            0, 0, 0, 0, "None", 0, "Handler", "Fail",
                        );
                        return JetHookOutcome::Fail(error);
                    }
                }
            }
            self.listeners.borrow_mut().retain(|listener| listener.sub.active());
            jet_event_observe(
                "DecisionHook", self.id, 0, 0, dispatch_id, "Terminal",
                0, 0, 0, 0, "None", 0, "None", "Continue",
            );
            JetHookOutcome::Continue(current)
        }
        pub fn listener_count(&self) -> i64 {
            self.listeners.borrow().iter().filter(|listener| listener.sub.active()).count() as i64
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

    // JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe
    #[cfg(unix)]
    fn jet_process_alive(pid: i64) -> bool {
        if pid <= 0 {
            return false;
        }
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        let Some(pid) = i32::try_from(pid).ok() else {
            return false;
        };
        let result = unsafe { kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
    }

    #[cfg(windows)]
    fn jet_process_alive(pid: i64) -> bool {
        if pid <= 0 {
            return false;
        }
        const SYNCHRONIZE: u32 = 0x0010_0000;
        const WAIT_TIMEOUT: u32 = 0x0000_0102;
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let Some(pid) = u32::try_from(pid).ok() else {
            return false;
        };
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let alive = unsafe { WaitForSingleObject(handle, 0) == WAIT_TIMEOUT };
        unsafe { CloseHandle(handle) };
        alive
    }

    #[cfg(not(any(unix, windows)))]
    fn jet_process_alive(pid: i64) -> bool {
        pid > 0 && pid == std::process::id() as i64
    }
    // JET_VETTED_UNSAFE_END: jet_watch_process_probe

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
