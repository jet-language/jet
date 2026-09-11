use cranelift_module::Module;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use jet_foundation::{
    HotSwap::HotSwapDecision,
    JitBackend::RunOutcome,
    MIR::{
        MirArtifactId, MirDecisionLedger, MirDecisionRow, MirFailureCarrier, MirFunction,
        MirFunctionId, MirProgram, MirRuntimeValue, MirTypeKind,
    },
    SchemaMigration::SchemaMigrationReceipt,
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;

use super::deopt::clear_deopt_state;
use super::functions_compile::compile_program;
use super::runtime_host::{new_jit_module, ResidentModule};
use super::safety::artifact_entry;
use super::tiers::{runtime_decision_rows, TierRow};
use super::{Concurrency, JitRuntime, RESIDENT_MODULE, RESIDENT_RUNTIME};
use crate::net_http_rt::{console_http_router_from_mux, ConsoleHttpRouter};
use crate::DB::ConsoleDbResource;

thread_local! {
    static RESIDENT_DECISION_LEDGER: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

fn publish_runtime_rows(rows: Vec<MirDecisionRow>) {
    if rows.is_empty() || std::env::var("JET_OBSERVE").ok().as_deref() != Some("1") {
        return;
    }
    let payload = RESIDENT_DECISION_LEDGER.with(|slot| {
        let current = slot.borrow().clone();
        let mut ledger = current
            .as_deref()
            .and_then(|value| MirDecisionLedger::from_json(value).ok());
        if ledger.is_none() {
            return None;
        }
        let mut ledger = ledger.take().expect("checked resident ledger");
        let mut all_rows = std::mem::take(&mut ledger.rows);
        all_rows.extend(rows);
        let payload = MirDecisionLedger::from_rows(ledger.identity, all_rows).canonical_json();
        *slot.borrow_mut() = Some(payload.clone());
        Some(payload)
    });
    if let Some(payload) = payload {
        jet_codegen::scheduler::jet_observe_record_decision_ledger_json(payload);
    }
}

pub(crate) fn publish_runtime_decisions(
    program: &MirProgram,
    _artifact: MirArtifactId,
    observed_rows: &[TierRow],
) {
    if observed_rows.is_empty() || std::env::var("JET_OBSERVE").ok().as_deref() != Some("1") {
        return;
    }
    publish_runtime_rows(runtime_decision_rows(program, observed_rows));
}

pub(crate) fn publish_runtime_deopt(function: MirFunctionId, reason: &str) {
    if std::env::var("JET_OBSERVE").ok().as_deref() != Some("1") {
        return;
    }
    let _ = (function, reason);
}

fn entry_function<'a>(program: &'a MirProgram, artifact: MirArtifactId) -> Option<&'a MirFunction> {
    let id = artifact_entry(program, artifact)?;
    program.functions.iter().find(|function| function.id == id)
}

fn main_returns_result(program: &MirProgram, artifact: MirArtifactId) -> bool {
    matches!(
        entry_function(program, artifact).map(|function| &function.failure),
        Some(MirFailureCarrier::Result { .. })
    )
}

fn main_returns_app(program: &MirProgram, artifact: MirArtifactId) -> bool {
    let Some(function) = entry_function(program, artifact) else {
        return false;
    };
    match function.return_type.kind() {
        MirTypeKind::Apply { name, .. } => {
            name.name.contains("App") || name.name.contains("Page")
        }
        _ => false,
    }
}

fn main_returns_default_err(program: &MirProgram, artifact: MirArtifactId) -> bool {
    match entry_function(program, artifact).map(|function| &function.failure) {
        Some(MirFailureCarrier::Result { error, .. }) => matches!(
            error.kind(),
            MirTypeKind::Apply { name, .. } if name.name == "Error"
        ),
        _ => false,
    }
}

fn main_error_type(program: &MirProgram, artifact: MirArtifactId) -> Option<String> {
    match entry_function(program, artifact).map(|function| &function.failure) {
        Some(MirFailureCarrier::Result { error, .. }) => Some(error.canonical_key()),
        _ => None,
    }
}

fn main_error_is_packed(_program: &MirProgram, _artifact: MirArtifactId) -> bool {
    false
}

pub(crate) fn fresh_runtime(release_devtools_policy: ReleaseDevtoolsPolicy) -> JitRuntime {
    fresh_runtime_with_allocator_cap(release_devtools_policy, None)
}

