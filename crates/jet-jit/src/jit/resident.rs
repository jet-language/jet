use cranelift_module::{FuncOrDataId, Module};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::AtomicBool;
use cranelift_jit::JITModule;
use jet_foundation::{
    HotSwap::HotSwapDecision,
    JitBackend::RunOutcome,
    MIR::{
        MirAccess, MirArtifactId, MirCallee, MirDecisionLedger, MirDecisionRow, MirFailureCarrier,
        MirFunction, MirFunctionId, MirOperation, MirProgram, MirRuntimeValue, MirSemanticOp,
        MirType,
    },
    SchemaMigration::SchemaMigrationReceipt,
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;

use super::deopt::clear_deopt_state;
use super::functions_compile::{
    compile_program_with_roots, compile_typed_entry_adapter, install_finalized_iterable_hooks,
    redefine_mir_functions, register_finalized_jit_closure_targets, CompiledMirProgram,
    SourceHelperCompileClosure,
};
use super::runtime_host::{new_jit_module, EntryErrorType, ResidentHotSwapPlan, ResidentModule};
use super::safety::artifact_entry;
use super::tiers::{runtime_decision_rows, TierRow};
use super::types_meta::mir_fn_name;
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

fn main_return_name(function: &MirFunction) -> Option<&str> {
    let return_type = function
        .return_type
        .result_parts()
        .map_or(&function.return_type, |(ok, _)| ok);
    return_type.nominal_name()
}

fn main_returns_app(program: &MirProgram, artifact: MirArtifactId) -> bool {
    entry_function(program, artifact)
        .and_then(main_return_name)
        .is_some_and(|name| name.contains("App") || name.contains("Page"))
}

fn main_serves_app(program: &MirProgram, artifact: MirArtifactId) -> bool {
    let serves_until_stopped = program
        .artifacts
        .iter()
        .find(|plan| plan.id == artifact)
        .and_then(|plan| plan.entry.as_ref())
        .is_some_and(|entry| entry.serves_until_stopped);
    serves_until_stopped
        && entry_function(program, artifact)
            .and_then(main_return_name)
            .is_some_and(|name| name == "App")
}

fn main_error_type(program: &MirProgram, artifact: MirArtifactId) -> Option<EntryErrorType> {
    let MirFailureCarrier::Result { error, .. } = &entry_function(program, artifact)?.failure else {
        return None;
    };
    if matches!(error.layout.abi, jet_foundation::MIR::MirAbi::Never) {
        return Some(EntryErrorType::Uninhabited);
    }
    // Compiler-owned carriers have exact canonical declaration keys. A user
    // type with the same leaf spelling must retain its own display descriptor.
    let builtin = error.nominal_id().and_then(|id| {
        program.types.iter().find(|definition| definition.id == id)
            .map(|definition| definition.key.as_str())
    });
    match builtin {
        Some(jet_foundation::Syntax::TYPE_ERR) => Some(EntryErrorType::Default),
        Some(jet_foundation::Syntax::TYPE_IO_ERROR) => Some(EntryErrorType::Io),
        _ => super::runtime_host::runtime_type_id(error).map(EntryErrorType::Descriptor),
    }
}

fn install_cli_function_pointers(
    module: &JITModule,
    compiled: &CompiledMirProgram,
) -> Result<(), String> {
    let Some(user_run) = crate::CLI::cli_user_run_target() else {
        return Ok(());
    };
    let user_run_id = compiled
        .function_ids
        .get(&user_run)
        .copied()
        .ok_or_else(|| format!("JIT CLI run function {user_run:?} was not compiled"))?;
    let user_run_ptr = module.get_finalized_function(user_run_id);
    if user_run_ptr.is_null() {
        return Err("JIT CLI run function has no finalized address".to_string());
    }
    crate::CLI::install_cli_run_ptr(user_run_ptr);
    for command in crate::CLI::cli_command_targets() {
        let Some(command_id) = compiled.function_ids.get(&command).copied() else {
            return Err(format!("JIT CLI command {command:?} was not compiled"));
        };
        let command_ptr = module.get_finalized_function(command_id);
        if command_ptr.is_null() {
            return Err(format!("JIT CLI command {command:?} has no finalized address"));
        }
        crate::CLI::install_cli_command_ptr(command, command_ptr);
    }
    Ok(())
}

pub(crate) fn fresh_runtime(release_devtools_policy: ReleaseDevtoolsPolicy) -> JitRuntime {
    fresh_runtime_with_allocator_cap(release_devtools_policy, None)
}
/// Recover the checked hosted program-allocator cap carried through MIR.
///
/// MIR currently preserves the package allocator as its canonical Debug text.
/// `Counting { cap: None }` is distinct from the absent fact: `Some(0)` keeps
/// the uncapped counting wrapper, while `None` selects the hidden system heap.
pub(crate) fn program_allocator_cap_bytes(program: &MirProgram) -> Option<u64> {
    const PREFIX: &str = "Counting { cap: Some(ByteSize { bytes: ";
    match program.facts.allocator.as_str() {
        "Counting { cap: None }" => Some(0),
        allocator => allocator
            .strip_prefix(PREFIX)
            .and_then(|value| value.strip_suffix(" }) }"))
            .and_then(|value| value.parse().ok()),
    }
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
        default_error_type: None,
        trait_object_types: HashMap::new(),
        native_interface_carriers: HashMap::new(),
        native_shared_interops: HashMap::new(),
        native_shared_roots: HashMap::new(),
        native_shared_guards: HashMap::new(),
        next_native_shared_guard_token: i64::MIN,
        native_shared_handles: HashMap::new(),
        invocation_carrier_epoch: 1,
        next_invocation_carrier_token: 1,
        jit_closure_targets: HashMap::new(),
        jit_closure_targets_by_ptr: HashMap::new(),
        jit_closure_execution_identity: None,
        native_interface_methods: HashMap::new(),
        native_callable_methods: HashMap::new(),
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
        jit_callable_env_lifetimes: Vec::new(),
        jit_callable_env_by_handle: HashMap::new(),
        scope_guards: Vec::new(),
        transactions: Vec::new(),
        atexit_handlers: Vec::new(),
        tasks: Vec::new(),
        task_controls: Vec::new(),
        task_skip_join_deadline: Vec::new(),
        task_groups: Vec::new(),
        cells: crate::Cell::CellState::new(),
        results: Vec::new(),
        default_errors: HashMap::new(),
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
        shared_weak_owners: HashMap::new(),
        shared_snapshots: HashMap::new(),
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
        deterministic_world: None,
        deterministic_world_scope: None,
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
    rt.deterministic_world_scope.take();
    rt.deterministic_world.take();
    let compile_strings = rt.compile_strings.clone();
    rt.heap.clear();
    rt.clear_invocation_carriers();
    rt.int_list_views.clear();
    rt.view_slots.clear();
    rt.clear_lazy_iters();
    rt.program_allocator.release_hosted_reservations();
    rt.heap.install_string_slots(&compile_strings);
    rt.compute.clear();
    rt.memo_values.clear();
    crate::Math::clear_math_values();
    crate::Collections::drop_loop_stream_resources(rt);
    rt.source_frames.clear();
    rt.current_line = 0;
    rt.current_function.clear();
    rt.current_source_line.clear();
    rt.host_fault = false;
    rt.host_fault_payload_captured = false;
    rt.transactions.clear();
    rt.jit_callables.clear();
    rt.scope_guards.clear();
    rt.atexit_handlers.clear();
    rt.channels.clear();
    rt.senders.clear();
    rt.tasks.clear();
    rt.task_controls.clear();
    rt.task_skip_join_deadline.clear();
    rt.task_groups.clear();
    rt.results.clear();
    rt.default_errors.clear();
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


fn install_program_source(runtime: &mut JitRuntime, program: &MirProgram) {
    // ponytail: one source buffer. Feature examples are single-file; split if
    // multi-file runtime stops start reporting the wrong snippet.
    if let Some(source) = program.source_files.first() {
        runtime.source_file = source.path.clone();
        runtime.source_text = source.source.clone();
    }
}

pub(crate) fn ensure_resident_module(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<(), String> {
    ensure_resident_module_with_roots(program, artifact, release_devtools_policy, &[])
}

pub(crate) fn ensure_resident_module_with_roots(
    program: &MirProgram,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
    helper_roots: &[MirFunctionId],
) -> Result<(), String> {
    let main_returns_result = main_returns_result(program, artifact);
    let main_error_type = main_error_type(program, artifact);
    let main_returns_app = main_returns_app(program, artifact);
    let main_serves_app = main_serves_app(program, artifact);
    if helper_roots.is_empty() {
        crate::CLI::prepare_cli_from_mir(program, artifact);
        crate::Ffi::bind_mir_ffi(program, artifact).map_err(|err| match err {
            crate::Ffi::BindError::Message(message) => message,
        })?;
    } else {
        super::functions_compile::source_helper_compile_closure(program, artifact, helper_roots)?;
    }
    let need_create = RESIDENT_MODULE.with(|slot| slot.borrow().is_none());
    if need_create {
        let (mut module, host) = new_jit_module()?;
        let mut runtime = RESIDENT_RUNTIME
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(|| {
                fresh_runtime_with_allocator_cap(
                    release_devtools_policy.clone(),
                    program_allocator_cap_bytes(program),
                )
            });
        let compiled = compile_program_with_roots(
            &mut module,
            &host,
            program,
            artifact,
            &mut runtime,
            helper_roots,
            None,
        )?;
        if helper_roots.is_empty() {
            install_cli_function_pointers(&module, &compiled)?;
        }
        install_finalized_iterable_hooks(&module, &mut runtime, &compiled)?;
        runtime.snapshot_compile_strings();
        install_program_source(&mut runtime, program);
        RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
        RESIDENT_MODULE.with(|slot| {
            *slot.borrow_mut() = Some(ResidentModule {
                module,
                host,
                main_id: compiled.entry_id,
                program: None,
                artifact: Some(artifact),
                execution: program.execution_identity(Some(artifact)).ok(),
                typed_entry_id: compiled.typed_entry_id,
                main_returns_result,
                main_returns_app,
                main_serves_app,
                main_error_type,
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
            let compiled = compile_program_with_roots(
                &mut resident.module,
                &resident.host,
                program,
                artifact,
                runtime,
                helper_roots,
                None,
            )?;
            if helper_roots.is_empty() {
                install_cli_function_pointers(&resident.module, &compiled)?;
            }
            install_finalized_iterable_hooks(&resident.module, runtime, &compiled)?;
            runtime.snapshot_compile_strings();
            install_program_source(runtime, program);
            resident.main_id = compiled.entry_id;
            resident.typed_entry_id = compiled.typed_entry_id;
            resident.main_returns_result = main_returns_result;
            resident.main_returns_app = main_returns_app;
            resident.main_serves_app = main_serves_app;
            resident.main_error_type = main_error_type;
            Ok(())
        })
    })
}

fn ensure_typed_entry_adapter(
    program: &MirProgram,
    artifact: MirArtifactId,
) -> Result<(), String> {
    let function = entry_function(program, artifact)
        .ok_or_else(|| format!("MIR artifact {artifact:?} has no entry function"))?;
    RESIDENT_MODULE.with(|slot| {
        let mut resident_guard = slot.borrow_mut();
        let resident = resident_guard
            .as_mut()
            .ok_or_else(|| "resident module missing".to_string())?;
        if resident.typed_entry_id.is_some() {
            return Ok(());
        }
        resident.typed_entry_id =
            compile_typed_entry_adapter(&mut resident.module, resident.main_id, function)?;
        Ok(())
    })
}

fn ensure_typed_helper_adapter_in_module(
    module: &mut dyn Module,
    function_row: &MirFunction,
) -> Result<(cranelift_module::FuncId, cranelift_module::FuncId), String> {
    let function = function_row.id;
    let target = match module.get_name(&mir_fn_name(function)) {
        Some(FuncOrDataId::Func(id)) => id,
        Some(_) => {
            return Err(format!(
                "Source helper function {function:?} has a non-function resident symbol"
            ))
        }
        None => {
            return Err(format!(
                "Source helper function {function:?} was not retained by the artifact"
            ))
        }
    };
    let adapter_name = format!("__jet_typed_entry_adapter_{}", function.0);
    let typed_target = match module.get_name(&adapter_name) {
        Some(FuncOrDataId::Func(id)) => id,
        Some(_) => {
            return Err(format!(
                "Source helper adapter for {function:?} has a non-function resident symbol"
            ))
        }
        None => compile_typed_entry_adapter(module, target, function_row)?.ok_or_else(|| {
            format!("Source helper function {function:?} has no typed entry adapter")
        })?,
    };
    Ok((target, typed_target))
}


pub(crate) fn invoke_word_entry(code: *const u8, args: &[i64]) -> Result<i64, String> {
    match args {
        [] => {
            let entry: extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
            Ok(entry())
        }
        [a] => {
            let entry: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(code) };
            Ok(entry(*a))
        }
        [a, b] => {
            let entry: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b))
        }
        [a, b, c] => {
            let entry: extern "C" fn(i64, i64, i64) -> i64 =
                unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b, *c))
        }
        [a, b, c, d] => {
            let entry: extern "C" fn(i64, i64, i64, i64) -> i64 =
                unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b, *c, *d))
        }
        [a, b, c, d, e] => {
            let entry: extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b, *c, *d, *e))
        }
        [a, b, c, d, e, f] => {
            let entry: extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b, *c, *d, *e, *f))
        }
        [a, b, c, d, e, f, g] => {
            let entry: extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64 =
                unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b, *c, *d, *e, *f, *g))
        }
        [a, b, c, d, e, f, g, h] => {
            let entry: extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64 =
                unsafe { std::mem::transmute(code) };
            Ok(entry(*a, *b, *c, *d, *e, *f, *g, *h))
        }
        _ => Err(format!(
            "typed Cranelift entry adapter received {} arguments; maximum is eight",
            args.len()
        )),
    }
}

