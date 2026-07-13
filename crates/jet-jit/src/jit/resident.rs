use jet_codegen::Codegen::TIR::JitProgram;
use jet_foundation::{Diagnostics::Diagnostic, JitBackend::RunOutcome, AST::Type};

use super::functions_compile::compile_program;
use super::runtime_host::{jit_result, new_jit_module, ResidentModule};
use super::{Concurrency, JitRuntime, RESIDENT_MODULE, RESIDENT_RUNTIME};

pub(crate) fn fresh_runtime() -> JitRuntime {
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
        results: Vec::new(),
        solvers: Vec::new(),
        trapped: None,
        deadline_exceeded: None,
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

fn jit_deadline_diag(rendered: &str) -> Diagnostic {
    let wait = rendered
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Error [E3003]: "))
        .unwrap_or("deadline exceeded while waiting in a scheduler wait point");
    Diagnostic::error(
        "E3003",
        wait.to_string(),
        "this wait point observed the task context deadline from `#Context(deadline: …)`"
            .to_string(),
        "raise the deadline budget or shorten the work before this wait point".to_string(),
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
    rt.results.clear();
    rt.solvers.clear();
}

pub(crate) fn resident_teardown() {
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    Concurrency::set_active_runtime(None);
}

pub(crate) fn ensure_resident_module(program: &JitProgram) -> Result<(), String> {
    let main_returns_result = program.funcs.iter().any(|func| {
        func.name == "run" && matches!(func.ret, Some(Type::Result { .. }))
    });
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
            resident.main_returns_result = main_returns_result;
            Ok(())
        })
    })
}

fn resident_invoke() -> Result<RunOutcome, String> {
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
        Concurrency::settle_pending_after_native();
        jet_codegen::scheduler::jet_scheduler_drain();
        Concurrency::set_active_runtime(None);
        if let Some(rendered) = runtime.deadline_exceeded.take() {
            reset_run_heap(runtime);
            return Ok(RunOutcome::Problems(vec![jit_deadline_diag(&rendered)]));
        }
        if let Some(msg) = runtime.trapped.take() {
            // A runtime panic unwound to `main`'s epilogue via the trapped-flag
            // branches (no Rust panic crossed a JIT frame — I1). Report it exactly
            // as the tier-0 interpreter reports the same panic (E0953), and scrub
            // the partial run's heap so the next hot-reload iteration in this
            // resident process starts clean.
            reset_run_heap(runtime);
            return Ok(RunOutcome::Problems(vec![jit_panic_diag(&msg)]));
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
    resident_teardown();
    RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));
    ensure_resident_module(program)?;
    resident_invoke()
}

pub(crate) fn resident_hot_swap(program: &JitProgram) -> Result<RunOutcome, String> {
    // Rebuild the module (Cranelift rejects redefining `jet_jit_main`) but keep
    // the live runtime heap — the M2 contract.
    let mut runtime =
        RESIDENT_RUNTIME.with(|slot| slot.borrow_mut().take().unwrap_or_else(fresh_runtime));
    RESIDENT_MODULE.with(|slot| *slot.borrow_mut() = None);
    let (mut module, host) = new_jit_module()?;
    let main_id = compile_program(&mut module, &host, program, &mut runtime, None)?;
    let main_returns_result = program.funcs.iter().any(|func| {
        func.name == "run" && matches!(func.ret, Some(Type::Result { .. }))
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