pub(crate) fn fresh_runtime_with_allocator_cap(
    release_devtools_policy: ReleaseDevtoolsPolicy,
    cap_bytes: Option<u64>,
) -> JitRuntime {
    JitRuntime {
        release_devtools_policy: release_devtools_policy,
        atomics: Vec::new(),
        source_file: String::new(),
        source_text: String::new(),
        current_function: String::new(),
        current_line: 0,
        current_source_line: String::new(),
        source_frames: Vec::new(),
        stdout: String::new(),
        stderr: String::new(),
        heap: jet_rt::JetArena::default(),
        data_loaders: Vec::new(),
        model_outputs: Vec::new(),
        model_sessions: Vec::new(),
        int_list_views: Vec::new(),
        mapped_files: Vec::new(),
        file_scopes: Vec::new(),
        mapped_views: Vec::new(),
        view_slots: Vec::new(),
        lazy_iters: Vec::new(),
        type_descriptors: HashMap::new(),
        type_descriptor_names: HashMap::new(),
        trait_object_types: HashMap::new(),
        dma_types: HashMap::new(),
        dma_transfers: HashMap::new(),
        iterable_hooks: HashMap::new(),
        hardware_host: None,
        hardware_setups: Vec::new(),
        hardware_handlers: Default::default(),
        pattern_descriptors: Vec::new(),
        program_allocator: std::sync::Arc::new(cap_bytes.map_or_else(
            jet_codegen::program_allocator::JetProgramAllocator::system,
            jet_codegen::program_allocator::JetProgramAllocator::counting,
        )),
        compute: crate::Compute::ComputeState::default(),
        compile_strings: Vec::new(),
        task_labels: Vec::new(),
        zip_plans: Vec::new(),
        invocations: 0,
        hot_swap_plan: None,
        memo_values: HashMap::new(),
        memo_functions: HashMap::new(),
        channels: Vec::new(),
        loop_cursors: Vec::new(),
        senders: Vec::new(),
        stream_consumers: HashMap::new(),
        stream_producers: HashMap::new(),
        stream_senders: HashMap::new(),
        stream_event_consumers: HashMap::new(),
        stream_keyed_consumers: HashMap::new(),
        stream_windows: HashMap::new(),
        next_stream_channel: -1,
        next_stream_sender: -1,
        next_option_lift2_thunk: 0,
        next_shared_txn_thunk: 0,
        jit_callables: Vec::new(),
        atexit_handlers: Vec::new(),
        tasks: Vec::new(),
        task_controls: Vec::new(),
        task_groups: Vec::new(),
        cells: crate::Cell::CellState::new(),
        results: Vec::new(),
        errors: Vec::new(),
        solvers: Vec::new(),
        rngs: Vec::new(),
        history_rngs: Vec::new(),
        history_provenance: None,
        history_callable_targets: HashMap::new(),
        fakes: Vec::new(),
        clocks: Vec::new(),
        process_specs: Vec::new(),
        process_children: Vec::new(),
        sketches: Vec::new(),
        args_specs: Vec::new(),
        args_parsed: Vec::new(),
        file_readers: Vec::new(),
        file_writers: Vec::new(),
        json_readers: Vec::new(),
        json_writers: Vec::new(),
        jsonl_readers: Vec::new(),
        jsonl_writers: Vec::new(),
        csv_readers: Vec::new(),
        csv_writers: Vec::new(),
        xml_readers: Vec::new(),
        xml_writers: Vec::new(),
        cbor_readers: Vec::new(),
        cbor_writers: Vec::new(),
        data_streams: Vec::new(),
        data_plots: Vec::new(),
        job_queues: Vec::new(),
        sets: Vec::new(),
        set_string_kinds: Vec::new(),
        deques: Vec::new(),
        bags: Vec::new(),
        sorted_sets: Vec::new(),
        sorted_set_string_kinds: Vec::new(),
        priority_queues: Vec::new(),
        lrus: Vec::new(),
        bit_sets: Vec::new(),
        byte_buffers: Vec::new(),
        allocators: Vec::new(),
        allocator_views: Vec::new(),
        gc_roots: Vec::new(),
        gc_edges: Vec::new(),
        pools: Vec::new(),
        shareds: Vec::new(),
        conditions: Vec::new(),
        shared_guard_states: HashMap::new(),
        expirings: Vec::new(),
        secrets: Vec::new(),
        crypto_values: Vec::new(),
        net_values: Vec::new(),
        service_callbacks: HashMap::new(),
        job_adapters: HashMap::new(),
        service_values: Vec::new(),
        game_scenes: Vec::new(),
        game_frames: Vec::new(),
        game_replays: Vec::new(),
        game_backends: Vec::new(),
        raylib_windows: Vec::new(),
        raylib_colors: Vec::new(),
        raylib_sounds: Vec::new(),
        raylib_atlases: Vec::new(),
        raylib_draw_calls: Vec::new(),
        time_values: Vec::new(),
        realtime_values: Vec::new(),
        regex_values: Vec::new(),
        decimal_values: Vec::new(),
        fraction_values: Vec::new(),
        complex_values: Vec::new(),
        trapped_flag: AtomicBool::new(false),
        trapped: None,
        host_fault: false,
        host_fault_payload_captured: false,
        exit_code: None,
        deadline_exceeded: None,
        readers: Vec::new(),
        cursors: Vec::new(),
        reflect_values: Vec::new(),
        layout_slots: Vec::new(),
        reactive: crate::Reactive::ReactiveState::default(),
        ui: crate::Ui::UiState::default(),
        web: crate::Web::WebState::default(),
    }
}
fn reset_run_heap(rt: &mut JitRuntime) {
    let compile_strings = rt.compile_strings.clone();
    rt.heap.clear();
    rt.int_list_views.clear();
    rt.view_slots.clear();
    rt.clear_lazy_iters();
    rt.program_allocator.release_hosted_reservations();
    rt.heap.install_string_slots(&compile_strings);
    rt.compute.clear();
    rt.memo_values.clear();
    crate::Math::clear_math_values();
    let _ = std::mem::take(&mut rt.stream_consumers);
    let _ = std::mem::take(&mut rt.stream_producers);
    let _ = std::mem::take(&mut rt.stream_senders);
    rt.next_stream_channel = -1;
    rt.next_stream_sender = -1;
    rt.source_frames.clear();
    rt.current_line = 0;
    rt.current_function.clear();
    rt.current_source_line.clear();
    rt.host_fault = false;
    rt.host_fault_payload_captured = false;
    rt.jit_callables.clear();
    rt.atexit_handlers.clear();
    rt.channels.clear();
    rt.senders.clear();
    rt.tasks.clear();
    rt.task_controls.clear();
    rt.task_groups.clear();
    rt.results.clear();
    rt.errors.clear();
    rt.solvers.clear();
    rt.rngs.clear();
    rt.fakes.clear();
    rt.clocks.clear();
    rt.sketches.clear();
    rt.process_specs.clear();
    rt.process_children.clear();
    rt.cells = crate::Cell::CellState::new();
    rt.exit_code = None;
}

