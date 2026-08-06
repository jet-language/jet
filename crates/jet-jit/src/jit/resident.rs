use jet_codegen::Codegen::TIR::JitProgram;
use jet_foundation::{JitBackend::RunOutcome, AST::Type};
use std::collections::HashMap;

use super::deopt::{
    clear_deopt_state, install_deopt_program, install_native_hook, register_native_fn,
};
use super::functions_compile::{compile_program, compile_program_tiered};
use super::runtime_host::{jit_result, new_jit_module, ResidentModule};
use super::tiers::TierPlan;
use super::{Concurrency, JitRuntime, RESIDENT_MODULE, RESIDENT_RUNTIME};

pub(crate) fn fresh_runtime() -> JitRuntime {
    JitRuntime {
        source_file: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        heap: jet_rt::JetArena::default(),
        compile_strings: Vec::new(),
        invocations: 0,
        channels: Vec::new(),
        senders: Vec::new(),
        stream_consumers: std::collections::HashMap::new(),
        stream_producers: std::collections::HashMap::new(),
        stream_senders: std::collections::HashMap::new(),
        next_stream_channel: -1,
        next_stream_sender: -1,
        tasks: Vec::new(),
        task_controls: Vec::new(),
        task_groups: Vec::new(),
        cells: crate::Cell::CellState::new(),
        results: Vec::new(),
        solvers: Vec::new(),
        rngs: Vec::new(),
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
        pools: Vec::new(),
        shareds: Vec::new(),
        conditions: Vec::new(),
        expirings: Vec::new(),
        secrets: Vec::new(),
        crypto_values: Vec::new(),
        net_values: Vec::new(),
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
        trapped: None,
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
    rt.heap.install_string_slots(&compile_strings);
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
    rt.channels.clear();
    rt.senders.clear();
    rt.tasks.clear();
    rt.task_controls.clear();
    rt.task_groups.clear();
    rt.results.clear();
    rt.solvers.clear();
    rt.rngs.clear();
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
    rt.pools.clear();
    rt.shareds.clear();
    rt.conditions.clear();
    rt.expirings.clear();
    rt.secrets.clear();
}

pub(crate) fn resident_teardown() {
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
    let main_returns_result = program.funcs.iter().any(|func| {
        func.name == program.entry && matches!(func.ret, Some(Type::Result { .. }))
    });
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
            Ok(())
        })
    })
}

pub(crate) fn resident_invoke() -> Result<RunOutcome, String> {
    let (code, main_returns_result) = RESIDENT_MODULE
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|r| {
                    (
                        r.module.get_finalized_function(r.main_id),
                        r.main_returns_result,
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
        let ptr: *mut JitRuntime = runtime;
        Concurrency::set_active_runtime(Some(ptr));
        let entry_result = if main_returns_result {
            let entry: extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
            Some(entry())
        } else {
            let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
            entry();
            None
        };
        // A trap, deadline, cancellation, propagated error, or explicit early
        // return may bypass a generated lexical epilogue. Drain any surviving
        // groups before interpreting the run outcome.
        Concurrency::close_active_task_groups();
        Concurrency::settle_pending_after_native();
        jet_codegen::scheduler::jet_scheduler_drain();
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

            // Runtime arithmetic / host traps: same exit 70 + `panic:` wording
            // as AOT `jet_panic` (I2 / I9). Never reclassify a live-program trap
            // as E0953 "comptime stopped the build" (#1483).
            let mut stderr = runtime.stderr.clone();
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            stderr.push_str("panic: ");
            stderr.push_str(&msg);
            stderr.push('\n');
            let stdout = runtime.stdout.clone();
            reset_run_heap(runtime);
            return Ok(RunOutcome::Ran {
                stdout,
                stderr,
                exit_code: 70,
            });
        }
        if let Some(handle) = entry_result {
            let result = jit_result(runtime, handle)
                .ok_or_else(|| "jit fallible entry returned invalid Result handle".to_string())?;
            if !result.ok {
                let message = runtime
                    .heap
                    .clone_string(result.bits as i64)
                    .ok_or_else(|| "jit fallible entry returned non-string error".to_string())?;
                runtime.stderr.push_str(&message);
                runtime.stderr.push('\n');
                return Ok(RunOutcome::Ran {
                    stdout: runtime.stdout.clone(),
                    stderr: runtime.stderr.clone(),
                    exit_code: 1,
                });
            }
        }
        Ok(RunOutcome::Ran {
            stdout: runtime.stdout.clone(),
            stderr: runtime.stderr.clone(),
            exit_code: 0,
        })
    })
}

pub(crate) fn resident_run_fresh(program: &JitProgram) -> Result<RunOutcome, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));
    super::tier_cache::begin_capture();
    let compiled = ensure_resident_module(program);
    if compiled.is_err() {
        super::tier_cache::abort_capture();
    }
    compiled?;
    let outcome = resident_invoke();
    if outcome.is_ok() {
        super::tier_cache::publish_capture();
    } else {
        super::tier_cache::abort_capture();
    }
    outcome
}

/// Mixed-tier run: Cranelift for covered funcs, interpreter stubs for named gaps.
pub(crate) fn resident_run_mixed(program: &JitProgram, plan: &TierPlan) -> Result<RunOutcome, String> {
    use cranelift_module::Module;

    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    clear_deopt_state();
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));

    let mut deopt_index = HashMap::new();
    let mut deopt_names = Vec::new();
    for (name, _) in &plan.deopt {
        let idx = deopt_names.len() as i64;
        deopt_names.push(name.clone());
        deopt_index.insert(name.clone(), idx);
    }
    // SAFETY: `program` outlives the invoke below (same stack frame).
    install_deopt_program(program, &deopt_names);

    let main_returns_result = program.funcs.iter().any(|func| {
        func.name == program.entry && matches!(func.ret, Some(Type::Result { .. }))
    });
    let (mut module, host) = new_jit_module()?;
    let mut runtime = RESIDENT_RUNTIME
        .with(|slot| slot.borrow_mut().take())
        .unwrap_or_else(fresh_runtime);
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
            "jet_jit_main".to_string()
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
        });
    });
    resident_invoke()
}

pub(crate) fn resident_hot_swap(program: &JitProgram) -> Result<RunOutcome, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    // Rebuild the module (Cranelift rejects redefining `jet_jit_main`) but keep
    // the live runtime heap — the M2 contract.
    let mut runtime =
        RESIDENT_RUNTIME.with(|slot| slot.borrow_mut().take().unwrap_or_else(fresh_runtime));
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    let (mut module, host) = new_jit_module()?;
    let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
    runtime.snapshot_compile_strings();
    let main_returns_result = program.funcs.iter().any(|func| {
        func.name == program.entry && matches!(func.ret, Some(Type::Result { .. }))
    });
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
            main_returns_result,
        });
    });
    resident_invoke()
}
