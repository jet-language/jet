//! c77 (D-JIT1=D) — the stable `JitBackend` execution seam.
//!
//! The `JitBackend` trait and `RunOutcome` live in `jet-foundation`
//! (moved by c139) so the `jet-jit/` workspace member can implement the trait
//! without a dependency cycle. Re-exported here for callers that use the
//! `jet::JitBackend::*` path.

// Re-export the seam types from jet-foundation.
pub use jet_foundation::JitBackend::{JitBackend, RunOutcome};

use crate::Diagnostics::Diagnostic;
use crate::Interpreter::run_checked;
use crate::AST::ProgramBundle;

/// Tier-0 backend: the comptime interpreter. Stateless between runs (no
/// resident heap), so every method funnels into [`run_checked`].
///
/// This is the permanent fallback (D-JIT1): even when Cranelift tier-1 is
/// active, calls outside `jit_covers` fall back here, never to silence.
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
        match run_checked(bundle, try_anyway) {
            RunOutcome::Ran { stdout, stderr } => Ok(RunOutcome::Ran { stdout, stderr }),
            RunOutcome::Problems(diags) => Err(diags),
        }
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        run_checked(bundle, try_anyway)
    }
}
