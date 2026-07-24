use std::sync::atomic::{AtomicBool, Ordering};

static JIT_EXECUTED: AtomicBool = AtomicBool::new(false);
static FALLBACK_INVOKED: AtomicBool = AtomicBool::new(false);

pub(crate) fn note_jit_execution() {
    JIT_EXECUTED.store(true, Ordering::SeqCst);
}

/// Test-only: whether strict Cranelift executed in this process.
#[doc(hidden)]
pub fn jit_executed_for_test() -> bool {
    JIT_EXECUTED.load(Ordering::SeqCst)
}

/// Test-only: reset JIT execution trace between assertions.
#[doc(hidden)]
pub fn reset_jit_trace_for_test() {
    JIT_EXECUTED.store(false, Ordering::SeqCst);
    FALLBACK_INVOKED.store(false, Ordering::SeqCst);
}

/// Test-only: record that a forbidden fallback backend was reached.
#[doc(hidden)]
pub fn note_fallback_invoked_for_test() {
    FALLBACK_INVOKED.store(true, Ordering::SeqCst);
}

/// Test-only: whether a forbidden fallback backend ran.
#[doc(hidden)]
pub fn fallback_invoked_for_test() -> bool {
    FALLBACK_INVOKED.load(Ordering::SeqCst)
}
