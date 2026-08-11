
struct Job { run:Box<dyn FnOnce()+Send>,blocking:bool }

thread_local! {
    static JET_SCHEDULER_CATCHING_PANIC: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

static JET_SCHEDULER_PANIC_HOOK: OnceLock<()> = OnceLock::new();

fn jet_scheduler_install_panic_hook() {
    JET_SCHEDULER_PANIC_HOOK.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let caught_task = JET_SCHEDULER_CATCHING_PANIC
                .try_with(|flag| flag.get())
                .unwrap_or(false);
            if !caught_task {
                previous(info);
            }
        }));
    });
}

fn jet_scheduler_catch_task_unwind<F, T>(f: F) -> std::thread::Result<T>
where
    F: FnOnce() -> T,
{
    jet_scheduler_install_panic_hook();
    JET_SCHEDULER_CATCHING_PANIC.with(|flag| {
        let previous = flag.replace(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        flag.set(previous);
        result
    })
}

fn jet_scheduler_fatal(msg: &str) -> ! {
    if jet_scheduler_panic_should_unwind() {
        panic!("{}", msg);
    }
    eprintln!("panic: {}", msg);
    std::process::exit(70);
}

// ── D-CANCELMODEL1=C: preemptive cancellation at wait points ─────────────────
// One unwind mechanism, two triggers (deadline E3003, task cancel). A cancelled
// task unwinds at its next wait point by panicking with the `JetCancelUnwind`
// marker; the task runner maps that payload to a `Cancelled` result and every
// Drop-backed cleanup runs on the way out — the same shape a blown deadline
// already produces. A shielded region (SHIELD_DEPTH > 0) DEFERS the unwind: wait
// points inside it complete normally and the deferred cancel/deadline lands when
// the outermost region exits. D-SHIELDNAME1=A (ratified 2026-07-11) spells this
// region `#Shield { … }`; codegen lowers the block to
// `jet_scheduler_shield_enter`/`_leave` around the body (Codegen/TIR emit).
struct JetCancelUnwind;

struct JetDeadlineUnwind {
    rendered: String,
}

#[allow(dead_code)] // emitted prelude interrupt dispatcher consumes this helper
fn jet_report_caught_unwind(payload: Box<dyn std::any::Any + Send>) {
    if let Some(deadline) = payload.downcast_ref::<JetDeadlineUnwind>() {
        eprintln!("{}", deadline.rendered);
    } else if let Some(message) = payload.downcast_ref::<String>() {
        eprintln!("panic: {message}");
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        eprintln!("panic: {message}");
    }
}

/// Result of calling a scheduler wait point from a native-code boundary that
/// cannot carry Rust unwinds (Cranelift, C, plugins). The boundary catches the
/// scheduler's internal control transfer before it reaches foreign frames.
pub enum JetSchedulerWait<T> {
    Ready(T),
    Cancelled,
    Deadline(String),
    Panicked(String),
}

pub fn jet_scheduler_wait_without_unwind<F, T>(f: F) -> JetSchedulerWait<T>
where
    F: FnOnce() -> T,
{
    jet_scheduler_install_panic_hook();
    let result = JET_IN_SCHEDULER_TASK.with(|in_task| {
        JET_SCHEDULER_CATCHING_PANIC.with(|catching| {
            let previous_task = in_task.replace(true);
            let previous_catching = catching.replace(true);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            catching.set(previous_catching);
            in_task.set(previous_task);
            result
        })
    });
    match result {
        Ok(value) => JetSchedulerWait::Ready(value),
        Err(payload) if payload.is::<JetCancelUnwind>() => JetSchedulerWait::Cancelled,
        Err(payload) if payload.is::<JetDeadlineUnwind>() => {
            let deadline = payload
                .downcast::<JetDeadlineUnwind>()
                .expect("deadline payload type checked");
            JetSchedulerWait::Deadline(deadline.rendered)
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "scheduler wait panicked".to_string());
            JetSchedulerWait::Panicked(message)
        }
    }
}

thread_local! {
    static SHIELD_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn jet_scheduler_shielded() -> bool {
    SHIELD_DEPTH.with(|d| d.get() > 0)
}

#[allow(dead_code)] // wired to a user sigil once D-SHIELDNAME1 ratifies
pub fn jet_scheduler_shield_enter() {
    // Outside a scheduler task/catch frame, `#Shield` is a transparent block.
    if current_task_control().is_some() && jet_scheduler_panic_should_unwind() {
        SHIELD_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
    }
}

#[allow(dead_code)] // wired to a user sigil once D-SHIELDNAME1 ratifies
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetShieldExit {
    None,
    Deadline,
    Cancelled,
}

/// Leave one shield depth without unwinding. JIT code uses this status form so
/// no Rust panic ever crosses a Cranelift frame; its Rust task wrapper delivers
/// the returned interrupt after native code has returned.
pub fn jet_scheduler_shield_leave_status() -> JetShieldExit {
    // Match `enter`: ambient deadlines must never begin unwinding merely because
    // ordinary non-task code crossed a lexical shield boundary.
    if current_task_control().is_none() || !jet_scheduler_panic_should_unwind() {
        return JetShieldExit::None;
    }
    let landed = SHIELD_DEPTH.with(|d| {
        let n = d.get().saturating_sub(1);
        d.set(n);
        n == 0
    });
    // If the body is already unwinding, decrement the depth but do not start a
    // second cancellation/deadline unwind from this Drop guard. The original
    // unwind already exits the task and runs every remaining cleanup.
    if landed && !std::thread::panicking() {
        // A deadline that closed while shielded is program-level; raise it first.
        if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
            return JetShieldExit::Deadline;
        }
        // A cancel that arrived while shielded now takes effect at region exit.
        if jet_scheduler_task_cancelled() {
            return JetShieldExit::Cancelled;
        }
    }
    JetShieldExit::None
}

