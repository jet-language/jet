//! M4: scheduler-backed task/channel host shims for the Cranelift JIT.

use jet_codegen::scheduler::{
    jet_scheduler_all, jet_scheduler_any, jet_scheduler_race, jet_scheduler_select_int_channels,
    jet_scheduler_deliver_shield_exit, jet_scheduler_shield_enter,
    jet_scheduler_shield_leave_status, jet_scheduler_sleep_ms, jet_scheduler_spawn_with_control,
    jet_scheduler_wait_without_unwind, JetSchedulerChannel, JetSchedulerJoin, JetSchedulerWait,
    JetShieldExit, JetTaskControl,
};
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static ACTIVE_RUNTIME: RefCell<Option<*mut super::JitRuntime>> = const { RefCell::new(None) };
    static PENDING_SHIELD_EXIT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static WAIT_VALUE: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
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
            with_runtime_mut(|rt| rt.set_deadline(rendered));
            JitWaitStatus::Interrupted as i64
        }
        JetSchedulerWait::Panicked(message) => {
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

extern "C" fn jet_jit_channel_sender(ch: i64) -> i64 {
    with_runtime_mut(|rt| {
        let ch = rt
            .channels
            .get(ch as usize)
            .expect("jit channel sender: bad handle");
        let id = rt.senders.len() as i64;
        rt.senders.push(ch.sender());
        id
    })
}

extern "C" fn jet_jit_sender_clone(s: i64) -> i64 {
    with_runtime_mut(|rt| {
        let tx = rt
            .senders
            .get(s as usize)
            .expect("jit sender clone: bad handle")
            .clone();
        let id = rt.senders.len() as i64;
        rt.senders.push(tx);
        id
    })
}

extern "C" fn jet_jit_sender_send(s: i64, v: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let tx = rt
            .senders
            .get(s as usize)
            .expect("jit sender send: bad handle");
        i64::from(tx.send(v))
    }))
}

/// `0` = closed; otherwise `received + 1` (encoding avoids colliding with `0`).
///
/// Blocks until a message arrives or the channel closes — matches AOT
/// `Channel.receive()` + `??` on `Result` (not `try_receive`).
extern "C" fn jet_jit_channel_receive_status(ch: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let chan = rt
            .channels
            .get(ch as usize)
            .expect("jit channel receive: bad handle");
        match chan.receive() {
            Some(v) => v + 1,
            None => 0,
        }
    }))
}

extern "C" fn jet_jit_channel_receive(ch: i64, _line: u32) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let chan = rt
            .channels
            .get(ch as usize)
            .expect("jit channel receive: bad handle");
        match chan.receive() {
            Some(v) => v,
            None => {
                rt.set_trap("channel closed");
                0
            }
        }
    }))
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

fn active_runtime_ptr() -> Option<*mut super::JitRuntime> {
    ACTIVE_RUNTIME.with(|slot| *slot.borrow())
}

/// Run JIT spawn body on a pool worker with the spawner's runtime heap wired up.
fn spawn_with_runtime<F>(f: F) -> i64
where
    F: FnOnce() -> i64 + Send + 'static,
{
    let rt_ptr = active_runtime_ptr().expect("jit spawn without active runtime");
    let rt_addr = rt_ptr as usize;
    let control = JetTaskControl::new();
    let join = jet_scheduler_spawn_with_control(
        move || {
            // SAFETY: `rt_ptr` is the resident heap for this JIT invocation; workers
            // only touch mutex-backed channel state and indexed sender slots.
            let rt_ptr = rt_addr as *mut super::JitRuntime;
            set_active_runtime(Some(rt_ptr));
            let _ = take_pending_shield_exit();
            let out = f();
            set_active_runtime(None);
            jet_scheduler_deliver_shield_exit(take_pending_shield_exit());
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
            rt.set_deadline(
                "Error [E3003]: deadline exceeded while waiting at shield exit\nWhy: this wait point observed the task context deadline from `#Context(deadline: …)`\nFix: raise the deadline budget or shorten the work before this wait point".to_string(),
            )
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

extern "C" fn jet_jit_task_cancel(task: i64) {
    with_runtime_mut(|rt| {
        rt.task_controls[task as usize].cancel();
    });
}

extern "C" fn jet_jit_task_join(task: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let join = rt.tasks[task as usize]
            .take()
            .expect("jit task join: already joined");
        join.join()
    }))
}

