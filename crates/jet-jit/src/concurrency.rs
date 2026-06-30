//! M4: scheduler-backed task/channel host shims for the Cranelift JIT.

use jet_codegen::scheduler::{
    jet_scheduler_drain, jet_scheduler_spawn, JetSchedulerChannel, JetSchedulerJoin,
};
use std::cell::RefCell;

thread_local! {
    static ACTIVE_RUNTIME: RefCell<Option<*mut super::JitRuntime>> = const { RefCell::new(None) };
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

fn trap_panic(msg: &str) -> ! {
    with_runtime_mut(|rt| {
        rt.stderr.push_str(&format!("panic: {msg}\n"));
        rt.stderr
            .push_str(&format!("  --> {}:1\n", rt.source_file));
    });
    std::process::exit(70);
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

extern "C" fn jet_jit_sender_send(s: i64, v: i64) {
    with_runtime_mut(|rt| {
        let tx = rt
            .senders
            .get(s as usize)
            .expect("jit sender send: bad handle");
        tx.send(v);
    });
}

/// `0` = closed; otherwise `received + 1` (encoding avoids colliding with `0`).
///
/// Blocks until a message arrives or the channel closes — matches AOT
/// `Channel.receive()` + `??` on `Result` (not `try_receive`).
extern "C" fn jet_jit_channel_receive_status(ch: i64) -> i64 {
    with_runtime_mut(|rt| {
        let chan = rt
            .channels
            .get(ch as usize)
            .expect("jit channel receive: bad handle");
        match chan.receive() {
            Some(v) => v + 1,
            None => 0,
        }
    })
}

extern "C" fn jet_jit_channel_receive(ch: i64, line: u32) -> i64 {
    with_runtime_mut(|rt| {
        let chan = rt
            .channels
            .get(ch as usize)
            .expect("jit channel receive: bad handle");
        match chan.receive() {
            Some(v) => v,
            None => {
                rt.stderr
                    .push_str("panic: channel closed\n");
                rt.stderr.push_str(&format!(
                    "  --> {}:{line}\n",
                    rt.source_file
                ));
                std::process::exit(70);
            }
        }
    })
}

extern "C" fn jet_jit_panic_channel_closed(_line: u32) -> i64 {
    trap_panic("channel closed");
}

type SpawnFn0 = extern "C" fn() -> i64;
type SpawnFn1 = extern "C" fn(i64) -> i64;
type SpawnFn2 = extern "C" fn(i64, i64) -> i64;
type SpawnFn3 = extern "C" fn(i64, i64, i64) -> i64;
type SpawnFn4 = extern "C" fn(i64, i64, i64, i64) -> i64;

fn store_task(join: JetSchedulerJoin<i64>) -> i64 {
    with_runtime_mut(|rt| {
        let id = rt.tasks.len() as i64;
        rt.tasks.push(Some(join));
        id
    })
}

fn active_runtime_ptr() -> Option<*mut super::JitRuntime> {
    ACTIVE_RUNTIME.with(|slot| *slot.borrow())
}

/// Run JIT spawn body on a pool worker with the spawner's runtime heap wired up.
fn spawn_with_runtime<F>(f: F) -> JetSchedulerJoin<i64>
where
    F: FnOnce() -> i64 + Send + 'static,
{
    let rt_ptr = active_runtime_ptr().expect("jit spawn without active runtime");
    let rt_addr = rt_ptr as usize;
    jet_scheduler_spawn(move || {
        // SAFETY: `rt_ptr` is the resident heap for this JIT invocation; workers
        // only touch mutex-backed channel state and indexed sender slots.
        let rt_ptr = rt_addr as *mut super::JitRuntime;
        set_active_runtime(Some(rt_ptr));
        let out = f();
        set_active_runtime(None);
        out
    })
}

extern "C" fn jet_jit_spawn0(f: SpawnFn0) -> i64 {
    store_task(spawn_with_runtime(move || f()))
}

extern "C" fn jet_jit_spawn1(f: SpawnFn1, c0: i64) -> i64 {
    store_task(spawn_with_runtime(move || f(c0)))
}

extern "C" fn jet_jit_spawn2(f: SpawnFn2, c0: i64, c1: i64) -> i64 {
    store_task(spawn_with_runtime(move || f(c0, c1)))
}

extern "C" fn jet_jit_spawn3(f: SpawnFn3, c0: i64, c1: i64, c2: i64) -> i64 {
    store_task(spawn_with_runtime(move || f(c0, c1, c2)))
}

extern "C" fn jet_jit_spawn4(f: SpawnFn4, c0: i64, c1: i64, c2: i64, c3: i64) -> i64 {
    store_task(spawn_with_runtime(move || f(c0, c1, c2, c3)))
}

extern "C" fn jet_jit_task_join(task: i64) -> i64 {
    with_runtime_mut(|rt| {
        let slot = rt
            .tasks
            .get_mut(task as usize)
            .expect("jit task join: bad handle");
        let join = slot.take().expect("jit task join: already joined");
        join.join()
    })
}

extern "C" fn jet_jit_scheduler_drain() {
    jet_scheduler_drain();
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
    pub scheduler_drain: cranelift_module::FuncId,
}

pub(crate) fn register_concurrency_symbols(
    builder: &mut cranelift_jit::JITBuilder,
) {
    builder.symbol("jet_jit_channel_new", jet_jit_channel_new as *const u8);
    builder.symbol("jet_jit_channel_sender", jet_jit_channel_sender as *const u8);
    builder.symbol("jet_jit_sender_clone", jet_jit_sender_clone as *const u8);
    builder.symbol("jet_jit_sender_send", jet_jit_sender_send as *const u8);
    builder.symbol("jet_jit_channel_receive", jet_jit_channel_receive as *const u8);
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
    builder.symbol("jet_jit_scheduler_drain", jet_jit_scheduler_drain as *const u8);
}

pub(crate) fn declare_concurrency_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<ConcurrencyHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_void = Signature::new(cc);
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
    sig_send.params.push(AbiParam::new(types::I64));

    let mut sig_spawn0 = Signature::new(cc);
    sig_spawn0.params.push(AbiParam::new(types::I64));
    sig_spawn0.returns.push(AbiParam::new(types::I64));

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
        scheduler_drain: import("jet_jit_scheduler_drain", &sig_void)?,
    })
}
