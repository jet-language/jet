// D-ASYNCRT1=A: compiled scheduler substrate for jet-jit host shims.
// Prelude/Scheduler.rs remains the emitted generated-program source.
#[cfg(test)]
thread_local! {
    static TEST_DEADLINE_EXCEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(all(test, target_os = "windows", feature = "jet_native_io"))]
mod iocp_runtime_tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    fn connected() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    fn wait_result<T: Send + 'static>(join: &JetSchedulerJoin<T>) -> JetSchedulerResult<T> {
        let start = Instant::now();
        loop {
            if let Some(result) = join.try_recv() { return result; }
            assert!(start.elapsed() < Duration::from_secs(10), "IOCP task timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn iocp_wake_cancel_deadline_stale_scale_and_cleanup() {
        assert_eq!(jet_scheduler_io_backend(), "iocp");

        let (client, mut server) = connected();
        let wake = jet_scheduler_spawn(move || {
            jet_scheduler_io_wait(&client, true, false, "iocp read");
            1i64
        });
        server.write_all(b"x").unwrap();
        assert!(matches!(wait_result(&wake), JetSchedulerResult::Value(1)));

        let (client, _server) = connected();
        let control = JetTaskControl::new();
        let cancelled = jet_scheduler_spawn_with_control(
            move || {
                jet_scheduler_io_wait(&client, true, false, "iocp cancel");
                2i64
            },
            control.clone(),
        );
        while io_poller().interests.lock().unwrap().is_empty() { std::thread::yield_now(); }
        control.cancel();
        assert!(matches!(wait_result(&cancelled), JetSchedulerResult::Cancelled));

        let (client, _server) = connected();
        let deadline = jet_scheduler_spawn(move || {
            TEST_DEADLINE_EXCEEDED.with(|value| value.set(true));
            jet_scheduler_io_wait(&client, true, false, "iocp deadline");
            3i64
        });
        assert!(matches!(wait_result(&deadline), JetSchedulerResult::Panicked));

        // Never-reused keys reject stale completions. Concurrent readers prove
        // eventual completion without starvation; each retires after readiness.
        #[link(name = "kernel32")]
        extern "system" {
            fn PostQueuedCompletionStatus(port: usize, bytes: u32, key: usize, ov: *mut std::ffi::c_void) -> i32;
        }
        let stale_before = METRIC_IO_STALE.load(Ordering::Relaxed);
        let port = io_poller().iocp_port.load(Ordering::Acquire);
        assert_ne!(port, 0);
        assert_ne!(unsafe {
            PostQueuedCompletionStatus(port, 0, usize::MAX, std::ptr::null_mut())
        }, 0);
        let stale_start = Instant::now();
        while METRIC_IO_STALE.load(Ordering::Relaxed) == stale_before {
            assert!(stale_start.elapsed() < Duration::from_secs(5), "stale IOCP packet not observed");
            std::thread::yield_now();
        }
        let before = io_poller().next_key.load(Ordering::Relaxed);
        let mut joins = Vec::new();
        let mut writers = Vec::new();
        for value in 0..64i64 {
            let (client, server) = connected();
            joins.push(jet_scheduler_spawn(move || {
                jet_scheduler_io_wait(&client, true, false, "iocp scale");
                value
            }));
            writers.push(server);
        }
        for writer in &mut writers { writer.write_all(b"x").unwrap(); }
        let mut values = Vec::new();
        for join in &joins {
            match wait_result(join) {
                JetSchedulerResult::Value(value) => values.push(value),
                _ => panic!("IOCP scale task did not complete"),
            }
        }
        values.sort_unstable();
        assert_eq!(values, (0..64).collect::<Vec<_>>());
        assert!(io_poller().next_key.load(Ordering::Relaxed) >= before + 64);
        let start = Instant::now();
        while !io_poller().interests.lock().unwrap().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5), "IOCP interests leaked");
            std::thread::yield_now();
        }
        jet_scheduler_drain();
        assert!(io_poller().streams.lock().unwrap().is_empty(), "IOCP socket clones leaked");
        let (active, allocated, retired) = jet_scheduler_metric_io_operations();
        assert_eq!(active, 0, "IOCP active registrations leaked");
        assert_eq!(allocated, retired, "OVERLAPPED allocations leaked");
    }
}

#[cfg(test)]
fn jet_deadline_remaining_ms() -> Option<i64> {
    TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.get().then_some(0))
}

#[cfg(not(test))]
fn jet_deadline_remaining_ms() -> Option<i64> {
    None
}

#[cfg(test)]
fn jet_deadline_exceeded(_kind: &str) -> ! {
    panic!("deadline exceeded");
}

#[cfg(not(test))]
fn jet_deadline_exceeded(_kind: &str) -> ! {
    std::process::exit(70)
}

thread_local! {
    static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn jet_scheduler_task_panic_enter() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(true));
}

pub fn jet_scheduler_task_panic_leave() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(false));
}

fn jet_scheduler_panic_should_unwind() -> bool {
    JET_IN_SCHEDULER_TASK.with(|c| c.get())
}
// D-ASYNCRT1=A (c126): M:N green-thread scheduler — work-stealing pool (M1) plus
// condvar park/wake substrate (M2): channel wake-on-send, timer sleep, IO poll
// hook, pause/cancel at yield points. std-only (I6).

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

type Job = Box<dyn FnOnce() + Send>;

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
pub fn jet_scheduler_shield_leave() {
    // Match `enter`: ambient deadlines must never begin unwinding merely because
    // ordinary non-task code crossed a lexical shield boundary.
    if current_task_control().is_none() || !jet_scheduler_panic_should_unwind() {
        return;
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
            jet_deadline_exceeded("shield exit");
        }
        // A cancel that arrived while shielded now takes effect at region exit.
        if jet_scheduler_task_cancelled() {
            jet_task_unwind_cancel();
        }
    }
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
    park: Arc<ParkSlot>,
    cancel_waiters: Mutex<Vec<std::sync::Weak<ParkSlot>>>,
}

impl JetTaskControl {
    pub fn new() -> Arc<Self> {
        Arc::new(JetTaskControl {
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            park: ParkSlot::new(),
            cancel_waiters: Mutex::new(Vec::new()),
        })
    }