pub(crate) fn resident_invoke() -> Result<RunOutcome, String> {
    resident_invoke_inner(None, None, None, None).map(|(outcome, _)| outcome)
}

pub(crate) fn resident_invoke_with_words_and_value(
    words: &[i64],
    return_type: &MirType,
    invocation_started: &mut bool,
) -> Result<(RunOutcome, MirRuntimeValue), String> {
    let (outcome, value) =
        resident_invoke_inner(None, Some(words), Some(return_type), Some(invocation_started))?;
    let value = value
        .ok_or_else(|| "typed Cranelift entry did not produce a typed return value".to_string())?;
    Ok((outcome, value))
}

pub(crate) fn resident_invoke_function_with_words_and_value(
    target: cranelift_module::FuncId,
    typed_target: Option<cranelift_module::FuncId>,
    words: &[i64],
    return_type: &MirType,
) -> Result<(RunOutcome, MirRuntimeValue), String> {
    let (outcome, value) =
        resident_invoke_inner(Some((target, typed_target)), Some(words), Some(return_type), None)?;
    let value = value
        .ok_or_else(|| "typed Cranelift helper did not produce a typed return value".to_string())?;
    Ok((outcome, value))
}

fn resident_invoke_inner(
    function: Option<(
        cranelift_module::FuncId,
        Option<cranelift_module::FuncId>,
    )>,
    words: Option<&[i64]>,
    typed_return: Option<&MirType>,
    mut invocation_started: Option<&mut bool>,
) -> Result<(RunOutcome, Option<MirRuntimeValue>), String> {
    let (code, typed_code, main_returns_result, main_returns_app, main_serves_app, main_error_type) =
        RESIDENT_MODULE.with(|slot| -> Result<_, String> {
            let mut resident_guard = slot.borrow_mut();
            let resident = resident_guard
                .as_mut()
                .ok_or_else(|| "resident module missing".to_string())?;
            resident
                .module
                .finalize_definitions()
                .map_err(|error| error.to_string())?;
            if let Some((target, typed_target)) = function {
                Ok((
                    resident.module.get_finalized_function(target),
                    typed_target.map(|id| resident.module.get_finalized_function(id)),
                    false,
                    false,
                    false,
                    None,
                ))
            } else {
                Ok((
                    resident.module.get_finalized_function(resident.main_id),
                    resident
                        .typed_entry_id
                        .map(|id| resident.module.get_finalized_function(id)),
                    resident.main_returns_result,
                    resident.main_returns_app,
                    resident.main_serves_app,
                    resident.main_error_type,
                ))
            }
        })?;

    if words.is_some() && typed_code.is_none() {
        return Err("typed Cranelift entry adapter is unavailable for this function".to_string());
    }
    let cli_adapter = crate::CLI::cli_run_requires_adapter();
    RESIDENT_RUNTIME.with(|slot| {
        let mut rt_guard = slot.borrow_mut();
        let runtime = rt_guard.as_mut().ok_or("resident runtime missing")?;
        runtime.invocations += 1;
        runtime.stdout.clear();
        runtime.stderr.clear();
        runtime.results.clear();
        runtime.default_errors.clear();
        jet_foundation::Outcome::jet_journey_reset();
        let ptr: *mut JitRuntime = runtime;
        let mut typed_handle = None;
        Concurrency::set_active_runtime(Some(ptr));
        jet_codegen::scheduler::jet_observe_runtime_start_from_env(Vec::new());
        let mut completion = jet_codegen::scheduler::jet_scheduler_task_completion_begin();
        if let Some(args) = words {
            let typed_code = typed_code
                .ok_or("typed Cranelift entry adapter is unavailable for this function")?;
            if let Some(started) = invocation_started.as_deref_mut() {
                *started = true;
            }
            let handle = invoke_word_entry(typed_code, args)?;
            typed_handle = Some(handle);
            if main_returns_result {
                if let Some(app) =
                    super::runtime_host::report_unhandled_entry_result(handle, main_error_type)
                {
                    if main_serves_app {
                        crate::Web::serve_app(app);
                    }
                }
            } else if main_serves_app {
                crate::Web::serve_app(handle);
            }
        } else if cli_adapter {
            let handle = crate::CLI::jet_jit_cli_main();
            if runtime.exit_code.is_none() {
                if main_returns_result {
                    if let Some(app) =
                        super::runtime_host::report_unhandled_entry_result(handle, main_error_type)
                    {
                        if main_serves_app {
                            crate::Web::serve_app(app);
                        }
                    }
                } else if main_serves_app {
                    crate::Web::serve_app(handle);
                }
            }
        } else if main_returns_result || main_returns_app {
            let entry: extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
            let handle = entry();
            if main_returns_result {
                if let Some(app) =
                    super::runtime_host::report_unhandled_entry_result(handle, main_error_type)
                {
                    if main_serves_app {
                        crate::Web::serve_app(app);
                    }
                }
            } else if main_serves_app {
                crate::Web::serve_app(handle);
            }
        } else {
            let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
            entry();
        }
        if runtime.host_fault {
        }
        completion.drain();
        drop(completion);
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
        let typed_value = match (typed_handle, typed_return) {
            (Some(handle), Some(return_type)) => {
                let type_id = super::runtime_host::runtime_type_id(return_type)
                    .ok_or_else(|| "typed return has no runtime type identity".to_string())?;
                match super::runtime_host::decode_jit_cell_value(runtime, handle, type_id) {
                    Ok(value) => Some(value),
                    Err(error) => {
                        reset_run_heap(runtime);
                        return Err(error);
                    }
                }
            }
            _ => None,
        };
        if let Some(rendered) = runtime.deadline_exceeded.take() {
            let mut stderr = rendered;
            if !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            let stdout = runtime.stdout.clone();
            reset_run_heap(runtime);
            return Ok((
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code: 70,
                },
                typed_value,
            ));
        }
        if let Some(outcome) = take_host_fault_outcome(runtime) {
            return Ok((outcome, typed_value));
        }
        if let Some(msg) = runtime.take_trap() {
            if msg == "__jet_rich_panic__" || runtime.exit_code.is_some() {
                let code = runtime.exit_code.take().unwrap_or(1);
                let stdout = runtime.stdout.clone();
                let stderr = runtime.stderr.clone();
                reset_run_heap(runtime);
                return Ok((
                    RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code: code,
                    },
                    typed_value,
                ));
            }
            let stderr = runtime.stderr.clone();
            let stdout = runtime.stdout.clone();
            reset_run_heap(runtime);
            return Ok((
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code: 1,
                },
                typed_value,
            ));
        }
        let stdout = runtime.stdout.clone();
        let stderr = runtime.stderr.clone();
        let exit_code = runtime.exit_code.take().unwrap_or(0);
        reset_run_heap(runtime);
        Ok((
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            },
            typed_value,
        ))
    })
}