pub fn jet_scheduler_deliver_shield_exit(exit: JetShieldExit) {
    match exit {
        JetShieldExit::None => {}
        JetShieldExit::Deadline => jet_deadline_exceeded("shield exit"),
        JetShieldExit::Cancelled => jet_task_unwind_cancel(),
    }
}

#[allow(dead_code)]
pub fn jet_scheduler_shield_leave() {
    jet_scheduler_deliver_shield_exit(jet_scheduler_shield_leave_status());
}

fn jet_task_unwind_cancel() -> ! {
    std::panic::panic_any(JetCancelUnwind);
}

/// A wait point observed cancellation on a non-shielded task: unwind now when we
/// are inside a scheduler task (a catch frame exists to turn the unwind into a
/// `Cancelled` result). Outside a task there is no catch frame, so return and let
/// the caller fall back to its cooperative sentinel (None/false/Closed).
fn jet_task_deliver_cancel() {
    if jet_scheduler_panic_should_unwind() {
        jet_task_unwind_cancel();
    }
}

/// Cancel check for wait points with no sentinel to return (task join): a no-op
/// unless the running task is cancelled and unshielded, in which case it unwinds
/// (inside a task) exactly like every other wait point.
fn jet_task_wait_point_cancel_check() {
    if jet_scheduler_task_cancelled() && !jet_scheduler_shielded() {
        jet_task_deliver_cancel();
    }
}

// ── M2: park/wake handles ─────────────────────────────────────────────────────

pub struct ParkSlot {
    notified: AtomicBool,
    lock: Mutex<()>,
    cv: Condvar,
}

impl ParkSlot {
    pub fn new() -> Arc<Self> {
        Arc::new(ParkSlot {
            notified: AtomicBool::new(false),
            lock: Mutex::new(()),
            cv: Condvar::new(),
        })
    }

    pub fn park(&self, timeout: Option<Duration>) {
        if self.notified.swap(false, Ordering::Acquire) {
            return;
        }
        let guard = self.lock.lock().unwrap();
        if self.notified.swap(false, Ordering::Acquire) {
            return;
        }
        // Tower #126: count real condvar blocks so scale tests can prove tasks
        // park (bounded blocks) rather than busy-wait (zero blocks, hot spin).
        METRIC_PARK_BLOCKS.fetch_add(1, Ordering::Relaxed);
        if let Some(t) = timeout {
            let _unused = self.cv.wait_timeout(guard, t).unwrap();
        } else {
            let _unused = self.cv.wait(guard).unwrap();
        }
        let _ = self.notified.swap(false, Ordering::Acquire);
    }

    pub fn wake(&self) {
        // Hold the same mutex the parker checks its predicate under, so a wake
        // that lands between the parker's `notified` re-check and its `cv.wait`
        // cannot be lost (otherwise a `park(None)` sleeps forever). Textbook
        // condvar handoff; without it, capacity-1 channel backpressure deadlocks.
        let _guard = self.lock.lock().unwrap();
        self.notified.store(true, Ordering::Release);
        self.cv.notify_one();
    }
}

pub struct JetTaskControl {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
    /// D-TASK-PAUSE-TIER1=E: 0 = WaitPoints (default), 1 = CheckLoops.
    pub pause_mode: std::sync::atomic::AtomicU8,
    park: Arc<ParkSlot>,
    cancel_waiters: Mutex<Vec<std::sync::Weak<ParkSlot>>>,
}

impl JetTaskControl {
    pub fn new() -> Arc<Self> {
        Arc::new(JetTaskControl {
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            pause_mode: std::sync::atomic::AtomicU8::new(0),
            park: ParkSlot::new(),
            cancel_waiters: Mutex::new(Vec::new()),
        })
    }

    pub fn pause(&self) {
        self.pause_with_mode(0);
    }

    pub fn pause_with_mode(&self, mode: u8) {
        self.pause_mode.store(mode, Ordering::Relaxed);
        self.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.park.wake();
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.park.wake();
        for waiter in self.cancel_waiters.lock().unwrap().drain(..) {
            if let Some(waiter) = waiter.upgrade() {
                waiter.wake();
            }
        }
    }

    fn register_cancel_waiter(&self, slot: &Arc<ParkSlot>) {
        let mut waiters = self.cancel_waiters.lock().unwrap();
        if self.cancelled.load(Ordering::Relaxed) {
            slot.wake();
        } else {
            waiters.push(Arc::downgrade(slot));
        }
    }

    fn remove_cancel_waiter(&self, slot: &ParkSlot) {
        self.cancel_waiters.lock().unwrap().retain(|waiter| {
            waiter
                .upgrade()
                .is_some_and(|waiter| !std::ptr::eq(Arc::as_ptr(&waiter), std::ptr::from_ref(slot)))
        });
    }

    fn wait_while_paused(&self) {
        while self.paused.load(Ordering::Relaxed) && !self.cancelled.load(Ordering::Relaxed) {
            self.park.park(None);
        }
    }
}

thread_local! {
    static TASK_CONTROL: std::cell::RefCell<Option<Arc<JetTaskControl>>> =
        const { std::cell::RefCell::new(None) };
}

pub fn jet_scheduler_set_task_control(c: Option<Arc<JetTaskControl>>) {
    TASK_CONTROL.with(|t| *t.borrow_mut() = c);
}

fn current_task_control() -> Option<Arc<JetTaskControl>> {
    TASK_CONTROL.with(|t| t.borrow().clone())
}

