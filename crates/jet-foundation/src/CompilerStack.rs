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
/// back out, and `jet_jit::on_compiler_stack` carries the trace flags and tier
/// rows. Each checks [`on_compiler_worker`] before capturing, so the inline
/// path stays allocation-free.
///
/// Values and panics propagate unchanged: `join` returns the work's value, and
/// a panic is re-raised with `resume_unwind`, so the ICE path and the
/// diagnostics a caller catches keep their shape instead of being reshaped
/// into an error.
pub fn run_on_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if on_compiler_worker() {
        return work();
    }
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jet-compiler".to_string())
            .stack_size(COMPILER_STACK_SIZE)
            .spawn_scoped(scope, move || {
                ON_COMPILER_WORKER.with(|active| active.set(true));
                work()
            })
            .unwrap_or_else(|error| crate::ice!(None, "could not start compiler worker: {error}"));
        worker
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}
