use jet_codegen::Codegen::TIR::{JitProgram, TFunc};
use jet_foundation::{JitBackend::RunOutcome, AST::Type};
use std::collections::HashMap;

use super::deopt::{
    clear_deopt_state, install_deopt_program, install_native_hook, register_native_fn,
};
use super::functions_compile::{compile_program, compile_program_tiered};
use super::runtime_host::{jit_result, new_jit_module, ResidentModule};
use super::tiers::TierPlan;
use super::{Concurrency, JitRuntime, RESIDENT_MODULE, RESIDENT_RUNTIME};
use crate::Collections;

fn main_func(program: &JitProgram) -> Option<&TFunc> {
    let name = if program.entry == jet_foundation::Names::mangle_generated("cli_main") {
        "run"
    } else {
        program.entry.as_str()
    };
    program.funcs.iter().find(|func| func.name == name)
}

fn main_error_type(program: &JitProgram) -> Option<Type> {
    main_func(program).and_then(|func| match &func.ret {
        Some(Type::Result { err, .. }) => Some(err.as_ref().clone()),
        _ => None,
    })
}

fn main_returns_result(program: &JitProgram) -> bool {
    main_func(program).is_some_and(|func| matches!(func.ret, Some(Type::Result { .. })))
}

fn main_returns_default_err(program: &JitProgram) -> bool {
    matches!(
        main_error_type(program),
        Some(Type::Named(name)) if name == jet_foundation::Syntax::TYPE_ERR
    )
}

fn main_error_is_packed(program: &JitProgram) -> bool {
    matches!(
        main_error_type(program),
        Some(Type::Named(name)) if program.enum_variants.contains_key(&name)
    )
}

fn main_returns_app(program: &JitProgram) -> bool {
    main_func(program).is_some_and(|func| match &func.ret {
        Some(Type::Named(name)) => name == "App",
        Some(Type::Result { ok, .. }) => {
            matches!(ok.as_ref(), Type::Named(name) if name == "App")
        }
        _ => false,
    })
}

pub(crate) fn fresh_runtime() -> JitRuntime {
    fresh_runtime_with_allocator_cap(None)
}

