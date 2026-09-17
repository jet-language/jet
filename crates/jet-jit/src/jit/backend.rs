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

