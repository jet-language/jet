// D-FAIL-BREACH1=A: the runtime call-depth counter is task-local.
//
// A resident engine may run several Jet tasks against one runtime value. The
// source call stack belongs to the task, not to that shared runtime. AOT uses
// this same kernel through `Prelude/Core.rs`; the JIT adapter only marshals
// entry and leave calls into it.
//
// The depth budget is per thread too. A program keeps the shared language
// budget `JET_RUNTIME_STACK_LIMIT` on every thread. Only a host that knows its
// threads' real stacks may scale it (`jet_runtime_stack_scale_to_host`), e.g.
// the bootstrap harness running the compiler on its 512 MiB thread. From then
// on every thread of the process derives its budget from its own stack: an
// explicitly sized thread reports its size (`jet_runtime_stack_thread_stack`),
// any other thread has std's spawn stack (`RUST_MIN_STACK`, else 2 MiB).
static JET_RUNTIME_STACK_HOST_SCALED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static JET_RUNTIME_STACK_DEPTH: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    // 0 = not derived yet on this thread.
    static JET_RUNTIME_STACK_BUDGET: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Native stack bytes budgeted per guarded Jet frame: the ratio the shared
/// budget already assumes, `JET_RUNTIME_STACK_LIMIT` frames on the 8 MiB
/// main-thread stack. A host scaling the budget to a bigger stack keeps the
/// same per-frame assumption, so the stop still fires before the guard page.
pub const JET_RUNTIME_STACK_FRAME_BYTES: usize = 8 * 1024 * 1024 / JET_RUNTIME_STACK_LIMIT;

/// The stack std gives a thread spawned without an explicit size: the same
/// `RUST_MIN_STACK` parse and 2 MiB default as `std::thread`.
fn jet_runtime_stack_spawn_default() -> usize {
    static SPAWN_STACK: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SPAWN_STACK.get_or_init(|| {
        std::env::var_os("RUST_MIN_STACK")
            .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
            .unwrap_or(2 * 1024 * 1024)
    })
}

fn jet_runtime_stack_budget(stack_bytes: Option<usize>) -> usize {
    if !JET_RUNTIME_STACK_HOST_SCALED.load(std::sync::atomic::Ordering::Acquire) {
        return JET_RUNTIME_STACK_LIMIT;
    }
    let stack_bytes = stack_bytes.unwrap_or_else(jet_runtime_stack_spawn_default);
    (stack_bytes / JET_RUNTIME_STACK_FRAME_BYTES).max(JET_RUNTIME_STACK_LIMIT)
}

/// Host-only: scale every thread's budget to its real stack, starting with
/// the calling thread, whose stack is `stack_bytes`. Call it first thing on
/// that thread, before it spawns any thread that runs Jet code.
pub fn jet_runtime_stack_scale_to_host(stack_bytes: usize) {
    JET_RUNTIME_STACK_HOST_SCALED.store(true, std::sync::atomic::Ordering::Release);
    jet_runtime_stack_thread_stack(stack_bytes);
}

/// A runtime thread spawned with an explicit `stack_bytes` reports it first
/// thing. Without a host-scaled process this keeps the shared budget.
pub fn jet_runtime_stack_thread_stack(stack_bytes: usize) {
    let budget = jet_runtime_stack_budget(Some(stack_bytes));
    JET_RUNTIME_STACK_BUDGET.with(|cell| cell.set(budget));
}

#[inline(always)]
pub fn jet_runtime_stack_enter() -> bool {
    let limit = JET_RUNTIME_STACK_BUDGET.with(|cell| match cell.get() {
        0 => {
            let budget = jet_runtime_stack_budget(None);
            cell.set(budget);
            budget
        }
        budget => budget,
    });
    JET_RUNTIME_STACK_DEPTH.with(|depth| {
        let next = depth.get().saturating_add(1);
        depth.set(next);
        next > limit
    })
}

#[inline(always)]
pub fn jet_runtime_stack_leave() {
    JET_RUNTIME_STACK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
}
