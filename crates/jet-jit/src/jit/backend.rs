use jet_foundation::{
    JitBackend::{JitBackend, RunOutcome},
    MIR::{
        MirAbi, MirAccess, MirArtifactId, MirFunctionId, MirProgram, MirRuntimeValue, MirType,
        MirTypeKind,
    },
};
use crate::SourceResources::SourceResourceSession;
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;

use super::api_debug::{
    cranelift_host_supported, try_resident, try_resident_hot_swap, try_resident_restart,
    try_resident_with_values_and_result,
};
use super::tiers::{record_trace, MirTierPlan};
use super::trace::note_deopt_invoked_for_test;

/// Failure in the Source entry handoff itself. These are compiler/host
/// invariant failures, not Source program diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceDeoptError {
    InvalidRequest(String),
    Callback(String),
    Backend(String),
    Resource(String),
    /// The callback contract is still invalid even when its owned guard
    /// retires; retain that cleanup evidence without treating it as success.
    MissingOutcome { cleanup: Option<String> },
}

impl std::fmt::Display for SourceDeoptError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid Source entry request: {message}"),
            Self::Callback(message) => write!(formatter, "Source entry callback failed: {message}"),
            Self::Backend(message) => write!(formatter, "Source native backend failed: {message}"),
            Self::Resource(message) => write!(formatter, "Source resource activation failed: {message}"),
            Self::MissingOutcome { cleanup } => {
                write!(formatter, "Source entry callback returned no terminal outcome")?;
                if let Some(cleanup) = cleanup {
                    write!(formatter, "; cleanup evidence: {cleanup}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SourceDeoptError {}

/// Which tier consumed a typed Source entry invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceExecutionTier {
    Native,
    Source,
}

pub struct SourceDeoptRun {
    pub outcome: RunOutcome,
    /// Exact recursive Source or native typed return. Native Cranelift
    /// supplies this for every supported scalar/Unit return; unsupported
    /// return metadata never enters the native path.
    pub value: Option<MirRuntimeValue>,
    /// Owned logical Source/Eval session guard. Native runs do not create one.
    pub session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
    pub tier: SourceExecutionTier,
}

/// One checked Source entry execution, including the physical invocation
/// root and the logical session returned by the Source callback.
pub struct SourceEntryExecution {
    pub outcome: RunOutcome,
    pub value: Option<MirRuntimeValue>,
    pub soft_stop: bool,
    /// Cumulative Source output snapshot; never append this to `RunOutcome`.
    pub stdout: String,
    pub stderr: String,
    pub tier: SourceExecutionTier,
    pub resources: SourceResourceSession,
    pub session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
}

/// Failure while explicitly retiring a Source execution after value adoption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceExecutionRetirementError {
    Session(String),
    Resource(String),
}

impl std::fmt::Display for SourceExecutionRetirementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(message) => write!(formatter, "Source session retirement failed: {message}"),
            Self::Resource(message) => write!(formatter, "Source resource retirement failed: {message}"),
        }
    }
}

impl std::error::Error for SourceExecutionRetirementError {}

/// Retire the logical Source session before dropping its physical resource
/// root. The fallback is retained when cleanup does not replace a field.
pub fn retire_source_entry(
    resources: SourceResourceSession,
    session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
    fallback: super::deopt::SourceExecutionRetirement,
) -> Result<super::deopt::SourceExecutionRetirement, SourceExecutionRetirementError> {
    let Some(session) = session else {
        resources
            .retire()
            .map_err(SourceExecutionRetirementError::Resource)?;
        return Ok(fallback);
    };
    let _activation = resources.activate();
    let retirement = session.retire();
    let resource_retirement = resources.retire();
    let retirement = retirement.map_err(SourceExecutionRetirementError::Session)?;
    resource_retirement.map_err(SourceExecutionRetirementError::Resource)?;
    Ok(super::deopt::SourceExecutionRetirement {
        outcome: retirement.outcome.or(fallback.outcome),
        value: retirement.value.or(fallback.value),
        soft_stop: retirement.soft_stop,
        stdout: retirement.stdout,
        stderr: retirement.stderr,
    })
}

impl SourceEntryExecution {
    /// Retire this execution after the caller has adopted its returned value.
    pub fn retire(
        self,
    ) -> Result<super::deopt::SourceExecutionRetirement, SourceExecutionRetirementError> {
        retire_source_entry(
            self.resources,
            self.session,
            super::deopt::SourceExecutionRetirement {
                outcome: Some(self.outcome),
                value: self.value,
                soft_stop: self.soft_stop,
                stdout: self.stdout,
                stderr: self.stderr,
            },
        )
    }
}