/// Park at a yield point; honors pause/cancel on the running task.
pub fn jet_scheduler_yield(wait_kind: &str, slot: &Arc<ParkSlot>, timeout: Option<Duration>) {
    let ctrl = current_task_control();
    // D-CANCELMODEL1=C: inside a shielded region, cancel/deadline are deferred to
    // region exit — this wait point behaves as if the task were not cancelled, so
    // the critical section can complete. Outside a shield, cancel unwinds here.
    let shielded = jet_scheduler_shielded();
    if let Some(ctrl) = &ctrl {
        if !shielded {
            if ctrl.cancelled.load(Ordering::Relaxed) {
                jet_task_deliver_cancel();
                return;
            }
            ctrl.wait_while_paused();
            if ctrl.cancelled.load(Ordering::Relaxed) {
                jet_task_deliver_cancel();
                return;
            }
            // Make this park reachable by cancel(): a task blocked on a channel,
            // timer, or IO wait must actually unblock when its handle is cancelled
            // — not only when the awaited event arrives. If cancel already fired,
            // this wakes the slot immediately so the park below returns at once.
            ctrl.register_cancel_waiter(slot);
        } else {
            ctrl.wait_while_paused();
        }
    }
    if shielded {
        // Shielded: ignore the deadline too; wait on the real event only.
        slot.park(timeout);
    } else if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            if let Some(ctrl) = &ctrl {
                ctrl.remove_cancel_waiter(slot);
            }
            jet_deadline_exceeded(wait_kind);
        }
        let cap = Duration::from_millis(remaining as u64);
        let wait = timeout.map(|t| t.min(cap)).unwrap_or(cap);
        slot.park(Some(wait));
        if let Some(left) = jet_deadline_remaining_ms() {
            if left <= 0 {
                if let Some(ctrl) = &ctrl {
                    ctrl.remove_cancel_waiter(slot);
                }
                jet_deadline_exceeded(wait_kind);
            }
        }
    } else {
        slot.park(timeout);
    }
    if let Some(ctrl) = &ctrl {
        if !shielded {
            ctrl.remove_cancel_waiter(slot);
            // Woken by cancel() while parked: unwind at this wait point rather than
            // returning to the task body (preemptive, D-CANCELMODEL1=C).
            if ctrl.cancelled.load(Ordering::Relaxed) {
                jet_task_deliver_cancel();
                return;
            }
        }
        ctrl.wait_while_paused();
    }
}

pub fn jet_scheduler_wake(slot: &ParkSlot) {
    slot.wake();
}

pub fn jet_scheduler_task_cancelled() -> bool {
    current_task_control()
        .map(|c| c.cancelled.load(Ordering::Relaxed))
        .unwrap_or(false)
}

pub fn jet_scheduler_wait_point_cancelled() -> bool {
    jet_scheduler_task_cancelled() && !jet_scheduler_shielded()
}