fn take_host_fault_outcome(runtime: &mut JitRuntime) -> Option<RunOutcome> {
    if !std::mem::take(&mut runtime.host_fault) {
        return None;
    }
    let payload_captured = std::mem::take(&mut runtime.host_fault_payload_captured);
    let what = if payload_captured {
        runtime
            .take_trap()
            .unwrap_or_else(|| "the JIT runtime helper failed".to_string())
    } else {
        runtime.take_trap();
        "the JIT runtime helper failed".to_string()
    };
    runtime.exit_code.take();
    let stdout = runtime.stdout.clone();
    reset_run_heap(runtime);
    Some(RunOutcome::Problems(vec![
        jet_foundation::Diagnostics::Diagnostic::runtime_host_fault(stdout, what),
    ]))
}

pub(crate) fn resident_teardown() {
    crate::CoreHost::reset_jit_interrupts();
    crate::Mod::clear();
    clear_deopt_state();
    crate::Collections::clear_packed_enum_show();
    crate::Watcher::clear_watcher_state();
    crate::Net::clear_net_state();
    crate::net_http_rt::clear_net_http_handles();
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    Concurrency::set_active_runtime(None);
    Concurrency::clear_http_shared_runtime();
}

pub(crate) fn ensure_resident_module(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<(), String> {
    let main_returns_result = main_returns_result(program, artifact);
    let main_returns_default_err = main_returns_default_err(program, artifact);
    let main_error_type = main_error_type(program, artifact);
    let main_error_is_packed = main_error_is_packed(program, artifact);
    let main_returns_app = main_returns_app(program, artifact);
    let need_create = RESIDENT_MODULE.with(|slot| slot.borrow().is_none());
    if need_create {
        let (mut module, host) = new_jit_module()?;
        let mut runtime = RESIDENT_RUNTIME
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(|| fresh_runtime(release_devtools_policy.clone()));
        let compiled = compile_program(&mut module, &host, program, artifact, &mut runtime)?;
        runtime.snapshot_compile_strings();
        RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
        RESIDENT_MODULE.with(|slot| {
            *slot.borrow_mut() = Some(ResidentModule {
                module,
                host,
                main_id: compiled.entry_id,
                main_returns_result,
                main_returns_app,
                main_returns_default_err,
                main_error_type,
                main_error_is_packed,
            });
        });
        return Ok(());
    }

    RESIDENT_MODULE.with(|mod_slot| {
        let mut mod_guard = mod_slot.borrow_mut();
        let resident = mod_guard.as_mut().ok_or("resident module missing")?;
        RESIDENT_RUNTIME.with(|rt_slot| {
            let mut rt_guard = rt_slot.borrow_mut();
            let runtime = rt_guard.as_mut().ok_or("resident runtime missing")?;
            let compiled = compile_program(
                &mut resident.module,
                &resident.host,
                program,
                artifact,
                runtime,
            )?;
            runtime.snapshot_compile_strings();
            resident.main_id = compiled.entry_id;
            resident.main_returns_result = main_returns_result;
            resident.main_returns_app = main_returns_app;
            resident.main_returns_default_err = main_returns_default_err;
            resident.main_error_type = main_error_type;
            resident.main_error_is_packed = main_error_is_packed;
            Ok(())
        })
    })
}

pub(crate) fn resident_invoke() -> Result<RunOutcome, String> {
    let (code, main_returns_result, main_returns_app) = RESIDENT_MODULE
        .with(|slot| {
            slot.borrow_mut().as_mut().map(|resident| {
                resident
                    .module
                    .finalize_definitions()
                    .expect("resident JIT functions must finalize");
                (
                    resident.module.get_finalized_function(resident.main_id),
                    resident.main_returns_result,
                    resident.main_returns_app,
                )
            })
        })
        .ok_or_else(|| "resident module missing".to_string())?;

    RESIDENT_RUNTIME.with(|slot| {
        let mut rt_guard = slot.borrow_mut();
        let runtime = rt_guard.as_mut().ok_or("resident runtime missing")?;
        runtime.invocations += 1;
        runtime.stdout.clear();
        runtime.stderr.clear();
        runtime.results.clear();
        runtime.errors.clear();
        jet_foundation::Outcome::jet_journey_reset();
        let ptr: *mut JitRuntime = runtime;
        Concurrency::set_active_runtime(Some(ptr));
        jet_codegen::scheduler::jet_observe_runtime_start_from_env(Vec::new());
        jet_codegen::scheduler::jet_scheduler_task_completion_begin();
        if main_returns_result || main_returns_app {
            let entry: extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
            let _ = entry();
        } else {
            let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
            entry();
        }
        jet_codegen::scheduler::jet_scheduler_task_completion_drain();
        jet_codegen::scheduler::jet_scheduler_task_completion_end();
        Concurrency::settle_pending_after_native();
        jet_codegen::scheduler::jet_scheduler_drain();
        super::runtime_host::run_jit_atexit_handlers(runtime);
        if let Some(report) = jet_codegen::scheduler::jet_observe_parked_tasks_report() {
            runtime.stderr.push_str(&report.rendered);
            runtime.exit_code = Some(report.exit_code);
        }
        jet_codegen::scheduler::jet_scheduler_drain_after_exit();
        jet_codegen::task_group::jet_task_deadline_clear_pending();
        Concurrency::set_active_runtime(None);
        if let Some(rendered) = runtime.deadline_exceeded.take() {
            let mut stderr = rendered;
            if !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            let stdout = runtime.stdout.clone();
            reset_run_heap(runtime);
            return Ok(RunOutcome::Ran {
                stdout,
                stderr,
                exit_code: 70,
            });
        }
        if let Some(outcome) = take_host_fault_outcome(runtime) {
            return Ok(outcome);
        }
        if let Some(msg) = runtime.take_trap() {
            if msg == "__jet_rich_panic__" || runtime.exit_code.is_some() {
                let code = runtime.exit_code.take().unwrap_or(1);
                let stdout = runtime.stdout.clone();
                let stderr = runtime.stderr.clone();
                reset_run_heap(runtime);
                return Ok(RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code: code,
                });
            }
            let stderr = runtime.stderr.clone();
            let stdout = runtime.stdout.clone();
            reset_run_heap(runtime);
            return Ok(RunOutcome::Ran {
                stdout,
                stderr,
                exit_code: 1,
            });
        }
        let stdout = runtime.stdout.clone();
        let stderr = runtime.stderr.clone();
        let exit_code = runtime.exit_code.take().unwrap_or(0);
        reset_run_heap(runtime);
        Ok(RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        })
    })
}

