//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: CraneliftBackend<F: JitBackend> where F is the tier-0
//! fallback. M0 delegates everything to F; M1 adds jit_covers() and
//! lower_tir_clif() to actually compile + run the covered subset natively.
//!
//! I6: Cranelift crates live here, not in the compiler `jet` crate (`Source/`).
//! The root package depends on jet-jit; jet-jit depends on cranelift-*.
//! D-JITDEP1 approved this as a scoped runtime-side exception.

use jet_foundation::{
    AST::ProgramBundle,
    Diagnostics::Diagnostic,
    JitBackend::{JitBackend, RunOutcome},
};

/// c139 tier-1 JIT backend over the `JitBackend` seam.
///
/// `F` is the tier-0 fallback (always `InterpreterBackend` in practice).
/// M0: every method delegates to `fallback`.
/// M1: `run` and `hot_swap` will JIT-compile functions inside `jit_covers()`
///     and delegate only the uncovered remainder to `fallback`.
pub struct CraneliftBackend<F: JitBackend> {
    fallback: F,
}

impl<F: JitBackend> CraneliftBackend<F> {
    /// Construct a CraneliftBackend wrapping `fallback` for tier-0 coverage.
    pub fn new(fallback: F) -> Self {
        CraneliftBackend { fallback }
    }
}

impl<F: JitBackend> JitBackend for CraneliftBackend<F> {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        // M0: delegate; M1 will JIT jit_covers() subset before falling back.
        self.fallback.run(bundle, try_anyway)
    }

    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        // M0: delegate; M2 will re-link the module in the resident process.
        self.fallback.hot_swap(module_name, bundle, try_anyway)
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        // M0: delegate; M2 will tear down and rebuild the resident JIT process.
        self.fallback.restart(bundle, try_anyway)
    }
}