    pub fn pause(&self) {
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
    if millis == 0 {
        return;
    }
    let slot = ParkSlot::new();
    timer_wheel().schedule(Instant::now() + Duration::from_millis(millis), slot.clone());
    jet_scheduler_yield("time sleep", &slot, Some(Duration::from_millis(millis)));
}

// ── M2: IO poll substrate (native epoll on Linux; portable fallback elsewhere) ─

struct IoInterest {
    stream_id: usize,
    slot: Arc<ParkSlot>,
    readable: bool,
    writable: bool,
}

struct IoPoller {
    interests: Mutex<Vec<IoInterest>>,
    streams: Mutex<HashMap<usize, Arc<Mutex<TcpStream>>>>,
    retire_requested: Mutex<HashSet<usize>>,
    notify: Condvar,
    next_key: AtomicUsize,
    #[cfg(target_os = "windows")]
    iocp_port: AtomicUsize,
}

// jet:scheduler-native-begin — vetted std-only OS FFI and poller dispatch.
#[allow(dead_code)]
impl IoPoller {
    fn register(
        self: &Arc<Self>,
        stream: Arc<Mutex<TcpStream>>,
        readable: bool,
        writable: bool,
    ) -> (usize, Arc<ParkSlot>) {
        let slot = ParkSlot::new();
        let mut streams = self.streams.lock().unwrap();
        let id = self.next_key.fetch_add(1, Ordering::Relaxed);
        streams.insert(id, stream);
        drop(streams);
        self.interests.lock().unwrap().push(IoInterest {
            stream_id: id,
            slot: slot.clone(),
            readable,
            writable,
        });
        self.notify.notify_one();
        #[cfg(target_os = "windows")]
        self.iocp_notify();
        (id, slot)
    }

    fn unregister(&self, id: usize) {
        self.interests.lock().unwrap().retain(|i| i.stream_id != id);
        self.retire_requested.lock().unwrap().insert(id);
        #[cfg(not(target_os = "windows"))]
        {
            self.streams.lock().unwrap().remove(&id);
            self.retire_requested.lock().unwrap().remove(&id);
        }
        #[cfg(target_os = "windows")]
        self.iocp_notify();
    }

    #[cfg(target_os = "windows")]
    fn iocp_notify(&self) {
        #[link(name = "kernel32")]
        extern "system" {
            fn PostQueuedCompletionStatus(port: usize, bytes: u32, key: usize, ov: *mut std::ffi::c_void) -> i32;
        }
        let port = self.iocp_port.load(Ordering::Acquire);
        if port != 0 {
            unsafe { PostQueuedCompletionStatus(port, 0, 0, std::ptr::null_mut()); }
        }
    }