thread_local! {
    static JET_BLOCKING_WAIT_COMPENSATION: std::cell::RefCell<Vec<bool>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn jet_scheduler_blocking_wait_enter() {
    let active=current_task_control().is_some();
    if active { scheduler().blocking_wait_enter(); }
    JET_BLOCKING_WAIT_COMPENSATION.with(|stack|stack.borrow_mut().push(active));
}

pub fn jet_scheduler_blocking_wait_leave() {
    if JET_BLOCKING_WAIT_COMPENSATION.with(|stack|stack.borrow_mut().pop())==Some(true) { scheduler().blocking_wait_leave(); }
}

pub fn jet_scheduler_blocking_wait_stats()->(usize,usize,usize) {
    let sched=scheduler();let state=sched.blocking_wait.lock().unwrap();(state.waits,state.threads,state.peak)
}

// ── M2: timer sleep (park/wake, not thread::sleep on pool workers) ───────────

struct TimerEntry {
    wake_at: Instant,
    slot: Arc<ParkSlot>,
}

struct TimerWheel {
    entries: Mutex<Vec<TimerEntry>>,
    notify: Condvar,
}

impl TimerWheel {
    fn schedule(&self, wake_at: Instant, slot: Arc<ParkSlot>) {
        self.entries.lock().unwrap().push(TimerEntry { wake_at, slot });
        self.notify.notify_one();
    }

    fn run(self: Arc<Self>) {
        loop {
            let now = Instant::now();
            let mut due = Vec::new();
            {
                let mut entries = self.entries.lock().unwrap();
                entries.retain(|e| {
                    if e.wake_at <= now {
                        due.push(e.slot.clone());
                        false
                    } else {
                        true
                    }
                });
            }
            for slot in due {
                slot.wake();
            }
            let sleep_for = {
                let entries = self.entries.lock().unwrap();
                entries
                    .iter()
                    .map(|e| e.wake_at.saturating_duration_since(now))
                    .min()
                    .unwrap_or(Duration::from_millis(50))
            };
            let g = self.entries.lock().unwrap();
            let _ = self
                .notify
                .wait_timeout(g, sleep_for.min(Duration::from_millis(50)))
                .unwrap();
        }
    }
}

static TIMER_WHEEL: OnceLock<Arc<TimerWheel>> = OnceLock::new();

fn timer_wheel() -> Arc<TimerWheel> {
    TIMER_WHEEL
        .get_or_init(|| {
            let wheel = Arc::new(TimerWheel {
                entries: Mutex::new(Vec::new()),
                notify: Condvar::new(),
            });
            let w = wheel.clone();
            thread::spawn(move || w.run());
            wheel
        })
        .clone()
}

pub fn jet_scheduler_sleep_ms(millis: u64) {
    jet_scheduler_park_ms("time sleep", millis);
}

pub fn jet_scheduler_park_ms(wait_kind: &'static str, millis: u64) {
    if millis == 0 {
        return;
    }
    let slot = ParkSlot::new();
    timer_wheel().schedule(Instant::now() + Duration::from_millis(millis), slot.clone());
    jet_scheduler_yield(wait_kind, &slot, Some(Duration::from_millis(millis)));
}

pub fn jet_scheduler_yield_now() {
    let slot = ParkSlot::new();
    jet_scheduler_yield("task yield", &slot, Some(Duration::ZERO));
}

/// Control-plane trace for the TLS current task (idle defaults when none).
pub fn jet_scheduler_current_task_trace() -> String {
    match current_task_control() {
        Some(ctrl) => {
            let paused = ctrl.paused.load(Ordering::Relaxed);
            let cancel = ctrl.cancelled.load(Ordering::Relaxed);
            jet_foundation::StructuralDebug::jet_task_control_trace(paused, cancel)
        }
        None => jet_foundation::StructuralDebug::jet_task_control_trace(false, false),
    }
}

// ── M2: scheduler-integrated channel (wake-on-send) ────────────────────────────

struct ChannelState<T> {
    queue: VecDeque<T>,
    recv_waiters: Vec<Arc<ParkSlot>>,
    send_waiters: Vec<Arc<ParkSlot>>,
    closed: bool,
    capacity: Option<usize>,
    sender_count: usize,
    receiver_count: usize,
}

pub(crate) struct ChannelInner<T> {
    state: Mutex<ChannelState<T>>,
}

pub struct JetSchedulerChannel<T> {
    inner: Arc<ChannelInner<T>>,
}

// D-TUPLE-DESTRUCT1: hand-written, not `#[derive(Clone)]` — the derive adds a
// spurious `T: Clone` bound (it can't see that only the `Arc` is cloned, not a
// `T`); an `Arc` clone never needs its payload to be `Clone`. Same reasoning as
// `JetSchedulerSender`'s manual impl right below.
impl<T> Clone for JetSchedulerChannel<T> {
    fn clone(&self) -> Self {
        self.inner.state.lock().unwrap().receiver_count += 1;
        JetSchedulerChannel {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for JetSchedulerChannel<T> {
    fn drop(&mut self) {
        let send_waiters = {
            let mut st = self.inner.state.lock().unwrap();
            st.receiver_count = st.receiver_count.saturating_sub(1);
            if st.receiver_count == 0 {
                st.closed = true;
                std::mem::take(&mut st.send_waiters)
            } else {
                Vec::new()
            }
        };
        for slot in send_waiters {
            slot.wake();
        }
    }
}

impl<T: Send> JetSchedulerChannel<T> {
    pub fn new() -> Self {
        Self::with_capacity(None)
    }

    pub fn bounded(capacity: usize) -> Self {
        // D-VERDICT-1637-1: capacity is a real memory/backpressure bound
        // (MathTaskMem.rs), never a rendezvous handshake — clamp matches
        // Prelude/Scheduler.rs so `tasks.channel<T>(0)` behaves identically
        // under JIT and AOT (no ratified zero-capacity semantics exist).
        Self::with_capacity(Some(capacity.max(1)))
    }

    fn with_capacity(capacity: Option<usize>) -> Self {
        JetSchedulerChannel {
            inner: Arc::new(ChannelInner {
                state: Mutex::new(ChannelState {
                    queue: VecDeque::new(),
                    recv_waiters: Vec::new(),
                    send_waiters: Vec::new(),
                    closed: false,
                    capacity,
                    sender_count: 0,
                    receiver_count: 1,
                }),
            }),
        }
    }

    pub fn sender(&self) -> JetSchedulerSender<T> {
        self.inner.state.lock().unwrap().sender_count += 1;
        JetSchedulerSender {
            inner: self.inner.clone(),
        }
    }

    pub fn receive(&self) -> Option<T> {
        loop {
            // D-CANCELMODEL1=C: a cancelled recv unwinds at this wait point (inside
            // a task) instead of returning the cooperative `None` sentinel. Shielded
            // regions defer the unwind and receive normally.
            if jet_scheduler_task_cancelled() && !jet_scheduler_shielded() {
                jet_task_deliver_cancel();
                return None;
            }
            if let Some(ctrl) = current_task_control() {
                ctrl.wait_while_paused();
            }
            let slot = ParkSlot::new();
            let wake_sender = {
                let mut st = self.inner.state.lock().unwrap();
                if let Some(v) = st.queue.pop_front() {
                    if let Some(slot) = st.send_waiters.pop() {
                        slot.wake();
                    }
                    return Some(v);
                }
                if st.closed {
                    return None;
                }
                st.recv_waiters.push(slot.clone());
                st.send_waiters.last().cloned()
            };
            if let Some(sender) = wake_sender {
                jet_scheduler_wake(&sender);
            }
            jet_scheduler_yield("channel receive", &slot, None);
            let mut st = self.inner.state.lock().unwrap();
            st.recv_waiters.retain(|w| !Arc::ptr_eq(w, &slot));
        }
    }

    pub fn try_receive(&self) -> Option<T> {
        let mut st = self.inner.state.lock().unwrap();
        let out = st.queue.pop_front();
        if out.is_some() {
            if let Some(slot) = st.send_waiters.pop() {
                slot.wake();
            }
        }
        out
    }

    pub fn close(&self) {
        let (recv_waiters, send_waiters) = {
            let mut st = self.inner.state.lock().unwrap();
            st.closed = true;
            (
                std::mem::take(&mut st.recv_waiters),
                std::mem::take(&mut st.send_waiters),
            )
        };
        for w in recv_waiters.into_iter().chain(send_waiters) {
            w.wake();
        }
    }
}

pub struct JetSchedulerSender<T> {
    inner: Arc<ChannelInner<T>>,
}

impl<T> Clone for JetSchedulerSender<T> {
    fn clone(&self) -> Self {
        self.inner.state.lock().unwrap().sender_count += 1;
        JetSchedulerSender {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for JetSchedulerSender<T> {
    fn drop(&mut self) {
        let recv_waiters = {
            let mut st = self.inner.state.lock().unwrap();
            st.sender_count = st.sender_count.saturating_sub(1);
            if st.sender_count == 0 {
                st.closed = true;
                std::mem::take(&mut st.recv_waiters)
            } else {
                Vec::new()
            }
        };
        for slot in recv_waiters {
            slot.wake();
        }
    }
}

impl<T: Send> JetSchedulerSender<T> {
    pub fn send(&self, value: T) -> bool {
        let mut value = Some(value);
        loop {
            // D-CANCELMODEL1=C: a cancelled send unwinds at this wait point instead
            // of returning the cooperative `false` sentinel. Shield defers.
            if jet_scheduler_task_cancelled() && !jet_scheduler_shielded() {
                jet_task_deliver_cancel();
                return false;
            }
            if let Some(ctrl) = current_task_control() {
                ctrl.wait_while_paused();
            }
            let slot = ParkSlot::new();
            let wake = {
                let mut st = self.inner.state.lock().unwrap();
                if st.closed || st.receiver_count == 0 {
                    return false;
                }
                let full = st.capacity.is_some_and(|cap| st.queue.len() >= cap);
                if full {
                    st.send_waiters.push(slot.clone());
                    None
                } else {
                    st.queue
                        .push_back(value.take().expect("channel send value missing"));
                    st.recv_waiters.pop()
                }
            };
            if let Some(slot) = wake {
                jet_scheduler_wake(&slot);
            }
            if value.is_none() {
                return true;
            }
            jet_scheduler_yield("channel send", &slot, None);
            let mut st = self.inner.state.lock().unwrap();
            st.send_waiters.retain(|w| !Arc::ptr_eq(w, &slot));
        }
    }
}

impl<T> JetSchedulerChannel<T> {
    #[allow(dead_code)]
    pub(crate) fn select_inner(&self) -> Arc<ChannelInner<T>> {
        self.inner.clone()
    }
}

// ── D-CONCSELECT1=A: scoped select multiplex ─────────────────────────────────

pub enum JetSelectOutcome<T> {
    Recv { arm: usize, value: T },
    After { arm: usize },
    Closed,
}

#[allow(dead_code)]
impl<T> ChannelInner<T> {
    fn try_pop(&self) -> Option<T> {
        let mut st = self.state.lock().unwrap();
        let out = st.queue.pop_front();
        if out.is_some() {
            if let Some(slot) = st.send_waiters.pop() {
                slot.wake();
            }
        }
        out
    }

    fn register_select_waiter(&self, slot: Arc<ParkSlot>) {
        self.state.lock().unwrap().recv_waiters.push(slot);
    }

    fn is_closed_and_empty(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.closed && state.queue.is_empty()
    }

    fn remove_waiter(&self, slot: &ParkSlot) {
        self.state
            .lock()
            .unwrap()
            .recv_waiters
            .retain(|w| !std::ptr::eq(Arc::as_ptr(w), std::ptr::from_ref(slot)));
    }
}

struct SelectRegistration<T> {
    recvs: Vec<Arc<ChannelInner<T>>>,
    master: Arc<ParkSlot>,
    cancel_control: Option<Arc<JetTaskControl>>,
}

impl<T> SelectRegistration<T> {
    fn new(
        recvs: &[Arc<ChannelInner<T>>],
        master: Arc<ParkSlot>,
        cancel_control: Option<Arc<JetTaskControl>>,
    ) -> Self {
        for channel in recvs {
            channel.register_select_waiter(master.clone());
        }
        if let Some(control) = &cancel_control {
            control.register_cancel_waiter(&master);
        }
        Self {
            recvs: recvs.to_vec(),
            master,
            cancel_control,
        }
    }
}

impl<T> Drop for SelectRegistration<T> {
    fn drop(&mut self) {
        for channel in &self.recvs {
            channel.remove_waiter(&self.master);
        }
        if let Some(control) = &self.cancel_control {
            control.remove_cancel_waiter(&self.master);
        }
    }
}

#[allow(dead_code)] // generated `jet_std::jet_select_wait` and JIT hooks
pub(crate) fn jet_scheduler_select<T: Send>(
    recvs: Vec<Arc<ChannelInner<T>>>,
    after_ms: Vec<u64>,
) -> JetSelectOutcome<T> {
    assert!(
        !recvs.is_empty() || !after_ms.is_empty(),
        "select: no arms registered"
    );

    for (i, ch) in recvs.iter().enumerate() {
        if let Some(v) = ch.try_pop() {
            return JetSelectOutcome::Recv { arm: i, value: v };
        }
    }
    if let Some((i, _)) = after_ms.iter().enumerate().find(|(_, ms)| **ms == 0) {
        return JetSelectOutcome::After { arm: i };
    }
    if after_ms.is_empty()
        && !recvs.is_empty()
        && recvs.iter().all(|ch| ch.is_closed_and_empty())
    {
        return JetSelectOutcome::Closed;
    }

    let master = ParkSlot::new();
    let cancel_control = current_task_control();
    let started = Instant::now();
    let _registration =
        SelectRegistration::new(&recvs, master.clone(), cancel_control.clone());
    // Close can race the first check and happen before waiter registration.
    // Recheck after registration so a lost close wake cannot park forever.
    if after_ms.is_empty()
        && !recvs.is_empty()
        && recvs.iter().all(|ch| ch.is_closed_and_empty())
    {
        return JetSelectOutcome::Closed;
    }
    for ms in &after_ms {
        timer_wheel().schedule(Instant::now() + Duration::from_millis(*ms), master.clone());
    }

    loop {
        // D-CANCELMODEL1=C: a cancelled select unwinds at this wait point (inside a
        // task) instead of returning the cooperative `Closed` sentinel. Shield defers.
        if jet_scheduler_task_cancelled() && !jet_scheduler_shielded() {
            jet_task_deliver_cancel();
            return JetSelectOutcome::Closed;
        }
        METRIC_PARKED.fetch_add(1, Ordering::Relaxed);
        jet_scheduler_yield("select wait", &master, None);
        METRIC_PARKED.fetch_sub(1, Ordering::Relaxed);
        for (i, ch) in recvs.iter().enumerate() {
            if let Some(v) = ch.try_pop() {
                return JetSelectOutcome::Recv { arm: i, value: v };
            }
        }
        if let Some((i, _)) = after_ms
            .iter()
            .enumerate()
            .find(|(_, ms)| started.elapsed() >= Duration::from_millis(**ms))
        {
            return JetSelectOutcome::After { arm: i };
        }
        if after_ms.is_empty()
            && !recvs.is_empty()
            && recvs.iter().all(|ch| ch.is_closed_and_empty())
        {
            return JetSelectOutcome::Closed;
        }
    }
}

// ── Scheduler metrics (Tower #126 observability) ─────────────────────────────

static METRIC_PARKED: AtomicUsize = AtomicUsize::new(0);
static METRIC_POLLER_WAKE: AtomicUsize = AtomicUsize::new(0);
static METRIC_PARK_BLOCKS: AtomicUsize = AtomicUsize::new(0);
// D-VERDICT-1637-1: no live JIT-host caller ever registers real IO through
// this scheduler (net_http_rt.rs runs its own local poll loop instead — see
// #1637 divergence list). METRIC_IO_ACTIVE/ALLOCATED/RETIRED stay wired into
// jet_scheduler_drain() below (mirrors Prelude/Scheduler.rs, one mechanism)
// but always read (0, 0, 0) here; the backend-name and windows IOCP metric
// reporting that only the deleted io.rs fork consumed were removed with it.
static METRIC_IO_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static METRIC_IO_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static METRIC_IO_RETIRED: AtomicUsize = AtomicUsize::new(0);

pub fn jet_scheduler_metric_parked() -> usize {
    METRIC_PARKED.load(Ordering::Relaxed)
}

pub fn jet_scheduler_metric_poller_wake() -> usize {
    METRIC_POLLER_WAKE.load(Ordering::Relaxed)
}

/// Total times a task actually blocked on a park condvar (not the notified
/// fast-path). A busy-wait scheduler would leave this at zero under contention.
pub fn jet_scheduler_metric_park_blocks() -> usize {
    METRIC_PARK_BLOCKS.load(Ordering::Relaxed)
}

// ── M1: work-stealing pool ───────────────────────────────────────────────────

struct WorkerSlot {
    queue: Mutex<VecDeque<Job>>,
}

// Fixed ceiling prevents blocking waiters from turning scheduler progress into an unbounded thread source.
const JET_BLOCKING_COMPENSATION_LIMIT:usize=8;
struct BlockingWaitState { waits:usize,threads:usize,peak:usize,reserve:bool }

struct Scheduler {
    workers: Vec<WorkerSlot>,
    global: Mutex<VecDeque<Job>>,
    notify: Condvar,
    live: AtomicUsize,
    shutdown: AtomicBool,
    blocking_wait:Mutex<BlockingWaitState>,
}

impl Scheduler {
    fn pop_local(&self, id: usize,allow_blocking:bool) -> Option<Job> {
        let mut queue=self.workers[id].queue.lock().unwrap();if allow_blocking{queue.pop_back()}else{queue.iter().rposition(|job|!job.blocking).map(|index|queue.remove(index).unwrap())}
    }

    fn pop_global(&self,allow_blocking:bool) -> Option<Job> {
        let mut queue=self.global.lock().unwrap();if allow_blocking{queue.pop_front()}else{queue.iter().position(|job|!job.blocking).map(|index|queue.remove(index).unwrap())}
    }

    fn steal_from(&self, thief: usize,allow_blocking:bool) -> Option<Job> {
        for (idx, w) in self.workers.iter().enumerate() {
            if idx == thief {
                continue;
            }
            let mut q = w.queue.lock().unwrap();
            let job=if allow_blocking{q.pop_front()}else{q.iter().position(|job|!job.blocking).map(|index|q.remove(index).unwrap())};
            if let Some(job) = job {
                return Some(job);
            }
        }
        None
    }

    fn take_work(&self, id: usize,allow_blocking:bool) -> Option<Job> {
        self.pop_local(id,allow_blocking)
            .or_else(|| self.pop_global(allow_blocking))
            .or_else(|| self.steal_from(id,allow_blocking))
    }

    fn worker_loop(self: &Arc<Self>, id: usize) {
        loop {
            if let Some(job) = self.take_work(id,true) {
                self.live.fetch_add(1, Ordering::Relaxed);
                (job.run)();
                self.live.fetch_sub(1, Ordering::Relaxed);
                self.notify.notify_one();
                continue;
            }
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            }
            let g = self.global.lock().unwrap();
            let _ = self
                .notify
                .wait_timeout(g, Duration::from_millis(10))
                .unwrap();
        }
    }

    fn blocking_wait_enter(self:&Arc<Self>){
        let spawn={let mut state=self.blocking_wait.lock().unwrap();state.waits+=1;let ordinary=state.threads-usize::from(state.reserve);let reserve=if ordinary<state.waits.min(JET_BLOCKING_COMPENSATION_LIMIT){false}else if state.waits>JET_BLOCKING_COMPENSATION_LIMIT&&!state.reserve{state.reserve=true;true}else{return};state.threads+=1;state.peak=state.peak.max(state.threads);Some(reserve)};
        if let Some(reserve)=spawn { let sched=self.clone();thread::Builder::new().name(if reserve{"jet-blocking-reserve"}else{"jet-blocking-compensation"}.into()).spawn(move||sched.compensation_loop(reserve)).unwrap_or_else(|_|jet_scheduler_fatal("could not start scheduler compensation worker")); }
    }

    fn blocking_wait_leave(&self){let mut state=self.blocking_wait.lock().unwrap();state.waits-=1;drop(state);self.notify.notify_all();}

    fn compensation_loop(self:&Arc<Self>,reserve:bool){
        loop{
            {let mut state=self.blocking_wait.lock().unwrap();if (reserve&&state.waits<=JET_BLOCKING_COMPENSATION_LIMIT)||(!reserve&&state.waits==0){state.threads-=1;if reserve{state.reserve=false}return}}
            if let Some(job)=self.take_work(0,!reserve){self.live.fetch_add(1,Ordering::Relaxed);(job.run)();self.live.fetch_sub(1,Ordering::Relaxed);self.notify.notify_one();continue}
            let guard=self.global.lock().unwrap();let _=self.notify.wait_timeout(guard,Duration::from_millis(2)).unwrap();
        }
    }

    fn submit(self: &Arc<Self>, job: Job) {
        self.global.lock().unwrap().push_back(job);
        self.notify.notify_one();
    }
}

static SCHEDULER: OnceLock<Arc<Scheduler>> = OnceLock::new();
static NEXT_TASK_COMPLETION_ORDER: Mutex<u128> = Mutex::new(0);

fn next_task_completion_order() -> u128 {
    let mut next = NEXT_TASK_COMPLETION_ORDER.lock().unwrap();
    let order = *next;
    *next = order.checked_add(1).expect("task completion order exhausted");
    order
}

fn worker_count() -> usize {
    if let Ok(raw) = std::env::var("JET_SCHEDULER_THREADS") {
        if let Ok(n) = raw.parse::<usize>() {
            return n.max(1);
        }
    }
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1)
}

fn scheduler() -> Arc<Scheduler> {
    SCHEDULER
        .get_or_init(|| {
            let n = worker_count();
            let sched = Arc::new(Scheduler {
                workers: (0..n)
                    .map(|_| WorkerSlot {
                        queue: Mutex::new(VecDeque::new()),
                    })
                    .collect(),
                global: Mutex::new(VecDeque::new()),
                notify: Condvar::new(),
                live: AtomicUsize::new(0),
                shutdown: AtomicBool::new(false),
                blocking_wait:Mutex::new(BlockingWaitState{waits:0,threads:0,peak:0,reserve:false}),
            });
            for id in 0..n {
                let s = sched.clone();
                thread::spawn(move || s.worker_loop(id));
            }
            sched
        })
        .clone()
}

// pub for JIT NetHttp/HTTPServer include (same crate-local visibility as AOT prelude).
#[derive(Debug)]
pub enum JetSchedulerResult<T> {
    Value(T),
    Panicked,
    Cancelled,
    Deadline(String),
}

fn jet_scheduler_propagate_deadline(rendered: String) -> ! {
    if jet_scheduler_panic_should_unwind() {
        std::panic::panic_any(JetDeadlineUnwind { rendered });
    }
    eprintln!("{rendered}");
    std::process::exit(70);
}

pub struct JetSchedulerJoin<T> {
    rx: std::sync::mpsc::Receiver<JetSchedulerResult<T>>,
    completion_order: Arc<OnceLock<u128>>,
}

impl<T> JetSchedulerJoin<T> {
    pub fn join(self) -> T {
        // D-CANCELMODEL1=C: join is a wait point. If the joining task is already
        // cancelled, unwind here before blocking.
        jet_task_wait_point_cancel_check();
        match self.rx.recv() {
            Ok(JetSchedulerResult::Value(v)) => v,
            Ok(JetSchedulerResult::Panicked) | Err(_) => {
                jet_scheduler_fatal("a task panicked");
            }
            Ok(JetSchedulerResult::Cancelled) => {
                // A joined child that was cancelled propagates cancellation up: inside
                // a task this unwinds as Cancelled, on the host it stops the program.
                jet_task_deliver_cancel();
                jet_scheduler_fatal("a task was cancelled");
            }
            Ok(JetSchedulerResult::Deadline(rendered)) => {
                jet_scheduler_propagate_deadline(rendered);
            }
        }
    }

    pub fn try_recv(&self) -> Option<JetSchedulerResult<T>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(JetSchedulerResult::Panicked),
        }
    }

    fn completion_order(&self) -> Option<u128> {
        self.completion_order.get().copied()
    }

    pub fn drain(self) {
        let _ = self.rx.recv();
    }
}

fn jet_scheduler_select_tasks<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
    mode: crate::task_group::JetTaskSelectMode,
) -> Vec<T> {
    use crate::task_group::{
        jet_task_deadline, jet_task_select, jet_task_wait_policy, JetTaskWaitInterrupt,
    };
    let result = jet_task_select(
        entries,
        mode,
        || {
            let deadline = matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0)
                .then(|| jet_task_deadline("task selection").render());
            jet_task_wait_policy(deadline, jet_scheduler_task_cancelled(), jet_scheduler_shielded())
                .map_err(|interrupt| match interrupt {
                    JetTaskWaitInterrupt::Deadline(rendered) => {
                        JetSchedulerResult::Deadline(rendered)
                    }
                    JetTaskWaitInterrupt::Cancelled => JetSchedulerResult::Cancelled,
                })
        },
        |(join, _)| join.completion_order(),
        |(join, _)| {
            join.try_recv().map(|result| match result {
                JetSchedulerResult::Value(value) => Ok(value),
                failure => Err(failure),
            })
        },
        |(_, control)| control.cancel(),
        |(join, _)| join.drain(),
    );
    jet_scheduler_drain();
    match result {
        Ok(values) => values,
        Err(JetSchedulerResult::Deadline(rendered)) => {
            jet_scheduler_propagate_deadline(rendered)
        }
        Err(JetSchedulerResult::Cancelled) => {
            jet_task_deliver_cancel();
            jet_scheduler_fatal("a task was cancelled")
        }
        Err(JetSchedulerResult::Panicked) => {
            jet_scheduler_fatal("a task panicked")
        }
        Err(JetSchedulerResult::Value(_)) => unreachable!(),
    }
}

/// D-CONCCOMB1: join every handle in list order; fail fast and cancel siblings on error.
pub fn jet_scheduler_all<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
) -> Vec<T> {
    jet_scheduler_select_tasks(entries, crate::task_group::JetTaskSelectMode::All)
}

