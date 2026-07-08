fn fresh_runtime() -> JitRuntime {
    JitRuntime {
        source_file: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        heap: jet_rt::JetArena::default(),
        invocations: 0,
        channels: Vec::new(),
        senders: Vec::new(),
        tasks: Vec::new(),
        task_controls: Vec::new(),
        trapped: None,
    }
}

/// Build the E0953 diagnostic for a trapped run, matching the tier-0
/// interpreter's own voice for the identical panic (the dev interpreter IS the
/// comptime tree-walker, so its runtime panics already render this way — see
/// `crates/jet-comptime/src/Comptime/Diagnostics.rs::comptime_panic`). The JIT
/// tier must report the SAME code/voice, not a new one, for parity.
fn jit_panic_diag(msg: &str) -> Diagnostic {
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        format!("while computing this value at compile time, the program panicked: {msg}"),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        None,
    )
}

/// Scrub heap state a trapped (partial) run created, so the NEXT resident
/// invocation (hot-reload iteration or plain re-run) in this same process
/// starts clean — a crashed run must never leak lists/strings/channels/tasks
/// into the following one. `source_file`/`invocations` are run-loop
/// bookkeeping, not per-run heap, and are left alone.
fn reset_run_heap(rt: &mut JitRuntime) {
    rt.heap.clear();
    rt.channels.clear();
    rt.senders.clear();
    rt.tasks.clear();
    rt.task_controls.clear();
}

fn resident_teardown() {
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    Concurrency::set_active_runtime(None);
}

fn ensure_resident_module(program: &JitProgram) -> Result<(), String> {
    let need_create = RESIDENT_MODULE.with(|slot| slot.borrow().is_none());
    if need_create {
        let (mut module, host) = new_jit_module()?;
        let mut runtime = RESIDENT_RUNTIME
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(fresh_runtime);
        let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
        RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
        RESIDENT_MODULE.with(|slot| {
            *slot.borrow_mut() = Some(ResidentModule {
                module,
                host,
                main_id,
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
            Ok(())
        })
    })
}

fn resident_invoke() -> Result<RunOutcome, String> {
    let code = RESIDENT_MODULE
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|r| r.module.get_finalized_function(r.main_id))
        })
        .ok_or_else(|| "resident module missing".to_string())?;

    RESIDENT_RUNTIME.with(|slot| {
        let mut rt_guard = slot.borrow_mut();
        let runtime = rt_guard.as_mut().ok_or("resident runtime missing")?;
        runtime.invocations += 1;
        runtime.stdout.clear();
        runtime.stderr.clear();
        let ptr: *mut JitRuntime = runtime;
        Concurrency::set_active_runtime(Some(ptr));
        let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
        entry();
        jet_codegen::scheduler::jet_scheduler_drain();
        Concurrency::set_active_runtime(None);
        if let Some(msg) = runtime.trapped.take() {
            // A runtime panic unwound to `main`'s epilogue via the trapped-flag
            // branches (no Rust panic crossed a JIT frame — I1). Report it exactly
            // as the tier-0 interpreter reports the same panic (E0953), and scrub
            // the partial run's heap so the next hot-reload iteration in this
            // resident process starts clean.
            reset_run_heap(runtime);
            return Ok(RunOutcome::Problems(vec![jit_panic_diag(&msg)]));
        }
        Ok(RunOutcome::Ran {
            stdout: runtime.stdout.clone(),
            stderr: runtime.stderr.clone(),
            exit_code: 0,
        })
    })
}

fn resident_run_fresh(program: &JitProgram) -> Result<RunOutcome, String> {
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));
    ensure_resident_module(program)?;
    resident_invoke()
}

fn resident_hot_swap(program: &JitProgram) -> Result<RunOutcome, String> {
    // Rebuild the module (Cranelift rejects redefining `jet_jit_main`) but keep
    // the live runtime heap — the M2 contract.
    let mut runtime =
        RESIDENT_RUNTIME.with(|slot| slot.borrow_mut().take().unwrap_or_else(fresh_runtime));
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    let (mut module, host) = new_jit_module()?;
    let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
    RESIDENT_MODULE.with(|slot| {
        *slot.borrow_mut() = Some(ResidentModule {
            module,
            host,
            main_id,
        });
    });
    resident_invoke()
}
