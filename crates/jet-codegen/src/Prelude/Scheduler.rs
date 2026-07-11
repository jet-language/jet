// D-ASYNCRT1=A (c126): M:N green-thread scheduler — work-stealing pool (M1) plus
// condvar park/wake substrate (M2): channel wake-on-send, timer sleep, IO poll
// hook, pause/cancel at yield points. std-only (I6).

use std::collections::VecDeque;
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
    streams: Mutex<Vec<Arc<Mutex<TcpStream>>>>,
    notify: Condvar,
}

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
        let id = streams.len();
        streams.push(stream);
        drop(streams);
        self.interests.lock().unwrap().push(IoInterest {
            stream_id: id,
            slot: slot.clone(),
            readable,
            writable,
        });
        self.notify.notify_one();
        (id, slot)
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

    // jet:scheduler-native-begin — stripped from user-visible prelude (I1); native IO lives in jet_codegen::scheduler only.
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
                    let Some(stream) = streams.get(interest.stream_id) else {
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
                    let Some(stream) = streams.get(interest.stream_id) else {
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
        // IOCP path: fall back to portable poll until full AcceptEx/WSA integration lands.
        // Backend name is honest (`iocp`) for metrics; readiness uses non-blocking probe.
        self.run_portable_poll();
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
                    let Some(stream) = streams.get(interest.stream_id) else {
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
                streams: Mutex::new(Vec::new()),
                notify: Condvar::new(),
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
    let (_id, slot) = io_poller().register(shared, read, write);
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
            if g.is_empty() && !pending_local {
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
}
