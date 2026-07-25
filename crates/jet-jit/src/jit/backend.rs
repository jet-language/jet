use jet_foundation::{
    JitBackend::{JitBackend, RunOutcome},
    AST::ProgramBundle,
};

use super::api_debug::{try_resident, try_resident_hot_swap, try_resident_restart};
use super::deopt::run_whole_interp;
use super::tiers::plan_tiers;
use super::trace::note_deopt_invoked_for_test;
use jet_codegen::Codegen::TIR;

/// c139 / #778 tiered JIT backend — Cranelift when covered, interpreter deopt
/// on named gaps (D-ONECORE1=A / D-LENS-RUN2=A). E2211 retired; silent deopt.
pub struct CraneliftBackend;

impl CraneliftBackend {
    pub fn new() -> Self {
        CraneliftBackend
    }
}

impl JitBackend for CraneliftBackend {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        let _ = try_anyway;
        TIR::install_comptime_bridge();
        match try_resident(bundle) {
            Ok(outcome) => outcome,
            Err(plan) => {
                note_deopt_invoked_for_test();
                run_whole_interp(bundle, &plan)
            }
        }
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        bundle: &ProgramBundle,
        _try_anyway: bool,
    ) -> Result<RunOutcome, Vec<jet_foundation::Diagnostics::Diagnostic>> {
        TIR::install_comptime_bridge();
        match try_resident_hot_swap(bundle) {
            Ok(outcome) => Ok(outcome),
            Err(plan) => {
                note_deopt_invoked_for_test();
                match run_whole_interp(bundle, &plan) {
                    RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    } => Ok(RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    }),
                    RunOutcome::Problems(diags) => Err(diags),
                }
            }
        }
    }

    fn restart(&mut self, bundle: &ProgramBundle, _try_anyway: bool) -> RunOutcome {
        TIR::install_comptime_bridge();
        match try_resident_restart(bundle) {
            Ok(outcome) => outcome,
            Err(plan) => {
                note_deopt_invoked_for_test();
                run_whole_interp(bundle, &plan)
            }
        }
    }
}

/// Test helper: classify tiers without executing.
#[doc(hidden)]
pub fn plan_bundle_tiers(bundle: &ProgramBundle) -> super::tiers::TierPlan {
    let program = TIR::lower_jit_program(bundle);
    plan_tiers(bundle, program.as_ref())
}
