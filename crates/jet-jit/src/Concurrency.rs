//! M4: scheduler-backed task/channel host shims for the Cranelift JIT.

use jet_codegen::scheduler::{
    jet_ctx_deadline_ms, jet_ctx_push_deadline, jet_scheduler_all, jet_scheduler_any,
    jet_scheduler_current_task_trace, jet_scheduler_deliver_shield_exit, jet_scheduler_race,
    jet_scheduler_propagate_deadline,
    jet_scheduler_panic_should_unwind,
    jet_scheduler_select_int_channels_timed, jet_scheduler_shield_enter,
    jet_scheduler_shield_leave_status, jet_scheduler_sleep_ms,
    jet_scheduler_spawn_blocking_with_control, jet_scheduler_wait_without_unwind,
    jet_scheduler_yield_now, jet_task_join_deadline_check, JetDeadlineGuard, JetSchedulerChannel,
    JetSchedulerJoin,
    JetSchedulerWait, JetShieldExit, JetTaskControl,
};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

// Every native host call that reaches the resident runtime crosses this lock.
// Spawned Cranelift frames share the same arena as their parent, so the raw
// runtime pointer must never be dereferenced concurrently.
static RUNTIME_ACCESS: Mutex<()> = Mutex::new(());
/// Published for HTTP `std::thread` workers that are not jet-scheduler tasks.
static HTTP_SHARED_RUNTIME: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ACTIVE_RUNTIME: RefCell<Option<*mut super::JitRuntime>> = const { RefCell::new(None) };
    static RUNTIME_ACCESS_DEPTH: Cell<usize> = const { Cell::new(0) };
    static IN_JIT_TASK: Cell<bool> = const { Cell::new(false) };
    static LOCAL_RICH_PANIC: Cell<bool> = const { Cell::new(false) };
    static PENDING_SHIELD_EXIT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static PENDING_CHILD_DEADLINE: RefCell<Option<String>> = const { RefCell::new(None) };
    static PENDING_TASK_GROUP_PERMIT: RefCell<Option<JitTaskGroupPermit>> = const { RefCell::new(None) };
    static RICH_PANIC_REASON: RefCell<Option<String>> = const { RefCell::new(None) };
    static WAIT_VALUE: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

pub(crate) struct JitTaskScope {
    previous_task: bool,
    previous_rich_panic: bool,
}

impl Drop for JitTaskScope {
    fn drop(&mut self) {
        IN_JIT_TASK.with(|task| task.set(self.previous_task));
        LOCAL_RICH_PANIC.with(|panic| panic.set(self.previous_rich_panic));
    }
}

pub(crate) fn enter_jit_task() -> JitTaskScope {
    let previous_task = IN_JIT_TASK.with(|task| {
        let previous = task.get();
        task.set(true);
        previous
    });
    let previous_rich_panic = LOCAL_RICH_PANIC.with(|panic| {
        let previous = panic.get();
        panic.set(false);
        previous
    });
    JitTaskScope {
        previous_task,
        previous_rich_panic,
    }
}

pub(crate) fn in_jit_task() -> bool {
    IN_JIT_TASK.with(Cell::get)
}

pub(crate) fn set_local_rich_panic() {
    LOCAL_RICH_PANIC.with(|panic| panic.set(true));
}

pub(crate) fn local_rich_panic_pending() -> bool {
    LOCAL_RICH_PANIC.with(Cell::get)
}

pub(crate) struct JitTaskGroupSlots {
    limit: usize,
    active: Mutex<usize>,
    closing: AtomicBool,
    wake: Condvar,
}

pub(crate) struct JitTaskGroupPermit {
    slots: Arc<JitTaskGroupSlots>,
}

