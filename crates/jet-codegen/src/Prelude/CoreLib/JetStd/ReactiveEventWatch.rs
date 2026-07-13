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
        cleanup: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    }

    impl JetSubscription {
        fn new() -> Self {
            JetSubscription {
                active: Rc::new(Cell::new(true)),
                cleanup: Rc::new(RefCell::new(None)),
            }
        }
        fn set_cleanup<F: Fn() + 'static>(&self, cleanup: F) {
            *self.cleanup.borrow_mut() = Some(Rc::new(cleanup));
        }
        pub fn unsubscribe(&self) {
            if self.active.replace(false) {
                let cleanup = self.cleanup.borrow().clone();
                if let Some(cleanup) = cleanup {
                    cleanup();
                }
            }
        }
        pub fn active(&self) -> bool {
            self.active.get()
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