pub(crate) fn resident_run_fresh(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(fresh_runtime_with_allocator_cap(
            release_devtools_policy.clone(),
            cap_bytes,
        ))
    });
    super::tier_cache::begin_capture();
    let compiled = ensure_resident_module(program, artifact, release_devtools_policy);
    if compiled.is_err() {
        super::tier_cache::abort_capture();
    }
    compiled?;
    let outcome = resident_invoke();
    if outcome.is_err() {
        super::tier_cache::abort_capture();
    }
    outcome
}

pub(crate) fn resident_hot_swap(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    crate::net_http_rt::clear_net_http_handles();
    crate::CoreHost::reset_jit_interrupts();
    let runtime = RESIDENT_RUNTIME.with(|slot| slot.borrow_mut().take());
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(runtime.unwrap_or_else(|| {
            fresh_runtime_with_allocator_cap(release_devtools_policy.clone(), cap_bytes)
        }))
    });
    ensure_resident_module(program, artifact, release_devtools_policy)?;
    resident_invoke()
}

pub fn discard_hot_swap_plan() {
    RESIDENT_RUNTIME.with(|slot| {
        if let Some(runtime) = slot.borrow_mut().as_mut() {
            runtime.hot_swap_plan = None;
        }
    });
}

pub fn apply_hot_swap(_decision: &HotSwapDecision) -> Result<(), String> {
    Ok(())
}

