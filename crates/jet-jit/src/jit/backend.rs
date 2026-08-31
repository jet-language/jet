use jet_foundation::{
    JitBackend::{JitBackend, RunOutcome},
    AST::ProgramBundle,
};

use super::api_debug::{
    cranelift_host_supported, try_resident, try_resident_hot_swap, try_resident_restart,
};
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

/// The resident session — the compiled module, the live heap, and the
/// `#Persist` store (D-HOTSWAP1 / D-PERSIST1) — is keyed by *thread*, not by
/// call and not by backend value: `hot_swap` after a `run` on a second
/// `CraneliftBackend` still re-links the same session. So `hot_swap` and
/// `restart` deliberately stay on their caller's thread; hopping each call
/// onto a fresh worker would drop the live state they exist to preserve. A
/// session owner that keeps a session alive across calls (the `jet dev` watch
/// loop, a harness driving swap/restart) installs [`crate::on_compiler_stack`]
/// once around the whole session instead, and re-entrancy makes the boundary
/// below free inside it.
impl JitBackend for CraneliftBackend {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        // `run` is the one-shot entry: it starts a session and finishes it,
        // so it is the one execution seam that can own the sized stack. Both
        // halves of the work below are unbounded-depth recursive descent —
        // `TIR::lower_jit_program` inside `try_resident`, and the whole-program
        // interpreter on the deopt route — so a caller reaching the backend
        // directly gets the same budget the driver's compile entries get.
        crate::on_compiler_stack(|| run_bundle_on_compiler_stack(bundle, try_anyway))
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        bundle: &ProgramBundle,
        _try_anyway: bool,
    ) -> Result<RunOutcome, Vec<jet_foundation::Diagnostics::Diagnostic>> {
        if let Some(diagnostic) = bundle
            .package_guarantees
            .application_authority
            .policy_diagnostic()
        {
            return Err(vec![diagnostic]);
        }
        let resident = crate::with_program_allocator(bundle, || {
            TIR::install_comptime_bridge();
            try_resident_hot_swap(bundle)
        });
        match resident {
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
        if let Some(diagnostic) = bundle
            .package_guarantees
            .application_authority
            .policy_diagnostic()
        {
            return RunOutcome::Problems(vec![diagnostic]);
        }
        let resident = crate::with_program_allocator(bundle, || {
            TIR::install_comptime_bridge();
            try_resident_restart(bundle)
        });
        match resident {
            Ok(outcome) => outcome,
            Err(plan) => {
                note_deopt_invoked_for_test();
                run_whole_interp(bundle, &plan)
            }
        }
    }
}

fn run_bundle_on_compiler_stack(bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
    let _loaded_mod_scope = crate::Mod::LoadScope;
    if let Some(diagnostic) = bundle
        .package_guarantees
        .application_authority
        .policy_diagnostic()
    {
        return RunOutcome::Problems(vec![diagnostic]);
    }
    // A one-shot run has the same fresh process-local Core state as AOT.
    // Hot-swap and restart deliberately skip this boundary and retain state.
    crate::reset_one_shot_core_state();
    let resident = crate::with_program_allocator(bundle, || {
        let _ = try_anyway;
        TIR::install_comptime_bridge();
        if cranelift_host_supported() {
            if let Err(reason) = crate::Ffi::bind_bundle_ffi(bundle) {
                let mut plan = plan_tiers(bundle, None);
                if let Some(gap) = plan.gap.as_mut() {
                    gap.reason = reason;
                }
                return Err(plan);
            }
        }
        try_resident(bundle)
    });
    match resident {
        Ok(outcome) => outcome,
        Err(plan) => {
            note_deopt_invoked_for_test();
            run_whole_interp(bundle, &plan)
        }
    }
}

/// Test helper: classify tiers without executing.
#[doc(hidden)]
pub fn plan_bundle_tiers(bundle: &ProgramBundle) -> super::tiers::TierPlan {
    crate::on_compiler_stack(|| {
        let program = TIR::lower_jit_program(bundle);
        plan_tiers(bundle, program.as_ref())
    })
}