impl JitTaskGroupSlots {
    fn close(&self) {
        let _active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.closing.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    fn acquire(self: &Arc<Self>) -> Option<JitTaskGroupPermit> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while *active >= self.limit && !self.closing.load(Ordering::Acquire) {
            active = self
                .wake
                .wait(active)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if self.closing.load(Ordering::Acquire) {
            return None;
        }
        *active += 1;
        Some(JitTaskGroupPermit { slots: self.clone() })
    }
}

impl Drop for JitTaskGroupPermit {
    fn drop(&mut self) {
        let mut active = self
            .slots
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *active = active.saturating_sub(1);
        self.slots.wake.notify_one();
    }
}

pub(crate) struct JitTaskGroup {
    pub(crate) children: Arc<jet_codegen::task_group::JetTaskGroupRuntime<i64>>,
    pub(crate) slots: Option<Arc<JitTaskGroupSlots>>,
    closing: AtomicBool,
}

impl JitTaskGroup {
    fn new(limit: i64, bounded: bool) -> Self {
        Self {
            children: Arc::new(jet_codegen::task_group::JetTaskGroupRuntime::new()),
            slots: bounded.then(|| {
                Arc::new(JitTaskGroupSlots {
                    limit: jet_codegen::task_group::jet_task_group_limit(limit),
                    active: Mutex::new(0),
                    closing: AtomicBool::new(false),
                    wake: Condvar::new(),
                })
            }),
            closing: AtomicBool::new(false),
        }
    }
}

struct RuntimeAccessGuard {
    _lock: Option<MutexGuard<'static, ()>>,
}

impl RuntimeAccessGuard {
    fn enter() -> Self {
        let lock = RUNTIME_ACCESS_DEPTH.with(|depth| {
            let current = depth.get();
            depth.set(current + 1);
            (current == 0).then(|| {
                RUNTIME_ACCESS
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
            })
        });
        Self { _lock: lock }
    }
}

impl Drop for RuntimeAccessGuard {
    fn drop(&mut self) {
        RUNTIME_ACCESS_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

#[repr(i64)]
enum JitWaitStatus {
    Ready = 0,
    Interrupted = 1,
    Panicked = 2,
}

fn wait_status<F>(f: F) -> i64
where
    F: FnOnce() -> i64,
{
    match jet_scheduler_wait_without_unwind(f) {
        JetSchedulerWait::Ready(value) => {
            WAIT_VALUE.with(|slot| slot.set(value));
            JitWaitStatus::Ready as i64
        }
        JetSchedulerWait::Cancelled => {
            set_pending_shield_exit(JetShieldExit::Cancelled);
            JitWaitStatus::Interrupted as i64
        }
        JetSchedulerWait::Deadline(rendered) => {
            if jet_scheduler_panic_should_unwind() {
                PENDING_CHILD_DEADLINE.with(|pending| {
                    *pending.borrow_mut() = Some(rendered);
                });
                JitWaitStatus::Interrupted as i64
            } else {
                with_runtime_mut(|rt| rt.set_deadline(rendered));
                JitWaitStatus::Interrupted as i64
            }
        }
        JetSchedulerWait::Panicked(message) => {
            with_runtime_mut(|rt| {
                let line = format!("panic: {message}\n");
                if !rt.stderr.ends_with(&line) {
                    rt.stderr.push_str(&line);
                }
            });
            trap_panic(&message);
            JitWaitStatus::Panicked as i64
        }
    }
}

/// Marshal the shared Prelude `Result<T, TaskFailure>` rail through the
/// resident JIT's one-i64 result carrier. The scheduler only interrupts for a
/// parent wait failure; a child `TaskFailure` remains an ordinary `Err` value.
fn wait_task_result<T, F, Encode>(f: F, encode: Encode) -> i64
where
    F: FnOnce() -> Result<T, jet_codegen::task_group::JetTaskFailure>,
    Encode: FnOnce(&mut super::JitRuntime, T) -> u64,
{
    match jet_scheduler_wait_without_unwind(f) {
        JetSchedulerWait::Ready(Ok(value)) => {
            let result = with_runtime_mut(|rt| {
                let payload = encode(rt, value);
                crate::runtime_host::alloc_jit_result(rt, true, payload)
            });
            WAIT_VALUE.with(|slot| slot.set(result));
            JitWaitStatus::Ready as i64
        }
        JetSchedulerWait::Ready(Err(failure)) => {
            let result = with_runtime_mut(|rt| {
                let payload = jet_codegen::task_group::jet_task_failure_abi(
                    failure,
                    |reason| rt.heap.alloc_string(reason) as u64,
                );
                crate::runtime_host::alloc_jit_result(rt, false, payload)
            });
            WAIT_VALUE.with(|slot| slot.set(result));
            JitWaitStatus::Ready as i64
        }
        JetSchedulerWait::Cancelled => {
            set_pending_shield_exit(JetShieldExit::Cancelled);
            JitWaitStatus::Interrupted as i64
        }
        JetSchedulerWait::Deadline(rendered) => {
            if jet_scheduler_panic_should_unwind() {
                PENDING_CHILD_DEADLINE.with(|pending| {
                    *pending.borrow_mut() = Some(rendered);
                });
                JitWaitStatus::Interrupted as i64
            } else {
                with_runtime_mut(|rt| rt.set_deadline(rendered));
                JitWaitStatus::Interrupted as i64
            }
        }
        JetSchedulerWait::Panicked(message) => {
            with_runtime_mut(|rt| {
                let line = format!("panic: {message}\n");
                if !rt.stderr.ends_with(&line) {
                    rt.stderr.push_str(&line);
                }
            });
            trap_panic(&message);
            JitWaitStatus::Panicked as i64
        }
    }
}

extern "C" fn jet_jit_wait_value() -> i64 {
    WAIT_VALUE.with(|slot| slot.get())
}

fn set_pending_shield_exit(exit: JetShieldExit) {
    let code = match exit {
        JetShieldExit::None => 0,
        JetShieldExit::Deadline => 2,
        JetShieldExit::Cancelled => 1,
    };
    PENDING_SHIELD_EXIT.with(|pending| pending.set(pending.get().max(code)));
}

fn take_pending_shield_exit() -> JetShieldExit {
    PENDING_SHIELD_EXIT.with(|pending| match pending.replace(0) {
        2 => JetShieldExit::Deadline,
        1 => JetShieldExit::Cancelled,
        _ => JetShieldExit::None,
    })
}

fn take_pending_child_deadline() -> Option<String> {
    PENDING_CHILD_DEADLINE.with(|pending| pending.borrow_mut().take())
}

fn take_pending_task_group_permit() -> Option<JitTaskGroupPermit> {
    PENDING_TASK_GROUP_PERMIT.with(|pending| pending.borrow_mut().take())
}

pub(crate) fn set_rich_panic_reason(reason: String) {
    RICH_PANIC_REASON.with(|slot| *slot.borrow_mut() = Some(reason));
}

fn take_rich_panic_reason() -> Option<String> {
    RICH_PANIC_REASON.with(|slot| slot.borrow_mut().take())
}

extern "C" fn jet_jit_pending_exit_status() -> i64 {
    let pending = PENDING_SHIELD_EXIT.with(|slot| slot.get() != 0);
    i64::from(pending || with_runtime_mut(|rt| rt.deadline_exceeded.is_some()))
}

/// Complete a control transfer after the top-level Cranelift frame returned.
/// Spawned frames use their Rust task wrapper; resident `run` uses this hook.
pub(crate) fn settle_pending_after_native() {
    match take_pending_shield_exit() {
        JetShieldExit::None => {}
        JetShieldExit::Cancelled => {
            with_runtime_mut(|rt| rt.set_trap("a task was cancelled"));
        }
        JetShieldExit::Deadline => {
            // Deadline text was recorded by the status/shield host before the
            // native early return. Resident reporting owns E3003.
        }
    }
}

pub(crate) fn with_runtime_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut super::JitRuntime) -> R,
    R: Default,
{
    let mut out = R::default();
    ACTIVE_RUNTIME.with(|slot| {
        if let Some(ptr) = *slot.borrow() {
            let _guard = RuntimeAccessGuard::enter();
            // SAFETY: set only for the duration of resident_invoke on this thread.
            unsafe {
                if let Some(rt) = ptr.as_mut() {
                    out = f(rt);
                }
            }
        }
    });
    out
}

pub(crate) fn set_active_runtime(ptr: Option<*mut super::JitRuntime>) {
    ACTIVE_RUNTIME.with(|slot| *slot.borrow_mut() = ptr);
    // Publish on install only. Clearing TLS (spawn worker epilogue / post-drain)
    // must not drop the shared pointer while HTTP OS threads still serve.
    if let Some(p) = ptr {
        HTTP_SHARED_RUNTIME.store(p as usize, Ordering::Release);
    }
}

pub(crate) fn clear_http_shared_runtime() {
    HTTP_SHARED_RUNTIME.store(0, Ordering::Release);
}

/// Pin the resident JIT heap onto the current thread for the duration of `f`.
/// HTTP `Server.serve` workers are raw OS threads; without this, host calls that
/// touch `with_runtime_mut` silently return `Default` (often `0`).
pub(crate) fn with_http_jet_runtime<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let had = active_runtime_ptr().is_some();
    if !had {
        let addr = HTTP_SHARED_RUNTIME.load(Ordering::Acquire);
        if addr != 0 {
            ACTIVE_RUNTIME.with(|slot| {
                *slot.borrow_mut() = Some(addr as *mut super::JitRuntime);
            });
        }
    }
    let out = f();
    if !had {
        ACTIVE_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    }
    out
}

/// Record a panic trap. Returns normally (caller yields a dummy value); JIT
/// code branches to its epilogue at the next `emit_trap_check` (I1 — no Rust
/// panic ever unwinds through a JIT frame).
fn trap_panic(msg: &str) -> i64 {
    with_runtime_mut(|rt| rt.set_trap(msg));
    0
}

extern "C" fn jet_jit_channel_new() -> i64 {
    with_runtime_mut(|rt| {
        let id = rt.channels.len() as i64;
        rt.channels.push(JetSchedulerChannel::new());
        id
    })
}

/// `tasks.channel<T>(capacity)` — bounded buffer (D-TASKRUNTIME1).
extern "C" fn jet_jit_channel_bounded(capacity: i64) -> i64 {
    with_runtime_mut(|rt| {
        let id = rt.channels.len() as i64;
        rt.channels
            .push(JetSchedulerChannel::bounded(capacity.max(0) as usize));
        id
    })
}

extern "C" fn jet_jit_generator_channel_new() -> i64 {
    with_runtime_mut(|rt| {
        let (producer, consumer) = jet_codegen::scheduler::jet_stream::<i64>();
        let channel = rt.next_stream_channel;
        rt.next_stream_channel -= 1;
        rt.stream_consumers.insert(channel, consumer);
        rt.stream_producers
            .insert(channel, Arc::new(producer));
        channel
    })
}

/// `core.time.now()` — wall millis (honours `LEX_TEST_EPOCH`).
extern "C" fn jet_jit_time_now() -> i64 {
    if let Ok(s) = std::env::var("JET_PROVE_REPLAY_TIME_MS") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    if let Ok(s) = std::env::var("LEX_TEST_EPOCH") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// `#Context(deadline: …)` enter.
extern "C" fn jet_jit_deadline_push(deadline_ms: i64) {
    DEADLINE_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push(jet_ctx_push_deadline(deadline_ms));
    });
}