    fn run(self: Arc<Self>) {
        #[cfg(all(target_os = "linux", feature = "jet_native_io"))]
        {
            self.run_linux_epoll();
        }
        #[cfg(all(
            any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ),
            feature = "jet_native_io"
        ))]
        {
            self.run_kqueue();
        }
        #[cfg(all(target_os = "windows", feature = "jet_native_io"))]
        {
            self.run_iocp();
        }
        #[cfg(not(any(
            all(target_os = "linux", feature = "jet_native_io"),
            all(
                any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "tvos",
                    target_os = "watchos",
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "openbsd"
                ),
                feature = "jet_native_io"
            ),
            all(target_os = "windows", feature = "jet_native_io")
        )))]
        {
            self.run_portable_poll();
        }
    }

    #[cfg(all(target_os = "linux", feature = "jet_native_io"))]
    fn run_linux_epoll(self: Arc<Self>) {
        use std::collections::HashMap;
        use std::os::unix::io::AsRawFd;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct EpollEvent {
            events: u32,
            data: u64,
        }

        const EPOLLIN: u32 = 1;
        const EPOLLOUT: u32 = 4;
        const EPOLL_CTL_ADD: i32 = 1;
        const EPOLL_CTL_DEL: i32 = 2;

        #[link(name = "c")]
        extern "C" {
            fn epoll_create1(flags: i32) -> i32;
            fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *const EpollEvent) -> i32;
            fn epoll_wait(
                epfd: i32,
                events: *mut EpollEvent,
                maxevents: i32,
                timeout: i32,
            ) -> i32;
        }

        let epfd = unsafe { epoll_create1(0x80000) }; // CLOEXEC
        assert!(epfd >= 0, "epoll_create1 failed");
        let mut fd_slots: HashMap<i32, Arc<ParkSlot>> = HashMap::new();

        loop {
            // Register new interests with epoll.
            let pending: Vec<(i32, u32, Arc<ParkSlot>)> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                let mut out = Vec::new();
                for interest in interests.iter() {
                    let Some(stream) = streams.get(&interest.stream_id) else {
                        continue;
                    };
                    let fd = stream.lock().unwrap().as_raw_fd();
                    if fd_slots.contains_key(&fd) {
                        continue;
                    }
                    let mut events = 0u32;
                    if interest.readable {
                        events |= EPOLLIN;
                    }
                    if interest.writable {
                        events |= EPOLLOUT;
                    }
                    out.push((fd, events, interest.slot.clone()));
                }
                out
            };
            for (fd, events, slot) in pending {
                let ev = EpollEvent {
                    events,
                    data: fd as u64,
                };
                let rc = unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &ev) };
                if rc == 0 {
                    fd_slots.insert(fd, slot);
                }
            }

            let mut events = [EpollEvent {
                events: 0,
                data: 0,
            }; 64];
            let n = unsafe { epoll_wait(epfd, events.as_mut_ptr(), 64, 50) };
            if n > 0 {
                METRIC_POLLER_WAKE.fetch_add(n as usize, Ordering::Relaxed);
                for ev in &events[..n as usize] {
                    let fd = ev.data as i32;
                    if let Some(slot) = fd_slots.remove(&fd) {
                        let _ = unsafe { epoll_ctl(epfd, EPOLL_CTL_DEL, fd, std::ptr::null()) };
                        let mut interests = self.interests.lock().unwrap();
                        interests.retain(|i| !Arc::ptr_eq(&i.slot, &slot));
                        slot.wake();
                    }
                }
            }
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "jet_native_io")))]
    fn run_linux_epoll(self: Arc<Self>) {
        self.run_portable_poll();
    }

    #[cfg(all(
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ),
        feature = "jet_native_io"
    ))]
    fn run_kqueue(self: Arc<Self>) {
        use std::collections::HashMap;
        use std::os::unix::io::AsRawFd;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Kevent {
            ident: usize,
            filter: i16,
            flags: u16,
            fflags: u32,
            data: i64,
            udata: *mut std::ffi::c_void,
        }

        const EVFILT_READ: i16 = -1;
        const EVFILT_WRITE: i16 = -2;
        const EV_ADD: u16 = 0x0001;
        const EV_DELETE: u16 = 0x0002;
        const EV_ONESHOT: u16 = 0x0010;

        #[link(name = "c")]
        extern "C" {
            fn kqueue() -> i32;
            fn kevent(
                kq: i32,
                changelist: *const Kevent,
                nchanges: i32,
                eventlist: *mut Kevent,
                nevents: i32,
                timeout: *const libc_timespec,
            ) -> i32;
        }

        #[repr(C)]
        struct libc_timespec {
            tv_sec: i64,
            tv_nsec: i64,
        }

        let kq = unsafe { kqueue() };
        assert!(kq >= 0, "kqueue() failed");
        let mut fd_slots: HashMap<i32, Arc<ParkSlot>> = HashMap::new();

        loop {
            let pending: Vec<(i32, i16, Arc<ParkSlot>)> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                let mut out = Vec::new();
                for interest in interests.iter() {
                    let Some(stream) = streams.get(&interest.stream_id) else {
                        continue;
                    };
                    let fd = stream.lock().unwrap().as_raw_fd();
                    if fd_slots.contains_key(&fd) {
                        continue;
                    }
                    if interest.readable {
                        out.push((fd, EVFILT_READ, interest.slot.clone()));
                    }
                    if interest.writable {
                        out.push((fd, EVFILT_WRITE, interest.slot.clone()));
                    }
                }
                out
            };
            for (fd, filter, slot) in pending {
                let ev = Kevent {
                    ident: fd as usize,
                    filter,
                    flags: EV_ADD | EV_ONESHOT,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                };
                let rc = unsafe { kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
                if rc == 0 {
                    fd_slots.insert(fd, slot);
                }
            }

            let mut events = [Kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            }; 64];
            let timeout = libc_timespec {
                tv_sec: 0,
                tv_nsec: 50_000_000,
            };
            let n = unsafe {
                kevent(
                    kq,
                    std::ptr::null(),
                    0,
                    events.as_mut_ptr(),
                    64,
                    &timeout,
                )
            };
            if n > 0 {
                METRIC_POLLER_WAKE.fetch_add(n as usize, Ordering::Relaxed);
                for ev in &events[..n as usize] {
                    let fd = ev.ident as i32;
                    if let Some(slot) = fd_slots.remove(&fd) {
                        let del = Kevent {
                            ident: ev.ident,
                            filter: ev.filter,
                            flags: EV_DELETE,
                            fflags: 0,
                            data: 0,
                            udata: std::ptr::null_mut(),
                        };
                        let _ = unsafe { kevent(kq, &del, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
                        let mut interests = self.interests.lock().unwrap();
                        interests.retain(|i| !Arc::ptr_eq(&i.slot, &slot));
                        slot.wake();
                    }
                }
            }
        }
    }

    #[cfg(not(all(
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ),
        feature = "jet_native_io"
    )))]
    fn run_kqueue(self: Arc<Self>) {
        self.run_portable_poll();
    }

    #[cfg(all(target_os = "windows", feature = "jet_native_io"))]
    fn run_iocp(self: Arc<Self>) {
        use std::collections::HashMap;
        use std::os::windows::io::AsRawSocket;
        #[repr(C)]
        struct Overlapped { internal: usize, internal_high: usize, offset: u32, offset_high: u32, event: usize }
        #[repr(C)]
        struct WsaBuf { len: u32, buf: *mut u8 }
        struct Active {
            _stream: Arc<Mutex<TcpStream>>,
            socket: usize,
            operations: Vec<*mut Overlapped>,
            cancel_requested: bool,
        }
        const INVALID_HANDLE_VALUE: usize = usize::MAX;
        const WSA_IO_PENDING: i32 = 997;
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateIoCompletionPort(file: usize, existing: usize, key: usize, threads: u32) -> usize;
            fn GetQueuedCompletionStatus(port: usize, bytes: *mut u32, key: *mut usize, ov: *mut *mut Overlapped, timeout_ms: u32) -> i32;
            fn CancelIoEx(file: usize, ov: *mut Overlapped) -> i32;
            fn GetLastError() -> u32;
        }
        #[link(name = "ws2_32")]
        extern "system" {
            fn WSARecv(socket: usize, buffers: *mut WsaBuf, count: u32, bytes: *mut u32, flags: *mut u32, ov: *mut Overlapped, completion: *mut std::ffi::c_void) -> i32;
            fn WSASend(socket: usize, buffers: *mut WsaBuf, count: u32, bytes: *mut u32, flags: u32, ov: *mut Overlapped, completion: *mut std::ffi::c_void) -> i32;
            fn WSAGetLastError() -> i32;
        }

        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, 1) };
        if port == 0 {
            METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
            for interest in self.interests.lock().unwrap().drain(..) { interest.slot.wake(); }
            return;
        }
        self.iocp_port.store(port, Ordering::Release);
        let mut active: HashMap<usize, Active> = HashMap::new();
        loop {
            let pending: Vec<(usize, Arc<Mutex<TcpStream>>, usize, bool, bool)> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                interests.iter()
                    .filter(|interest| !active.contains_key(&interest.stream_id))
                    .filter_map(|interest| streams.get(&interest.stream_id).map(|stream| (
                        interest.stream_id,
                        stream.clone(),
                        stream.lock().unwrap().as_raw_socket() as usize,
                        interest.readable,
                        interest.writable,
                    )))
                    .collect()
            };
            for (id, stream, socket, readable, writable) in pending {
                let associated = unsafe { CreateIoCompletionPort(socket, port, id + 1, 0) };
                if associated == 0 {
                    let _error = unsafe { GetLastError() };
                    METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
                    if let Some(slot) = self.interests.lock().unwrap().iter()
                        .find(|interest| interest.stream_id == id).map(|interest| interest.slot.clone())
                    { slot.wake(); }
                    self.interests.lock().unwrap().retain(|interest| interest.stream_id != id);
                    self.streams.lock().unwrap().remove(&id);
                    continue;
                }
                let mut operations = Vec::new();
                for read in [true, false] {
                    if (read && !readable) || (!read && !writable) { continue; }
                    let raw = Box::into_raw(Box::new(Overlapped {
                        internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: 0,
                    }));
                    METRIC_IO_ALLOCATED.fetch_add(1, Ordering::Relaxed);
                    let mut buffer = WsaBuf { len: 0, buf: std::ptr::null_mut() };
                    let mut bytes = 0;
                    let rc = if read {
                        let mut flags = 0;
                        unsafe { WSARecv(socket, &mut buffer, 1, &mut bytes, &mut flags, raw, std::ptr::null_mut()) }
                    } else {
                        unsafe { WSASend(socket, &mut buffer, 1, &mut bytes, 0, raw, std::ptr::null_mut()) }
                    };
                    if rc == 0 || unsafe { WSAGetLastError() } == WSA_IO_PENDING {
                        operations.push(raw);
                    } else {
                        unsafe { drop(Box::from_raw(raw)); }
                        METRIC_IO_RETIRED.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if operations.is_empty() {
                    if let Some(slot) = self.interests.lock().unwrap().iter()
                        .find(|interest| interest.stream_id == id).map(|interest| interest.slot.clone())
                    { slot.wake(); }
                } else {
                    active.insert(id, Active { _stream: stream, socket, operations, cancel_requested: false });
                    METRIC_IO_ACTIVE.fetch_add(1, Ordering::Relaxed);
                }
            }

            let retiring: Vec<usize> = self.retire_requested.lock().unwrap().iter().copied().collect();
            for id in retiring {
                if let Some(entry) = active.get_mut(&id) {
                    if !entry.cancel_requested {
                        entry.cancel_requested = true;
                        for operation in &entry.operations { unsafe { CancelIoEx(entry.socket, *operation); } }
                    }
                } else {
                    self.streams.lock().unwrap().remove(&id);
                    self.retire_requested.lock().unwrap().remove(&id);
                }
            }

            for (id, entry) in active.iter_mut() {
                if !self.interests.lock().unwrap().iter().any(|interest| interest.stream_id == *id)
                    && !entry.cancel_requested {
                    entry.cancel_requested = true;
                    for operation in &entry.operations { unsafe { CancelIoEx(entry.socket, *operation); } }
                }
            }

            let (mut bytes, mut key, mut operation) = (0, 0usize, std::ptr::null_mut());
            let ok = unsafe { GetQueuedCompletionStatus(port, &mut bytes, &mut key, &mut operation, u32::MAX) };
            if operation.is_null() {
                if ok == 0 {
                    METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
                    for interest in self.interests.lock().unwrap().drain(..) { interest.slot.wake(); }
                    return;
                }
                if key != 0 { METRIC_IO_STALE.fetch_add(1, Ordering::Relaxed); }
                continue;
            }
            if ok == 0 && unsafe { GetLastError() } != 995 {
                METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { drop(Box::from_raw(operation)); }
            METRIC_IO_RETIRED.fetch_add(1, Ordering::Relaxed);
            let id = key.saturating_sub(1);
            let Some(entry) = active.get_mut(&id) else { continue; };
            entry.operations.retain(|candidate| *candidate != operation);
            let slot = {
                let mut interests = self.interests.lock().unwrap();
                let slot = interests.iter().find(|interest| interest.stream_id == id)
                    .map(|interest| interest.slot.clone());
                interests.retain(|interest| interest.stream_id != id);
                slot
            };
            if let Some(slot) = slot {
                METRIC_POLLER_WAKE.fetch_add(1, Ordering::Relaxed);
                for pending in &entry.operations { unsafe { CancelIoEx(entry.socket, *pending); } }
                slot.wake();
            }
            if entry.operations.is_empty() {
                active.remove(&id);
                METRIC_IO_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                self.streams.lock().unwrap().remove(&id);
                self.retire_requested.lock().unwrap().remove(&id);
            }
        }
    }
    // jet:scheduler-native-end

    #[cfg(not(all(target_os = "windows", feature = "jet_native_io")))]
    fn run_iocp(self: Arc<Self>) {
        self.run_portable_poll();
    }

    #[allow(dead_code)]
    fn run_portable_poll(self: Arc<Self>) {
        use std::io::Write;
        loop {
            let ready: Vec<Arc<ParkSlot>> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                let mut slots = Vec::new();
                for interest in interests.iter() {
                    let Some(stream) = streams.get(&interest.stream_id) else {
                        continue;
                    };
                    let mut s = stream.lock().unwrap();
                    let _ = s.set_nonblocking(true);
                    let mut buf = [0u8; 1];
                    if interest.readable {
                        match s.peek(&mut buf) {
                            Ok(0) | Ok(_) => slots.push(interest.slot.clone()),
                            Err(_) => {}
                        }
                    }
                    if interest.writable {
                        match s.write(&[]) {
                            Ok(_) => slots.push(interest.slot.clone()),
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(_) => slots.push(interest.slot.clone()),
                        }
                    }
                }
                slots
            };
            if !ready.is_empty() {
                let mut interests = self.interests.lock().unwrap();
                for slot in &ready {
                    interests.retain(|i| !Arc::ptr_eq(&i.slot, slot));
                    slot.wake();
                }
            }
            let g = self.interests.lock().unwrap();
            let _ = self
                .notify
                .wait_timeout(g, Duration::from_millis(5))
                .unwrap();
        }
    }
}

