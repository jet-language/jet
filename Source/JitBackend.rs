//! c77 (D-JIT1=D) — the evaluator execution seam behind the in-process dev
//! loop (`jet dev --canvas`).
//!
//! The `JitBackend` trait and `RunOutcome` live in `jet-foundation` so the
//! evaluator host crate shares them without a dependency cycle. Re-exported
//! here for callers that use the `jet::JitBackend::*` path.

// Re-export the seam types from jet-foundation.
pub use jet_foundation::JitBackend::{JitBackend, RunOutcome};

use crate::Diagnostics::Diagnostic;
use crate::Interpreter::run_checked;
use jet_foundation::MIR::{MirArtifactId, MirProgram};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
/// The evaluator backend. Stateless between runs (no resident heap), so every
/// method funnels into [`run_checked`].
///
/// Used by the in-process dev loop (D-JIT1); `jet run` and the default
/// `jet dev` build and execute instead (D-EXEC2=A).
pub struct InterpreterBackend;

impl InterpreterBackend {
    /// Bind the program's native bridge and run it with every canonical
    /// ambient host registered.
    fn run_with_ambient(
        &self,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &ReleaseDevtoolsPolicy,
    ) -> RunOutcome {
        if let Err(error) = jet_evalhost::bind_interpreter_ffi(program, artifact) {
            return RunOutcome::Problems(vec![interpreter_bridge_failure(error)]);
        }
        jet_evalhost::with_interpreter_ambient(|ambient| {
            jet_evalhost::register_db_interpreter_ambient(ambient);
            jet_evalhost::register_raylib_interpreter_ambient(ambient);
            jet_evalhost::register_ui_interpreter_ambient(ambient);
            jet_evalhost::register_receipt_interpreter_ambient(ambient);
            jet_evalhost::register_encoding_interpreter_ambient(ambient);
            jet_evalhost::register_plugin_interpreter_ambient(ambient);
            jet_evalhost::register_crypto_interpreter_ambient(ambient);
            if let Some(facts) = program.facts.hardware_profile.clone() {
                jet_evalhost::register_hardware_interpreter_ambient(
                    ambient,
                    program.facts.hardware_profile_id.clone(),
                    facts,
                );
            }
            run_checked(program, artifact, try_anyway, policy)
        })
    }
}
fn interpreter_bridge_failure(error: String) -> Diagnostic {
    Diagnostic::runtime_host_fault(String::new(), format!("interpreter FFI bridge failed to load: {error}"))
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
        jet_evalhost::reset_one_shot_core_state();
        let outcome = self.run_with_ambient(program, artifact, try_anyway, policy);
        jet_evalhost::release_one_shot_core_state();
        outcome
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        program: &MirProgram,
        artifact: MirArtifactId,
        try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        match self.run_with_ambient(program, artifact, try_anyway, policy) {
            RunOutcome::Problems(diags) => Err(diags),
            ran => Ok(ran),
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
        self.run_with_ambient(program, artifact, try_anyway, policy)
    }
}
