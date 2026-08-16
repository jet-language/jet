//! The explicit stack every compiler entry point runs on.
//!
//! Lowering is a recursive descent over user syntax, so the frame budget is
//! per nesting level, never per program size — and the front end already caps
//! that depth: [`crate::Diagnostics::MAX_SOURCE_NESTING`] (256) is the deepest
//! nesting sema and the TIR evaluator accept, deeper source is reported as
//! `E1403`. So the worst case a valid program can demand is bounded
//! arithmetic, not a guess. Measured per level: ~144 KiB for TIR lowering
//! (per method-call level) and ~51 KiB for Cranelift lowering (per expression
//! level).
//!
//! * 256 x 144 KiB = 36 MiB — deepest TIR lowering alone
//! * 256 x 51 KiB = 12.75 MiB — deepest Cranelift lowering alone
//! * 256 x 195 KiB = 48.75 MiB — a program paying both at every level
//!
//! # Why this crate owns it
//!
//! Four independent crates install this boundary and they must share one
//! re-entrancy flag, or a nested entry would spawn a worker inside a worker:
//!
//! * `jet-sema` — `check_bundle_opts_for_output_with_context`, the funnel every
//!   public `Sema::check_bundle*` shares
//! * `jet-driver` — the compile/check/run funnels, and the loader funnel every
//!   public `Loader::load_entry*` shares
//! * `jet-codegen` — `TIR::lower_jit_program`
//! * `jet-jit` — its public bundle entries
//!
//! Installing at those funnels rather than at their callers is the whole
//! point. Each one is public API, so an embedder holding its own bundle, a
//! test harness, or the LSP reaches the recursive descent on whatever stack it
//! happens to have — and wrapping callers one at a time never converges,
//! because the overflow just moves to the next unwrapped caller.
//!
//! I6 keeps the seam one-directional — `jet-jit` must not depend on
//! `jet-driver`, and `jet-sema` depends on neither — so the flag lives in the
//! deepest crate all four take a path dependency on. That is the same reason
//! the `JitBackend` / `RunOutcome` execution seam lives here rather than in
//! any one side.

use std::cell::Cell;

thread_local! {
    /// Set on the worker itself, never on the thread that spawned it:
    /// thread-locals do not cross a spawn, so the flag has to be established
    /// by the code that runs with the big stack.
    static ON_COMPILER_WORKER: Cell<bool> = const { Cell::new(false) };
}

/// 64 MiB covers the 48.75 MiB worst case with room for the parser/sema
/// frames riding along, and matches the canonical TIR evaluator's own worker.
/// A thread stack is reserved address space committed page by page, so an
/// ordinary compile still touches only the pages it uses.
///
/// The budget is recursion depth only; unwind space is not a second term.
/// Both unwind phases run on the panicking thread's own stack, so a raise
/// that runs out of it faults on the guard page and std's per-thread handler
/// says so: `thread '<name>' has overflowed its stack`, then
/// `fatal runtime error: stack overflow` (`std::sys::pal::unix::stack_overflow`).
/// That text is the only abort this number can answer for.
///
/// `fatal runtime error: failed to initiate panic, error 5` is a different
/// defect and a bigger stack cannot move it. Error 5 is `_URC_END_OF_STACK`
/// (`library/unwind/src/types.rs`), which libgcc's phase-1 loop returns when
/// it walked off the top of the stack without finding a *handler*
/// (`unwind.inc` -> `uw_frame_state_for`): a panic raised where no
/// `catch_unwind` is above it at all. Two sources produce it.
///
/// The first is a thread-local destructor — glibc runs those from
/// `__call_tls_dtors` after the thread's Rust entry frame and its
/// `catch_unwind` have already returned, which is also why the
/// `extern "C" destroy` shim does not convert it into the "non-unwinding
/// panic" abort: that shim is a phase-2 cleanup pad, and phase 2 never starts.
///
/// The second is a Cranelift JIT frame. `cranelift-jit` registers no unwind
/// information for the code it emits, so phase 1 cannot walk past one and any
/// outer `catch_unwind` is unreachable even though it exists. That class is
/// fixed at the boundary, not here: `jet-jit`'s `host_seam` generates an
/// `extern "C"` shim per host symbol that catches and converts inside its own
/// C frame (#1997). If you are reading this because you saw error 5, check
/// which of the two you have before touching this number — neither is a
/// stack-space condition, and this constant answers for exactly one abort,
/// the `has overflowed its stack` pair above.
pub const COMPILER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Raising the accepted nesting depth must raise the stack that lowers it.
const _: () = assert!(
    COMPILER_STACK_SIZE >= crate::Diagnostics::MAX_SOURCE_NESTING * 195 * 1024,
    "the compiler worker stack must cover the deepest nesting the front end accepts",
);