pub(crate) fn resident_run_fresh(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<RunOutcome, String> {
    resident_run_fresh_inner(
        program,
        cap_bytes,
        artifact,
        release_devtools_policy,
        None,
        None,
        None,
    )
    .map(|(outcome, _)| outcome)
}

pub(crate) fn resident_run_fresh_with_values_and_result(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
    values: &[MirRuntimeValue],
    return_type: &MirType,
    invocation_started: &mut bool,
) -> ResidentHelperAttempt {
    let result = resident_run_fresh_inner(
        program,
        cap_bytes,
        artifact,
        release_devtools_policy,
        Some(values),
        Some(return_type),
        Some(invocation_started),
    );
    match result {
        Ok((outcome, Some(value))) => ResidentHelperAttempt::Invoked {
            outcome,
            value: Some(value),
            failure: None,
            writebacks: Vec::new(),
        },
        Ok((outcome, None)) => ResidentHelperAttempt::Invoked {
            outcome,
            value: None,
            failure: Some("typed Cranelift entry did not produce a typed return value".to_string()),
            writebacks: Vec::new(),
        },
        Err(error) if *invocation_started => ResidentHelperAttempt::Invoked {
            outcome: resident_invocation_failure_outcome(),
            value: None,
            failure: Some(error),
            writebacks: Vec::new(),
        },
        Err(error) => ResidentHelperAttempt::NotInvoked(error),
    }
}

pub(crate) struct ResidentHelperWriteback {
    pub parameter_index: usize,
    pub value: MirRuntimeValue,
}

pub(crate) enum ResidentHelperAttempt {
    NotInvoked(String),
    Invoked {
        outcome: RunOutcome,
        value: Option<MirRuntimeValue>,
        failure: Option<String>,
        writebacks: Vec<ResidentHelperWriteback>,
    },
}

pub(crate) fn resident_run_fresh_with_function_values_and_result(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    helper_roots: &[MirFunctionId],
    function: MirFunctionId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
    values: &[MirRuntimeValue],
    return_type: &MirType,
    invocation_started: &mut bool,
) -> ResidentHelperAttempt {
    if let Err(error) = jet_rt::__gc::initialize_trace() {
        return ResidentHelperAttempt::NotInvoked(error.to_string());
    }
    let closure = match super::functions_compile::source_helper_compile_closure(
        program,
        artifact,
        helper_roots,
    ) {
        Ok(closure) => closure,
        Err(error) => return ResidentHelperAttempt::NotInvoked(error),
    };
    if let Err(error) = crate::Ffi::bind_mir_ffi_for_source_helper(
        program,
        artifact,
        &closure.foreigns,
        &closure.links,
        closure.needs_data_provider,
    ) {
        return ResidentHelperAttempt::NotInvoked(error);
    }
    resident_run_private_helper(
        program,
        cap_bytes,
        artifact,
        helper_roots,
        &closure,
        function,
        release_devtools_policy,
        values,
        return_type,
        invocation_started,
    )
}

fn resident_run_private_helper(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    helper_roots: &[MirFunctionId],
    source_helper_closure: &SourceHelperCompileClosure,
    function_id: MirFunctionId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
    values: &[MirRuntimeValue],
    return_type: &MirType,
    invocation_started: &mut bool,
) -> ResidentHelperAttempt {
    let prepared = (|| {
        let (mut module, host) = new_jit_module()?;
        let mut runtime = fresh_runtime_with_allocator_cap(
            release_devtools_policy.clone(),
            cap_bytes,
        );
        let compiled = compile_program_with_roots(
            &mut module,
            &host,
            program,
            artifact,
            &mut runtime,
            helper_roots,
            Some(source_helper_closure),
        )?;
        install_finalized_iterable_hooks(&module, &mut runtime, &compiled)?;
        runtime.snapshot_compile_strings();
        install_program_source(&mut runtime, program);
        let function = program
            .functions
            .iter()
            .find(|candidate| candidate.id == function_id)
            .ok_or_else(|| format!("Source helper function {function_id:?} is missing"))?;
        let mut write_arguments = Vec::new();
        let words = values
            .iter()
            .zip(&function.params)
            .enumerate()
            .map(|(parameter_index, (value, parameter))| {
                let type_id = super::runtime_host::runtime_type_id(&parameter.ty).ok_or_else(|| {
                    format!(
                        "typed parameter `{}` has no runtime type identity",
                        parameter.name
                    )
                })?;
                let value =
                    super::runtime_host::encode_jit_cell_value(&mut runtime, value, type_id)?;
                if parameter.access == MirAccess::Write {
                    let mut slot = Box::new(value);
                    let address = (&mut *slot as *mut i64) as i64;
                    write_arguments.push(ResidentWriteArgumentSlot {
                        parameter_index,
                        type_id,
                        raw: slot,
                    });
                    Ok(address)
                } else {
                    Ok::<i64, String>(value)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (_, typed_target) = ensure_typed_helper_adapter_in_module(&mut module, function)?;
        module
            .finalize_definitions()
            .map_err(|error| error.to_string())?;
        let typed_code = module.get_finalized_function(typed_target);
        Ok((module, host, runtime, typed_code, words, write_arguments))
    })();
    let (module, host, mut runtime, typed_code, words, write_arguments) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return ResidentHelperAttempt::NotInvoked(error),
    };
    let runtime_ptr = &mut runtime as *mut JitRuntime;
    let invocation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resident_invoke_private_helper(
            runtime_ptr,
            typed_code,
            &words,
            return_type,
            invocation_started,
        )
    }));
    let (writebacks, writeback_failure) = if *invocation_started {
        resident_decode_helper_writebacks(&mut runtime, &write_arguments)
    } else {
        (Vec::new(), None)
    };
    let combine_failure = |failure: Option<String>| match (failure, writeback_failure.as_ref()) {
        (Some(failure), Some(writeback)) => {
            Some(format!("{failure}; writable output projection failed: {writeback}"))
        }
        (Some(failure), None) => Some(failure),
        (None, Some(writeback)) => {
            Some(format!("writable output projection failed: {writeback}"))
        }
        (None, None) => None,
    };
    let attempt = match invocation {
        Ok(Ok((outcome, value))) => ResidentHelperAttempt::Invoked {
            outcome,
            value: Some(value),
            failure: combine_failure(None),
            writebacks,
        },
        Ok(Err(error)) if !*invocation_started => ResidentHelperAttempt::NotInvoked(error),
        Ok(Err(error)) => ResidentHelperAttempt::Invoked {
            outcome: resident_helper_failure_outcome(&runtime),
            value: None,
            failure: combine_failure(Some(error)),
            writebacks,
        },
        Err(payload) if !*invocation_started => {
            ResidentHelperAttempt::NotInvoked(panic_message(payload.as_ref()))
        }
        Err(payload) => {
            let error = panic_message(payload.as_ref());
            ResidentHelperAttempt::Invoked {
                outcome: resident_helper_failure_outcome(&runtime),
                value: None,
                failure: combine_failure(Some(error)),
                writebacks,
            }
        }
    };
    reset_run_heap(&mut runtime);
    drop(host);
    drop(module);
    attempt
}