/// D-NURSERY1=A: `g.all([h1, h2, …])` — returns a new `[Int]` list handle.
extern "C" fn jet_jit_task_all(task_list: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let ids = task_ids_from_list(rt, task_list);
        let entries = take_task_entries(rt, &ids);
        store_i64_list(rt, jet_scheduler_all(entries))
    }))
}

/// D-CONCCOMB1=A: `g.race([h1, h2, …])` — first successful result.
extern "C" fn jet_jit_task_race(task_list: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let ids = task_ids_from_list(rt, task_list);
        let entries = take_task_entries(rt, &ids);
        jet_scheduler_race(entries)
    }))
}

/// D-CONCCOMB1=A: `g.any([h1, h2, …])` — first completed result.
extern "C" fn jet_jit_task_any(task_list: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let ids = task_ids_from_list(rt, task_list);
        let entries = take_task_entries(rt, &ids);
        jet_scheduler_any(entries)
    }))
}

/// D-CONCSELECT1=A: `g.select().recv(…).wait()` — multiplex channel/timer arms.
extern "C" fn jet_jit_select_wait(recv_list: i64, after_list: i64) -> i64 {
    wait_status(|| with_runtime_mut(|rt| {
        let ch_ids = task_ids_from_list(rt, recv_list);
        let after_ids = task_ids_from_list(rt, after_list);
        let channels: Vec<JetSchedulerChannel<i64>> = ch_ids
            .iter()
            .map(|&id| {
                rt.channels
                    .get(id as usize)
                    .expect("jit select: bad channel handle")
                    .clone()
            })
            .collect();
        let timers: Vec<u64> = after_ids.iter().map(|&ms| (ms.max(0)) as u64).collect();
        jet_scheduler_select_int_channels(&channels, timers)
    }))
}

extern "C" fn jet_jit_sleep(millis: i64) -> i64 {
    wait_status(|| {
        jet_scheduler_sleep_ms(millis.max(0) as u64);
        0
    })
}

pub(crate) struct ConcurrencyHostFns {
    pub channel_new: cranelift_module::FuncId,
    pub channel_sender: cranelift_module::FuncId,
    pub sender_clone: cranelift_module::FuncId,
    pub sender_send: cranelift_module::FuncId,
    pub channel_receive: cranelift_module::FuncId,
    pub channel_receive_status: cranelift_module::FuncId,
    pub panic_channel_closed: cranelift_module::FuncId,
    pub spawn0: cranelift_module::FuncId,
    pub spawn1: cranelift_module::FuncId,
    pub spawn2: cranelift_module::FuncId,
    pub spawn3: cranelift_module::FuncId,
    pub spawn4: cranelift_module::FuncId,
    pub task_join: cranelift_module::FuncId,
    pub task_cancel: cranelift_module::FuncId,
    pub task_all: cranelift_module::FuncId,
    pub task_race: cranelift_module::FuncId,
    pub task_any: cranelift_module::FuncId,
    pub select_wait: cranelift_module::FuncId,
    pub shield_enter: cranelift_module::FuncId,
    pub shield_leave: cranelift_module::FuncId,
    pub wait_value: cranelift_module::FuncId,
    pub sleep: cranelift_module::FuncId,
}

