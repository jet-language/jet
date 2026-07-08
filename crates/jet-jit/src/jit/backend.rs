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
                // A resident runtime trap (E0953) proves the native lowering
                // reached user code, but the in-process JIT cannot yet emit the
                // exact AOT panic stderr/exit envelope. Default `jet dev` uses
                // the transparent fallback ladder here so panic demos still
                // match `jet run`; hot_swap/restart keep returning the resident
                // diagnostic for live-loop safety tests.
                Ok(RunOutcome::Problems(_)) => return self.fallback.run(bundle, try_anyway),
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