struct ResidentWriteArgumentSlot {
    parameter_index: usize,
    type_id: u64,
    raw: Box<i64>,
}

fn resident_decode_helper_writebacks(
    runtime: &mut JitRuntime,
    slots: &[ResidentWriteArgumentSlot],
) -> (Vec<ResidentHelperWriteback>, Option<String>) {
    let mut writebacks = Vec::with_capacity(slots.len());
    let mut failure = None;
    for slot in slots {
        match super::runtime_host::decode_jit_cell_value(runtime, *slot.raw, slot.type_id) {
            Ok(value) => writebacks.push(ResidentHelperWriteback {
                parameter_index: slot.parameter_index,
                value,
            }),
            Err(error) => {
                if failure.is_none() {
                    failure = Some(error);
                }
            }
        }
    }
    (writebacks, failure)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_string()))
        .unwrap_or_else(|| "unknown panic payload".to_string())
}

fn resident_helper_failure_outcome(runtime: &JitRuntime) -> RunOutcome {
    RunOutcome::Ran {
        stdout: runtime.stdout.clone(),
        stderr: runtime.stderr.clone(),
        exit_code: runtime.exit_code.unwrap_or(1),
    }
}
pub(crate) fn resident_invocation_failure_outcome() -> RunOutcome {
    RESIDENT_RUNTIME.with(|slot| match slot.borrow().as_ref() {
        Some(runtime) => resident_helper_failure_outcome(runtime),
        None => RunOutcome::Ran {
            stdout: String::new(),
            stderr: "resident runtime missing after typed invocation".to_string(),
            exit_code: 1,
        },
    })
}

