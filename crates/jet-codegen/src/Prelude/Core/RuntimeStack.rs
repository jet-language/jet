// D-FAIL-BREACH1=A: the runtime call-depth counter is task-local.
//
// A resident engine may run several Jet tasks against one runtime value. The
// source call stack belongs to the task, not to that shared runtime. AOT uses
// this same kernel through `Prelude/Core.rs`; the JIT adapter only marshals
// entry and leave calls into it.
//
// The depth budget is per thread too. Every thread starts with the shared
// language budget `JET_RUNTIME_STACK_LIMIT`; only a host that spawned the
// thread knows its real stack size, so only that host may widen it
// (`jet_runtime_stack_budget_for_stack`), e.g. the bootstrap harness running
// the compiler on its 512 MiB thread.
thread_local! {
    static JET_RUNTIME_STACK_DEPTH: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    static JET_RUNTIME_STACK_BUDGET: std::cell::Cell<usize> =
        const { std::cell::Cell::new(JET_RUNTIME_STACK_LIMIT) };
}

/// Native stack bytes budgeted per guarded Jet frame: the ratio the shared
/// budget already assumes, `JET_RUNTIME_STACK_LIMIT` frames on the 8 MiB
/// main-thread stack. A host scaling the budget to a bigger stack keeps the
/// same per-frame assumption, so the stop still fires before the guard page.
pub const JET_RUNTIME_STACK_FRAME_BYTES: usize = 8 * 1024 * 1024 / JET_RUNTIME_STACK_LIMIT;

/// Set the calling thread's depth budget from the thread's real stack size.
/// Call it first thing on a thread the host spawned with `stack_bytes`; the
/// budget never drops below the shared language budget.
pub fn jet_runtime_stack_budget_for_stack(stack_bytes: usize) {
    let limit = (stack_bytes / JET_RUNTIME_STACK_FRAME_BYTES).max(JET_RUNTIME_STACK_LIMIT);
    JET_RUNTIME_STACK_BUDGET.with(|budget| budget.set(limit));
}

#[inline(always)]
pub fn jet_runtime_stack_enter() -> bool {
    let limit = JET_RUNTIME_STACK_BUDGET.with(std::cell::Cell::get);
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