static IO_POLLER: OnceLock<Arc<IoPoller>> = OnceLock::new();

fn io_poller() -> Arc<IoPoller> {
    IO_POLLER
        .get_or_init(|| {
            let poller = Arc::new(IoPoller {
                interests: Mutex::new(Vec::new()),
                streams: Mutex::new(HashMap::new()),
                retire_requested: Mutex::new(HashSet::new()),
                notify: Condvar::new(),
                next_key: AtomicUsize::new(0),
                #[cfg(target_os = "windows")]
                iocp_port: AtomicUsize::new(0),
            });
            let p = poller.clone();
            thread::spawn(move || p.run());
            poller
        })
        .clone()
}

/// Park until `stream` looks readable or writable (non-blocking probe via poller).
pub fn jet_scheduler_io_wait(stream: &TcpStream, read: bool, write: bool, wait_kind: &str) {
    let shared = Arc::new(Mutex::new(stream.try_clone().expect("tcp clone")));
    let poller = io_poller();
    let (id, slot) = poller.register(shared, read, write);
    struct Registration(Arc<IoPoller>, usize);
    impl Drop for Registration {
        fn drop(&mut self) { self.0.unregister(self.1); }
    }
    let _registration = Registration(poller, id);
    jet_scheduler_yield(wait_kind, &slot, None);
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
            let parked = {
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
                true
            };
            if parked {
                jet_scheduler_yield("channel receive", &slot, None);
                let mut st = self.inner.state.lock().unwrap();
                st.recv_waiters.retain(|w| !Arc::ptr_eq(w, &slot));
            }
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
                    st.queue.push_back(value.take().expect("channel send value missing"));
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
static METRIC_IO_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static METRIC_IO_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static METRIC_IO_RETIRED: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "windows")]
static METRIC_IO_STALE: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "windows")]
static METRIC_IO_FAILURES: AtomicUsize = AtomicUsize::new(0);
static IO_BACKEND: OnceLock<&'static str> = OnceLock::new();

