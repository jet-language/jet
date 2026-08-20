// D-FAIL-BREACH1=A: the runtime call-depth counter is task-local.
//
// A resident engine may run several Jet tasks against one runtime value. The
// source call stack belongs to the task, not to that shared runtime. AOT uses
// this same kernel through `Prelude/Core.rs`; the JIT adapter only marshals
// entry and leave calls into it.
thread_local! {
    static JET_RUNTIME_STACK_DEPTH: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

pub fn jet_runtime_stack_enter() -> bool {
    JET_RUNTIME_STACK_DEPTH.with(|depth| {
        let next = depth.get().saturating_add(1);
        depth.set(next);
        next > JET_RUNTIME_STACK_LIMIT
    })
}

pub fn jet_runtime_stack_leave() {
    JET_RUNTIME_STACK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
}
