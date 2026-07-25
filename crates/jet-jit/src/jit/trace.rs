use std::cell::Cell;

thread_local! {
    static JIT_EXECUTED: Cell<bool> = const { Cell::new(false) };
    static FALLBACK_INVOKED: Cell<bool> = const { Cell::new(false) };
    static DEOPT_INVOKED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn note_jit_execution() {
    JIT_EXECUTED.with(|flag| flag.set(true));
}

/// Test-only: whether strict Cranelift executed on this thread.
#[doc(hidden)]
pub fn jit_executed_for_test() -> bool {
    JIT_EXECUTED.with(Cell::get)
}

/// Test-only: reset JIT execution trace between assertions.
#[doc(hidden)]
pub fn reset_jit_trace_for_test() {
    JIT_EXECUTED.with(|flag| flag.set(false));
    FALLBACK_INVOKED.with(|flag| flag.set(false));
    DEOPT_INVOKED.with(|flag| flag.set(false));
}

/// Test-only: record that a forbidden AOT fallback backend was reached.
#[doc(hidden)]
pub fn note_fallback_invoked_for_test() {
    FALLBACK_INVOKED.with(|flag| flag.set(true));
}

/// Test-only: whether a forbidden AOT fallback backend ran.
#[doc(hidden)]
pub fn fallback_invoked_for_test() -> bool {
    FALLBACK_INVOKED.with(Cell::get)
}

/// Test-only: record interpreter deopt (allowed under D-LENS-RUN2=A).
#[doc(hidden)]
pub fn note_deopt_invoked_for_test() {
    DEOPT_INVOKED.with(|flag| flag.set(true));
}

/// Test-only: whether interpreter deopt ran.
#[doc(hidden)]
pub fn deopt_invoked_for_test() -> bool {
    DEOPT_INVOKED.with(Cell::get)
}

/// Snapshot of per-thread JIT/deopt flags (for worker-thread handoff).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct JitTraceFlags {
    pub jit_executed: bool,
    pub fallback_invoked: bool,
    pub deopt_invoked: bool,
}

#[doc(hidden)]
pub fn jit_trace_flags_for_test() -> JitTraceFlags {
    JitTraceFlags {
        jit_executed: jit_executed_for_test(),
        fallback_invoked: fallback_invoked_for_test(),
        deopt_invoked: deopt_invoked_for_test(),
    }
}

/// Apply worker-thread flags onto the current thread after a stacked worker returns.
#[doc(hidden)]
pub fn merge_jit_trace_flags_for_test(flags: JitTraceFlags) {
    if flags.jit_executed {
        note_jit_execution();
    }
    if flags.fallback_invoked {
        note_fallback_invoked_for_test();
    }
    if flags.deopt_invoked {
        note_deopt_invoked_for_test();
    }
}
