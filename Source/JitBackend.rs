//! c77 (D-JIT1=D) — the stable `JitBackend` execution seam.
//!
//! Three-mode execution (run / build / dev) routes program execution through
//! one trait so the hot-reload watch loop never special-cases an engine. Today
//! there is exactly one implementation, [`InterpreterBackend`], which delegates
//! to the M9.5 comptime tree-walker via [`crate::Interpreter::run_checked`].
//!
//! ## Tier-0 (now) vs tier-1 (gated follow-on)
//!
//! - **Tier-0 — the interpreter.** Permanent. Run-to-completion: each run is a
//!   fresh evaluation with no resident process holding heap state between file
//!   edits. A "hot swap" here means *re-applying the freshly parsed+checked
//!   bundle and running it again* — correct and complete for what tier-0 is,
//!   not live in-memory state preservation (tier-0 has no live heap to
//!   preserve).
//! - **Tier-1 — a Cranelift JIT.** A FUTURE tier behind owner dep-approval
//!   (I6: Cranelift is an external crate). It would hold a resident process and
//!   so could preserve live heap state across a type-stable swap. This file
//!   builds ONLY the seam; no Cranelift code ships here. When it lands it slots
//!   in as a second `impl JitBackend` with zero churn to callers.
//!
//! rustc is never in this loop (I2): the seam runs the interpreter, never a
//! native build.

use crate::AST::ProgramBundle;
use crate::Diagnostics::Diagnostic;
use crate::Interpreter::{run_checked, RunOutcome};

/// The execution seam shared by every tier (interpreter now, Cranelift later).
///
/// Callers (the `jet dev`/`jet serve` watch loop) hold a `&mut dyn JitBackend`
/// and never name a concrete engine, so a future tier-1 is a drop-in.
pub trait JitBackend {
    /// Run a checked bundle to completion. `try_anyway` skips the E2201
    /// boundary scan (D-DEV1). Output is byte-identical to the release build
    /// (Q2 hard rule).
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome;

    /// Apply a type-stable edit to `module_name` and run the new bundle.
    ///
    /// On tier-0 this re-applies the freshly checked bundle and runs it (the
    /// honest tier-0 meaning of "swap": code re-applied, types unchanged). The
    /// caller has already confirmed type stability via
    /// [`crate::Sema::HotSwap::type_stable_check`]; an `Err` here means the run
    /// itself produced diagnostics (e.g. an E2202 fuel stop), not a type
    /// mismatch.
    ///
    /// A future Cranelift tier-1 would instead re-link the changed module into
    /// the resident process, preserving live heap state.
    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>>;

    /// Restart cleanly on a type/layout-changing edit and run the new bundle.
    /// On tier-0 this is the same fresh evaluation as [`JitBackend::run`]; the
    /// distinct method exists so the watch loop can announce the restart and so
    /// tier-1 can tear down and rebuild its resident process here.
    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome;
}

/// Tier-0 backend: the comptime interpreter. Stateless between runs (no
/// resident heap), so every method funnels into [`run_checked`].
#[derive(Default)]
pub struct InterpreterBackend;

impl InterpreterBackend {
    pub fn new() -> Self {
        InterpreterBackend
    }
}

impl JitBackend for InterpreterBackend {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        run_checked(bundle, try_anyway)
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        // Tier-0 swap: re-apply the new bundle and run it. This is a genuine
        // re-application, not a placeholder — the interpreter holds no resident
        // state, so "swap" is exactly "run the new code". Live-heap-preserving
        // swap is the Cranelift tier-1 (owner dep-approval gate).
        match run_checked(bundle, try_anyway) {
            RunOutcome::Ran { stdout, stderr } => Ok(RunOutcome::Ran { stdout, stderr }),
            RunOutcome::Problems(diags) => Err(diags),
        }
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        run_checked(bundle, try_anyway)
    }
}
