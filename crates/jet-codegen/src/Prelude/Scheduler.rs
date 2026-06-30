// D-ASYNCRT1=A (c126): M:N green-thread scheduler — work-stealing pool (M1) plus
// condvar park/wake substrate (M2): channel wake-on-send, timer sleep, IO poll
// hook, pause/cancel at yield points. std-only (I6).

use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

type Job = Box<dyn FnOnce() + Send>;

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
        if let Some(t) = timeout {
            let _unused = self.cv.wait_timeout(guard, t).unwrap();
        } else {
            let _unused = self.cv.wait(guard).unwrap();
        }
        let _ = self.notified.swap(false, Ordering::Acquire);
    }

    pub fn wake(&self) {
        self.notified.store(true, Ordering::Release);
        self.cv.notify_one();
    }
}

pub struct JetTaskControl {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
    park: Arc<ParkSlot>,
}

impl JetTaskControl {
    pub fn new() -> Arc<Self> {
        Arc::new(JetTaskControl {
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            park: ParkSlot::new(),
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
pub fn jet_scheduler_yield(wait_kind: &str, slot: &ParkSlot, timeout: Option<Duration>) {
    if let Some(ctrl) = current_task_control() {
        if ctrl.cancelled.load(Ordering::Relaxed) {
            return;
        }
        ctrl.wait_while_paused();
        if ctrl.cancelled.load(Ordering::Relaxed) {
            return;
        }
    }
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded(wait_kind);
        }
        let cap = Duration::from_millis(remaining as u64);
        let wait = timeout.map(|t| t.min(cap)).unwrap_or(cap);
        slot.park(Some(wait));
        if let Some(left) = jet_deadline_remaining_ms() {
            if left <= 0 {
                jet_deadline_exceeded(wait_kind);
            }
        }
    } else {
        slot.park(timeout);
    }
    if let Some(ctrl) = current_task_control() {
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

// ── M2: IO poll substrate (non-blocking try + park/wake) ─────────────────────

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
    closed: bool,
}

struct ChannelInner<T> {
    state: Mutex<ChannelState<T>>,
}

pub struct JetSchedulerChannel<T> {
    inner: Arc<ChannelInner<T>>,
}

impl<T: Send> JetSchedulerChannel<T> {
    pub fn new() -> Self {
        JetSchedulerChannel {
            inner: Arc::new(ChannelInner {
                state: Mutex::new(ChannelState {
                    queue: VecDeque::new(),
                    recv_waiters: Vec::new(),
                    closed: false,
                }),
            }),
        }
    }

    pub fn sender(&self) -> JetSchedulerSender<T> {
        JetSchedulerSender {
            inner: self.inner.clone(),
        }
    }

    pub fn receive(&self) -> Option<T> {
        loop {
            if jet_scheduler_task_cancelled() {
                return None;
            }
            if let Some(ctrl) = current_task_control() {
                ctrl.wait_while_paused();
                if ctrl.cancelled.load(Ordering::Relaxed) {
                    return None;
                }
            }
            let slot = ParkSlot::new();
            let parked = {
                let mut st = self.inner.state.lock().unwrap();
                if let Some(v) = st.queue.pop_front() {
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
        self.inner.state.lock().unwrap().queue.pop_front()
    }

    pub fn close(&self) {
        let waiters = {
            let mut st = self.inner.state.lock().unwrap();
            st.closed = true;
            std::mem::take(&mut st.recv_waiters)
        };
        for w in waiters {
            w.wake();
        }
    }
}

pub struct JetSchedulerSender<T> {
    inner: Arc<ChannelInner<T>>,
}

impl<T> Clone for JetSchedulerSender<T> {
    fn clone(&self) -> Self {
        JetSchedulerSender {
            inner: self.inner.clone(),
        }
    }
}

impl<T: Send> JetSchedulerSender<T> {
    pub fn send(&self, value: T) {
        let wake = {
            let mut st = self.inner.state.lock().unwrap();
            st.queue.push_back(value);
            st.recv_waiters.pop()
        };
        if let Some(slot) = wake {
            jet_scheduler_wake(&slot);
        }
    }
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
}

pub struct JetSchedulerJoin<T> {
    rx: std::sync::mpsc::Receiver<JetSchedulerResult<T>>,
}

impl<T> JetSchedulerJoin<T> {
    pub fn join(self) -> T {
        match self.rx.recv() {
            Ok(JetSchedulerResult::Value(v)) => v,
            Ok(JetSchedulerResult::Panicked) | Err(_) => {
                eprintln!("panic: a task panicked");
                std::process::exit(70);
            }
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
        jet_scheduler_set_task_control(Some(control));
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        jet_scheduler_set_task_control(None);
        let _ = tx.send(match out {
            Ok(v) => JetSchedulerResult::Value(v),
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
