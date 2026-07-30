    // ── D-SIMD2 / D-LINALG1: built-in math value types ───────────────────────────
    // SIMD lanes + linear-algebra vectors/matrices. The pinned stable rustc has no
    // `std::simd` (portable_simd is unstable), so lane types are a SCALAR-ARRAY
    // fallback: a `[f32; 4]` / `[f64; 2]` newtype with element-wise ops. This is
    // correct and memory-safe by construction (I1) — no intrinsics, no feature gate,
    // no `un`+`safe`. A `std::simd` backend can replace these structs later behind
    // the same surface without touching generated code. Linalg types are column-major
    // F64 arrays. All ops return fresh values (value semantics); `Copy` for ergonomics.

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct F32x4(pub [f32; 4]);
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct F64x2(pub [f64; 2]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Vec2(pub [f64; 2]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Vec3(pub [f64; 3]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Vec4(pub [f64; 4]);
    // Column-major: element (row r, col c) is `.0[c * N + r]`.
    #[derive(Clone, Copy, PartialEq)]
    pub struct Mat3(pub [f64; 9]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Mat4(pub [f64; 16]);

    macro_rules! jet_lane_ops {
        ($T:ident, $E:ty, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] * o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Div for $T {
                type Output = $T;
                fn div(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] / o.0[i];
                    }
                    $T(r)
                }
            }
        };
    }
    jet_lane_ops!(F32x4, f32, 4);
    jet_lane_ops!(F64x2, f64, 2);

    macro_rules! jet_vec_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            // `v * w` is element-wise (Hadamard); the dot/cross products are methods.
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] * o.0[i];
                    }
                    $T(r)
                }
            }
        };
    }
    jet_vec_ops!(Vec2, 2);
    jet_vec_ops!(Vec3, 3);
    jet_vec_ops!(Vec4, 4);

    macro_rules! jet_mat_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..($N * $N) {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..($N * $N) {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            // `m * n` is matrix multiply (column-major).
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = [0.0f64; $N * $N];
                    for c in 0..$N {
                        for row in 0..$N {
                            let mut acc = 0.0f64;
                            for k in 0..$N {
                                acc += self.0[k * $N + row] * o.0[c * $N + k];
                            }
                            r[c * $N + row] = acc;
                        }
                    }
                    $T(r)
                }
            }
        };
    }
    jet_mat_ops!(Mat3, 3);
    jet_mat_ops!(Mat4, 4);

    // `Mat * Vec` transforms the vector (column-major).
    impl std::ops::Mul<Vec3> for Mat3 {
        type Output = Vec3;
        fn mul(self, v: Vec3) -> Vec3 {
            let mut r = [0.0f64; 3];
            for row in 0..3 {
                let mut a = 0.0f64;
                for k in 0..3 {
                    a += self.0[k * 3 + row] * v.0[k];
                }
                r[row] = a;
            }
            Vec3(r)
        }
    }
    impl std::ops::Mul<Vec4> for Mat4 {
        type Output = Vec4;
        fn mul(self, v: Vec4) -> Vec4 {
            let mut r = [0.0f64; 4];
            for row in 0..4 {
                let mut a = 0.0f64;
                for k in 0..4 {
                    a += self.0[k * 4 + row] * v.0[k];
                }
                r[row] = a;
            }
            Vec4(r)
        }
    }

    impl super::JetShow for F32x4 {
        fn jet_show(&self) -> String {
            format!("F32x4({:?})", self.0)
        }
    }
    impl super::JetShow for F64x2 {
        fn jet_show(&self) -> String {
            format!("F64x2({:?})", self.0)
        }
    }
    macro_rules! jet_math_debug {
        ($type:ident, $($field:literal => $index:literal),+ $(,)?) => {
            impl std::fmt::Debug for $type {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let mut debug = f.debug_struct(stringify!($type));
                    $(debug.field($field, &self.0[$index]);)+
                    debug.finish()
                }
            }
            impl super::JetShow for $type {
                fn jet_show(&self) -> String {
                    format!("{self:?}")
                }
            }
            impl super::JetDebug for $type {
                fn jet_debug(&self) -> String {
                    format!("{self:?}")
                }
            }
        };
    }
    jet_math_debug!(Vec2, "x" => 0, "y" => 1);
    jet_math_debug!(Vec3, "x" => 0, "y" => 1, "z" => 2);
    jet_math_debug!(Vec4, "x" => 0, "y" => 1, "z" => 2, "w" => 3);
    jet_math_debug!(
        Mat3,
        "m00" => 0, "m10" => 1, "m20" => 2,
        "m01" => 3, "m11" => 4, "m21" => 5,
        "m02" => 6, "m12" => 7, "m22" => 8,
    );
    jet_math_debug!(
        Mat4,
        "m00" => 0, "m10" => 1, "m20" => 2, "m30" => 3,
        "m01" => 4, "m11" => 5, "m21" => 6, "m31" => 7,
        "m02" => 8, "m12" => 9, "m22" => 10, "m32" => 11,
        "m03" => 12, "m13" => 13, "m23" => 14, "m33" => 15,
    );

    struct JetTaskState<T: Send + 'static> {
        handle: std::sync::Mutex<Option<super::JetSchedulerJoin<T>>>,
        control: std::sync::Arc<super::JetTaskControl>,
        // Typed operations such as AsyncEvent convert their inherited deadline
        // into their own result value. Re-checking the caller deadline after
        // join would replace that value with E3003 and violate the typed API.
        skip_join_deadline: bool,
    }

    trait JetTaskGroupChild: Send + Sync {
        fn cancel(&self);
        fn join(&self);
    }

    impl<T: Send + 'static> JetTaskGroupChild for JetTaskState<T> {
        fn cancel(&self) {
            if self.handle.lock().unwrap().is_some() {
                self.control.cancel();
            }
        }

        fn join(&self) {
            if let Some(handle) = self.handle.lock().unwrap().take() {
                handle.join();
            }
        }
    }

    /// D-TASKSCOPE1=A / D-TASKGROUP-PARAM1=A: the internal runtime identity
    /// shared by a lexical taskgroup and every named helper that receives it.
    pub struct JetTaskGroup {
        children: JetTaskGroupRuntime<std::sync::Arc<dyn JetTaskGroupChild>>,
    }

    impl JetTaskGroup {
        pub fn new() -> Self {
            Self {
                children: JetTaskGroupRuntime::new(),
            }
        }

        pub fn spawn<F, T>(&self, f: F) -> JetTask<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            let task = JetTask::spawn(f);
            self.children.register(task.state.clone());
            task
        }

        pub fn close(&self) {
            self.children
                .close_with(|child| child.cancel(), |child| child.join());
        }
    }

    impl Drop for JetTaskGroup {
        fn drop(&mut self) {
            self.close();
        }
    }

    pub struct JetTask<T: Send + 'static> {
        state: std::sync::Arc<JetTaskState<T>>,
    }

    impl<T: Send + 'static> Default for JetTask<T> {
        fn default() -> Self {
            JetTask {
                state: std::sync::Arc::new(JetTaskState {
                    handle: std::sync::Mutex::new(None),
                    control: super::JetTaskControl::new(),
                    skip_join_deadline: false,
                }),
            }
        }
    }
    impl<T: Send + 'static> JetTask<T> {
        pub fn spawn<F: FnOnce() -> T + Send + 'static>(f: F) -> JetTask<T> {
            let inherited_deadline = super::jet_ctx_deadline_ms();
            let control = super::JetTaskControl::new();
            JetTask {
                state: std::sync::Arc::new(JetTaskState {
                    handle: std::sync::Mutex::new(Some(
                        super::jet_scheduler_spawn_blocking_with_control(
                            move || {
                                let _deadline_guard =
                                    inherited_deadline.map(super::jet_ctx_push_deadline);
                                f()
                            },
                            control.clone(),
                        ),
                    )),
                    control,
                    skip_join_deadline: false,
                }),
            }
        }
        pub(crate) fn spawn_typed_deadline<F: FnOnce() -> T + Send + 'static>(
            f: F,
            control: std::sync::Arc<super::JetTaskControl>,
        ) -> JetTask<T> {
            let inherited_deadline = super::jet_ctx_deadline_ms();
            JetTask {
                state: std::sync::Arc::new(JetTaskState {
                    handle: std::sync::Mutex::new(Some(
                        super::jet_scheduler_spawn_blocking_with_control(
                            move || {
                                let _deadline_guard =
                                    inherited_deadline.map(super::jet_ctx_push_deadline);
                                let _typed_deadline_boundary =
                                    super::JetTypedDeadlineBoundary::enter();
                                f()
                            },
                            control.clone(),
                        ),
                    )),
                    control,
                    skip_join_deadline: true,
                }),
            }
        }
        // D-COROUTINE1=A: control-plane hooks on the M:N scheduler substrate.
        pub fn pause(&self) {
            self.state.control.pause();
        }
        pub fn resume(&self) {
            self.state.control.resume();
        }
        pub fn cancel(&self) {
            self.state.control.cancel();
        }
        pub fn trace(&self) -> String {
            let paused = self
                .state
                .control
                .paused
                .load(std::sync::atomic::Ordering::Relaxed);
            let cancel = self
                .state
                .control
                .cancelled
                .load(std::sync::atomic::Ordering::Relaxed);
            format!("paused={},cancel={}", paused, cancel)
        }
        pub fn join(self) -> T {
            if !self.state.skip_join_deadline {
                super::jet_deadline_check("task join");
            }
            let v = self
                .state
                .handle
                .lock()
                .unwrap()
                .take()
                .expect("task already joined")
                .join();
            if !self.state.skip_join_deadline {
                super::jet_deadline_check("task join");
            }
            v
        }
        pub fn detach(self) {
            let _ = self.state.handle.lock().unwrap().take();
        }
    }

    /// D-CONCCOMB1=A: join every handle; fail fast and cancel siblings on error.
    pub fn jet_task_all<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> Vec<T> {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|t| {
                (
                    t.state
                        .handle
                        .lock()
                        .unwrap()
                        .take()
                        .expect("all: task already joined"),
                    t.state.control.clone(),
                )
            })
            .collect();
        super::jet_scheduler_all(entries)
    }

    /// D-CONCCOMB1=A + D-RACEWIN1: first successful result; cancel siblings via scheduler.
    pub fn jet_task_race<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> T {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|t| {
                (
                    t.state
                        .handle
                        .lock()
                        .unwrap()
                        .take()
                        .expect("race: task already joined"),
                    t.state.control.clone(),
                )
            })
            .collect();
        super::jet_scheduler_race(entries)
    }

    /// D-CONCCOMB1=A: first completed result (success or failure path — v1 propagates panic).
    pub fn jet_task_any<T: Send + 'static>(tasks: Vec<JetTask<T>>) -> T {
        let entries: Vec<_> = tasks
            .into_iter()
            .map(|t| {
                (
                    t.state
                        .handle
                        .lock()
                        .unwrap()
                        .take()
                        .expect("any: task already joined"),
                    t.state.control.clone(),
                )
            })
            .collect();
        super::jet_scheduler_any(entries)
    }

    /// D-CONCSELECT1=A: fluent select builder accumulated at compile time, executed at `.wait()`.
    pub struct JetSelectBuilder<T: Send + 'static> {
        recvs: Vec<JetReceiver<T>>,
        after_values: Vec<(i64, T)>,
    }

    impl<T: Send + 'static> JetSelectBuilder<T> {
        pub fn start() -> JetSelectBuilder<T> {
            JetSelectBuilder {
                recvs: Vec::new(),
                after_values: Vec::new(),
            }
        }
        pub fn recv(mut self, ch: JetReceiver<T>) -> JetSelectBuilder<T> {
            self.recvs.push(ch);
            self
        }
        pub fn after(mut self, ms: i64) -> JetSelectBuilder<T>
        where
            T: Default,
        {
            self.after_values.push((ms, T::default()));
            self
        }
        pub fn after_value(mut self, ms: i64, value: T) -> JetSelectBuilder<T> {
            self.after_values.push((ms, value));
            self
        }
        pub fn read(self, _stream: super::JetTCPStream) -> JetSelectBuilder<T> {
            self
        }
        pub fn wait(self) -> T {
            let recv_refs: Vec<&JetReceiver<T>> = self.recvs.iter().collect();
            jet_select_wait(&recv_refs, self.after_values)
        }
    }

    /// D-CONCSELECT1=A: multiplex channel/timer arms registered by `g.select()`.
    pub fn jet_select_wait<T: Send + 'static>(
        recvs: &[&JetReceiver<T>],
        after_values: Vec<(i64, T)>,
    ) -> T {
        let inners: Vec<_> = recvs.iter().map(|c| c.inner.select_inner()).collect();
        let timers: Vec<u64> = after_values.iter().map(|(ms, _)| (*ms).max(0) as u64).collect();
        match super::jet_scheduler_select(inners, timers) {
            super::JetSelectOutcome::Recv { value, .. } => value,
            super::JetSelectOutcome::After { arm } => after_values
                .into_iter()
                .nth(arm)
                .map(|(_, value)| value)
                .unwrap_or_else(|| super::jet_panic("<core.tasks>", 0, "select timer arm missing value")),
            super::JetSelectOutcome::Closed => {
                super::jet_panic("<core.tasks>", 0, "select closed");
            }
        }
    }

    /// D-TUPLE-DESTRUCT1: `tasks.channel<T>()` — mirrors Rust's `mpsc::channel()`:
    /// returns the `(Sender<T>, Receiver<T>)` pair directly (no combined "Channel"
    /// handle, and no `.sender()` method — a second sender is `tx.clone()`).
    pub fn channel<T: Send>() -> (JetSender<T>, JetReceiver<T>) {
        let inner = super::JetSchedulerChannel::new();
        let tx = inner.sender();
        (JetSender { tx }, JetReceiver { inner })
    }

    /// D-TASKRUNTIME1=A: bounded channel; `capacity` is a real memory/backpressure bound.
    pub fn channel_bounded<T: Send>(capacity: i64) -> (JetSender<T>, JetReceiver<T>) {
        let inner = super::JetSchedulerChannel::bounded(capacity.max(1) as usize);
        let tx = inner.sender();
        (JetSender { tx }, JetReceiver { inner })
    }

    /// D-TASKRUNTIME1=A: one-shot timer channel; wakes through the scheduler timer wheel.
    pub fn after(ms: i64) -> JetReceiver<()> {
        let (tx, rx) = channel::<()>();
        let delay = ms.max(0) as u64;
        let _ = super::jet_scheduler_spawn(move || {
            super::jet_scheduler_sleep_ms(delay);
            tx.send(());
        });
        rx
    }

    /// D-TASKRUNTIME1=A: one-shot typed timer channel for select timeout values.
    pub fn after_value<T: Send + 'static>(ms: i64, value: T) -> JetReceiver<T> {
        let (tx, rx) = channel::<T>();
        let delay = ms.max(0) as u64;
        let _ = super::jet_scheduler_spawn(move || {
            super::jet_scheduler_sleep_ms(delay);
            tx.send(value);
        });
        rx
    }

    /// D-TASKRUNTIME1=A: interval timer channel; sends 1, 2, ... until process exit.
    pub fn interval(ms: i64) -> JetReceiver<i64> {
        let (tx, rx) = channel::<i64>();
        let delay = ms.max(1) as u64;
        let _ = std::thread::spawn(move || {
            let mut tick = 1i64;
            loop {
                super::jet_scheduler_sleep_ms(delay);
                if !tx.tx.send(tick) {
                    break;
                }
                tick += 1;
            }
        });
        rx
    }

    pub struct JetReceiver<T> {
        inner: super::JetSchedulerChannel<T>,
    }
    // D-TUPLE-DESTRUCT1: the tuple-destructure bind convention clones each
    // extracted field (`(tx, rx) := tasks.channel<T>()` clones `rx` off the
    // synthesized `(Sender<T>, Receiver<T>)` struct, same as `Sender` below). The
    // underlying scheduler channel is `Arc`-backed and already supports concurrent
    // receivers (the same substrate `g.select()` races multiple receive arms
    // against), so cloning a `Receiver` is a cheap, sound pointer copy — not a
    // single-consumer `std::sync::mpsc::Receiver`.
    impl<T> Clone for JetReceiver<T> {
        fn clone(&self) -> Self {
            JetReceiver {
                inner: self.inner.clone(),
            }
        }
    }
    impl<T: Send> JetReceiver<T> {
        pub fn receive(&self) -> Result<T, Closed> {
            // D-CANCELMODEL1=C: cancellation is handled preemptively inside
            // `inner.receive()` — a cancelled recv unwinds at the wait point rather
            // than returning a cooperative `Closed`. No pre-check sentinel here.
            if let Some(remaining) = super::jet_deadline_remaining_ms() {
                if remaining <= 0 {
                    super::jet_deadline_exceeded("channel receive");
                }
            }
            match self.inner.receive() {
                Some(v) => {
                    super::jet_deadline_check("channel receive");
                    Ok(v)
                }
                None => Err(Closed::Closed),
            }
        }
    }

    pub struct JetSender<T> {
        tx: super::JetSchedulerSender<T>,
    }
    impl<T: Send> JetSender<T> {
        pub fn send(&self, value: T) {
            let _ = self.tx.send(value);
        }
    }
    impl<T> Clone for JetSender<T> {
        fn clone(&self) -> Self {
            JetSender {
                tx: self.tx.clone(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Closed {
        Closed,
    }

    // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` — a lock-guarded shared handle,
    // "a copyable door". `Shared.new(x)` constructs; `.read(f)`/`.edit(f)` run a
    // closure against a read- or write-locked view, the lock scoped to the
    // closure call only (no guard object ever escapes it). Cloning is always a
    // cheap `Arc` clone, never a deep copy of `T` — that's what lets it cross a
    // `tasks.spawn` boundary with no `take`.
    pub struct JetShared<T>(std::sync::Arc<std::sync::RwLock<T>>);
    impl<T> JetShared<T> {
        pub fn new(value: T) -> Self {
            JetShared(std::sync::Arc::new(std::sync::RwLock::new(value)))
        }
        pub fn read<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&T) -> R,
        {
            let guard = self.0.read().unwrap_or_else(|e| e.into_inner());
            f(&*guard)
        }
        pub fn edit<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&mut T) -> R,
        {
            let mut guard = self.0.write().unwrap_or_else(|e| e.into_inner());
            f(&mut *guard)
        }
        // D-STM1=A (ratified 2026-07-12, card #506): the Shared plane of
        // `#Transact`. Inside a transaction block, `handle.edit(f)` lowers to
        // `edit_txn` — the mutation is DEFERRED, not applied now. Every deferred
        // edit across every touched handle is buffered on the current thread's
        // transaction, then applied together at the block's commit under all the
        // handles' write locks held at once, in a canonical (pointer) order that
        // cannot deadlock. Either every handle's change lands or none does, and no
        // task ever observes an intermediate state. `f` runs against a fresh
        // `&mut T` at commit time (not a snapshot), so a delta like `b.balance -=
        // 100` composes correctly with a concurrent transfer. The result is void
        // by construction — the write hasn't happened yet — which sema enforces
        // (E0750). Codegen is dumb (I3): the whole strategy is this runtime.
        pub fn edit_txn<F>(&self, f: F)
        where
            F: FnOnce(&mut T) + 'static,
            T: 'static,
        {
            super::jet_stm::record_edit(self.0.clone(), Box::new(f));
        }
    }
    impl<T> Clone for JetShared<T> {
        fn clone(&self) -> Self {
            JetShared(self.0.clone())
        }
    }
    // D-MEM1 S6: an opaque-handle placeholder, mirroring `JetTCPListener`'s
    // `JetShow` (Prelude/CoreLib.rs) — `Shared<T>`'s point is the lock-guarded
    // access methods, not a direct print of the handle itself.
    impl<T> super::JetShow for JetShared<T> {
        fn jet_show(&self) -> String {
            "Shared(..)".to_string()
        }
    }

    // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` — a generational arena. `Id<T>` is
    // a lightweight index+generation handle: plain data, `Copy`, comparable,
    // regardless of whether `T` itself is (it never touches `T` at runtime —
    // hand-written impls below, not `#[derive]`, so no `T: Copy`/`Clone`/`Eq`
    // bound leaks onto every `Id<T>`).
    enum JetPoolSlot<T> {
        Occupied(u32, T),
        Vacant(u32),
    }

    pub struct JetPool<T> {
        slots: Vec<JetPoolSlot<T>>,
        free: Vec<usize>,
    }

    impl<T> JetPool<T> {
        pub fn new() -> Self {
            JetPool {
                slots: Vec::new(),
                free: Vec::new(),
            }
        }

        pub fn add(&mut self, value: T) -> JetId<T> {
            if let Some(idx) = self.free.pop() {
                let gen = match self.slots[idx] {
                    JetPoolSlot::Vacant(g) => g,
                    JetPoolSlot::Occupied(..) => {
                        unreachable!("a free-list slot is always Vacant")
                    }
                };
                self.slots[idx] = JetPoolSlot::Occupied(gen, value);
                return JetId::new(idx as u32, gen);
            }
            let idx = self.slots.len();
            self.slots.push(JetPoolSlot::Occupied(0, value));
            JetId::new(idx as u32, 0)
        }

        /// D-POOLID-API1=A: removes the slot `id` names, bumping its generation
        /// so any other copy of `id` becomes stale — mirrors `Map.remove`'s
        /// `Option<T>` convention (a miss returns `None`, not a panic).
        pub fn remove(&mut self, id: JetId<T>) -> Option<T> {
            let idx = id.index as usize;
            let occupied = matches!(
                self.slots.get(idx),
                Some(JetPoolSlot::Occupied(g, _)) if *g == id.generation
            );
            if !occupied {
                return None;
            }
            let next_gen = id.generation.wrapping_add(1);
            let old = std::mem::replace(&mut self.slots[idx], JetPoolSlot::Vacant(next_gen));
            self.free.push(idx);
            match old {
                JetPoolSlot::Occupied(_, v) => Some(v),
                JetPoolSlot::Vacant(_) => unreachable!("just checked Occupied above"),
            }
        }

        /// A snapshot `Vec` of every live id — small, `Copy` elements, so a
        /// fresh allocation per call is the simplest correct thing (D-MEM1 S6
        /// notes deferred a genuine lazy `Iterator` as unneeded polish).
        pub fn ids(&self) -> Vec<JetId<T>> {
            self.slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| match s {
                    JetPoolSlot::Occupied(g, _) => Some(JetId::new(i as u32, *g)),
                    JetPoolSlot::Vacant(_) => None,
                })
                .collect()
        }
    }
    // D-MEM1 S6: an opaque-handle placeholder, same rationale as `JetShared`'s
    // `JetShow` just above.
    impl<T> super::JetShow for JetPool<T> {
        fn jet_show(&self) -> String {
            format!("Pool({} slots)", self.slots.len())
        }
    }

    pub struct JetId<T> {
        index: u32,
        generation: u32,
        _marker: std::marker::PhantomData<fn() -> T>,
    }
    impl<T> JetId<T> {
        fn new(index: u32, generation: u32) -> Self {
            JetId {
                index,
                generation,
                _marker: std::marker::PhantomData,
            }
        }
    }
    impl<T> Clone for JetId<T> {
        fn clone(&self) -> Self {
            *self
        }
    }
    impl<T> Copy for JetId<T> {}
    impl<T> PartialEq for JetId<T> {
        fn eq(&self, other: &Self) -> bool {
            self.index == other.index && self.generation == other.generation
        }
    }
    impl<T> Eq for JetId<T> {}
    impl<T> super::user_Equatable for JetId<T> {
        fn equal(&self, rhs: &Self) -> bool {
            self == rhs
        }
    }
    impl<T> std::hash::Hash for JetId<T> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.index.hash(state);
            self.generation.hash(state);
        }
    }
    impl<T> std::fmt::Debug for JetId<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Id(#{}@{})", self.index, self.generation)
        }
    }
    // D-MEM1 S6: print/interpolation/derived-Debug support — `Id<T>` shows up as
    // an ordinary struct field (`parent: Id<Node>?`), whose containing struct's
    // generated `jet_debug()` calls `.jet_debug()` on every field.
    impl<T> super::JetShow for JetId<T> {
        fn jet_show(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }
    impl<T> super::JetDisplay for JetId<T> {
        fn jet_display(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }
    impl<T> super::JetDebug for JetId<T> {
        fn jet_debug(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }

    /// `pool[id]` read (`Expr::Index`, `IndexKind::Pool`): a generation-checked
    /// clone of `T`. Panics naming the stale-access class on a mismatched or
    /// vacant slot, mirroring the array-out-of-bounds panic precedent
    /// (`jet_index_vec`) — a runtime panic, not a new diagnostic code.
    pub fn jet_pool_get<T: Clone>(pool: &JetPool<T>, id: JetId<T>, file: &str, line: u32) -> T {
        match pool.slots.get(id.index as usize) {
            Some(JetPoolSlot::Occupied(gen, v)) if *gen == id.generation => v.clone(),
            _ => super::jet_panic(
                file,
                line,
                "this Id no longer refers to a live value — its pool slot was removed",
            ),
        }
    }

    /// `pool[id] = v` / `pool[id].field = v` (`LValue::Index` / `LValue::Field`
    /// nested on a `Pool` index): a genuine mutable place, not a value
    /// round-trip — a nested field write edits the real slot. Same stale-access
    /// panic as `jet_pool_get`.
    pub fn jet_pool_get_mut<'a, T>(
        pool: &'a mut JetPool<T>,
        id: JetId<T>,
        file: &str,
        line: u32,
    ) -> &'a mut T {
        let idx = id.index as usize;
        let valid = matches!(
            pool.slots.get(idx),
            Some(JetPoolSlot::Occupied(gen, _)) if *gen == id.generation
        );
        if !valid {
            super::jet_panic(
                file,
                line,
                "this Id no longer refers to a live value — its pool slot was removed",
            );
        }
        match &mut pool.slots[idx] {
            JetPoolSlot::Occupied(_, v) => v,
            JetPoolSlot::Vacant(_) => unreachable!("just checked Occupied above"),
        }
    }
