use jet_foundation::{
    JitBackend::{JitBackend, RunOutcome},
    MIR::{MirArtifactId, MirProgram},
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;

use super::api_debug::{
    cranelift_host_supported, try_resident, try_resident_hot_swap, try_resident_restart,
};
use super::tiers::{record_trace, MirTierPlan};
use super::trace::note_deopt_invoked_for_test;

/// Tier-1 backend for a checked canonical MIR package.
pub struct CraneliftBackend;

impl CraneliftBackend {
    pub fn new() -> Self {
        CraneliftBackend
    }
}

fn plan_failure(plan: &MirTierPlan) -> RunOutcome {
    note_deopt_invoked_for_test();
    // Publish the planner's own rows before converting the tier failure to
    // the ordinary runtime diagnostic.  `record_trace` is the sole notice
    // channel and suppresses the concise notice when expert tracing is on.
    record_trace(plan.rows.clone());
    let detail = plan
        .gap
        .as_ref()
        .map(|gap| {
            format!(
                "Cranelift cannot execute MIR function `{}`: {}",
                gap.function_name, gap.reason
            )
        })
        .unwrap_or_else(|| "Cranelift cannot execute the checked MIR program".to_string());
    RunOutcome::Problems(vec![jet_foundation::Diagnostics::Diagnostic::runtime_host_fault(
        String::new(),
        detail,
    )])
}

fn plan_failure_diagnostics(plan: &MirTierPlan) -> Vec<jet_foundation::Diagnostics::Diagnostic> {
    match plan_failure(plan) {
        RunOutcome::Problems(diagnostics) => diagnostics,
        RunOutcome::Ran { .. } => Vec::new(),
    }
}

impl JitBackend for CraneliftBackend {
    type InvocationPolicy = ReleaseDevtoolsPolicy;

    fn run(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        _try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome {
        crate::on_compiler_stack(|| {
            crate::reset_one_shot_core_state();
            if !cranelift_host_supported() {
                return plan_failure(&super::tiers::plan_mir_tiers(program, artifact));
            }
            match try_resident(program, artifact, policy) {
                Ok(outcome) => outcome,
                Err(plan) => plan_failure(&plan),
            }
        })
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        program: &MirProgram,
        artifact: MirArtifactId,
        _try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> Result<RunOutcome, Vec<jet_foundation::Diagnostics::Diagnostic>> {
        match try_resident_hot_swap(program, artifact, policy) {
            Ok(outcome) => Ok(outcome),
            Err(plan) => Err(plan_failure_diagnostics(&plan)),
        }
    }

    fn restart(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        _try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome {
        match try_resident_restart(program, artifact, policy) {
            Ok(outcome) => outcome,
            Err(plan) => plan_failure(&plan),
        }
    }
}

