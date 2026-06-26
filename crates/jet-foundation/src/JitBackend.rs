//! c139 (D-JITDEP1 / D-JIT2=A) — the stable `JitBackend` execution seam.
//!
//! Lives in `jet-foundation` so the `jet-jit/` sibling workspace member
//! (which carries the Cranelift dep) can implement the trait without
//! depending on the root `jet` crate (which would create a dep cycle).
//!
//! `Source/JitBackend.rs` re-exports everything here and adds
//! `InterpreterBackend` (the tier-0 interpreter impl).

use crate::AST::ProgramBundle;
use crate::Diagnostics::Diagnostic;

/// What a single dev/serve iteration produced.
///
/// Identical shape to the AOT compilation result (Q2 hard rule):
/// `Ran.stdout`/`Ran.stderr` are byte-identical to the compiled binary's output.
#[derive(Debug, Clone)]
pub enum RunOutcome {
    /// The program ran to completion. `stdout`/`stderr` are byte-identical
    /// to the compiled program (Q2 — enforced by the differential battery in
    /// `tests/dev.rs`).
    Ran { stdout: String, stderr: String },
    /// Front-end or runtime diagnostics. Includes E2201 boundary notes and
    /// E2202 fuel stops.
    Problems(Vec<Diagnostic>),
}

/// The execution seam shared by every tier (interpreter now, Cranelift later).
///
/// Callers hold a `&mut dyn JitBackend` and never name a concrete engine, so
/// a future tier-1 (or the c140 bytecode VM / c141 native JIT) is a drop-in.
pub trait JitBackend {
    /// Run a checked bundle to completion.
    /// `try_anyway` skips the E2201 boundary scan (D-DEV1).
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome;

    /// Apply a type-stable edit to `module_name` and run the new bundle.
    ///
    /// The caller has already confirmed type stability via
    /// `Sema::HotSwap::type_stable_check`. `Err` means the run produced
    /// diagnostics (e.g. E2202 fuel stop), not a type mismatch.
    ///
    /// Cranelift tier-1 re-links the changed module in the resident process,
    /// preserving live heap state. Tier-0 re-evaluates from scratch.
    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>>;

    /// Restart cleanly on a type/layout-changing edit and run the new bundle.
    ///
    /// Tier-0: same as `run`. Tier-1: tear down the resident process, rebuild,
    /// and announce the restart per D-HOTSWAP1.
    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome;
}