#[allow(unreachable_code)]
pub fn jet_scheduler_io_backend() -> &'static str {
    *IO_BACKEND.get_or_init(|| {
        #[cfg(all(target_os = "linux", feature = "jet_native_io"))]
        {
            return "epoll";
        }
        #[cfg(all(
            any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ),
            feature = "jet_native_io"
        ))]
        {
            return "kqueue";
        }
        #[cfg(all(target_os = "windows", feature = "jet_native_io"))]
        {
            return "iocp";
        }
        "portable-poll"
    })
}

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

pub fn jet_scheduler_metric_io_operations() -> (usize, usize, usize) {
    (
        METRIC_IO_ACTIVE.load(Ordering::Relaxed),
        METRIC_IO_ALLOCATED.load(Ordering::Relaxed),
        METRIC_IO_RETIRED.load(Ordering::Relaxed),
    )
}

// ── M1: work-stealing pool ───────────────────────────────────────────────────

struct WorkerSlot {
    queue: Mutex<VecDeque<Job>>,
}

struct Scheduler {
    workers: Vec<WorkerSlot>,
    global: Mutex<VecDeque<Job>>,
    notify: Condvar,
    live: AtomicUsize,
    shutdown: AtomicBool,
}

impl Scheduler {
    fn pop_local(&self, id: usize) -> Option<Job> {
        self.workers[id].queue.lock().unwrap().pop_back()
    }

    fn pop_global(&self) -> Option<Job> {
        self.global.lock().unwrap().pop_front()
    }

    fn steal_from(&self, thief: usize) -> Option<Job> {
        for (idx, w) in self.workers.iter().enumerate() {
            if idx == thief {
                continue;
            }
            let mut q = w.queue.lock().unwrap();
            if let Some(job) = q.pop_front() {
                return Some(job);
            }
        }
        None
    }

    fn take_work(&self, id: usize) -> Option<Job> {
        self.pop_local(id)
            .or_else(|| self.pop_global())
            .or_else(|| self.steal_from(id))
    }

    fn worker_loop(self: &Arc<Self>, id: usize) {
        loop {
            if let Some(job) = self.take_work(id) {
                self.live.fetch_add(1, Ordering::Relaxed);
                job();
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

    fn submit(self: &Arc<Self>, job: Job) {
        self.global.lock().unwrap().push_back(job);
        self.notify.notify_one();
    }
}

static SCHEDULER: OnceLock<Arc<Scheduler>> = OnceLock::new();

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
            });
            for id in 0..n {
                let s = sched.clone();
                thread::spawn(move || s.worker_loop(id));
            }
            sched
        })
        .clone()
}

enum JetSchedulerResult<T> {
    Value(T),
    Panicked,
    Cancelled,
}

pub struct JetSchedulerJoin<T> {
    rx: std::sync::mpsc::Receiver<JetSchedulerResult<T>>,
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
        }
    }

    fn try_recv(&self) -> Option<JetSchedulerResult<T>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(JetSchedulerResult::Panicked),
        }
    }

    fn drain(self) {
        let _ = self.rx.recv();
    }
}

/// D-CONCCOMB1: join every handle in list order; fail fast and cancel siblings on error.
pub fn jet_scheduler_all<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
) -> Vec<T> {
    assert!(!entries.is_empty(), "all: empty task list");
    let n = entries.len();
    let mut out: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let mut pending = n;
    loop {
        for (i, (join, _)) in entries.iter().enumerate() {
            if out[i].is_some() {
                continue;
            }
            let Some(res) = join.try_recv() else {
                continue;
            };
            match res {
                JetSchedulerResult::Value(v) => {
                    out[i] = Some(v);
                    pending -= 1;
                    if pending == 0 {
                        for (join, _) in entries {
                            join.drain();
                        }
                        return out
                            .into_iter()
                            .map(|v| v.expect("all: missing result"))
                            .collect();
                    }
                }
                JetSchedulerResult::Panicked => {
                    for (_, ctrl) in &entries {
                        ctrl.cancel();
                    }
                    for (join, _) in entries {
                        join.drain();
                    }
                    jet_scheduler_drain();
                    jet_scheduler_fatal("a task panicked");
                }
                JetSchedulerResult::Cancelled => {
                    for (_, ctrl) in &entries {
                        ctrl.cancel();
                    }
                    for (join, _) in entries {
                        join.drain();
                    }
                    jet_scheduler_drain();
                    jet_scheduler_fatal("a task was cancelled");
                }
            }
        }
        thread::sleep(Duration::from_micros(50));
    }
}

/// D-CONCCOMB1/D-RACEWIN1: first successful result wins; cancel losers.
pub fn jet_scheduler_race<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
) -> T {
    assert!(!entries.is_empty(), "race: empty task list");
    let n = entries.len();
    let mut settled = vec![false; n];
    let mut settled_count = 0usize;
    loop {
        for (i, (join, _)) in entries.iter().enumerate() {
            if settled[i] {
                continue;
            }
            let Some(res) = join.try_recv() else {
                continue;
            };
            match res {
                JetSchedulerResult::Value(v) => {
                    for (j, (_, ctrl)) in entries.iter().enumerate() {
                        if j != i {
                            ctrl.cancel();
                        }
                    }
                    let mut losers = Vec::new();
                    for (j, (join, _)) in entries.into_iter().enumerate() {
                        if j != i {
                            losers.push(join);
                        }
                    }
                    for join in losers {
                        join.drain();
                    }
                    jet_scheduler_drain();
                    return v;
                }
                JetSchedulerResult::Panicked | JetSchedulerResult::Cancelled => {
                    settled[i] = true;
                    settled_count += 1;
                }
            }
        }
        if settled_count == n {
            jet_scheduler_fatal("a task panicked");
        }
        thread::sleep(Duration::from_micros(50));
    }
}

/// D-CONCCOMB1: first completed result wins (success or failure path visible).
pub fn jet_scheduler_any<T: Send + 'static>(
    entries: Vec<(JetSchedulerJoin<T>, Arc<JetTaskControl>)>,
) -> T {
    assert!(!entries.is_empty(), "any: empty task list");
    loop {
        for (i, (join, _)) in entries.iter().enumerate() {
            let Some(res) = join.try_recv() else {
                continue;
            };
            for (j, (_, ctrl)) in entries.iter().enumerate() {
                if j != i {
                    ctrl.cancel();
                }
            }
            let mut losers = Vec::new();
            for (j, (join, _)) in entries.into_iter().enumerate() {
                if j != i {
                    losers.push(join);
                }
            }
            for join in losers {
                join.drain();
            }
            jet_scheduler_drain();
            return match res {
                JetSchedulerResult::Value(v) => v,
                JetSchedulerResult::Panicked | JetSchedulerResult::Cancelled => {
                    jet_scheduler_fatal("a task panicked");
                }
            };
        }
        thread::sleep(Duration::from_micros(50));
    }
}