fn resident_invoke_private_helper(
    runtime_ptr: *mut JitRuntime,
    typed_code: *const u8,
    words: &[i64],
    return_type: &MirType,
    invocation_started: &mut bool,
) -> Result<(RunOutcome, MirRuntimeValue), String> {
    if words.len() > 8 {
        return Err(format!(
            "typed Cranelift entry adapter received {} arguments; maximum is eight",
            words.len()
        ));
    }
    let _active_runtime = Concurrency::activate_local_runtime(runtime_ptr);
    Concurrency::with_runtime_string(|runtime| {
        runtime.invocations += 1;
        runtime.stdout.clear();
        runtime.stderr.clear();
        runtime.results.clear();
        runtime.default_errors.clear();
        Ok(())
    })?;
    jet_foundation::Outcome::jet_journey_reset();
    jet_codegen::scheduler::jet_observe_runtime_start_from_env(Vec::new());
    let mut completion = jet_codegen::scheduler::jet_scheduler_task_completion_begin();
    *invocation_started = true;
    let handle = invoke_word_entry(typed_code, words)?;
    completion.drain();
    drop(completion);
    Concurrency::settle_pending_after_native();
    jet_codegen::scheduler::jet_scheduler_drain();
    Concurrency::with_runtime_string(|runtime| {
        super::runtime_host::run_jit_atexit_handlers(runtime);
        if let Some(report) = jet_codegen::scheduler::jet_observe_parked_tasks_report() {
            runtime.stderr.push_str(&report.rendered);
            runtime.exit_code = Some(report.exit_code);
        }
        Ok(())
    })?;
    jet_codegen::scheduler::jet_scheduler_drain_after_exit();
    jet_codegen::task_group::jet_task_deadline_clear_pending();
    Concurrency::with_runtime_result("resident runtime missing".to_string(), |runtime| {
        let type_id = super::runtime_host::runtime_type_id(return_type)
            .ok_or_else(|| "typed return has no runtime type identity".to_string())?;
        let value = super::runtime_host::decode_jit_cell_value(runtime, handle, type_id)?;
        if let Some(stderr) = runtime.deadline_exceeded.take() {
            let mut stderr = stderr;
            if !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            let stdout = runtime.stdout.clone();
            return Ok((
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code: 70,
                },
                value,
            ));
        }
        if let Some(outcome) = take_host_fault_outcome(runtime) {
            return Ok((outcome, value));
        }
        if let Some(message) = runtime.take_trap() {
            if message == "__jet_rich_panic__" || runtime.exit_code.is_some() {
                let exit_code = runtime.exit_code.take().unwrap_or(1);
                let stdout = runtime.stdout.clone();
                let stderr = runtime.stderr.clone();
                return Ok((
                    RunOutcome::Ran {
                        stdout,
                        stderr,
                        exit_code,
                    },
                    value,
                ));
            }
            let stdout = runtime.stdout.clone();
            let stderr = runtime.stderr.clone();
            return Ok((
                RunOutcome::Ran {
                    stdout,
                    stderr,
                    exit_code: 1,
                },
                value,
            ));
        }
        let stdout = runtime.stdout.clone();
        let stderr = runtime.stderr.clone();
        let exit_code = runtime.exit_code.take().unwrap_or(0);
        Ok((
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            },
            value,
        ))
    })
}