/// D-CONCCOMB1/D-RACEWIN1: first successful result wins; cancel losers.
pub fn jet_scheduler_race<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
) -> T {
    jet_scheduler_select_tasks(entries, crate::task_group::JetTaskSelectMode::Race)
        .pop()
        .expect("race result missing")
}

/// D-CONCCOMB1: first completed result wins (success or failure path visible).
pub fn jet_scheduler_any<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
) -> T {
    jet_scheduler_select_tasks(entries, crate::task_group::JetTaskSelectMode::Any)
        .pop()
        .expect("any result missing")
}

/// D-CONCSELECT1=A: JIT/AOT entry for fluent `g.select()` over scheduler channels.
pub fn jet_scheduler_select_int_channels(
    channels: &[JetSchedulerChannel<i64>],
    after_ms: Vec<u64>,
) -> i64 {
    jet_scheduler_select_int_channels_timed(channels, after_ms.into_iter().map(|ms| (ms, 0)).collect())
}

/// Select over int channels plus typed timer arms `(ms, value)`.
/// Timer win returns `value` (D-TASKRUNTIME1 / `g.select().after(ms, v)`).
pub fn jet_scheduler_select_int_channels_timed(
    channels: &[JetSchedulerChannel<i64>],
    timers: Vec<(u64, i64)>,
) -> i64 {
    let inners: Vec<_> = channels.iter().map(|c| c.select_inner()).collect();
    let after_ms: Vec<u64> = timers.iter().map(|(ms, _)| *ms).collect();
    match jet_scheduler_select(inners, after_ms) {
        JetSelectOutcome::Recv { value, .. } => value,
        JetSelectOutcome::After { arm } => timers
            .get(arm)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| jet_scheduler_fatal("select timer arm has no receive value")),
        JetSelectOutcome::Closed => {
            jet_scheduler_fatal("select closed");
        }
    }
}