/// Whether this thread already *is* a compiler worker.
///
/// Callers that carry thread-local state across the boundary check this first
/// so they skip the capture/restore dance entirely on the inline path.
pub fn on_compiler_worker() -> bool {
    ON_COMPILER_WORKER.with(Cell::get)
}

/// Run `work` with the compiler's stack budget.
///
/// Re-entrant by construction: on a thread that already has the budget the
/// closure runs inline, so the boundary can be installed at *every* public
/// entry without any path ever nesting two workers. That is what removes the
/// whack-a-mole — an entry does not have to know whether some outer entry
/// already crossed.
///
/// This primitive carries no thread-local state of its own. Each installing
/// crate knows which of its own thread-locals the work reads or publishes and
/// wraps this with exactly that capture/restore: `jet_driver::run_compiler_work`
/// and `Sema::check_bundle_opts_for_output_with_context` carry the comptime
/// ambient hooks, `TIR::lower_jit_program` carries `LAST_JIT_LOWER_FAILURE`
/// back out, and `jet_jit::on_compiler_stack` carries the trace flags, the tier
/// rows and the `core.perf` fidelity signal. Each checks [`on_compiler_worker`]
/// before capturing, so the inline path stays allocation-free.
///
/// A worker's storage lasts one outermost call, so an installing crate owes the
/// capture/restore not only to state a caller sets up or inspects around the
/// call, but to any thread-local whose contract is *per session* — state a
/// later call on the same caller thread is entitled to read. Such state is
/// lent to the worker and taken back (`jet_jit`'s fidelity signal), or the
/// session owner installs this boundary once around the whole session so every
/// call runs inline on one worker (`jet_jit`'s resident module, live heap and
/// `crate::Persist` store). Making it process-wide instead is not a third
/// option: that leaks one session into a concurrent unrelated one.
///
/// Values and panics both cross unchanged. The value rides out through
/// `join`; a panic is captured on the worker with `catch_unwind` and re-raised
/// on the caller with `resume_unwind`, so the caller observes the original
/// payload at the original panic location — the ICE path and the diagnostics a
/// caller catches keep their shape instead of being reshaped into an error.
///
/// The capture sits directly around `work`, not on the join result, so the
/// transport owes nothing to `std::thread::scope`'s internals. The worker never
/// dies panicking, `ScopeData::a_thread_panicked` is never set, and `scope` can
/// never substitute its own `"a scoped thread panicked"` payload for the
/// compiler's. `resume_unwind` then runs on the caller's own frame, outside the
/// scope, so the caller's unwind is raised exactly once, by us.
///
/// What the boundary cannot move is the panic *hook*: it runs at panic time, on
/// the worker, before any payload crosses back. A hook that reads thread-local
/// state therefore reads the worker's — that is why a compiler panic's message
/// lands outside libtest's per-test output capture, and why
/// `JET_SCHEDULER_CATCHING_PANIC` (`jet-codegen/src/Prelude/Scheduler.rs`) is
/// read on the worker rather than on the caller. `resume_unwind` deliberately
/// does not run the hook a second time.
pub fn run_on_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if on_compiler_worker() {
        return work();
    }
    let outcome = std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jet-compiler".to_string())
            .stack_size(COMPILER_STACK_SIZE)
            .spawn_scoped(scope, move || {
                ON_COMPILER_WORKER.with(|active| active.set(true));
                // Asserted, not proven: the payload is re-raised immediately
                // and unchanged, so no caller ever observes state this panic
                // left behind without unwinding through it itself.
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            })
            .unwrap_or_else(|error| crate::ice!(None, "could not start compiler worker: {error}"));
        // An outer `Err` is a panic raised after `work` returned, by the thread
        // epilogue itself; fold it into the one channel.
        worker.join().unwrap_or_else(Err)
    });
    outcome.unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}