fn resident_run_fresh_inner(
    program: &MirProgram,
    cap_bytes: Option<u64>,
    artifact: MirArtifactId,
    release_devtools_policy: &ReleaseDevtoolsPolicy,
    values: Option<&[MirRuntimeValue]>,
    return_type: Option<&MirType>,
    mut invocation_started: Option<&mut bool>,
) -> Result<(RunOutcome, Option<MirRuntimeValue>), String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(fresh_runtime_with_allocator_cap(
            release_devtools_policy.clone(),
            cap_bytes,
        ))
    });
    super::tier_cache::begin_capture();
    let compiled = ensure_resident_module_with_roots(
        program,
        artifact,
        release_devtools_policy,
        &[],
    );
    if compiled.is_err() {
        super::tier_cache::abort_capture();
    }
    compiled?;
    let encoded_words = values
        .map(|values| {
            let function = entry_function(program, artifact)
                .ok_or_else(|| format!("MIR artifact {artifact:?} has no entry function"))?;
            if !function.capture_params.is_empty() || function.params.len() != values.len() {
                return Err(format!(
                    "typed Cranelift function expected {} parameters, got {} values",
                    function.params.len(),
                    values.len()
                ));
            }
            RESIDENT_RUNTIME.with(|slot| {
                let mut runtime_guard = slot.borrow_mut();
                let runtime = runtime_guard
                    .as_mut()
                    .ok_or_else(|| "resident runtime missing".to_string())?;
                values
                    .iter()
                    .zip(&function.params)
                    .map(|(value, parameter)| {
                        if parameter.access == MirAccess::Write {
                            return Err(format!(
                                "typed Cranelift function cannot marshal writable parameter `{}`",
                                parameter.name
                            ));
                        }
                        let type_id = super::runtime_host::runtime_type_id(&parameter.ty)
                            .ok_or_else(|| {
                                format!(
                                    "typed parameter `{}` has no runtime type identity",
                                    parameter.name
                                )
                            })?;
                        super::runtime_host::encode_jit_cell_value(runtime, value, type_id)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
        })
        .transpose()
        .map_err(|error| {
            super::tier_cache::abort_capture();
            error
        })?;
    let words = encoded_words.as_deref();
    if words.is_some() {
        if let Err(error) = ensure_typed_entry_adapter(program, artifact) {
            super::tier_cache::abort_capture();
            return Err(error);
        }
    }
    let outcome = match words {
        Some(words) => {
            let return_type = return_type.ok_or_else(|| {
                "typed Cranelift function has values but no checked return type".to_string()
            })?;
            let invocation_started = invocation_started
                .as_deref_mut()
                .ok_or_else(|| "typed Source entry lacks invocation tracking".to_string())?;
            resident_invoke_with_words_and_value(words, return_type, invocation_started)
                .map(|(outcome, value)| (outcome, Some(value)))
        }
        None => resident_invoke().map(|outcome| (outcome, None)),
    };
    if outcome.is_err() {
        super::tier_cache::abort_capture();
    }
    outcome
}
enum ResidentRedefineStatus {
    Applied,
    Unsupported,
}

fn resident_direct_call(operation: &MirOperation) -> Option<MirFunctionId> {
    match operation {
        MirOperation::Call {
            callee:
                MirCallee::User(function)
                | MirCallee::Associated { function, .. }
                | MirCallee::Method { function, .. },
            ..
        } => Some(*function),
        _ => None,
    }
}

fn resident_callable_argument(function: &MirFunction, args: &[jet_foundation::MIR::MirCallArg]) -> bool {
    args.iter().any(|arg| {
        function
            .values
            .iter()
            .find(|(value, _, _, _)| *value == arg.value)
            .is_some_and(|(_, ty, _, _)| ty.function_signature().is_some())
    })
}

fn resident_selective_operation_supported(
    program: &MirProgram,
    function: &MirFunction,
    operation: &MirOperation,
) -> bool {
    match operation {
        MirOperation::Closure { .. }
        | MirOperation::IndirectCall { .. }
        | MirOperation::LoopIterInit { .. }
        | MirOperation::Semantic(
            MirSemanticOp::BuiltinMethod { .. }
            | MirSemanticOp::ClosureMethod { .. }
            | MirSemanticOp::HandleMethod { .. },
        ) => false,
        MirOperation::CoreCall { call, args, .. } => {
            if resident_callable_argument(function, args) {
                return false;
            }
            let Some(row) = program.core_calls.iter().find(|row| row.id == *call) else {
                return false;
            };
            !matches!(
                (row.module.as_str(), row.member.as_str()),
                ("core.data", "csv" | "json")
                    | ("core.encoding.csv", "decode" | "query")
                    | ("core.encoding.json", "decode")
            )
        }
        MirOperation::Call {
            callee: MirCallee::Core(_),
            args,
            ..
        } => !resident_callable_argument(function, args),
        _ => true,
    }
}

fn resident_selective_function_ids(
    program: &MirProgram,
    plan: &ResidentHotSwapPlan,
) -> Result<Option<BTreeSet<MirFunctionId>>, String> {
    if plan.changed_functions.is_empty() {
        return Ok(None);
    }
    let mut selected_ids = BTreeSet::new();
    for key in &plan.changed_functions {
        let Some(function) = program.functions.iter().find(|function| function.key == *key) else {
            return Ok(None);
        };
        selected_ids.insert(function.id);
    }
    let mut changed = true;
    while changed {
        changed = false;
        let current = selected_ids.iter().copied().collect::<Vec<_>>();
        for selected in current {
            let function = program
                .functions
                .iter()
                .find(|function| function.id == selected)
                .ok_or_else(|| format!("MIR function {:?} is missing", selected))?;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    if !resident_selective_operation_supported(
                        program,
                        function,
                        &instruction.operation,
                    ) {
                        return Ok(None);
                    }
                    if let Some(callee) = resident_direct_call(&instruction.operation) {
                        let Some(callee_function) =
                            program.functions.iter().find(|function| function.id == callee)
                        else {
                            return Err(format!("MIR function {:?} is missing", callee));
                        };
                        if !callee_function.target_applicability.cranelift {
                            return Err(format!(
                                "MIR function {:?} calls function {:?} unavailable to Cranelift",
                                selected, callee
                            ));
                        }
                        changed |= selected_ids.insert(callee);
                    }
                }
            }
        }
    }
    Ok(Some(selected_ids))
}

fn install_selected_cli_function_pointers(
    module: &JITModule,
    selected_ids: &BTreeSet<MirFunctionId>,
) -> Result<(), String> {
    let function_pointer = |function_id: MirFunctionId| -> Result<*const u8, String> {
        let Some(FuncOrDataId::Func(id)) = module.get_name(&mir_fn_name(function_id)) else {
            return Err(format!("JIT CLI function {:?} was not compiled", function_id));
        };
        let ptr = module.get_finalized_function(id);
        if ptr.is_null() {
            return Err(format!(
                "JIT CLI function {:?} has no finalized address",
                function_id
            ));
        }
        Ok(ptr)
    };
    if let Some(user_run) = crate::CLI::cli_user_run_target() {
        if selected_ids.contains(&user_run) {
            crate::CLI::install_cli_run_ptr(function_pointer(user_run)?);
        }
    }
    for command in crate::CLI::cli_command_targets() {
        if selected_ids.contains(&command) {
            crate::CLI::install_cli_command_ptr(command, function_pointer(command)?);
        }
    }
    Ok(())
}


fn resident_redefine(
    program: &MirProgram,
    artifact: MirArtifactId,
    plan: &ResidentHotSwapPlan,
    resident: &mut ResidentModule,
    runtime: &mut JitRuntime,
) -> Result<ResidentRedefineStatus, String> {
    let Some(selected_ids) = resident_selective_function_ids(program, plan)? else {
        return Ok(ResidentRedefineStatus::Unsupported);
    };
    for function_id in &selected_ids {
        let function = program
            .functions
            .iter()
            .find(|function| function.id == *function_id)
            .ok_or_else(|| format!("MIR function {:?} is missing", function_id))?;
        if !matches!(
            resident.module.get_name(&mir_fn_name(*function_id)),
            Some(FuncOrDataId::Func(_))
        ) {
            return Ok(ResidentRedefineStatus::Unsupported);
        }
        if function.generator.is_some()
            && !matches!(
                resident
                    .module
                    .get_name(&format!("{}__generator", mir_fn_name(*function_id))),
                Some(FuncOrDataId::Func(_))
            )
        {
            return Ok(ResidentRedefineStatus::Unsupported);
        }
    }
    redefine_mir_functions(
        &mut resident.module,
        &resident.host,
        program,
        &selected_ids,
        runtime,
    )?;
    resident
        .module
        .finalize_definitions()
        .map_err(|error| error.to_string())?;
    register_finalized_jit_closure_targets(
        &resident.module,
        program,
        artifact,
        selected_ids.iter().copied(),
        runtime,
    )?;
    runtime.snapshot_compile_strings();
    install_program_source(runtime, program);
    Ok(ResidentRedefineStatus::Applied)
}

fn take_resident_hot_swap_plan() -> Option<ResidentHotSwapPlan> {
    RESIDENT_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .and_then(|runtime| runtime.hot_swap_plan.take())
    })
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

    if let Some(plan) = take_resident_hot_swap_plan() {
        let status = RESIDENT_MODULE.with(|mod_slot| {
            let mut mod_guard = mod_slot.borrow_mut();
            let Some(resident) = mod_guard.as_mut() else {
                return Ok(ResidentRedefineStatus::Unsupported);
            };
            RESIDENT_RUNTIME.with(|rt_slot| {
                let mut rt_guard = rt_slot.borrow_mut();
                let Some(runtime) = rt_guard.as_mut() else {
                    return Ok(ResidentRedefineStatus::Unsupported);
                };
                resident_redefine(program, artifact, &plan, resident, runtime)
            })
        })?;
        if matches!(status, ResidentRedefineStatus::Applied) {
            return resident_invoke();
        }
    }

    let runtime = RESIDENT_RUNTIME.with(|slot| slot.borrow_mut().take());
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(runtime.map_or_else(
            || fresh_runtime_with_allocator_cap(release_devtools_policy.clone(), cap_bytes),
            |mut runtime| {
                runtime.program_allocator.release_hosted_reservations();
                runtime.program_allocator = std::sync::Arc::new(cap_bytes.map_or_else(
                    jet_codegen::program_allocator::JetProgramAllocator::system,
                    jet_codegen::program_allocator::JetProgramAllocator::counting,
                ));
                runtime
            },
        ))
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