/// D-CONCSELECT1=A: JIT/AOT entry for fluent `g.select()` over scheduler channels.
pub fn jet_scheduler_select_int_channels(
    channels: &[JetSchedulerChannel<i64>],
    after_ms: Vec<u64>,
) -> i64 {
    let inners: Vec<_> = channels.iter().map(|c| c.select_inner()).collect();
    match jet_scheduler_select(inners, after_ms) {
        JetSelectOutcome::Recv { value, .. } => value,
        JetSelectOutcome::After { .. } => {
            jet_scheduler_fatal("select timer arm has no receive value");
        }
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

pub fn jet_scheduler_spawn_with_control<F, T>(
    f: F,
    control: Arc<JetTaskControl>,
) -> JetSchedulerJoin<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    scheduler().submit(Box::new(move || {
        jet_scheduler_set_task_control(Some(control.clone()));
        jet_scheduler_task_panic_enter();
        control.wait_while_paused();
        // Cancel while still paused (never resumed) aborts; cancel after resume only
        // affects yield points inside `f`, not startup — matches 157_task_controls.
        if control.paused.load(Ordering::Relaxed) && control.cancelled.load(Ordering::Relaxed) {
            jet_scheduler_task_panic_leave();
            jet_scheduler_set_task_control(None);
            let _ = tx.send(JetSchedulerResult::Cancelled);
            return;
        }
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        std::panic::set_hook(prev_hook);
        jet_scheduler_task_panic_leave();
        jet_scheduler_set_task_control(None);
        let _ = tx.send(match out {
            Ok(v) => JetSchedulerResult::Value(v),
            // D-CANCELMODEL1=C: a `JetCancelUnwind` payload is a task that unwound at
            // a wait point because it was cancelled — report Cancelled, not Panicked.
            Err(e) if e.is::<JetCancelUnwind>() => JetSchedulerResult::Cancelled,
            Err(_) => JetSchedulerResult::Panicked,
        });
    }));
    JetSchedulerJoin { rx }
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

#[cfg(test)]
mod interrupt_boundary_tests {
    use super::*;

    fn select_with_timeout(
        channels: Vec<JetSchedulerChannel<i64>>,
        timers: Vec<u64>,
    ) -> JetSelectOutcome<i64> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let inners = channels.into_iter().map(|ch| ch.select_inner()).collect();
            let _ = tx.send(jet_scheduler_select(inners, timers));
        });
        rx.recv_timeout(Duration::from_millis(250))
            .expect("select did not wake after every channel closed")
    }

    #[test]
    fn closed_select_failure_unwinds_inside_runtime_boundary() {
        jet_scheduler_task_panic_enter();
        let result = std::panic::catch_unwind(|| jet_scheduler_fatal("select closed"));
        jet_scheduler_task_panic_leave();
        assert!(result.is_err());
    }

    #[test]
    fn select_returns_closed_when_one_channel_is_closed_and_empty() {
        let channel = JetSchedulerChannel::<i64>::new();
        channel.close();
        assert!(matches!(
            select_with_timeout(vec![channel], Vec::new()),
            JetSelectOutcome::Closed
        ));
    }

    #[test]
    fn select_returns_closed_only_when_all_channels_are_closed_and_empty() {
        let first = JetSchedulerChannel::<i64>::new();
        let second = JetSchedulerChannel::<i64>::new();
        first.close();
        second.close();
        assert!(matches!(
            select_with_timeout(vec![first, second], Vec::new()),
            JetSelectOutcome::Closed
        ));
    }

    #[test]
    fn select_wakes_when_last_open_channel_closes_after_park() {
        let channel = JetSchedulerChannel::<i64>::new();
        let closer = channel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            closer.close();
        });
        assert!(matches!(
            select_with_timeout(vec![channel], Vec::new()),
            JetSelectOutcome::Closed
        ));
    }

    #[test]
    fn select_ready_value_and_timer_keep_precedence_over_closed() {
        let valued = JetSchedulerChannel::<i64>::new();
        let sender = valued.sender();
        assert!(sender.send(7));
        drop(sender);
        assert!(matches!(
            select_with_timeout(vec![valued], Vec::new()),
            JetSelectOutcome::Recv { value: 7, .. }
        ));

        let closed = JetSchedulerChannel::<i64>::new();
        closed.close();
        assert!(matches!(
            select_with_timeout(vec![closed], vec![0]),
            JetSelectOutcome::After { arm: 0 }
        ));

        let closed = JetSchedulerChannel::<i64>::new();
        closed.close();
        assert!(matches!(
            select_with_timeout(vec![closed], vec![10]),
            JetSelectOutcome::After { arm: 0 }
        ));
    }

    #[test]
    fn select_cancellation_keeps_precedence_over_waiting() {
        let control = JetTaskControl::new();
        control.cancel();
        jet_scheduler_set_task_control(Some(control));
        let channel = JetSchedulerChannel::<i64>::new();
        let outcome = jet_scheduler_select(vec![channel.select_inner()], Vec::new());
        jet_scheduler_set_task_control(None);
        assert!(matches!(outcome, JetSelectOutcome::Closed));
    }

    #[test]
    fn select_cancellation_after_park_wakes_and_cleans_waiters() {
        let control = JetTaskControl::new();
        let channel = JetSchedulerChannel::<i64>::new();
        let inner = channel.select_inner();
        let selected_inner = inner.clone();
        let selected_control = control.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            jet_scheduler_set_task_control(Some(selected_control));
            let outcome = jet_scheduler_select(vec![selected_inner], Vec::new());
            jet_scheduler_set_task_control(None);
            let _ = tx.send(outcome);
        });

        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            let channel_registered = !inner.state.lock().unwrap().recv_waiters.is_empty();
            let cancel_registered = !control.cancel_waiters.lock().unwrap().is_empty();
            if channel_registered && cancel_registered {
                break;
            }
            assert!(Instant::now() < deadline, "select did not park");
            std::thread::yield_now();
        }

        control.cancel();
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(250))
                .expect("cancelled select did not wake"),
            JetSelectOutcome::Closed
        ));
        assert!(inner.state.lock().unwrap().recv_waiters.is_empty());
        assert!(control.cancel_waiters.lock().unwrap().is_empty());
    }

    #[test]
    fn select_deadline_unwind_cleans_all_waiters() {
        let control = JetTaskControl::new();
        let channel = JetSchedulerChannel::<i64>::new();
        let inner = channel.select_inner();
        jet_scheduler_set_task_control(Some(control.clone()));
        jet_scheduler_task_panic_enter();
        let result = {
            TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(true));
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                jet_scheduler_select(vec![inner.clone()], Vec::new())
            }))
        };
        TEST_DEADLINE_EXCEEDED.with(|deadline| deadline.set(false));
        jet_scheduler_task_panic_leave();
        jet_scheduler_set_task_control(None);

        assert!(result.is_err());
        assert!(inner.state.lock().unwrap().recv_waiters.is_empty());
        assert!(control.cancel_waiters.lock().unwrap().is_empty());
    }

    // Tower #126 scale guard. Drives `n` tasks against a capacity-1 channel so
    // every send hits backpressure and must PARK until the receiver drains it.
    // Deterministic (no rustc, no wall-clock thresholds) and it fails on the three
    // audited failure modes:
    //   * lost wake / deadlock  → the watchdog trips instead of hanging forever,
    //   * busy-wait             → zero real condvar blocks recorded,
    //   * waiter leak           → send/recv waiter vectors are not drained.
    fn run_backpressure_scale(n: i64) {
        let handle = std::thread::spawn(move || {
            let before = jet_scheduler_metric_park_blocks();
            let channel = JetSchedulerChannel::<i64>::bounded(1);
            for _ in 0..n {
                let sender = channel.sender();
                let _ = jet_scheduler_spawn(move || {
                    sender.send(1);
                });
            }
            let mut total = 0i64;
            for _ in 0..n {
                total += channel.receive().expect("channel closed before all sends drained");
            }
            jet_scheduler_drain();
            let blocks = jet_scheduler_metric_park_blocks().saturating_sub(before);
            let inner = channel.select_inner();
            let st = inner.state.lock().unwrap();
            (total, blocks, st.send_waiters.len(), st.recv_waiters.len())
        });

        let start = Instant::now();
        let budget = Duration::from_secs(120);
        while !handle.is_finished() {
            assert!(
                start.elapsed() < budget,
                "scale workload hung: a park never woke (lost-wake / deadlock)"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let (total, blocks, send_leak, recv_leak) = handle.join().expect("scale worker panicked");

        assert_eq!(total, n, "every task's message must be delivered exactly once");
        assert!(
            blocks > 0,
            "no task ever blocked on a park condvar under capacity-1 backpressure — \
             the scheduler is busy-waiting, not parking"
        );
        assert_eq!(send_leak, 0, "send waiters leaked after drain (unbounded growth)");
        assert_eq!(recv_leak, 0, "recv waiters leaked after drain (unbounded growth)");
    }

    #[test]
    fn scale_10k_tasks_park_under_backpressure() {
        run_backpressure_scale(10_000);
    }

    #[test]
    #[ignore = "local 100k parked-task scale proof; run with --ignored"]
    fn scale_100k_tasks_park_under_backpressure() {
        run_backpressure_scale(100_000);
    }

    // Tower #126: prove pause/cancel are real control over a *running* task —
    // they actually park/unblock it at its wait point, not merely flip a flag a
    // `trace()` can read.

    #[test]
    fn pause_holds_a_running_task_at_its_wait_point_until_resume() {
        use std::sync::atomic::AtomicUsize;
        let control = JetTaskControl::new();
        let ready = JetSchedulerChannel::<i64>::new();
        let ready_tx = ready.sender();
        let work = JetSchedulerChannel::<i64>::new();
        let work_tx = work.sender();
        let progressed = Arc::new(AtomicUsize::new(0));

        let task_ready = ready_tx;
        let task_work = work;
        let task_progressed = progressed.clone();
        let _join = jet_scheduler_spawn_with_control(
            move || {
                task_ready.send(1);
                // Parks here until a value arrives AND the task is not paused.
                let _ = task_work.receive();
                task_progressed.fetch_add(1, Ordering::SeqCst);
            },
            control.clone(),
        );

        // Task has reached the wait point.
        assert_eq!(ready.receive(), Some(1));
        control.pause();
        // Make the value available: a flag-only "pause" would let the task run.
        work_tx.send(42);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            progressed.load(Ordering::SeqCst),
            0,
            "paused task consumed the value and ran past its wait point — pause is not real"
        );

        control.resume();
        let start = Instant::now();
        while progressed.load(Ordering::SeqCst) == 0 {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "resumed task never progressed"
            );
            std::thread::yield_now();
        }
        jet_scheduler_drain();
    }

    // D-CANCELMODEL1=C: cancel is PREEMPTIVE — a cancelled parked task unwinds at
    // its wait point, runs Drop-backed cleanup, never runs the code after the wait,
    // and its result becomes Cancelled (not a delivered value).
    #[test]
    fn cancel_unwinds_a_parked_task_runs_drop_and_reports_cancelled() {
        use std::sync::atomic::AtomicUsize;

        // 0 = untouched, 1 = ran past the wait (BUG), 2 = Drop ran during unwind.
        struct DropMark(Arc<AtomicUsize>);
        impl Drop for DropMark {
            fn drop(&mut self) {
                // Only record the unwind cleanup; a normal return sets 1 first.
                let _ = self.0.compare_exchange(
                    0,
                    2,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
        }

        let control = JetTaskControl::new();
        let ready = JetSchedulerChannel::<i64>::new();
        let ready_tx = ready.sender();
        // Nothing is ever sent on `work`: only cancellation can free the task.
        let work = JetSchedulerChannel::<i64>::new();
        let outcome = Arc::new(AtomicUsize::new(0));

        let task_ready = ready_tx;
        let task_work = work;
        let task_outcome = outcome.clone();
        let join = jet_scheduler_spawn_with_control(
            move || {
                let _mark = DropMark(task_outcome.clone());
                task_ready.send(1);
                // Cancel unwinds HERE; the store below must never run.
                let _got = task_work.receive();
                task_outcome.store(1, Ordering::SeqCst);
                0i64
            },
            control.clone(),
        );

        assert_eq!(ready.receive(), Some(1));
        // Task is now parked forever unless cancel actually unwinds it.
        control.cancel();
        let start = Instant::now();
        loop {
            match join.try_recv() {
                Some(JetSchedulerResult::Cancelled) => break,
                Some(other) => panic!(
                    "cancelled task must report Cancelled, got {}",
                    match other {
                        JetSchedulerResult::Value(_) => "Value",
                        JetSchedulerResult::Panicked => "Panicked",
                        JetSchedulerResult::Cancelled => unreachable!(),
                    }
                ),
                None => {
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "cancel did not unwind the parked task"
                    );
                    std::thread::yield_now();
                }
            }
        }
        assert_eq!(
            outcome.load(Ordering::SeqCst),
            2,
            "unwind must run Drop cleanup and skip the code after the wait point"
        );
        jet_scheduler_drain();
    }

    // D-CANCELMODEL1=C shield: a cancel that arrives while a shielded region runs
    // is DEFERRED — wait points inside complete normally, and the unwind lands only
    // when the region exits. Runtime machinery is syntax-free until D-SHIELDNAME1.
    #[test]
    fn shielded_region_defers_cancel_until_it_exits() {
        use std::sync::atomic::AtomicUsize;
        let control = JetTaskControl::new();
        let ready = JetSchedulerChannel::<i64>::new();
        let ready_tx = ready.sender();
        // A value IS delivered so the shielded recv can complete despite the cancel.
        let work = JetSchedulerChannel::<i64>::new();
        let work_tx = work.sender();
        // 0 none, bit1 = shielded recv completed, then unwind => Cancelled result.
        let stage = Arc::new(AtomicUsize::new(0));

        let task_ready = ready_tx;
        let task_work = work;
        let task_stage = stage.clone();
        let join = jet_scheduler_spawn_with_control(
            move || {
                task_ready.send(1);
                jet_scheduler_shield_enter();
                // Wait point INSIDE the shield: must NOT unwind on the pending cancel.
                let got = task_work.receive();
                if got == Some(42) {
                    task_stage.store(1, Ordering::SeqCst);
                }
                jet_scheduler_shield_leave(); // pending cancel unwinds HERE
                task_stage.store(9, Ordering::SeqCst); // must never run
                0i64
            },
            control.clone(),
        );

        assert_eq!(ready.receive(), Some(1));
        // Cancel while the task is parked inside the shield.
        control.cancel();
        // Give cancel a moment; the shielded recv must still be waiting for its value.
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            stage.load(Ordering::SeqCst),
            0,
            "shielded recv completed or unwound before its value arrived"
        );
        work_tx.send(42); // completes the shielded recv
        let start = Instant::now();
        loop {
            match join.try_recv() {
                Some(JetSchedulerResult::Cancelled) => break,
                Some(_) => panic!("shielded task must end Cancelled after the region"),
                None => {
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "deferred cancel never landed at shield exit"
                    );
                    std::thread::yield_now();
                }
            }
        }
        assert_eq!(
            stage.load(Ordering::SeqCst),
            1,
            "shielded recv must complete (stage 1) and the post-shield code must not run"
        );
        jet_scheduler_drain();
    }

    // D-CANCELMODEL1=C shield/deadline interaction: a deadline that closes while
    // shielded is likewise deferred to region exit (E3003 unwind), staying
    // consistent with the cancel case.
    #[test]
    fn shield_defers_deadline_until_it_exits() {
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));
        jet_scheduler_task_panic_enter();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            jet_scheduler_shield_enter();
            // Deadline is exceeded, but we are shielded: no unwind here.
            TEST_DEADLINE_EXCEEDED.with(|d| d.set(true));
            let slot = ParkSlot::new();
            slot.wake();
            jet_scheduler_yield("shielded wait", &slot, Some(Duration::from_millis(1)));
            // Reaching here proves the shielded wait did not unwind on the deadline.
            jet_scheduler_shield_leave(); // deadline unwinds HERE
            "no-unwind"
        }));
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));
        jet_scheduler_set_task_control(None);
        jet_scheduler_task_panic_leave();
        assert!(
            result.is_err(),
            "deadline deferred by the shield must unwind when the region exits"
        );
    }

    // Exercise the exact RAII shape emitted by Codegen/TIR/emit/statements.rs.
    // These helpers deliberately do not call `_leave` from test bodies: Drop is
    // what must cover every control-flow and unwind edge.
    struct EmittedShieldGuard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for EmittedShieldGuard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }

    macro_rules! emitted_shield {
        ($body:block) => {{
            jet_scheduler_shield_enter();
            let _shield_guard = EmittedShieldGuard(Some(|| jet_scheduler_shield_leave()));
            $body
        }};
    }

    fn emitted_early_return() -> i64 {
        emitted_shield!({ return 17 });
    }

    fn emitted_try_exit() -> Result<i64, &'static str> {
        emitted_shield!({ Err("stop")? });
        Ok(1)
    }

    #[test]
    fn emitted_shield_guard_covers_control_flow_unwind_and_reset_matrix() {
        // Outside a task/catch frame, even an expired ambient deadline is inert.
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(true));
        emitted_shield!({ assert!(!jet_scheduler_shielded()) });
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));

        jet_scheduler_task_panic_enter();
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));

        emitted_shield!({
            assert!(jet_scheduler_shielded());
            emitted_shield!({ assert!(jet_scheduler_shielded()) });
            assert!(jet_scheduler_shielded());
        });
        assert!(!jet_scheduler_shielded(), "nested guards must balance depth");
        assert_eq!(emitted_early_return(), 17);
        assert!(!jet_scheduler_shielded(), "return must drop the guard");
        assert_eq!(emitted_try_exit(), Err("stop"));
        assert!(!jet_scheduler_shielded(), "? must drop the guard");

        // A body panic wins over pending cancel/deadline: guard decrements depth
        // but must not begin a second panic while unwinding.
        for pending_deadline in [false, true] {
            let control = JetTaskControl::new();
            control.cancel();
            jet_scheduler_set_task_control(Some(control));
            TEST_DEADLINE_EXCEEDED.with(|d| d.set(pending_deadline));
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                emitted_shield!({ panic!("body panic") });
            }));
            let text = panic
                .expect_err("body must panic")
                .downcast::<&'static str>()
                .map(|s| *s)
                .unwrap_or("");
            assert_eq!(text, "body panic");
            assert!(!jet_scheduler_shielded(), "panic must reset shield depth");
            TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));
        }

        // When both become pending during a normal body, deadline has priority.
        let control = JetTaskControl::new();
        control.cancel();
        jet_scheduler_set_task_control(Some(control));
        let both = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            emitted_shield!({ TEST_DEADLINE_EXCEEDED.with(|d| d.set(true)) });
        }));
        let payload = both.expect_err("pending deadline must land at guard drop");
        let text = if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            payload.downcast_ref::<&'static str>().copied().unwrap_or("")
        };
        assert_eq!(text, "deadline exceeded");
        assert!(!jet_scheduler_shielded());
        TEST_DEADLINE_EXCEEDED.with(|d| d.set(false));

        // Same worker/thread can run a later task with clean depth and control.
        jet_scheduler_set_task_control(Some(JetTaskControl::new()));
        emitted_shield!({ assert!(jet_scheduler_shielded()) });
        assert!(!jet_scheduler_shielded(), "subsequent task must start clean");
        jet_scheduler_set_task_control(None);
        jet_scheduler_task_panic_leave();
    }
}