pub(crate) fn fresh_runtime_with_allocator_cap(cap_bytes: Option<u64>) -> JitRuntime {
    JitRuntime {
        source_file: String::new(),
        source_text: String::new(),
        current_function: String::new(),
        current_line: 0,
        current_source_line: String::new(),
        source_frames: Vec::new(),
        stdout: String::new(),
        stderr: String::new(),
        heap: jet_rt::JetArena::default(),
        int_list_views: Vec::new(),
        program_allocator: std::sync::Arc::new(cap_bytes.map_or_else(
            jet_codegen::program_allocator::JetProgramAllocator::system,
            jet_codegen::program_allocator::JetProgramAllocator::counting,
        )),
        compute: crate::Compute::ComputeState::default(),
        compile_strings: Vec::new(),
        zip_plans: Vec::new(),
        invocations: 0,
        memo_values: std::collections::HashMap::new(),
        channels: Vec::new(),
        senders: Vec::new(),
        stream_consumers: std::collections::HashMap::new(),
        stream_producers: std::collections::HashMap::new(),
        stream_senders: std::collections::HashMap::new(),
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
        service_values: Vec::new(),
        game_scenes: Vec::new(),
        game_frames: Vec::new(),
        game_replays: Vec::new(),
        game_backends: Vec::new(),
        raylib_windows: Vec::new(),
        raylib_colors: Vec::new(),
        raylib_sounds: Vec::new(),
        time_values: Vec::new(),
        regex_values: Vec::new(),
        decimal_values: Vec::new(),
        fraction_values: Vec::new(),
        complex_values: Vec::new(),
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

/// Scrub heap state a trapped (partial) run created, so the NEXT resident
/// invocation (hot-reload iteration or plain re-run) in this same process
/// starts clean — a crashed run must never leak lists/strings/channels/tasks
/// into the following one. `source_file`/`invocations` are run-loop
/// bookkeeping, not per-run heap, and are left alone.
fn reset_run_heap(rt: &mut JitRuntime) {
    // Keep compile-time string slots: machine code still names those handles, and
    // publish_capture reads the heap after this scrub for the warm-run artifact.
    let compile_strings = rt.compile_strings.clone();
    rt.heap.clear();
    rt.int_list_views.clear();
    rt.program_allocator.release_hosted_reservations();
    rt.heap.install_string_slots(&compile_strings);
    rt.compute.clear();
    rt.memo_values.clear();
    crate::Data::clear_lazy_state();
    crate::Math::clear_math_values();
    let stream_consumers = std::mem::take(&mut rt.stream_consumers);
    let stream_producers = std::mem::take(&mut rt.stream_producers);
    let stream_senders = std::mem::take(&mut rt.stream_senders);
    drop(stream_senders);
    drop(stream_producers);
    drop(stream_consumers);
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
    rt.args_specs.clear();
    rt.args_parsed.clear();
    rt.file_readers.clear();
    rt.file_writers.clear();
    rt.json_readers.clear();
    rt.json_writers.clear();
    rt.jsonl_readers.clear();
    rt.jsonl_writers.clear();
    rt.csv_readers.clear();
    rt.csv_writers.clear();
    rt.xml_readers.clear();
    rt.xml_writers.clear();
    rt.cbor_readers.clear();
    rt.cbor_writers.clear();
    rt.data_streams.clear();
    rt.sets.clear();
    rt.set_string_kinds.clear();
    rt.deques.clear();
    rt.bags.clear();
    rt.sorted_sets.clear();
    rt.sorted_set_string_kinds.clear();
    rt.priority_queues.clear();
    rt.lrus.clear();
    rt.bit_sets.clear();
    rt.byte_buffers.clear();
    rt.allocators.clear();
    rt.allocator_views.clear();
    rt.gc_roots.clear();
    rt.gc_edges.clear();
    rt.pools.clear();
    rt.shared_guard_states.clear();
    rt.shareds.clear();
    rt.conditions.clear();
    rt.expirings.clear();
    rt.secrets.clear();
    rt.service_values.clear();
}

fn take_host_fault_outcome(runtime: &mut JitRuntime) -> Option<RunOutcome> {
    if !std::mem::take(&mut runtime.host_fault) {
        return None;
    }
    let payload_captured = std::mem::take(&mut runtime.host_fault_payload_captured);
    let what = if payload_captured {
        runtime
            .trapped
            .take()
            .unwrap_or_else(|| "the JIT runtime helper failed".to_string())
    } else {
        runtime.trapped.take();
        "the JIT runtime helper failed".to_string()
    };
    runtime.exit_code.take();
    let stdout = runtime.stdout.clone();
    reset_run_heap(runtime);
    Some(RunOutcome::Problems(vec![
        jet_foundation::Diagnostics::Diagnostic::runtime_host_fault(
            stdout,
            what,
        ),
    ]))
}

pub(crate) fn resident_teardown() {
    // The interrupt adapter stores raw resident-code addresses. Drain and clear
    // that registry before dropping the module so a dispatcher wake cannot call
    // code from the previous resident image.
    crate::CoreHost::reset_jit_interrupts();
    clear_deopt_state();
    crate::Collections::clear_packed_enum_show();
    crate::Watcher::clear_watcher_state();
    crate::Net::clear_net_state();
    crate::net_http_rt::clear_net_http_handles();
    // Keep FFI cdylib binding across teardown→recompile in the same try_resident;
    // `bind_bundle_ffi` / `clear_ffi` own its lifetime at the outer entry points.
    // CLI plan stays installed across teardown→recompile in the same try_resident;
    // prepare_cli_from_bundle / clear_cli_plan own its lifetime.
    // Keep STRUCT_REDACT: resident_run_fresh teardowns then recompiles in the
    // same try_resident that installed redact; clearing here dropped JetDebug.
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    Concurrency::set_active_runtime(None);
    Concurrency::clear_http_shared_runtime();
}

pub(crate) fn ensure_resident_module(program: &JitProgram) -> Result<(), String> {
    let main_returns_result = main_returns_result(program);
    let main_returns_default_err = main_returns_default_err(program);
    let main_error_type = main_error_type(program);
    let main_error_is_packed = main_error_is_packed(program);
    let main_returns_app = main_returns_app(program);
    let need_create = RESIDENT_MODULE.with(|slot| slot.borrow().is_none());
    if need_create {
        let (mut module, host) = new_jit_module()?;
        let mut runtime = RESIDENT_RUNTIME
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(fresh_runtime);
        let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
        runtime.snapshot_compile_strings();
        RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
        RESIDENT_MODULE.with(|slot| {
            *slot.borrow_mut() = Some(ResidentModule {
                module,
                host,
                main_id,
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
            resident.main_id = compile_program(
                &mut resident.module,
                &resident.host,
                program,
                runtime,
                Some(resident.main_id),
            )?;
            runtime.snapshot_compile_strings();
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
    let (
        code,
        main_returns_result,
        main_returns_app,
        main_returns_default_err,
        main_error_type,
        main_error_is_packed,
    ) = RESIDENT_MODULE
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|r| {
                    (
                        r.module.get_finalized_function(r.main_id),
                        r.main_returns_result,
                        r.main_returns_app,
                        r.main_returns_default_err,
                        r.main_error_type.clone(),
                        r.main_error_is_packed,
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
        // The E3002 journey is a per-run buffer now that it drains at the report
        // edge, so it clears with the others: a failure this run recovered must
        // not prefix the next in-process run's report.
        jet_foundation::Outcome::jet_journey_reset();
        let ptr: *mut JitRuntime = runtime;
        Concurrency::set_active_runtime(Some(ptr));
        jet_codegen::scheduler::jet_scheduler_task_completion_begin();
        let entry_app = if main_returns_result {
            let entry: extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
            Some(entry())
        } else if main_returns_app {
            let entry: extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
            Some(entry())
        } else {
            let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
            entry();
            None
        };
        jet_codegen::scheduler::jet_scheduler_task_completion_drain();
        jet_codegen::scheduler::jet_scheduler_task_completion_end();
        Concurrency::settle_pending_after_native();
        jet_codegen::scheduler::jet_scheduler_drain();
        super::runtime_host::run_jit_atexit_handlers(runtime);
        jet_codegen::task_group::jet_task_deadline_clear_pending();
        Concurrency::set_active_runtime(None);
        Concurrency::clear_http_shared_runtime();
        if let Some(rendered) = runtime.deadline_exceeded.take() {
            // Match AOT `#Context(deadline:)`: keep prior stdout and emit the
            // compiler-owned E3003 without reclassifying it as a task panic.
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
        if let Some(msg) = runtime.trapped.take() {
            // Rich require/panic already wrote AOT-matching stderr and set exit_code.
            if msg == "__jet_rich_panic__" || runtime.exit_code.is_some() {
                let code = runtime.exit_code.take().unwrap_or(1);
                reset_run_heap(runtime);
                return Ok(RunOutcome::Ran {
                    stdout: runtime.stdout.clone(),
                    stderr: runtime.stderr.clone(),
                    exit_code: code,
                });
            }

            // The host has already marshalled this stop through the shared
            // Foundation renderer. Never invent a second engine-local report.
            let stderr = runtime.stderr.clone();
            let stdout = runtime.stdout.clone();
            reset_run_heap(runtime);
            return Ok(RunOutcome::Ran {
                stdout,
                stderr,
                exit_code: 70,
            });
        }
        if let Some(code) = runtime.exit_code.take() {
            let stdout = runtime.stdout.clone();
            let stderr = runtime.stderr.clone();
            reset_run_heap(runtime);
            return Ok(RunOutcome::Ran {
                stdout,
                stderr,
                exit_code: code,
            });
        }
        if let Some(handle) = entry_app {
            let result = jit_result(runtime, handle)
                .filter(|_| main_returns_result);
            if main_returns_result {
                let result = result
                    .ok_or_else(|| "jit fallible entry returned invalid Result handle".to_string())?;
                if !result.ok {
                    let message = if main_returns_default_err {
                        let error = runtime
                            .errors
                            .get((result.bits as i64).saturating_sub(1) as usize)
                            .ok_or_else(|| "jit fallible entry returned invalid Err handle".to_string())?;
                        jet_foundation::Outcome::jet_render_err(error)
                    } else if main_error_is_packed {
                        let Some(Type::Named(name)) = main_error_type.as_ref() else {
                            return Err("jit packed entry error lost its type".to_string());
                        };
                        Collections::render_packed_enum(
                            result.bits as i64,
                            name,
                            &runtime.heap,
                        )
                    } else {
                        runtime
                            .heap
                            .clone_string(result.bits as i64)
                            .ok_or_else(|| "jit fallible entry returned non-string error".to_string())?
                    };
                    // One report edge, shared with `jet_entry_report` in the
                    // AOT Prelude: the rendered error leads and the accumulated
                    // E3002 trail follows it, and only an escaping failure reports.
                    runtime
                        .stderr
                        .push_str(&jet_foundation::Outcome::jet_journey_report(&message));
                    return Ok(RunOutcome::Ran {
                        stdout: runtime.stdout.clone(),
                        stderr: runtime.stderr.clone(),
                        exit_code: 1,
                    });
                }
                if main_returns_app {
                    crate::Web::serve_app(result.bits as i64);
                }
            } else if main_returns_app {
                crate::Web::serve_app(handle);
            }
        }
        Ok(RunOutcome::Ran {
            stdout: runtime.stdout.clone(),
            stderr: runtime.stderr.clone(),
            exit_code: 0,
        })
    })
}

pub(crate) fn resident_run_fresh(
    program: &JitProgram,
    cap_bytes: Option<u64>,
) -> Result<RunOutcome, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(fresh_runtime_with_allocator_cap(cap_bytes))
    });
    super::tier_cache::begin_capture();
    let compiled = ensure_resident_module(program);
    if compiled.is_err() {
        super::tier_cache::abort_capture();
    }
    compiled?;
    let outcome = resident_invoke();
    if outcome.is_ok() {
        // Only a plan with an empty deopt list reaches this entry, so the whole
        // program is the tier roster. The artifact carries it because a warm
        // replay has no plan of its own to report to `--trace-tiers`.
        let native_fns: Vec<&str> = program.funcs.iter().map(|f| f.name.as_str()).collect();
        super::tier_cache::publish_capture(&native_fns);
    } else {
        super::tier_cache::abort_capture();
    }
    outcome
}

/// Mixed-tier run: Cranelift for covered funcs, interpreter stubs for named gaps.
pub(crate) fn resident_run_mixed(
    program: &JitProgram,
    plan: &TierPlan,
    cap_bytes: Option<u64>,
) -> Result<RunOutcome, String> {
    use cranelift_module::Module;

    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    clear_deopt_state();
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(fresh_runtime_with_allocator_cap(cap_bytes))
    });

    let mut deopt_index = HashMap::new();
    let mut deopt_names = Vec::new();
    for (name, _) in &plan.deopt {
        let idx = deopt_names.len() as i64;
        deopt_names.push(name.clone());
        deopt_index.insert(name.clone(), idx);
    }
    // SAFETY: `program` outlives the invoke below (same stack frame).
    install_deopt_program(program, &deopt_names);

    let main_returns_result = main_returns_result(program);
    let main_returns_default_err = main_returns_default_err(program);
    let main_error_type = main_error_type(program);
    let main_error_is_packed = main_error_is_packed(program);
    let main_returns_app = main_returns_app(program);
    let (mut module, host) = new_jit_module()?;
    let mut runtime = RESIDENT_RUNTIME
        .with(|slot| slot.borrow_mut().take())
        .unwrap_or_else(|| fresh_runtime_with_allocator_cap(cap_bytes));
    let main_id = compile_program_tiered(
        &mut module,
        &host,
        program,
        &mut runtime,
        None,
        &deopt_index,
    )?;
    runtime.snapshot_compile_strings();

    for f in &program.funcs {
        if !plan.native.contains(&f.name) {
            continue;
        }
        let sym = if f.name == program.entry {
            "__jet_jit_main".to_string()
        } else {
            super::types_meta::jit_fn_name(&f.name)
        };
        if let Some(cranelift_module::FuncOrDataId::Func(func_id)) = module.get_name(&sym) {
            let code = module.get_finalized_function(func_id);
            register_native_fn(f.name.clone(), code, f);
        }
    }
    install_native_hook();

    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
            main_returns_result,
            main_returns_app,
            main_returns_default_err,
            main_error_type,
            main_error_is_packed,
        });
    });
    resident_invoke()
}