/// Tier-1 backend for a checked canonical MIR package.
pub struct CraneliftBackend;

impl CraneliftBackend {
    pub fn new() -> Self {
        CraneliftBackend
    }
    /// Run a checked Source entry through native Cranelift when the checked
    /// entry has a supported scalar ABI. Unsupported recursive/aggregate
    /// carriers take the genuine whole-entry Source path instead.
    pub fn run_with_source_deopt(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        entry_values: Vec<MirRuntimeValue>,
        _try_anyway: bool,
        policy: &ReleaseDevtoolsPolicy,
    ) -> Result<SourceDeoptRun, SourceDeoptError> {
        let entry = program
            .artifacts
            .iter()
            .find(|candidate| candidate.id == artifact)
            .and_then(|candidate| candidate.entry.as_ref())
            .and_then(|entry| entry.function)
            .ok_or_else(|| {
                SourceDeoptError::InvalidRequest(format!(
                    "artifact {artifact:?} has no checked entry function"
                ))
            })?;
        let function = program
            .functions
            .iter()
            .find(|candidate| candidate.id == entry)
            .ok_or_else(|| {
                SourceDeoptError::InvalidRequest(format!(
                    "Source entry function {entry:?} is missing"
                ))
            })?;
        if function.params.len() != entry_values.len() {
            return Err(SourceDeoptError::InvalidRequest(format!(
                "Source entry expected {} checked parameters, got {} typed values",
                function.params.len(),
                entry_values.len()
            )));
        }
        let plan = super::tiers::plan_mir_tiers(program, artifact);
        if cranelift_host_supported()
            && plan.deopt.is_empty()
            && native_entry_values_supported(function, &entry_values)
            && native_return_supported(&function.return_type)
        {
            if let Ok((outcome, value)) = try_resident_with_values_and_result(
                program,
                artifact,
                &entry_values,
                &function.return_type,
                policy,
            ) {
                return Ok(SourceDeoptRun {
                    outcome,
                    value: Some(value),
                    session: None,
                    tier: SourceExecutionTier::Native,
                });
            }
        }

        let request = super::deopt::source_entry_deopt_request(
            program,
            artifact,
            entry,
            entry_values,
        )
        .map_err(SourceDeoptError::InvalidRequest)?;
        let reply = super::deopt::dispatch_source_deopt(request)
            .map_err(SourceDeoptError::Callback)?;
        let super::deopt::SourceDeoptReply {
            bits,
            outcome,
            value,
            session,
            soft_stop,
            stdout,
            stderr,
        } = reply;
        if let Some(outcome) = outcome {
            let reply = super::deopt::SourceDeoptReply {
                bits,
                outcome: Some(outcome.clone()),
                value,
                session,
                soft_stop,
                stdout,
                stderr,
            };
            super::deopt::publish_source_deopt_reply(&reply, false);
            return Ok(SourceDeoptRun {
                outcome,
                value: reply.value,
                session: reply.session,
                tier: SourceExecutionTier::Source,
            });
        }
        let cleanup = match session {
            Some(session) => match session.retire() {
                Ok(retirement) => Some(format!(
                    "guard retirement completed (outcome={}, value={}, soft_stop={})",
                    retirement.outcome.is_some(),
                    retirement.value.is_some(),
                    retirement.soft_stop
                )),
                Err(error) => Some(format!("internal Source session retirement error: {error}")),
            },
            None => None,
        };
        Err(SourceDeoptError::MissingOutcome { cleanup })
    }
    /// Prove that an HTTP/1.1 worker cannot outlive the resident JIT image.
    ///
    /// This deliberately goes through the resident compiler and the Prelude
    /// server adapters.  The hooks only provide barriers for the proof: the
    /// callback, shutdown, worker join, and runtime teardown are all real
    /// paths.
    pub fn http_worker_runtime_lifetime_proof_for_test(
        &self,
        program: &MirProgram,
        artifact: MirArtifactId,
        handler_name: &str,
    ) -> Result<(), String> {
        crate::on_compiler_stack(|| {
            use std::io::Write;
            use std::net::TcpStream;
            use std::sync::mpsc;
            use std::sync::{Arc, Mutex};
            use std::thread;
            use std::time::Duration;

            if !cranelift_host_supported() {
                return Err("Cranelift host path is unsupported on this architecture".to_string());
            }

            let function_id = program
                .functions
                .iter()
                .find(|function| {
                    function.name == handler_name || function.key == handler_name
                })
                .map(|function| function.id)
                .ok_or_else(|| format!("MIR handler `{handler_name}` is missing"))?;

            super::Concurrency::set_http_test_handler_hook(None);
            super::Concurrency::set_http_test_shutdown_hook(None);
            super::resident::resident_teardown();

            let policy = ReleaseDevtoolsPolicy::development();
            if let Err(error) =
                super::resident::ensure_resident_module(program, artifact, &policy)
            {
                super::resident::resident_teardown();
                return Err(error);
            }

            let handler_func = match super::RESIDENT_MODULE.with(|mod_slot| {
                let mut mod_guard = mod_slot.borrow_mut();
                let resident = mod_guard
                    .as_mut()
                    .ok_or_else(|| "resident module missing".to_string())?;
                super::RESIDENT_RUNTIME.with(|rt_slot| {
                    let mut rt_guard = rt_slot.borrow_mut();
                    let runtime = rt_guard
                        .as_mut()
                        .ok_or_else(|| "resident runtime missing".to_string())?;
                    super::functions_compile::compile_mir_function(
                        &mut resident.module,
                        &resident.host,
                        program,
                        function_id,
                        runtime,
                    )
                })
            }) {
                Ok(function) => function,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let handler_ptr = match super::RESIDENT_MODULE.with(|mod_slot| {
                let mut mod_guard = mod_slot.borrow_mut();
                let resident = mod_guard
                    .as_mut()
                    .ok_or_else(|| "resident module missing".to_string())?;
                resident
                    .module
                    .finalize_definitions()
                    .map_err(|error| error.to_string())?;
                let ptr = resident.module.get_finalized_function(handler_func);
                if ptr.is_null() {
                    Err("resident HTTP handler has no finalized function address".to_string())
                } else {
                    Ok(ptr as i64)
                }
            }) {
                Ok(ptr) => ptr,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let callable = match super::RESIDENT_RUNTIME.with(|rt_slot| {
                let mut rt_guard = rt_slot.borrow_mut();
                let runtime = rt_guard
                    .as_mut()
                    .ok_or_else(|| "resident runtime missing".to_string())?;
                let handle = super::runtime_host::bind_jit_callable_handle(
                    runtime,
                    handler_ptr,
                    0,
                    false,
                );
                let runtime_ptr = runtime as *mut super::JitRuntime;
                super::Concurrency::set_active_runtime(Some(runtime_ptr));
                if handle == 0 {
                    Err("resident HTTP handler callable binding failed".to_string())
                } else {
                    Ok(handle)
                }
            }) {
                Ok(handle) => handle,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let mux = super::net_http_rt::test_http_mux();
            let handler = super::net_http_rt::test_capture_http_handler(callable);
            if let Err(error) =
                super::net_http_rt::test_http_mux_add_handler(mux, "GET", "/", &handler)
            {
                super::resident::resident_teardown();
                return Err(error);
            }
            let server = match super::net_http_rt::test_http_server_bind(
                "127.0.0.1:0".to_string(),
                mux,
            ) {
                Ok(server) => server,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };
            let address = match super::net_http_rt::test_http_server_local_addr(server) {
                Ok(address) => address,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let (entry_tx, entry_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release_rx = Arc::new(Mutex::new(release_rx));
            super::Concurrency::set_http_test_handler_hook(Some(Arc::new(move || {
                let _ = entry_tx.send(());
                let _ = release_rx.lock().ok().and_then(|receiver| receiver.recv().ok());
            })));

            let (shutdown_tx, shutdown_rx) = mpsc::channel();
            super::Concurrency::set_http_test_shutdown_hook(Some(Arc::new(move || {
                let _ = shutdown_tx.send(());
            })));

            let serve_thread = thread::spawn(move || {
                super::net_http_rt::test_http_server_serve(server)
            });
            let cleanup_on_error = |release_tx: &mpsc::Sender<()>,
                                    serve_thread: thread::JoinHandle<Result<(), String>>| {
                let _ = release_tx.send(());
                super::Concurrency::set_http_test_handler_hook(None);
                super::Concurrency::set_http_test_shutdown_hook(None);
                super::resident::resident_teardown();
                let _ = serve_thread.join();
            };

            let mut client = match TcpStream::connect(&address) {
                Ok(client) => client,
                Err(error) => {
                    cleanup_on_error(&release_tx, serve_thread);
                    return Err(format!("HTTP lifetime proof client connect failed: {error}"));
                }
            };
            if let Err(error) = client.set_read_timeout(Some(Duration::from_secs(5))) {
                cleanup_on_error(&release_tx, serve_thread);
                return Err(format!("HTTP lifetime proof client setup failed: {error}"));
            }
            if let Err(error) = client.write_all(
                b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            ) {
                cleanup_on_error(&release_tx, serve_thread);
                return Err(format!("HTTP lifetime proof request failed: {error}"));
            }

            if entry_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                cleanup_on_error(&release_tx, serve_thread);
                return Err("HTTP worker never entered the JIT handler".to_string());
            }
            if super::Concurrency::runtime_access_available_for_test() {
                cleanup_on_error(&release_tx, serve_thread);
                return Err(
                    "HTTP worker handler entry did not hold the runtime access guard".to_string(),
                );
            }

            let (lock_held_tx, lock_held_rx) = mpsc::channel();
            let release_thread = thread::spawn(move || {
                let lock_held = match shutdown_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) => !super::Concurrency::runtime_access_available_for_test(),
                    Err(_) => false,
                };
                let _ = lock_held_tx.send(lock_held);
                let _ = release_tx.send(());
            });

            super::resident::resident_teardown();
            super::Concurrency::set_http_test_handler_hook(None);
            super::Concurrency::set_http_test_shutdown_hook(None);

            let shutdown_saw_guard = lock_held_rx
                .recv()
                .map_err(|_| "HTTP shutdown proof coordinator failed".to_string())?;
            release_thread
                .join()
                .map_err(|_| "HTTP shutdown proof coordinator panicked".to_string())?;
            if !shutdown_saw_guard {
                let _ = serve_thread.join();
                return Err(
                    "HTTP shutdown did not observe the worker runtime access guard".to_string(),
                );
            }

            serve_thread
                .join()
                .map_err(|_| "HTTP server thread panicked".to_string())?
                .map_err(|error| format!("HTTP server failed during lifetime proof: {error}"))?;
            drop(client);
            Ok(())
        })
    }
    /// Exercise the Prelude HTTP/2 dispatch ownership and drain boundary.
    pub fn http2_dispatch_drain_proof_for_test(&self) -> Result<(), String> {
        super::net_http_rt::test_http2_dispatch_drain()
    }
}

/// Execute one checked whole-entry Source request through the native tier or
/// the active Source callback. This is the shared entry executor used by both
/// generated bootstrap glue and the RuntimeMir adapter; it owns callback
/// activation, frame-state isolation, and the physical Source arena lease.
pub fn execute_source_entry<Resume>(
    program: &MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    resources: SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    policy: &ReleaseDevtoolsPolicy,
    resume: Resume,
) -> Result<SourceEntryExecution, SourceDeoptError>
where
    Resume: Fn(super::deopt::SourceDeoptRequest) -> Result<super::deopt::SourceDeoptReply, String>
        + 'static,
{
    program
        .validate()
        .map_err(|error| SourceDeoptError::InvalidRequest(format!("checked Source MIR is invalid: {error}")))?;
    let artifact_entry = program
        .artifacts
        .iter()
        .find(|candidate| candidate.id == artifact)
        .and_then(|candidate| candidate.entry.as_ref())
        .and_then(|entry| entry.function)
        .ok_or_else(|| {
            SourceDeoptError::InvalidRequest(format!(
                "artifact {artifact:?} has no checked entry function"
            ))
        })?;
    if artifact_entry != entry_function {
        return Err(SourceDeoptError::InvalidRequest(format!(
            "Source entry function {entry_function:?} does not match artifact {artifact:?} entry {artifact_entry:?}"
        )));
    }
    if resources
        .arena()
        .is_retired()
        .map_err(SourceDeoptError::Resource)?
    {
        return Err(SourceDeoptError::Resource(
            "Source resource session is retired".to_string(),
        ));
    }
    let _activation = resources.activate();
    let mut backend = CraneliftBackend::new();
    let native_run = super::deopt::with_source_deopt_scope(resume, || {
        backend.run_with_source_deopt(program, artifact, entry_values, false, policy)
    })?;
    let native_output = run_outcome_output(&native_run.outcome);
    let native_outcome = native_run.outcome;
    let native_value = native_run.value;
    let native_tier = native_run.tier;
    let session = native_run.session;
    let (outcome, soft_stop, value, stdout, stderr, tier) =
        match super::deopt::take_source_deopt_result_with_output() {
            Some((outcome, soft_stop, value, stdout, stderr)) => (
                outcome,
                soft_stop,
                value,
                stdout,
                stderr,
                SourceExecutionTier::Source,
            ),
            None => (
                native_outcome,
                false,
                native_value,
                native_output.0,
                native_output.1,
                native_tier,
            ),
        };
    Ok(SourceEntryExecution {
        outcome,
        value,
        soft_stop,
        stdout,
        stderr,
        tier,
        resources,
        session,
    })
}

fn run_outcome_output(outcome: &RunOutcome) -> (String, String) {
    match outcome {
        RunOutcome::Ran { stdout, stderr, .. } => (stdout.clone(), stderr.clone()),
        RunOutcome::Problems(_) => (String::new(), String::new()),
    }
}


fn integer_fits(kind: &MirTypeKind, raw: i64) -> bool {
    match kind {
        MirTypeKind::IntN { signed, bits } => {
            if *bits == 0 || *bits > 64 {
                return false;
            }
            if *signed {
                *bits == 64
                    || {
                        let half = 1_i128 << (*bits - 1);
                        let raw = i128::from(raw);
                        (-half..half).contains(&raw)
                    }
            } else if *bits == 64 {
                raw >= 0
            } else {
                let limit = 1_i128 << *bits;
                (0..limit).contains(&i128::from(raw))
            }
        }
        MirTypeKind::InlineRange { base, lo, hi } => {
            integer_fits(&base.kind, raw) && (*lo..=*hi).contains(&raw)
        }
        MirTypeKind::Tagged { inner, .. } => integer_fits(&inner.kind, raw),
        MirTypeKind::Quantity { base, .. } => integer_fits(&base.kind, raw),
        _ => true,
    }
}
fn native_entry_values_supported(
    function: &jet_foundation::MIR::MirFunction,
    values: &[MirRuntimeValue],
) -> bool {
    if !function.capture_params.is_empty()
        || function.params.len() != values.len()
        || function.params.len() > 8
    {
        return false;
    }
    !function.params.iter().any(|parameter| {
        parameter.access == MirAccess::Write || clif_ty_from_mir(&parameter.ty).is_none()
    })
}

fn native_return_supported(ty: &MirType) -> bool {
    clif_ty_from_mir(ty).is_some() && !matches!(ty.layout.abi, MirAbi::Function | MirAbi::Never)
}

fn native_float_word(value: &MirRuntimeValue, f32_value: bool) -> Option<i64> {
    match (f32_value, value) {
        (
            false,
            MirRuntimeValue::Float {
                value,
                f32: false,
            },
        ) => Some(value.to_bits() as i64),
        (
            true,
            MirRuntimeValue::Float {
                value,
                f32: true,
            },
        ) => Some((*value as f32).to_bits() as i64),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_entry_float_words_preserve_declared_bit_width() {
        let f64_bits = 0x3ff8_0000_0000_0042_u64;
        assert_eq!(
            native_float_word(
                &MirRuntimeValue::Float {
                    value: f64::from_bits(f64_bits),
                    f32: false,
                },
                false,
            ),
            Some(f64_bits as i64)
        );
        let f32_bits = 0x3fc0_1234_u32;
        assert_eq!(
            native_float_word(
                &MirRuntimeValue::Float {
                    value: f32::from_bits(f32_bits) as f64,
                    f32: true,
                },
                true,
            ),
            Some(f32_bits as i64)
        );
    }

    #[test]
    fn native_entry_integer_words_reject_checked_range_overflow() {
        let signed = MirTypeKind::IntN {
            signed: true,
            bits: 8,
        };
        assert!(integer_fits(&signed, -128));
        assert!(integer_fits(&signed, 127));
        assert!(!integer_fits(&signed, 128));

        let unsigned = MirTypeKind::IntN {
            signed: false,
            bits: 8,
        };
        assert!(integer_fits(&unsigned, 255));
        assert!(!integer_fits(&unsigned, -1));
        assert!(!integer_fits(&unsigned, 256));
    }
}

