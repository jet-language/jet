use jet_foundation::{
    JitBackend::{JitBackend, RunOutcome},
    AST::ProgramBundle,
};

use super::api_debug::{try_resident, try_resident_hot_swap, try_resident_restart};
use super::gap::{e2211_diagnostic, JitGap};

/// c139 tier-1 JIT backend — strict Cranelift lens (D-LENS-RUN1 / card #728).
///
/// Missing resident coverage is E2211 compiler debt. No AOT or interpreter
/// fallback participates on default `jet run` / `jet dev` paths.
pub struct CraneliftBackend;

impl CraneliftBackend {
    pub fn new() -> Self {
        CraneliftBackend
    }

    fn gap_outcome(gap: JitGap, bundle: &ProgramBundle) -> RunOutcome {
        RunOutcome::Problems(vec![e2211_diagnostic(&gap, bundle)])
    }
}

impl JitBackend for CraneliftBackend {
    fn run(&mut self, bundle: &ProgramBundle, _try_anyway: bool) -> RunOutcome {
        match try_resident(bundle) {
            Ok(outcome) => outcome,
            Err(gap) => Self::gap_outcome(gap, bundle),
        }
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        bundle: &ProgramBundle,
        _try_anyway: bool,
    ) -> Result<RunOutcome, Vec<jet_foundation::Diagnostics::Diagnostic>> {
        match try_resident_hot_swap(bundle) {
            Ok(outcome) => Ok(outcome),
            Err(gap) => Err(vec![e2211_diagnostic(&gap, bundle)]),
        }
    }

    fn restart(&mut self, bundle: &ProgramBundle, _try_anyway: bool) -> RunOutcome {
        match try_resident_restart(bundle) {
            Ok(outcome) => outcome,
            Err(gap) => Self::gap_outcome(gap, bundle),
        }
    }
}
