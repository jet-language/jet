//! c139 (D-JITDEP1 / D-JIT2=A) — the stable `JitBackend` execution seam.
//!
//! Lives in `jet-foundation` so the `jet-jit/` sibling workspace member
//! (which carries the Cranelift dep) can implement the trait without
//! depending on the root `jet` crate (which would create a dep cycle).
//!
//! `Source/JitBackend.rs` re-exports everything here and adds
//! `InterpreterBackend` (the tier-0 interpreter impl).

use crate::Diagnostics::Diagnostic;
use crate::MIR::{MirArtifactId, MirProgram};
/// What a single dev/serve iteration produced.
///
/// Identical shape to the AOT compilation result (Q2 hard rule):
/// `Ran.stdout`/`Ran.stderr`/`Ran.exit_code` are byte-identical to the compiled
/// binary's output.
#[derive(Debug, Clone)]
pub enum RunOutcome {
    /// The program ran to completion. `stdout`/`stderr`/`exit_code` are
    /// byte-identical to the compiled program (Q2 — enforced by the
    /// differential battery in `tests/dev.rs`).
    Ran {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    /// Front-end or runtime diagnostics, including E2202 fuel stops and
    /// canonical-evaluator refusals.
    Problems(Vec<Diagnostic>),
}

/// The execution seam shared by every tier (interpreter now, Cranelift later).
///
/// Callers hold a `&mut dyn JitBackend` and never name a concrete engine, so
/// a future tier-1 (or c140 bytecode VM / c141 native JIT) is a drop-in.
pub trait JitBackend {
    /// The canonical, invocation-local policy carried by this backend.
    ///
    /// Foundation cannot depend on the package model without creating a
    /// dependency cycle, so the concrete adapters bind this projection to
    /// `ReleaseDevtoolsPolicy` themselves.  The policy is deliberately an
    /// input to every execution transition rather than backend-global state.
    type InvocationPolicy: ?Sized;

    /// Run an optimized canonical MIR artifact to completion.
    /// `try_anyway` is an invocation option passed to the canonical evaluator.
    fn run(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome;

    /// Apply a type-stable edit to `module_name` and run the new artifact.
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
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> Result<RunOutcome, Vec<Diagnostic>>;

    /// Restart cleanly on a type/layout-changing edit and run the new artifact.
    ///
    /// Tier-0: same as `run`. Tier-1: tear down the resident process, rebuild,
    /// and announce the restart per D-HOTSWAP1.
    fn restart(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome;
}
