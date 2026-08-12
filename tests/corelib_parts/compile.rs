#[test]
fn invariant_refinement_proves_fixed_array_index() {
    let src = r#"
#Invariant("value >= 0 && value < 4")
Index4 :: distinct Int

fn pick(xs: [String#4], i: Index4) => String {
    return ~xs[i]
}

fn run() {
    words :: [String#4].{ "zero", "one", "two", "three" }
    print(pick(words, Index4.from_int(2)))
}
"#;
    let out = compile_temp("refinement_index.jet", src);
    assert!(
        !out.rust.contains("jet_index_vec(&"),
        "proof-carrying fixed-array index should not emit runtime list bounds helper:\n{}",
        out.rust
    );
}

#[test]
fn comptime_find_glob_records_sorted_lock_inputs() {
    let dir = std::env::temp_dir().join(format!(
        "jet_comptime_find_{}_{}",
        std::process::id(),
        "lock"
    ));
    fs::create_dir_all(dir.join("inputs/nested")).unwrap();
    fs::write(dir.join("inputs/alpha-1.txt"), "alpha").unwrap();
    fs::write(dir.join("inputs/nested/beta-2.txt"), "beta").unwrap();
    fs::write(dir.join("inputs/nested/gamma-3.txt"), "gamma").unwrap();
    fs::write(dir.join("inputs/nested/beta-2.md"), "skip").unwrap();
    let src = r#"
$paths :: find("inputs/**/{{alpha,beta}}-[0-9].t?t")

fn run() {
    print($paths.join("|"))
}
"#;
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected find fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let paths: Vec<&str> = out
        .comptime_inputs
        .iter()
        .map(|input| input.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["inputs/alpha-1.txt", "inputs/nested/beta-2.txt"]
    );
    assert!(out
        .comptime_inputs
        .iter()
        .all(|input| input.hash.len() == 64));
}


#[test]
fn core_args_audit_surface_runs_and_reports_suggestions() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_args_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.args as args

fn run() {
    spec :: args.spec()
        .flag_short("verbose", "v", "print extra detail")
        .option_env("profile", "config profile", "NAME", "JET_ARGS_PROFILE")
        .option_int("jobs", "worker count", "N")
        .repeat("tag", "classification tag", "TAG")
    parsed :: spec.parse(["tool", "-vv", "--jobs", "8", "--tag", "a", "--tag=b"]) ?? panic("parse failed")
    print(parsed.flag("verbose"))
    print(parsed.option("profile") ?? "")
    print(parsed.option_int("jobs") ?? 0)
    print(parsed.options("tag").len())
    if spec.parse(["tool", "--verbse"]) == {
        .Ok(_) -> {
            print("unexpected")
        }
        .Err(e) -> {
            print(e)
        }
    }
}
"#;
    let (_code, stdout, stderr) = build_and_run(
        &dir,
        "args_audit",
        src,
        &[("JET_ARGS_PROFILE", "dev")],
        None,
    );
    assert!(
        stdout.contains("unknown option `--verbse`")
            && stdout.contains("did you mean `--verbose`?"),
        "core.args suggestion missing:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.starts_with("true\ndev\n8\n2\n"));
}

#[test]
fn core_args_parse_or_exit_handles_cli_boundaries_and_keeps_parse_pure() {
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_args_exit_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.args as args
use core.io as io

fn run() {
    spec :: args.spec()
        .flag("verbose", "print extra detail")
    parsed :: spec.parse_or_exit(io.args())
    embedded :: spec.parse(["embedded", "--verbose"]) ?? panic("pure parse failed")
    print(parsed.flag("verbose"))
    print(embedded.flag("verbose"))
}
"#;
    let path = dir.join("args_parse_or_exit.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected parse_or_exit fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rust = dir.join("args_parse_or_exit.rs");
    let bin = dir.join("args_parse_or_exit");
    let mut command = Command::new("rustc");
    common::add_generated_rust(&mut command, &rust, &out.rust, out.ffi.is_some(), &[]);
    let built = command.arg("-o").arg(&bin).output().unwrap();
    assert!(
        built.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let normal = Command::new(&bin).arg("--verbose").output().unwrap();
    assert_eq!(normal.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&normal.stdout), "true\ntrue\n");
    assert!(normal.stderr.is_empty());

    let help = Command::new(&bin).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("Usage: args_parse_or_exit [options]"));
    assert!(help_stdout.contains("--help"));
    assert!(help.stderr.is_empty());

    let bad = Command::new(&bin).arg("--verbse").output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    let bad_stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(bad_stderr.contains("unknown option `--verbse`"));
    assert!(bad_stderr.contains("did you mean `--verbose`?"));
}

#[test]
fn core_args_nested_subcommand_does_not_overflow() {
    let src = r#"
use core.args as args

fn run() {
    serve :: args.spec()
        .option_int("port", "listen port", "PORT")
    spec :: args.spec()
        .flag_short("verbose", "v", "print extra detail")
        .option_int("jobs", "worker count", "N")
        .option_default("mode", "run mode", "MODE", "fast")
        .option_choice("color", "color policy", "WHEN", "auto,always,never")
        .repeat("tag", "classification tag", "TAG")
        .subcommand("serve", "run the server", serve)
        .version("args-audit 1.0")
    print(spec.help().contains("serve"))
}
"#;
    let out = compile_temp("args_nested_subcommand.jet", src);
    assert!(out.rust.contains("jet_args_subcommand"));
}

#[test]
fn core_os_facts_and_interrupt_hook_compile() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_os_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os

fn run() {
    os.on_interrupt(() => {
        print("interrupted")
    })
    print(os.name().len() > 0)
    print(os.family().len() > 0)
    print(os.arch().len() > 0)
    print(os.cpu_count() >= 1)
    print(os.pid() >= 1)
    print(os.hostname().len() > 0)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "os_facts", src, &[], None);
    assert_eq!(code, 0, "core.os program failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_named_and_indirect_callbacks_match_dev_tiers() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_interrupt_callback_forms_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();

    let aot_source = r#"
use core.os as os

fn named_callback() {
    print("named")
}

fn run() {
    indirect :: named_callback
    os.on_interrupt(named_callback)
    os.on_interrupt(indirect)
    print("registered")
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "named_indirect_aot", aot_source, &[], None);
    assert_eq!(code, 0, "named/indirect AOT callback program failed: {stderr}");
    assert_eq!(stdout, "registered\n");

    let dev_source = r#"
use core.os as os

fn named_callback() {
    print("named")
}

fn stop_callback() {
    print("stop")
    panic("stop")
}

fn run() {
    indirect :: stop_callback
    os.on_interrupt(named_callback)
    os.on_interrupt(indirect)
    loop {
        tick :: 0
    }
}
"#;
    let path = dir.join("named_indirect_dev.jet");
    fs::write(&path, dev_source).unwrap();

    for (tier, force_interpreter) in [("resident JIT", false), ("forced interpreter", true)] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command
            .args(["dev", path.to_str().unwrap(), "--watch=off"])
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if force_interpreter {
            command.arg("--interpret");
        } else {
            command.arg("--trace-tiers");
        }
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("spawn {tier} callback program: {error}"));
        std::thread::sleep(Duration::from_secs(5));
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        assert_eq!(
            unsafe { kill(child.id() as i32, 2) },
            0,
            "send SIGINT to {tier} callback program"
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .expect("collect timed-out callback child output");
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                panic!(
                    "{tier} callback program did not exit after SIGINT: stdout={stdout:?} stderr={stderr:?}"
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(output.stdout).expect("callback stdout is UTF-8");
        let stderr = String::from_utf8(output.stderr).expect("callback stderr is UTF-8");
        assert!(
            status.code() == Some(70),
            "{tier} callback program failed: stdout={stdout:?} stderr={stderr:?}"
        );
        assert_eq!(stdout, "named\nstop\n", "{tier} callback dispatch drifted");
        assert!(
            stderr.contains("panic: stop"),
            "{tier} callback lost the runtime panic diagnostic: {stderr:?}"
        );
        if force_interpreter {
            assert!(
                !stderr.contains("E2201") && !stderr.contains("unsupported"),
                "{tier} emitted an unsupported-feature diagnostic: {stderr}"
            );
        } else {
            assert!(
                stderr.contains("tier1 native"),
                "{tier} proof did not report a native tier: {stderr}"
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_os_interrupt_prelude_is_emitted_only_when_used() {
    let facts_only = compile_temp(
        "os_facts_only.jet",
        r#"
use core.os as os

fn run() {
    print(os.name())
}
"#,
    );
    assert!(
        !facts_only.rust.contains("mod jet_os_interrupt")
            && !facts_only.rust.contains("SetConsoleCtrlHandler")
            && !facts_only.rust.contains("jet_std_os_on_interrupt"),
        "ordinary core.os facts should not inherit signal FFI"
    );
    assert!(
        facts_only.rust.contains("JET_INTERRUPT_HANDLER_DEPTH")
            && facts_only.rust.contains("fn jet_runtime_should_unwind()"),
        "safe central panic-boundary state must remain available without signal FFI"
    );

    let with_interrupt = compile_temp(
        "os_interrupt.jet",
        r#"
use core.os as os

fn run() {
    os.on_interrupt(() => {
        print("interrupted")
    })
}
"#,
    );
    assert!(
        with_interrupt.rust.contains("mod jet_os_interrupt")
            && with_interrupt.rust.contains("SetConsoleCtrlHandler")
            && with_interrupt.rust.contains("CTRL_C_EVENT")
            && with_interrupt.rust.contains("AtomicUsize")
            && with_interrupt.rust.contains("catch_unwind")
            && with_interrupt.rust.contains("struct PanicBoundary")
            && with_interrupt.rust.contains("impl Drop for PanicBoundary")
            && with_interrupt.rust.contains("#[cfg(not(any(unix, windows)))]")
            && with_interrupt.rust.contains("interrupt handling is unavailable on this target")
            && with_interrupt.rust.contains("jet_std_os_on_interrupt")
            && !with_interrupt.rust.contains("let _ = handler"),
        "on_interrupt should keep its Unix/Windows dispatcher and no silent no-op"
    );
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_handlers_are_additive_and_ordered() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let dir = std::env::temp_dir().join(format!("jet_corelib_interrupt_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os
use core.process as process

fn run() {
    os.on_interrupt(() => { panic("first handler failed") })
    os.on_interrupt(() => {
        print("second")
        process.exit(0)
    })
    print("ready")
    loop { }
}

"#;
    let out = compile_temp("os_interrupt_runtime.jet", src);
    let rs = dir.join("main.rs");
    let bin = dir.join("interrupt-runtime");
    let mut command = Command::new("rustc");
    common::add_generated_rust(&mut command, &rs, &out.rust, out.ffi.is_some(), &[]);
    let rustc = command.arg("-o").arg(&bin).output().unwrap();
    assert!(rustc.status.success(), "rustc failed:\n{}", String::from_utf8_lossy(&rustc.stderr));

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n", "registration was not ready before run continued");
    unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let status = child.wait().unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(status.success(), "interrupt child failed: {status}");
    assert_eq!(rest, "second\n");
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_deadline_diagnostic_unwinds_inside_handler_boundary() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_interrupt_deadline_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os
use core.process as process
use core.time as time

fn run() {
    os.on_interrupt(() => {
        #Context(deadline: time.now()) {
            time.sleep(5)
        }
    })
    os.on_interrupt(() => {
        print("second")
        process.exit(0)
    })
    print("ready")
    loop { }
}
"#;
    let out = compile_temp("os_interrupt_deadline.jet", src);
    assert!(
        out.rust.contains("jet_interrupt_handler_panic_enter")
            && out.rust.contains("jet_interrupt_handler_panic_leave"),
        "interrupt handlers need a boundary distinct from scheduler-task identity"
    );
    let rs = dir.join("main.rs");
    let bin = dir.join("interrupt-deadline");
    let mut command = Command::new("rustc");
    common::add_generated_rust(&mut command, &rs, &out.rust, out.ffi.is_some(), &[]);
    let rustc = command.arg("-o").arg(&bin).output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc failed:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let output = child.wait_with_output().unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(
        output.status.success(),
        "interrupt child failed: {}",
        output.status
    );
    assert_eq!(rest, "second\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3003]: deadline exceeded while waiting in time sleep"));
    assert!(stderr.contains("Why: this wait point observed the task context deadline"));
    assert!(stderr.contains("Fix: raise the deadline budget or shorten the work"));
}

#[test]
fn core_os_interrupt_runtime_failures_use_the_boundary_aware_helpers() {
    let task_mem = include_str!("../../crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs");
    let time = include_str!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs");
    let scheduler = include_str!("../../crates/jet-codegen/src/Prelude/Scheduler.rs");
    let core = include_str!("../../crates/jet-codegen/src/Prelude/Core.rs");
    assert!(!task_mem.contains("process::exit(70)"));
    assert!(!time.contains("process::exit(70)"));
    assert_eq!(scheduler.matches("process::exit(70)").count(), 0);
    assert!(task_mem.contains("super::jet_panic("));
    // The deadline helper binds the rendered E3003 to a local so it can feed
    // interrupt handlers, native wait boundaries, and explicitly typed deadline
    // tasks while ordinary scheduler tasks retain the exact process-fatal E3003
    // diagnostic. It still routes its fatal path through `jet_runtime_diagnostic`,
    // never `process::exit`.
    assert!(time.contains("jet_runtime_diagnostic(rendered)"));
    assert!(time.contains("jet_interrupt_handler_should_unwind()"));
    assert!(time.contains("jet_scheduler_wait_boundary_should_unwind()"));
    assert!(time.contains("jet_typed_deadline_boundary_should_unwind()"));
    assert!(scheduler.contains("fn jet_scheduler_fatal(msg: &str) -> !"));
    assert!(scheduler.contains("jet_runtime_diagnostic(format!(\"panic: {msg}\")"));
    assert!(scheduler.contains("jet_runtime_diagnostic(rendered)"));
    assert!(scheduler.contains("struct JetSchedulerWaitBoundary"));
    assert!(scheduler.contains("let _boundary = JetSchedulerWaitBoundary::enter()"));
    assert!(scheduler.contains("struct JetTypedDeadlineBoundary"));
    let (ordinary_task_spawn, typed_task_spawn) = task_mem
        .split_once("pub(crate) fn spawn_typed_deadline")
        .expect("typed-deadline task spawn must remain explicit");
    assert!(!ordinary_task_spawn.contains("JetTypedDeadlineBoundary::enter()"));
    let typed_task_spawn = typed_task_spawn
        .split_once("pub fn pause")
        .expect("typed-deadline task spawn boundary")
        .0;
    assert!(typed_task_spawn.contains("let _typed_deadline_boundary"));
    assert!(typed_task_spawn.contains("super::JetTypedDeadlineBoundary::enter()"));
    assert_eq!(core.matches("std::process::exit(70)").count(), 1);
    assert!(core.contains(
        "Self::SchedulerFatal { msg } => jet_runtime_diagnostic(format!(\"panic: {msg}\"))"
    ));
    assert!(core.contains("fn jet_runtime_should_unwind() -> bool"));
    assert!(core.contains("jet_scheduler_in_task() || jet_interrupt_handler_should_unwind()"));
    assert!(core.contains("if jet_runtime_should_unwind()"));
    assert!(core.contains("if jet_interrupt_handler_should_unwind()"));
}