pub fn apply_hot_swap_with_program(
    program: &MirProgram,
    artifact: MirArtifactId,
    _decision: &HotSwapDecision,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<Vec<SchemaMigrationReceipt>, String> {
    ensure_resident_module(program, artifact, release_devtools_policy)?;
    Ok(Vec::new())
}

#[derive(Clone, Debug)]
pub struct ConsoleServiceBinding {
    name: String,
    identity: String,
    state: String,
}

impl ConsoleServiceBinding {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn state(&self) -> &str {
        &self.state
    }
}

pub struct ResidentConsoleLease {
    startup_stdout: String,
    startup_stderr: String,
    router: ConsoleHttpRouter,
    database_resources: Vec<ConsoleDbResource>,
    service_bindings: Vec<ConsoleServiceBinding>,
}

impl ResidentConsoleLease {
    pub fn startup_stdout(&self) -> &str {
        &self.startup_stdout
    }

    pub fn startup_stderr(&self) -> &str {
        &self.startup_stderr
    }

    pub fn router(&self) -> ConsoleHttpRouter {
        self.router.clone()
    }

    pub fn take_database_resources(&mut self) -> Vec<ConsoleDbResource> {
        std::mem::take(&mut self.database_resources)
    }

    pub fn take_service_bindings(&mut self) -> Vec<ConsoleServiceBinding> {
        std::mem::take(&mut self.service_bindings)
    }
}

pub fn resident_boot_console(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<ResidentConsoleLease, String> {
    let outcome = resident_run_fresh(program, None, artifact, release_devtools_policy)?;
    let (startup_stdout, startup_stderr) = match outcome {
        RunOutcome::Ran { stdout, stderr, .. } => (stdout, stderr),
        RunOutcome::Problems(problems) => {
            return Err(problems
                .into_iter()
                .map(|problem| problem.what)
                .collect::<Vec<_>>()
                .join("\n"));
        }
        other => return Err(format!("console boot did not produce a resident run: {other:?}")),
    };
    Ok(ResidentConsoleLease {
        startup_stdout,
        startup_stderr,
        router: console_http_router_from_mux(crate::net_http_rt::jet_app_http_mux_new()),
        database_resources: Vec::new(),
        service_bindings: Vec::new(),
    })
}
