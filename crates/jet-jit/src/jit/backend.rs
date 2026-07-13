use jet_foundation::{
    Diagnostics::Diagnostic,
    JitBackend::{JitBackend, RunOutcome},
    AST::ProgramBundle,
};

use super::api_debug::{try_resident, try_resident_hot_swap, try_resident_restart};

/// c139 tier-1 JIT backend over the `JitBackend` seam.
///
/// `F` is the tier-0 fallback (always `InterpreterBackend` in practice).
/// M0: every method delegates to `fallback`.
/// M1: `run` JIT-compiles functions inside `resident_jit_safe()` and delegates only
///     the uncovered remainder to `fallback`.
/// M2: `hot_swap` re-links changed code in the resident process; `restart`
///     tears down live state.
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
        if let Some(result) = try_resident(bundle) {
            match result {
                Ok(RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code,
                }) => {
                    return RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    };
                }
                // Native execution already happened. Its compiler-owned
                // diagnostic is authoritative; re-running through another tier
                // would duplicate effects and hide resident boundary bugs.
                Ok(problems @ RunOutcome::Problems(_)) => return problems,
                Err(_) => return self.fallback.run(bundle, try_anyway),
            }
        }
        self.fallback.run(bundle, try_anyway)
    }

    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        if let Some(result) = try_resident_hot_swap(bundle) {
            return match result {
                Ok(out) => Ok(out),
                Err(_) => self.fallback.hot_swap(module_name, bundle, try_anyway),
            };
        }
        self.fallback.hot_swap(module_name, bundle, try_anyway)
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        if let Some(result) = try_resident_restart(bundle) {
            match result {
                Ok(out) => return out,
                Err(_) => return self.fallback.restart(bundle, try_anyway),
            }
        }
        self.fallback.restart(bundle, try_anyway)
    }
}