pub(crate) fn register_concurrency_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_channel_new", jet_jit_channel_new as *const u8);
    builder.symbol(
        "jet_jit_channel_sender",
        jet_jit_channel_sender as *const u8,
    );
    builder.symbol("jet_jit_sender_clone", jet_jit_sender_clone as *const u8);
    builder.symbol("jet_jit_sender_send", jet_jit_sender_send as *const u8);
    builder.symbol(
        "jet_jit_channel_receive",
        jet_jit_channel_receive as *const u8,
    );
    builder.symbol(
        "jet_jit_channel_receive_status",
        jet_jit_channel_receive_status as *const u8,
    );
    builder.symbol(
        "jet_jit_panic_channel_closed",
        jet_jit_panic_channel_closed as *const u8,
    );
    builder.symbol("jet_jit_spawn0", jet_jit_spawn0 as *const u8);
    builder.symbol("jet_jit_spawn1", jet_jit_spawn1 as *const u8);
    builder.symbol("jet_jit_spawn2", jet_jit_spawn2 as *const u8);
    builder.symbol("jet_jit_spawn3", jet_jit_spawn3 as *const u8);
    builder.symbol("jet_jit_spawn4", jet_jit_spawn4 as *const u8);
    builder.symbol("jet_jit_task_join", jet_jit_task_join as *const u8);
    builder.symbol("jet_jit_task_cancel", jet_jit_task_cancel as *const u8);
    builder.symbol("jet_jit_task_all", jet_jit_task_all as *const u8);
    builder.symbol("jet_jit_task_race", jet_jit_task_race as *const u8);
    builder.symbol("jet_jit_task_any", jet_jit_task_any as *const u8);
    builder.symbol("jet_jit_select_wait", jet_jit_select_wait as *const u8);
    builder.symbol("jet_jit_shield_enter", jet_jit_shield_enter as *const u8);
    builder.symbol("jet_jit_shield_leave", jet_jit_shield_leave as *const u8);
    builder.symbol("jet_jit_wait_value", jet_jit_wait_value as *const u8);
    builder.symbol("jet_jit_sleep", jet_jit_sleep as *const u8);
}

pub(crate) fn declare_concurrency_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<ConcurrencyHostFns, String> {
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
    let sig_void = Signature::new(cc);
    let mut sig_noarg_i64 = Signature::new(cc);
    sig_noarg_i64.returns.push(AbiParam::new(types::I64));

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    let mut sig_spawn1 = sig_spawn0.clone();
    sig_spawn1.params.push(AbiParam::new(types::I64));
    let mut sig_spawn2 = sig_spawn1.clone();
    sig_spawn2.params.push(AbiParam::new(types::I64));
    let mut sig_spawn3 = sig_spawn2.clone();
    sig_spawn3.params.push(AbiParam::new(types::I64));
    let mut sig_spawn4 = sig_spawn3.clone();
    sig_spawn4.params.push(AbiParam::new(types::I64));

    Ok(ConcurrencyHostFns {
        channel_new: import("jet_jit_channel_new", &sig_channel_new)?,
        channel_sender: import("jet_jit_channel_sender", &sig_i64)?,
        sender_clone: import("jet_jit_sender_clone", &sig_i64)?,
        sender_send: import("jet_jit_sender_send", &sig_send)?,
        channel_receive: import("jet_jit_channel_receive", &sig_recv)?,
        channel_receive_status: import("jet_jit_channel_receive_status", &sig_i64)?,
        panic_channel_closed: import("jet_jit_panic_channel_closed", &sig_panic_line)?,
        spawn0: import("jet_jit_spawn0", &sig_spawn0)?,
        spawn1: import("jet_jit_spawn1", &sig_spawn1)?,
        spawn2: import("jet_jit_spawn2", &sig_spawn2)?,
        spawn3: import("jet_jit_spawn3", &sig_spawn3)?,
        spawn4: import("jet_jit_spawn4", &sig_spawn4)?,
        task_join: import("jet_jit_task_join", &sig_i64)?,
        task_cancel: import("jet_jit_task_cancel", &sig_void_i64)?,
        task_all: import("jet_jit_task_all", &sig_i64)?,
        task_race: import("jet_jit_task_race", &sig_i64)?,
        task_any: import("jet_jit_task_any", &sig_i64)?,
        select_wait: import("jet_jit_select_wait", &sig_i64_i64)?,
        shield_enter: import("jet_jit_shield_enter", &sig_void)?,
        shield_leave: import("jet_jit_shield_leave", &sig_noarg_i64)?,
        wait_value: import("jet_jit_wait_value", &sig_noarg_i64)?,
        sleep: import("jet_jit_sleep", &sig_i64)?,
    })
}