pub(crate) fn resident_hot_swap(
    program: &JitProgram,
    cap_bytes: Option<u64>,
) -> Result<RunOutcome, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    // Rebuild the module (Cranelift rejects redefining `__jet_jit_main`) but keep
    // the live runtime heap — the M2 contract.
    crate::CoreHost::reset_jit_interrupts();
    let mut runtime = RESIDENT_RUNTIME
        .with(|slot| slot.borrow_mut().take())
        .unwrap_or_else(|| fresh_runtime_with_allocator_cap(cap_bytes));
    runtime.program_allocator.release_hosted_reservations();
    runtime.program_allocator = std::sync::Arc::new(cap_bytes.map_or_else(
        jet_codegen::program_allocator::JetProgramAllocator::system,
        jet_codegen::program_allocator::JetProgramAllocator::counting,
    ));
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    let (mut module, host) = new_jit_module()?;
    let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
    runtime.snapshot_compile_strings();
    let main_returns_result = main_returns_result(program);
    let main_returns_default_err = main_returns_default_err(program);
    let main_error_type = main_error_type(program);
    let main_error_is_packed = main_error_is_packed(program);
    let main_returns_app = main_returns_app(program);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
            main_returns_result,
            main_returns_app,
            main_returns_default_err,
            main_error_type,
            main_error_is_packed,
        });
    });
    resident_invoke()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_helper_fault_discards_raw_rust_text_at_the_engine_boundary() {
        let mut runtime = fresh_runtime();
        runtime.stdout.push_str("before\n");
        runtime.trapped =
            Some("thread 'main' panicked at crates/jet-jit/src/jit/runtime_host.rs".into());
        runtime.host_fault = true;

        let RunOutcome::Problems(diagnostics) =
            take_host_fault_outcome(&mut runtime).expect("host fault outcome")
        else {
            panic!("host helper fault returned the wrong outcome");
        };
        let (stdout, what) = diagnostics[0]
            .runtime_host_fault_parts()
            .expect("typed host fault");
        assert_eq!(stdout, "before\n");
        assert_eq!(what, "the JIT runtime helper failed");
        assert!(!what.contains("thread 'main' panicked"));
        assert!(!what.contains("runtime_host.rs"));
    }
}