/// Submit `f` to the M:N pool and return a join handle.
pub fn jet_scheduler_spawn<F, T>(f: F) -> JetSchedulerJoin<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    jet_scheduler_spawn_with_control(f, JetTaskControl::new())
}

pub fn jet_scheduler_spawn_blocking<F,T>(f:F)->JetSchedulerJoin<T>
where F:FnOnce()->T+Send+'static,T:Send+'static,
{
    jet_scheduler_spawn_blocking_with_control(f,JetTaskControl::new())
}

pub fn jet_scheduler_spawn_with_control<F, T>(
    f: F,
    control: Arc<JetTaskControl>,
) -> JetSchedulerJoin<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    jet_scheduler_spawn_with_control_kind(f,control,false)
}

pub fn jet_scheduler_spawn_blocking_with_control<F,T>(f:F,control:Arc<JetTaskControl>)->JetSchedulerJoin<T>
where F:FnOnce()->T+Send+'static,T:Send+'static,
{
    jet_scheduler_spawn_with_control_kind(f,control,true)
}

fn jet_scheduler_spawn_with_control_kind<F,T>(f:F,control:Arc<JetTaskControl>,blocking:bool)->JetSchedulerJoin<T>
where F:FnOnce()->T+Send+'static,T:Send+'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let completion_order = Arc::new(OnceLock::new());
    let task_completion_order = completion_order.clone();
    scheduler().submit(Job{blocking,run:Box::new(move || {
        jet_scheduler_set_task_control(Some(control.clone()));
        jet_scheduler_task_panic_enter();
        control.wait_while_paused();
        // Cancel while still paused (never resumed) aborts; cancel after resume only
        // affects yield points inside `f`, not startup — matches 157_task_controls.
        if control.paused.load(Ordering::Relaxed) && control.cancelled.load(Ordering::Relaxed) {
            jet_scheduler_task_panic_leave();
            jet_scheduler_set_task_control(None);
            task_completion_order
                .set(next_task_completion_order())
                .expect("task completion recorded twice");
            let _ = tx.send(JetSchedulerResult::Cancelled);
            return;
        }
        let out = jet_scheduler_catch_task_unwind(f);
        jet_scheduler_task_panic_leave();
        jet_scheduler_set_task_control(None);
        let result = match out {
            Ok(v) => JetSchedulerResult::Value(v),
            // D-CANCELMODEL1=C: a `JetCancelUnwind` payload is a task that unwound at
            // a wait point because it was cancelled — report Cancelled, not Panicked.
            Err(e) if e.is::<JetCancelUnwind>() => JetSchedulerResult::Cancelled,
            Err(e) if e.is::<JetDeadlineUnwind>() => {
                let deadline = e
                    .downcast::<JetDeadlineUnwind>()
                    .expect("deadline payload type checked");
                JetSchedulerResult::Deadline(deadline.rendered)
            }
            Err(_) => JetSchedulerResult::Panicked,
        };
        task_completion_order
            .set(next_task_completion_order())
            .expect("task completion recorded twice");
        let _ = tx.send(result);
    })});
    JetSchedulerJoin {
        rx,
        completion_order,
    }
}

/// Block until the scheduler queue drains (for tests/shutdown hooks).
pub fn jet_scheduler_drain() {
    let sched = scheduler();
    for _ in 0..5000 {
        if sched.live.load(Ordering::Relaxed) == 0 {
            let g = sched.global.lock().unwrap();
            let pending_local = sched
                .workers
                .iter()
                .any(|w| !w.queue.lock().unwrap().is_empty());
            let io_drained = METRIC_IO_ACTIVE.load(Ordering::Acquire) == 0
                && METRIC_IO_ALLOCATED.load(Ordering::Acquire)
                    == METRIC_IO_RETIRED.load(Ordering::Acquire);
            if g.is_empty() && !pending_local && io_drained {
                return;
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
}

// Keep Stream suspension and cancellation in the same Prelude source that is
// embedded into AOT programs. The compiled scheduler is only the JIT adapter's
// copy of that source.