extern "C" fn jet_jit_deadline_pop() {
    DEADLINE_STACK.with(|stack| {
        let _ = stack.borrow_mut().pop();
    });
}

thread_local! {
    static DEADLINE_STACK: RefCell<Vec<JetDeadlineGuard>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn jet_jit_channel_close(ch: i64) {
    if ch < 0 {
        let (consumer, pending_producer) = with_runtime_mut(|rt| {
            (
                rt.stream_consumers.remove(&ch),
                rt.stream_producers.remove(&ch),
            )
        });
        // Drop the producer first when the wrapper has not claimed it yet, so
        // the canonical consumer Drop can receive completion immediately.
        drop(pending_producer);
        drop(consumer);
        return;
    }
    with_runtime_mut(|rt| {
        if let Some(channel) = rt.channels.get(ch as usize) {
            channel.close();
        }
    });
}

extern "C" fn jet_jit_channel_sender(ch: i64) -> i64 {
    with_runtime_mut(|rt| {
        if let Some(producer) = rt.stream_producers.remove(&ch) {
            let id = rt.next_stream_sender;
            rt.next_stream_sender -= 1;
            rt.stream_senders.insert(id, producer);
            return id;
        }
        let channel = rt
            .channels
            .get(ch as usize)
            .expect("jit channel sender: bad handle");
        let id = rt.senders.len() as i64;
        rt.senders.push(Some(channel.sender()));
        id
    })
}

extern "C" fn jet_jit_sender_clone(s: i64) -> i64 {
    with_runtime_mut(|rt| {
        if let Some(sender) = rt.stream_senders.get(&s).cloned() {
            let id = rt.next_stream_sender;
            rt.next_stream_sender -= 1;
            rt.stream_senders.insert(id, sender);
            return id;
        }
        let tx = rt
            .senders
            .get(s as usize)
            .and_then(Option::as_ref)
            .expect("jit sender clone: bad handle")
            .clone();
        let id = rt.senders.len() as i64;
        rt.senders.push(Some(tx));
        id
    })
}

extern "C" fn jet_jit_sender_send(s: i64, v: i64) -> i64 {
    if s < 0 {
        let sender = with_runtime_mut(|rt| rt.stream_senders.get(&s).cloned())
            .expect("jit stream sender send without active runtime");
        return wait_status(|| i64::from(sender.send_stream(v)));
    }
    let tx = with_runtime_mut(|rt| {
        Some(
            rt.senders
            .get(s as usize)
            .and_then(Option::as_ref)
            .expect("jit sender send: bad handle")
            .clone(),
        )
    })
    .expect("jit sender send without active runtime");
    wait_status(|| i64::from(tx.send(v)))
}

extern "C" fn jet_jit_sender_close(s: i64, failed: i64) {
    if s < 0 {
        let sender = with_runtime_mut(|rt| rt.stream_senders.remove(&s));
        if failed != 0 {
            if let Some(sender) = sender.as_ref() {
                sender.fail();
            }
        }
        drop(sender);
        return;
    }
    with_runtime_mut(|rt| {
        if let Some(sender) = rt.senders.get_mut(s as usize) {
            *sender = None;
        }
    });
}

/// `0` = closed; otherwise `received + 1`. A Stream pull acknowledges the
/// preceding value before waiting for the next one. That acknowledgement is
/// the exact suspension boundary after `yield`.
extern "C" fn jet_jit_generator_channel_receive_status(ch: i64) -> i64 {
    let mut consumer = with_runtime_mut(|rt| rt.stream_consumers.remove(&ch))
        .expect("jit generator receive without active runtime");
    let mut producer_failed = false;
    let status = wait_status(|| match consumer.pull() {
        Some(value) => value + 1,
        None => {
            producer_failed = consumer.failed();
            0
        }
    });
    if status == JitWaitStatus::Ready as i64 && producer_failed {
        // Match AOT `JetStreamIter::next`: a producer that completed with a
        // failure must not become ordinary EOF in the resident adapter. The
        // lowering checks this trap before dispatching the EOF branch.
        trap_panic("stream producer failed");
    }
    if status == JitWaitStatus::Ready as i64 {
        if WAIT_VALUE.with(|slot| slot.get()) == 0 {
            drop(consumer);
        } else {
            with_runtime_mut(|rt| {
                rt.stream_consumers.insert(ch, consumer);
            });
        }
    } else {
        with_runtime_mut(|rt| {
            rt.stream_consumers.insert(ch, consumer);
        });
    }
    status
}

/// Blocks until a message arrives or the channel closes — matches AOT
/// `Channel.receive()` + `??` on `Result` (not `try_receive`).
/// parity: guard tests/dev.rs::scheduler_spawn_runs_via_jit
extern "C" fn jet_jit_channel_receive_status(ch: i64) -> i64 {
    let chan = with_runtime_mut(|rt| {
        Some(
            rt
            .channels
            .get(ch as usize)
            .expect("jit channel receive: bad handle")
            .clone(),
        )
    })
    .expect("jit channel receive without active runtime");
    wait_status(|| {
        match chan.receive() {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

extern "C" fn jet_jit_channel_receive(ch: i64, _line: u32) -> i64 {
    let chan = with_runtime_mut(|rt| {
        Some(
            rt
            .channels
            .get(ch as usize)
            .expect("jit channel receive: bad handle")
            .clone(),
        )
    })
    .expect("jit channel receive without active runtime");
    wait_status(|| {
        match chan.receive() {
            Some(v) => v,
            None => {
                with_runtime_mut(|rt| rt.set_trap("channel closed"));
                0
            }
        }
    })
}

extern "C" fn jet_jit_panic_channel_closed(_line: u32) -> i64 {
    trap_panic("channel closed")
}

type SpawnFn0 = extern "C" fn() -> i64;
type SpawnFn1 = extern "C" fn(i64) -> i64;
type SpawnFn2 = extern "C" fn(i64, i64) -> i64;
type SpawnFn3 = extern "C" fn(i64, i64, i64) -> i64;
type SpawnFn4 = extern "C" fn(i64, i64, i64, i64) -> i64;

fn store_task(join: JetSchedulerJoin<i64>, control: Arc<JetTaskControl>) -> i64 {
    with_runtime_mut(|rt| {
        let id = rt.tasks.len() as i64;
        rt.tasks.push(Some(join));
        rt.task_controls.push(control);
        id
    })
}

fn task_ids_from_list(rt: &mut super::JitRuntime, list: i64) -> Vec<i64> {
    rt.heap
        .clone_int_list(list)
        .expect("jit task combinator: bad list handle")
}

fn store_i64_list(rt: &mut super::JitRuntime, values: Vec<i64>) -> i64 {
    rt.heap.alloc_int_list(values)
}

fn take_task_entries(
    rt: &mut super::JitRuntime,
    ids: &[i64],
) -> Vec<(JetSchedulerJoin<i64>, Arc<JetTaskControl>)> {
    ids.iter()
        .map(|&id| {
            let idx = id as usize;
            let join = rt.tasks[idx]
                .take()
                .expect("jit task combinator: task already joined");
            let control = rt.task_controls[idx].clone();
            (join, control)
        })
        .collect()
}

pub(crate) fn active_runtime_ptr() -> Option<*mut super::JitRuntime> {
    ACTIVE_RUNTIME.with(|slot| *slot.borrow())
}

/// Run JIT spawn body on a pool worker with the spawner's runtime heap wired up.
fn spawn_with_runtime<F>(f: F) -> i64
where
    F: FnOnce() -> i64 + Send + 'static,
{
    let rt_ptr = active_runtime_ptr().expect("jit spawn without active runtime");
    let rt_addr = rt_ptr as usize;
    let inherited_deadline = jet_ctx_deadline_ms();
    let control = JetTaskControl::new();
    let permit = take_pending_task_group_permit();
    let join = jet_scheduler_spawn_blocking_with_control(
        move || {
            let _task_scope = enter_jit_task();
            let _permit = permit;
            // SAFETY: `rt_ptr` is the resident heap for this JIT invocation; workers
            // only touch mutex-backed channel state and indexed sender slots.
            let rt_ptr = rt_addr as *mut super::JitRuntime;
            set_active_runtime(Some(rt_ptr));
            let _deadline = inherited_deadline.map(jet_ctx_push_deadline);
            let _ = take_pending_shield_exit();
            let out = f();
            let child_deadline = take_pending_child_deadline();
            // A rich panic is a typed child failure under D-CONC-FAIL1. Remove
            // its top-level diagnostic state before re-raising, or the shared
            // resident runtime would report the child as a parent panic too.
            let panic_reason = take_rich_panic_reason();
            let rich = panic_reason.is_some();
            set_active_runtime(None);
            jet_scheduler_deliver_shield_exit(take_pending_shield_exit());
            if let Some(rendered) = child_deadline {
                jet_scheduler_propagate_deadline(rendered);
            }
            if rich {
                panic!("{}", panic_reason.unwrap_or_else(|| "a task panicked".to_string()));
            }
            out
        },
        control.clone(),
    );
    store_task(join, control)
}

extern "C" fn jet_jit_shield_enter() {
    jet_scheduler_shield_enter();
}

extern "C" fn jet_jit_shield_leave() -> i64 {
    let exit = jet_scheduler_shield_leave_status();
    if matches!(exit, JetShieldExit::Deadline) {
        with_runtime_mut(|rt| {
            rt.set_deadline(jet_codegen::task_group::jet_task_deadline("shield exit").render())
        });
    }
    set_pending_shield_exit(exit);
    i64::from(!matches!(exit, JetShieldExit::None))
}

extern "C" fn jet_jit_spawn0(f: SpawnFn0) -> i64 {
    spawn_with_runtime(move || f())
}

extern "C" fn jet_jit_spawn1(f: SpawnFn1, c0: i64) -> i64 {
    spawn_with_runtime(move || f(c0))
}

extern "C" fn jet_jit_spawn2(f: SpawnFn2, c0: i64, c1: i64) -> i64 {
    spawn_with_runtime(move || f(c0, c1))
}

extern "C" fn jet_jit_spawn3(f: SpawnFn3, c0: i64, c1: i64, c2: i64) -> i64 {
    spawn_with_runtime(move || f(c0, c1, c2))
}

extern "C" fn jet_jit_spawn4(f: SpawnFn4, c0: i64, c1: i64, c2: i64, c3: i64) -> i64 {
    spawn_with_runtime(move || f(c0, c1, c2, c3))
}

extern "C" fn jet_jit_task_group_new(limit: i64, bounded: i64) -> i64 {
    with_runtime_mut(|rt| {
        let id = rt.task_groups.len() as i64;
        rt.task_groups
            .push(Some(JitTaskGroup::new(limit, bounded != 0)));
        id
    })
}

extern "C" fn jet_jit_task_group_acquire(group: i64) {
    let slots = with_runtime_mut(|rt| {
        rt.task_groups
            .get(group as usize)
            .and_then(Option::as_ref)
            .and_then(|group| group.slots.clone())
    });
    if let Some(slots) = slots {
        PENDING_TASK_GROUP_PERMIT.with(|pending| {
            *pending.borrow_mut() = slots.acquire();
        });
    }
}

extern "C" fn jet_jit_task_group_register(group: i64, task: i64) {
    with_runtime_mut(|rt| {
        rt.task_groups[group as usize]
            .as_ref()
            .expect("jit task group already closed")
            .children
            .register(task);
    });
}

fn close_task_group(group: i64) -> i64 {
    let children = with_runtime_mut(|rt| {
        let group = rt.task_groups[group as usize].as_ref();
        group.and_then(|group| {
            if group.closing.swap(true, Ordering::AcqRel) {
                return None;
            }
            if let Some(slots) = &group.slots {
                // A child that is already being drained may still reach a
                // nested spawn. Let that spawn proceed without a permit so
                // lexical close can register and drain it instead of waiting
                // for the permit held by the child being drained.
                slots.close();
            }
            Some(group.children.clone())
        })
    });
    let Some(children) = children else {
        return JitWaitStatus::Ready as i64;
    };
    // Keep group slot live through shared drain: joined children may register.
    children.close_with(
        |task| {
            with_runtime_mut(|rt| {
                let idx = *task as usize;
                if rt.tasks.get(idx).is_some_and(Option::is_some) {
                    rt.task_controls[idx].cancel();
                }
            });
        },
        |task| {
            let join = with_runtime_mut(|rt| rt.tasks[task as usize].take());
            if let Some(join) = join {
                // A lexical group's close has no result surface. It still
                // waits every child, while D-CONC-FAIL1 keeps child failure
                // from becoming a process-level panic.
                // Drain directly: `join()` is a parent wait point and may
                // unwind when the parent is already cancelled. Lexical close
                // must still consume the child completion before dropping the
                // group's runtime slot.
                join.drain();
            }
        },
    );
    with_runtime_mut(|rt| {
        let _ = rt.task_groups[group as usize].take();
    });
    JitWaitStatus::Ready as i64
}

extern "C" fn jet_jit_task_group_close(group: i64) -> i64 {
    close_task_group(group)
}

pub(crate) fn close_active_task_groups() {
    let len = with_runtime_mut(|rt| rt.task_groups.len());
    for group in (0..len).rev() {
        let _ = close_task_group(group as i64);
    }
}

extern "C" fn jet_jit_task_cancel(task: i64) {
    with_runtime_mut(|rt| {
        rt.task_controls[task as usize].cancel();
    });
}

extern "C" fn jet_jit_task_detach(task: i64) {
    // D-DETACH1: drop the join handle; task keeps running.
    with_runtime_mut(|rt| {
        let _ = rt.tasks[task as usize].take();
    });
}

extern "C" fn jet_jit_task_pause(task: i64) {
    with_runtime_mut(|rt| {
        rt.task_controls[task as usize].pause();
    });
}

extern "C" fn jet_jit_task_resume(task: i64) {
    with_runtime_mut(|rt| {
        rt.task_controls[task as usize].resume();
    });
}

extern "C" fn jet_jit_task_yield() {
    jet_scheduler_yield_now();
}

extern "C" fn jet_jit_task_current_trace() -> i64 {
    with_runtime_mut(|rt| {
        let text = jet_scheduler_current_task_trace();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_task_join(task: i64) -> i64 {
    // `emit_async(…).join()` is typed as TaskJoin in TIR, but the thin async
    // event host returns a completed DispatchReport handle (1-based index into
    // `dispatch_reports`), not a scheduler task. Treat missing/already-joined
    // slots as identity so the report handle passes through.
    let join = with_runtime_mut(|rt| {
        let idx = task as usize;
        if idx >= rt.tasks.len() {
            return None;
        }
        rt.tasks[idx].take()
    });
    match join {
        Some(j) => wait_task_result(
            || {
                jet_task_join_deadline_check();
                let result = j.join();
                jet_task_join_deadline_check();
                result
            },
            |_rt, value| value as u64,
        ),
        None => {
            WAIT_VALUE.with(|slot| slot.set(task));
            JitWaitStatus::Ready as i64
        }
    }
}

/// D-CONC-SPAWN1=D / D-CONC-FAIL1=A: `task.all { … }` returns a Result
/// handle; the list remains the successful payload and TaskFailure is packed
/// only at this ABI boundary.
extern "C" fn jet_jit_task_all(task_list: i64) -> i64 {
    let entries = with_runtime_mut(|rt| {
        let ids = task_ids_from_list(rt, task_list);
        take_task_entries(rt, &ids)
    });
    wait_task_result(
        || jet_scheduler_all(entries),
        |rt, values| store_i64_list(rt, values) as u64,
    )
}

/// D-CONC-SPAWN1=D / D-CONC-FAIL1=A: first successful result or TaskFailure.
extern "C" fn jet_jit_task_race(task_list: i64) -> i64 {
    let entries = with_runtime_mut(|rt| {
        let ids = task_ids_from_list(rt, task_list);
        take_task_entries(rt, &ids)
    });
    wait_task_result(|| jet_scheduler_race(entries), |_rt, value| value as u64)
}

/// D-CONC-SPAWN1=D / D-CONC-FAIL1=A: first completed result or TaskFailure.
extern "C" fn jet_jit_task_any(task_list: i64) -> i64 {
    let entries = with_runtime_mut(|rt| {
        let ids = task_ids_from_list(rt, task_list);
        take_task_entries(rt, &ids)
    });
    wait_task_result(|| jet_scheduler_any(entries), |_rt, value| value as u64)
}

/// D-CONCSELECT1=A: `g.select().recv(…).after(ms[, v]).wait()`.
/// `after_list` is flat `[ms0, val0, ms1, val1, …]` (even length). Empty → no timers.
extern "C" fn jet_jit_select_wait(recv_list: i64, after_list: i64) -> i64 {
    let (channels, timers) = with_runtime_mut(|rt| {
        let ch_ids = task_ids_from_list(rt, recv_list);
        let after_flat = task_ids_from_list(rt, after_list);
        let channels: Vec<JetSchedulerChannel<i64>> = ch_ids
            .iter()
            .map(|&id| {
                rt.channels
                    .get(id as usize)
                    .expect("jit select: bad channel handle")
                    .clone()
            })
            .collect();
        let mut timers = Vec::new();
        let mut i = 0;
        while i + 1 < after_flat.len() {
            timers.push((after_flat[i].max(0) as u64, after_flat[i + 1]));
            i += 2;
        }
        // Legacy: odd trailing ms-only entries (no value) → value 0.
        if i < after_flat.len() {
            timers.push((after_flat[i].max(0) as u64, 0));
        }
        (channels, timers)
    });
    wait_status(|| jet_scheduler_select_int_channels_timed(&channels, timers))
}

/// `tasks.after(ms, value)` — one-shot timer channel that receives `value`.
extern "C" fn jet_jit_after_value(ms: i64, value: i64) -> i64 {
    // Sender is stashed in `rt.senders` so `with_runtime_mut` stays `Default`-safe.
    let (ch_id, sender_id) = with_runtime_mut(|rt| {
        let id = rt.channels.len() as i64;
        let ch = JetSchedulerChannel::new();
        let tx = ch.sender();
        rt.channels.push(ch);
        let sid = rt.senders.len() as i64;
        rt.senders.push(Some(tx));
        (id, sid)
    });
    let tx = with_runtime_mut(|rt| {
        Some(
            rt.senders
                .get(sender_id as usize)
                .and_then(Option::as_ref)
                .expect("jit after_value: missing sender")
                .clone(),
        )
    })
    .expect("jit after_value without active runtime");
    let delay = ms.max(0) as u64;
    let inherited_deadline = jet_ctx_deadline_ms();
    let control = JetTaskControl::new();
    let _join = jet_scheduler_spawn_blocking_with_control(
        move || {
            let _deadline = inherited_deadline.map(jet_ctx_push_deadline);
            let _ = wait_status(|| {
                jet_scheduler_sleep_ms(delay);
                0
            });
            let _ = tx.send(value);
            // Drop tx → close send side after one shot.
        },
        control,
    );
    // Fire-and-forget join handle (D-DETACH1 shape for timer tasks).
    ch_id
}

/// `tasks.interval(ms)` — ticking channel sending 1, 2, …
extern "C" fn jet_jit_interval(ms: i64) -> i64 {
    let (ch_id, sender_id) = with_runtime_mut(|rt| {
        let id = rt.channels.len() as i64;
        let ch = JetSchedulerChannel::new();
        let tx = ch.sender();
        rt.channels.push(ch);
        let sid = rt.senders.len() as i64;
        rt.senders.push(Some(tx));
        (id, sid)
    });
    let tx = with_runtime_mut(|rt| {
        Some(
            rt.senders
                .get(sender_id as usize)
                .and_then(Option::as_ref)
                .expect("jit interval: missing sender")
                .clone(),
        )
    })
    .expect("jit interval without active runtime");
    let delay = ms.max(1) as u64;
    // Detached ticker thread — matches prelude `interval` (std::thread::spawn).
    std::thread::spawn(move || {
        let mut tick = 1i64;
        loop {
            jet_scheduler_sleep_ms(delay);
            if !tx.send(tick) {
                break;
            }
            tick += 1;
        }
    });
    ch_id
}

extern "C" fn jet_jit_sleep(millis: i64) -> i64 {
    wait_status(|| {
        jet_scheduler_sleep_ms(millis.max(0) as u64);
        0
    })
}

host_fns! {
    struct ConcurrencyHostFns;
    register: register_concurrency_symbols;
    declare: declare_concurrency_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_channel_new = Signature::new(cc);
        sig_channel_new.returns.push(AbiParam::new(types::I64));
        let mut sig_i64 = Signature::new(cc);
        sig_i64.params.push(AbiParam::new(types::I64));
        sig_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64 = sig_i64.clone();
        sig_i64_i64.params.push(AbiParam::new(types::I64));
        let mut sig_recv = sig_i64.clone();
        sig_recv.params.push(AbiParam::new(types::I32));
        let mut sig_panic_line = Signature::new(cc);
        sig_panic_line.params.push(AbiParam::new(types::I32));
        sig_panic_line.returns.push(AbiParam::new(types::I64));
        let mut sig_send = Signature::new(cc);
        sig_send.params.push(AbiParam::new(types::I64));
        sig_send.returns.push(AbiParam::new(types::I64));
        sig_send.params.push(AbiParam::new(types::I64));
        let mut sig_spawn0 = Signature::new(cc);
        sig_spawn0.params.push(AbiParam::new(types::I64));
        sig_spawn0.returns.push(AbiParam::new(types::I64));
        let mut sig_void_i64 = Signature::new(cc);
        sig_void_i64.params.push(AbiParam::new(types::I64));
        let mut sig_void_i64_i64 = sig_void_i64.clone();
        sig_void_i64_i64.params.push(AbiParam::new(types::I64));
        let sig_void = Signature::new(cc);
        let mut sig_noarg_i64 = Signature::new(cc);
        sig_noarg_i64.returns.push(AbiParam::new(types::I64));

        let mut sig_spawn1 = sig_spawn0.clone();
        sig_spawn1.params.push(AbiParam::new(types::I64));
        let mut sig_spawn2 = sig_spawn1.clone();
        sig_spawn2.params.push(AbiParam::new(types::I64));
        let mut sig_spawn3 = sig_spawn2.clone();
        sig_spawn3.params.push(AbiParam::new(types::I64));
        let mut sig_spawn4 = sig_spawn3.clone();
        sig_spawn4.params.push(AbiParam::new(types::I64));

    }
    channel_new: "jet_jit_channel_new" => jet_jit_channel_new: sig_channel_new;
    channel_bounded: "jet_jit_channel_bounded" => jet_jit_channel_bounded: sig_i64;
    generator_channel_new: "jet_jit_generator_channel_new" => jet_jit_generator_channel_new: sig_channel_new;
    channel_close: "jet_jit_channel_close" => jet_jit_channel_close: sig_void_i64;
    channel_sender: "jet_jit_channel_sender" => jet_jit_channel_sender: sig_i64;
    sender_clone: "jet_jit_sender_clone" => jet_jit_sender_clone: sig_i64;
    sender_send: "jet_jit_sender_send" => jet_jit_sender_send: sig_send;
    sender_close: "jet_jit_sender_close" => jet_jit_sender_close: sig_void_i64_i64;
    generator_receive_status: "jet_jit_generator_channel_receive_status" => jet_jit_generator_channel_receive_status: sig_i64;
    channel_receive: "jet_jit_channel_receive" => jet_jit_channel_receive: sig_recv;
    channel_receive_status: "jet_jit_channel_receive_status" => jet_jit_channel_receive_status: sig_i64;
    panic_channel_closed: "jet_jit_panic_channel_closed" => jet_jit_panic_channel_closed: sig_panic_line;
    spawn0: "jet_jit_spawn0" => jet_jit_spawn0: sig_spawn0;
    spawn1: "jet_jit_spawn1" => jet_jit_spawn1: sig_spawn1;
    spawn2: "jet_jit_spawn2" => jet_jit_spawn2: sig_spawn2;
    spawn3: "jet_jit_spawn3" => jet_jit_spawn3: sig_spawn3;
    spawn4: "jet_jit_spawn4" => jet_jit_spawn4: sig_spawn4;
    task_group_new: "jet_jit_task_group_new" => jet_jit_task_group_new: sig_i64_i64;
    task_group_acquire: "jet_jit_task_group_acquire" => jet_jit_task_group_acquire: sig_void_i64;
    task_group_register: "jet_jit_task_group_register" => jet_jit_task_group_register: sig_void_i64_i64;
    task_group_close: "jet_jit_task_group_close" => jet_jit_task_group_close: sig_i64;
    task_join: "jet_jit_task_join" => jet_jit_task_join: sig_i64;
    task_cancel: "jet_jit_task_cancel" => jet_jit_task_cancel: sig_void_i64;
    task_detach: "jet_jit_task_detach" => jet_jit_task_detach: sig_void_i64;
    task_pause: "jet_jit_task_pause" => jet_jit_task_pause: sig_void_i64;
    task_resume: "jet_jit_task_resume" => jet_jit_task_resume: sig_void_i64;
    task_yield: "jet_jit_task_yield" => jet_jit_task_yield: sig_void;
    task_current_trace: "jet_jit_task_current_trace" => jet_jit_task_current_trace: sig_noarg_i64;
    task_all: "jet_jit_task_all" => jet_jit_task_all: sig_i64;
    task_race: "jet_jit_task_race" => jet_jit_task_race: sig_i64;
    task_any: "jet_jit_task_any" => jet_jit_task_any: sig_i64;
    select_wait: "jet_jit_select_wait" => jet_jit_select_wait: sig_i64_i64;
    after_value: "jet_jit_after_value" => jet_jit_after_value: sig_i64_i64;
    interval: "jet_jit_interval" => jet_jit_interval: sig_i64;
    shield_enter: "jet_jit_shield_enter" => jet_jit_shield_enter: sig_void;
    shield_leave: "jet_jit_shield_leave" => jet_jit_shield_leave: sig_noarg_i64;
    pending_exit_status: "jet_jit_pending_exit_status" => jet_jit_pending_exit_status: sig_noarg_i64;
    wait_value: "jet_jit_wait_value" => jet_jit_wait_value: sig_noarg_i64;
    sleep: "jet_jit_sleep" => jet_jit_sleep: sig_i64;
    time_now: "jet_jit_time_now" => jet_jit_time_now: sig_noarg_i64;
    deadline_push: "jet_jit_deadline_push" => jet_jit_deadline_push: sig_void_i64;
    deadline_pop: "jet_jit_deadline_pop" => jet_jit_deadline_pop: sig_void;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[test]
    fn lexical_close_drains_a_child_registered_during_drain() {
        let mut runtime = super::super::resident::fresh_runtime();
        let runtime_ptr = &mut runtime as *mut super::super::JitRuntime;
        let runtime_addr = runtime_ptr as usize;
        ACTIVE_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime_ptr));

        let group = jet_jit_task_group_new(1, 1);
        let slots = with_runtime_mut(|rt| {
            rt.task_groups[group as usize]
                .as_ref()
                .and_then(|group| group.slots.clone())
        })
        .expect("bounded task group slots");
        let outer_permit = slots.acquire().expect("outer task group permit");
        let nested_id = std::sync::Arc::new(AtomicI64::new(-1));
        let nested_id_for_child = nested_id.clone();
        let nested_acquired = std::sync::Arc::new(AtomicBool::new(false));
        let nested_acquired_for_child = nested_acquired.clone();
        let control = JetTaskControl::new();
        let child_control = control.clone();
        let child = jet_scheduler_spawn_blocking_with_control(
            move || {
                let _outer_permit = outer_permit;
                ACTIVE_RUNTIME.with(|slot| {
                    *slot.borrow_mut() = Some(runtime_addr as *mut super::super::JitRuntime)
                });
                let result = catch_unwind(AssertUnwindSafe(|| {
                    while !child_control.cancelled.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }

                    // The group close has already claimed the outer child
                    // and issued cancellation. This nested scheduler handle
                    // must still register against the live group slot before
                    // the outer child drains.
                    jet_jit_task_group_acquire(group);
                    nested_acquired_for_child.store(
                        take_pending_task_group_permit().is_some(),
                        Ordering::Release,
                    );
                    let nested_control = JetTaskControl::new();
                    let nested_join =
                        jet_scheduler_spawn_blocking_with_control(|| 0, nested_control.clone());
                    let nested = with_runtime_mut(|rt| {
                        let id = rt.tasks.len() as i64;
                        rt.tasks.push(Some(nested_join));
                        rt.task_controls.push(nested_control);
                        id
                    });
                    nested_id_for_child.store(nested, Ordering::Release);
                    jet_jit_task_group_register(group, nested);
                }));
                ACTIVE_RUNTIME.with(|slot| *slot.borrow_mut() = None);
                if result.is_err() { -1 } else { 0 }
            },
            control.clone(),
        );
        let outer = runtime.tasks.len() as i64;
        runtime.tasks.push(Some(child));
        runtime.task_controls.push(control);
        jet_jit_task_group_register(group, outer);

        assert_eq!(close_task_group(group), JitWaitStatus::Ready as i64);
        let nested = nested_id.load(Ordering::Acquire);
        assert!(nested >= 0, "nested child was not created");
        assert!(
            !nested_acquired.load(Ordering::Acquire),
            "nested child acquired a permit after lexical close began"
        );
        assert!(runtime.tasks[nested as usize].is_none(), "nested child was not drained");
        assert!(runtime.task_groups[group as usize].is_none(), "group slot was not removed");

        ACTIVE_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    }
}