pub fn apply_hot_swap(decision: &HotSwapDecision) -> Result<(), String> {
    if !decision.is_compatible() {
        return Err("incompatible hot-swap decision cannot redefine resident code".to_string());
    }
    let mut preserved_state_keys = Vec::new();
    let mut fresh_state_keys = Vec::new();
    for fact in decision.state_facts() {
        if fact.is_preserved() {
            preserved_state_keys.push(fact.key.clone());
        } else {
            fresh_state_keys.push(fact.key.clone());
        }
    }
    let plan = ResidentHotSwapPlan {
        module: decision.module.clone(),
        changed_functions: decision.changed_functions.clone(),
        preserved_state_keys,
        fresh_state_keys,
        rechecked_items: decision.rechecked().to_vec(),
    };
    RESIDENT_RUNTIME.with(|slot| {
        let mut guard = slot.borrow_mut();
        let runtime = guard
            .as_mut()
            .ok_or_else(|| "resident runtime missing".to_string())?;
        runtime.hot_swap_plan = Some(plan);
        Ok(())
    })
}

pub fn apply_hot_swap_with_program(
    _program: &MirProgram,
    _artifact: MirArtifactId,
    _decision: &HotSwapDecision,
    _release_devtools_policy: &ReleaseDevtoolsPolicy,
) -> Result<Vec<SchemaMigrationReceipt>, String> {
    // The caller runs `resident_hot_swap` immediately after this staging hook.
    // Compiling here would build the complete candidate MIR a second time;
    // schema migration currently has no JIT-side receipts to apply.
    RESIDENT_RUNTIME.with(|slot| {
        if let Some(runtime) = slot.borrow_mut().as_mut() {
            runtime.hot_swap_plan = None;
        }
    });
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
    let outcome = resident_run_fresh(
        program,
        program_allocator_cap_bytes(program),
        artifact,
        release_devtools_policy,
    )?;
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
