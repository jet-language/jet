//! c77 (D-JIT1=D) — the stable `JitBackend` execution seam.
//!
//! The `JitBackend` trait and `RunOutcome` live in `jet-foundation`
//! (moved by c139) so the `jet-jit/` workspace member can implement the trait
//! without a dependency cycle. Re-exported here for callers that use the
//! `jet::JitBackend::*` path.

// Re-export the seam types from jet-foundation.
pub use jet_foundation::JitBackend::{JitBackend, RunOutcome};

use crate::Diagnostics::Diagnostic;
use crate::Interpreter::{run_checked, InterpreterInvocation};
use jet_foundation::MIR::{MirArtifactId, MirProgram};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
/// Tier-0 backend: the comptime interpreter. Stateless between runs (no
/// resident heap), so every method funnels into [`run_checked`].
///
/// Used for explicit `jet run --interpret` and `jet dev --interpret`
/// (D-JIT1 / D-LENS-RUN1).
///
/// Explicit interpreter selection is not a JIT fallback. Cranelift's fallback
/// and deopt sites own their trace tripwires, so this backend leaves them clear.
pub struct InterpreterBackend {
    invocation: InterpreterInvocation,
}

impl InterpreterBackend {
    pub fn new(invocation: InterpreterInvocation) -> Self {
        Self { invocation }
    }
}

/// These methods deliberately run on their caller's thread. A resident session
/// keeps `#Persist` state (D-PERSIST1) in thread-local storage seeded while the
/// MIR program is lowered, and `restart` below clears exactly that store, so
/// hopping each call onto a fresh worker would drop persisted bindings between
/// hot swaps. The sized compiler stack therefore wraps the whole session from
/// outside — `Interpreter::dev_run_snapshot` and the `run_*_once` entries — never
/// one call inside it.
impl JitBackend for InterpreterBackend {
    type InvocationPolicy = ReleaseDevtoolsPolicy;

    fn run(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome {
        // A one-shot invocation starts a fresh #Persist store. Resident
        // sessions use `hot_swap` and retain the store until `restart`.
        jet_foundation::Persist::shared_clear();
        jet_jit::reset_one_shot_core_state();
        jet_jit::with_interpreter_ambient(|ambient| {
            jet_jit::register_db_interpreter_ambient(ambient);
            jet_jit::register_raylib_interpreter_ambient(ambient);
            jet_jit::register_ui_interpreter_ambient(ambient);
            jet_jit::register_receipt_interpreter_ambient(ambient);
            jet_jit::register_encoding_interpreter_ambient(ambient);
            jet_jit::register_plugin_interpreter_ambient(ambient);
            jet_jit::register_crypto_interpreter_ambient(ambient);
            if let Some(facts) = program.facts.hardware_profile.clone() {
                jet_jit::register_hardware_interpreter_ambient(
                    ambient,
                    program.facts.hardware_profile_id.clone(),
                    facts,
                );
            }
            run_checked(program, artifact, try_anyway, self.invocation, policy)
        })
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        match jet_jit::with_interpreter_ambient(|ambient| {
            jet_jit::register_db_interpreter_ambient(ambient);
            jet_jit::register_raylib_interpreter_ambient(ambient);
            jet_jit::register_ui_interpreter_ambient(ambient);
            jet_jit::register_receipt_interpreter_ambient(ambient);
            jet_jit::register_encoding_interpreter_ambient(ambient);
            jet_jit::register_plugin_interpreter_ambient(ambient);
            jet_jit::register_crypto_interpreter_ambient(ambient);
            if let Some(facts) = program.facts.hardware_profile.clone() {
                jet_jit::register_hardware_interpreter_ambient(
                    ambient,
                    program.facts.hardware_profile_id.clone(),
                    facts,
                );
            }
            run_checked(program, artifact, try_anyway, self.invocation, policy)
        }) {
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

    fn restart(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome {
        // D-HOTSWAP1 / D-PERSIST1: interpreter restart drops shared persist.
        jet_foundation::Persist::shared_clear();
        jet_jit::with_interpreter_ambient(|ambient| {
            jet_jit::register_db_interpreter_ambient(ambient);
            jet_jit::register_raylib_interpreter_ambient(ambient);
            jet_jit::register_ui_interpreter_ambient(ambient);
            jet_jit::register_receipt_interpreter_ambient(ambient);
            jet_jit::register_encoding_interpreter_ambient(ambient);
            jet_jit::register_plugin_interpreter_ambient(ambient);
            jet_jit::register_crypto_interpreter_ambient(ambient);
            if let Some(facts) = program.facts.hardware_profile.clone() {
                jet_jit::register_hardware_interpreter_ambient(
                    ambient,
                    program.facts.hardware_profile_id.clone(),
                    facts,
                );
            }
            run_checked(program, artifact, try_anyway, self.invocation, policy)
        })
    }
}
