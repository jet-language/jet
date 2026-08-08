//! Tower #1253 and #1255 — the `ProcessSpec` builders stay in the JIT host
//! table.
//!
//! `env_clear`, `detached`, and `terminal` had no entry in `lower_ctx` or in
//! `safety::resident_safe_call_arg`, so default `jet run` deopted and tier 0
//! then refused `process.cmd` with E0956. The program worked under `jet build`
//! and failed under `jet run`, which is the lens gap D-LENS-RUN1 forbids.
//!
//! `env` and `env_remove` had the same missing entries, and `cwd` had a host
//! shim that dropped its argument, so the child ran in the wrong directory and
//! nothing reported it.

use std::fs;
use std::path::Path;
use std::io::Write;
use std::process::{Command, Output, Stdio};

mod common;

use jet::Interpreter::dev_iteration;
use jet_foundation::JitBackend::RunOutcome;

#[test]
fn root_receiver_calls_share_the_direct_call_path() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_root_receiver_calls");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("root_calls.jet");
    fs::write(
        &file,
        r#"fn scale(#Root value: Int, factor: Int) => Int {
    return value * factor
}

fn add_half(#Root value: Float) => Float {
    return value + 0.5
}

fn run() {
    total :: 3
    print(total.scale(4))
    print(scale(total, 4))
    print(total.add_half())
    total.print()
}
"#,
    )
    .unwrap();

    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "#Root fixture diagnostics: {diagnostics:#?}");
    assert!(
        jet_jit::tir_lowers_bundle(&bundle),
        "#Root fixture must lower to TIR: {}",
        jet_jit::tir_lower_fail_reason(&bundle)
    );
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "#Root fixture must be resident-safe: {}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );

    let outcome = dev_iteration(file.to_str().unwrap(), false, false);
    let stdout = match outcome {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(diags) => panic!("#Root fixture failed: {diags:?}"),
    };
    assert_eq!(stdout, "12\n12\n3.5\n3\n");
    assert!(jet_jit::jit_executed_for_test(), "#Root fixture must execute in resident JIT");
    assert!(!jet_jit::fallback_invoked_for_test(), "#Root fixture must not fall back to the interpreter");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bounded_workers_example_has_total_tir() {
    // Sema and TIR lowering recurse per source-nesting level with large debug
    // frames (#1319). Direct calls to jet_jit's lowering helpers bypass the
    // 32 MiB compiler worker thread that public entry points like
    // `jet::compile` route through, so they still run on libtest's default
    // 2 MiB thread. bounded_workers's four sequential `tasks.spawn` closures
    // plus channel/generic inference push that over the edge. Same fix as
    // #1614/#1615: give this test the same 16 MiB thread jit_coverage_audit
    // uses, instead of overflowing.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(bounded_workers_example_has_total_tir_inner)
        .expect("bounded_workers TIR check thread")
        .join()
        .expect("bounded_workers TIR check thread panicked");
}

fn bounded_workers_example_has_total_tir_inner() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/concurrency/bounded_workers.jet");
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(
        jet_jit::tir_lowers_bundle(&bundle),
        "{}",
        jet_jit::tir_lower_fail_reason(&bundle)
    );
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "{}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
}

/// The Cranelift host path is not available on every architecture. Mirrors
/// `tests/dev.rs`: CI sets `JET_REQUIRE_CRANELIFT_HOST=1` so a missing host is
/// a loud failure, never a quiet green skip.
fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!(
            "cranelift-jit host path unsupported on this architecture \
             (JET_REQUIRE_CRANELIFT_HOST=1); remove the host from the parity \
             matrix or restore native JIT support"
        );
    } else {
        eprintln!(
            "note: cranelift-jit host path unsupported on this architecture; \
             skipping resident JIT assertion"
        );
        true
    }
}

/// Every zero-argument `ProcessSpec` builder runs resident, and each one
/// reports what the AOT lens reports:
///   * `env_clear` still runs the command and captures its output;
///   * `detached` drops the streams, so the output is empty;
///   * `terminal` runs through the native PTY backend and returns its output.
///
/// The expected text is the recorded `jet run --release` stdout for the same
/// program, so a divergence here is a lens gap.
#[test]
fn zero_arg_process_spec_builders_run_resident() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_zero_arg_builders");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("builders.jet");
    fs::write(
        &file,
        r#"use core.process as process

fn run() {
    cleared :: process.cmd(["echo", "x"]).env_clear().run() ?? panic("env_clear failed")
    print(cleared.success)
    print(cleared.output.trim())
    detached :: process.cmd(["echo", "y"]).detached().run() ?? panic("detached failed")
    print(detached.success)
    print("[{detached.output.trim()}]")
    if process.cmd(["printf", "z"]).terminal().run() == {
        .Ok(v) -> { print(v.output.contains("z")) }
        .Err(_) -> { print("terminal err") }
        else -> {}
    }
}
"#,
    )
    .unwrap();

    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, false);
    let stdout = match outcome {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(diags) => panic!(
            "zero-argument ProcessSpec builders must run under default `jet run`: {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        ),
    };
    assert_eq!(stdout, "true\nx\ntrue\n[]\ntrue\n");
    assert!(
        jet_jit::jit_executed_for_test(),
        "the builders must lower to host calls, not deopt"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "a missing host entry deopts to tier 0, which then raises E0956"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D (#1181): the expert policy,
/// checked capability keys, and child terminal handle stay resident too.
#[cfg(unix)]
#[test]
fn expert_terminal_model_runs_resident_and_fails_closed() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_terminal_policy");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("terminal_policy.jet");
    fs::write(
        &file,
        r#"use core.process as process

fn run() {
    policy :: TerminalPolicy.{
        size: TerminalSize.{ cols: 120, rows: 40 },
        mode: .Raw
    }
    plan :: process.cmd(["echo", "terminal"]).terminal(policy)
    print(plan.capabilities().has(TerminalFact.terminal))
    facts :: plan.capabilities()
    print(facts.has(TerminalFact.resize))
    print(facts.has(TerminalFact.raw))
    print(facts.has("preview_x"))
    if plan.run() == {
        .Ok(result) -> { print(result.output.contains("terminal")) }
        .Err(_) -> { print("terminal err") }
    }
    terminal_child :: process.cmd(["printf", "child"]).terminal().spawn() ?? panic("terminal spawn failed")
    if terminal_child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize.{ cols: 80, rows: 24 }) ?? panic("resize failed")
            print("terminal present")
        }
        .None -> { print("terminal absent") }
    }
    terminal_waited :: terminal_child.wait() ?? panic("terminal wait failed")
    print(terminal_waited.output.contains("child"))
    child :: process.cmd(["echo", "plain"]).stdout(.Capture).spawn() ?? panic("spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize.{ cols: 80, rows: 24 }) ?? panic("resize failed")
            print("terminal present")
        }
        .None -> { print("terminal absent") }
    }
    waited :: child.wait() ?? panic("wait failed")
    print(waited.output.trim())
}
"#,
    )
    .unwrap();

    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, false);
    let stdout = match outcome {
        RunOutcome::Ran { stdout, .. } => stdout,
        RunOutcome::Problems(diags) => panic!(
            "expert terminal model must run under default `jet run`: {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        ),
    };
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\nfalse\ntrue\nterminal present\ntrue\nterminal absent\nplain\n"
    );
    assert!(
        jet_jit::jit_executed_for_test(),
        "terminal policy and capabilities must lower to resident host calls"
    );
    assert!(
        !jet_jit::fallback_invoked_for_test(),
        "the expert terminal model must not deopt to tier 0"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn process_run_checked_matches_default_and_aot_lenses() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_run_checked");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("run_checked.jet");
    fs::write(
        &file,
        r#"use core.process as process

fn run() {
    ok :: process.cmd(["sh", "-c", "exit 0"]).run_checked() ?? panic("zero exit failed")
    print(ok.success)
    print(ok.code)

    plain :: process.cmd(["sh", "-c", "exit 7"]).run() ?? panic("plain run failed")
    print(plain.success)
    print(plain.code)

    if process.cmd(["sh", "-c", "printf 'checked-stderr-start:' >&2; printf '%05000d' 0 >&2; printf ':checked-stderr-end' >&2; exit 7"]).run_checked() == {
        .Ok(v) -> { print("checked:unexpected") }
        .Err(e) -> { print("checked-error") }
        else -> {}
    }

    if process.cmd(["sh", "-c", "printf 'signal-stderr' >&2; kill -TERM $$"]).run_checked() == {
        .Ok(v) -> { print("signal:unexpected") }
        .Err(e) -> { print("signal-error") }
        else -> {}
    }
}
"#,
    )
    .unwrap();

    let default = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg(&file)
        .arg("--trace-tiers")
        .env("JET_RUN_CACHE_DIR", dir.join("cache"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run checked process fixture through default lens");
    let release = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg("--release")
        .arg(&file)
        .env("NO_COLOR", "1")
        .output()
        .expect("run checked process fixture through AOT lens");

    assert_eq!(default.status.code(), Some(0), "{default:?}");
    assert_eq!(release.status.code(), Some(0), "{release:?}");
    assert_eq!(default.stdout, release.stdout);
    assert_eq!(
        String::from_utf8(default.stdout).unwrap(),
        "true\n0\nfalse\n7\nchecked-error\nsignal-error\n"
    );

    let trace = String::from_utf8(default.stderr).unwrap();
    assert!(
        trace.contains("run") && trace.contains("tier1 native"),
        "{trace}"
    );
    assert!(!trace.contains("tier0 interp"), "{trace}");
    assert!(!trace.contains("E0956"), "{trace}");

    let details = dir.join("run_checked_details.jet");
    fs::write(
        &details,
        r#"use core.process as process

fn run() {
    if process.cmd(["sh", "-c", "printf 'checked-stderr-start:' >&2; printf '%05000d' 0 >&2; printf ':checked-stderr-end' >&2; exit 7"]).run_checked() == {
        .Ok(v) -> { print("checked:unexpected") }
        .Err(e) -> { print(e) }
        else -> {}
    }
    if process.cmd(["sh", "-c", "printf 'signal-stderr' >&2; kill -TERM $$"]).run_checked() == {
        .Ok(v) -> { print("signal:unexpected") }
        .Err(e) -> { print(e) }
        else -> {}
    }
}
"#,
    )
    .unwrap();
    let detailed = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg("--release")
        .arg(&details)
        .env("NO_COLOR", "1")
        .output()
        .expect("run checked error detail fixture through AOT lens");
    assert_eq!(detailed.status.code(), Some(0), "{detailed:?}");
    let stdout = String::from_utf8(detailed.stdout).unwrap();
    assert!(stdout.contains("I/O error during close `sh`"), "{stdout}");
    assert!(stdout.contains("code=7"), "{stdout}");
    assert!(stdout.contains("stderr=checked-stderr-start:"), "{stdout}");
    assert!(!stdout.contains("checked-stderr-end"), "{stdout}");
    assert!(stdout.contains("code=-1, signal=15, stderr=signal-stderr"), "{stdout}");
    assert!(stdout.len() < 5000, "checked stderr was not bounded: {}", stdout.len());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prompt_helpers_preserve_behavior_through_named_deopt() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_prompt_named_deopt");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prompts.jet");
    fs::write(
        &file,
        r##"use core.io as io
use core.text as text

fn prompt_gap() => String {
    confirmed :: io.confirm("Continue?")
    choice :: io.choose("Choose:", ["staging", "production"]) ?? panic("choose failed")
    secret_kind := "unexpected"
    if io.input_secret("Secret: ") == {
        .Ok(_) -> { secret_kind = "secret" }
        .Err(error) -> {
            if error == {
                .InvalidInput(_) -> { secret_kind = "non-tty" }
                else -> { secret_kind = "other" }
            }
        }
    }
    return "{confirmed}|{choice}|{secret_kind}|{text.casefold("Straße")}"
}

fn run() {
    print(prompt_gap())
}
"##,
    )
    .unwrap();

    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).expect("prompt fixture loads");
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        !diags.iter().any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
        "prompt fixture must check: {diags:?}"
    );
    let plan = jet_jit::plan_bundle_tiers(&bundle);
    assert!(!plan.whole_interp, "regression needs named deopt: {plan:?}");
    assert!(
        plan.deopt.iter().any(|(name, _)| name == "prompt_gap"),
        "prompt_gap must be assigned to named deopt: {plan:?}"
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg(&file)
        .arg("--trace-tiers")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run prompt named-deopt fixture");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\nnot-a-number\n3\n2\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Continue? [y/N] "), "{stdout}");
    assert!(stdout.contains("  1) staging\n  2) production\n"), "{stdout}");
    assert!(stdout.contains("Enter a number from 1 to 2."), "{stdout}");
    assert!(stdout.ends_with("false|production|non-tty|strasse\n"), "{stdout}");
    let trace = String::from_utf8(output.stderr).unwrap();
    assert!(
        trace.contains("prompt_gap") && trace.contains("tier0 interp"),
        "{trace}"
    );
    assert!(!trace.contains("E0956"), "{trace}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn args_parse_or_exit_runs_resident_for_return_help_and_usage_error() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_args_parse_or_exit");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("args.jet");
    fs::write(
        &file,
        r#"use core.args as args
use core.io as io

fn run() {
    spec :: args.spec()
        .flag("verbose", "print extra detail")
    parsed :: spec.parse_or_exit(io.args())
    print(parsed.flag("verbose"))
}
"#,
    )
    .unwrap();

    let run = |case: &str, arg: &str| {
        Command::new(env!("CARGO_BIN_EXE_jet"))
            .arg("run")
            .arg(&file)
            .arg("--trace-tiers")
            .arg("--")
            .arg(arg)
            .env("JET_RUN_CACHE_DIR", dir.join(format!("cache_{case}")))
            .env("NO_COLOR", "1")
            .output()
            .expect("run parse_or_exit resident fixture")
    };
    let assert_no_fallback = |case: &str, output: &Output| {
        let trace = String::from_utf8_lossy(&output.stderr);
        assert!(!trace.contains("tier0 interp"), "{case} trace:\n{trace}");
        assert!(!trace.contains("E0956"), "{case} trace:\n{trace}");
    };

    let normal = run("normal", "--verbose");
    assert_eq!(normal.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&normal.stdout), "true\n");
    let normal_trace = String::from_utf8_lossy(&normal.stderr);
    assert!(
        normal_trace.contains("run") && normal_trace.contains("tier1 native"),
        "normal trace:\n{normal_trace}"
    );
    assert_no_fallback("normal", &normal);

    let help = run("help", "--help");
    assert_eq!(help.status.code(), Some(0));
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(
        help_stdout.contains("Usage: ") && help_stdout.contains("[options]"),
        "{help_stdout}"
    );
    assert!(help_stdout.contains("--help"), "{help_stdout}");
    assert_no_fallback("help", &help);

    let bad = run("bad", "--verbse");
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    let bad_stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(bad_stderr.contains("unknown option `--verbse`"), "{bad_stderr}");
    assert!(
        bad_stderr.contains("did you mean `--verbose`?"),
        "{bad_stderr}"
    );
    assert_no_fallback("bad", &bad);

    let _ = fs::remove_dir_all(&dir);
}

fn jet_string(value: &Path) -> String {
    value
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn run_jet(file: &Path, release: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.arg("run");
    if release {
        command.arg("--release");
    }
    command
        .arg(file)
        .env("NO_COLOR", "1")
        .env("JET_SPEC_REMOVE", "host-value")
        .output()
        .expect("run ProcessSpec lens fixture")
}

/// Portable child for the ProcessSpec fixture. The fixture invokes this test
/// binary by absolute path, so it needs no shell or platform utility.
#[test]
fn process_probe_helper() {
    if std::env::var("JET_PROCESS_PROBE").as_deref() != Ok("1") {
        return;
    }
    let cwd = std::env::current_dir().unwrap();
    let logical = std::env::var("JET_LOGICAL_ENV").unwrap();
    let spec_set = std::env::var("JET_SPEC_SET").unwrap();
    let removed = std::env::var_os("JET_SPEC_REMOVE").is_none();
    fs::write(
        "process-probe.txt",
        format!(
            "cwd={}|logical={logical}|set={spec_set}|removed={removed}",
            cwd.display()
        ),
    )
    .unwrap();
}

/// Argument-taking builders stay resident and match the AOT lens byte for
/// byte. The child also proves that `core.env` mutations and ProcessSpec
/// overrides share one logical environment.
#[test]
fn arg_process_spec_builders_reach_the_child() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let dir = common::unique_tmp("jit_process_arg_builders");
    fs::create_dir_all(&dir).unwrap();
    let child_dir = fs::canonicalize(&dir).unwrap();
    let child_probe = child_dir.join("process-probe.txt");
    let bad_value_path = child_dir.join("bad-env-value.txt");
    fs::write(&bad_value_path, b"bad\0value").unwrap();
    let test_binary = std::env::current_exe().unwrap();
    let file = dir.join("builders.jet");
    fs::write(
        &file,
        format!(
            r#"use core.env as env
use core.files as files
use core.process as process

fn run() {{
    env.set("JET_PROCESS_PROBE", "1")
    env.set("JET_LOGICAL_ENV", "logical-value")
    print((env.get("JET_LOGICAL_ENV") ?? "missing") == "logical-value")
    names :: env.vars() ?? panic("env vars failed")
    print(names.contains("JET_LOGICAL_ENV"))
    env.set("JET_LOGICAL_GONE", "gone")
    print(env.unset("JET_LOGICAL_GONE") ?? false)
    print((env.get("JET_LOGICAL_GONE") ?? "missing") == "missing")
    child :: process.cmd(["{test_binary}", "--exact", "process_probe_helper", "--nocapture"])
        .cwd("{child_dir}")
        .env("JET_SPEC_SET", "spec-value")
        .env_remove("JET_SPEC_REMOVE")
        .run() ?? panic("child failed")
    print(child.success)
    print(files.read("{child_probe}") ?? panic("probe read failed"))
    if process.cmd(["{test_binary}"]).env("BAD=NAME", "value").run() == {{
        .Ok(v) -> {{ print("process name accepted") }}
        .Err(e) -> {{ print("process name rejected") }}
        else -> {{}}
    }}
    bad_value :: files.read("{bad_value_path}") ?? panic("bad value read failed")
    if process.cmd(["{test_binary}"]).env("JET_BAD_VALUE", bad_value).run() == {{
        .Ok(v) -> {{ print("process value accepted") }}
        .Err(e) -> {{ print("process value rejected") }}
        else -> {{}}
    }}
}}
"#,
            test_binary = jet_string(&test_binary),
            child_dir = jet_string(&child_dir),
            child_probe = jet_string(&child_probe),
            bad_value_path = jet_string(&bad_value_path),
        ),
    )
    .unwrap();

    // Default `jet run` cannot deopt this program: tier 0 rejects
    // `process.cmd` with E0956. Removing any cwd/env/env_remove residency or
    // dispatch entry therefore makes this command fail.
    let default = run_jet(&file, false);
    let release = run_jet(&file, true);
    assert_eq!(
        default.status.code(),
        release.status.code(),
        "default stdout:\n{}\ndefault stderr:\n{}\nrelease stdout:\n{}\nrelease stderr:\n{}",
        String::from_utf8_lossy(&default.stdout),
        String::from_utf8_lossy(&default.stderr),
        String::from_utf8_lossy(&release.stdout),
        String::from_utf8_lossy(&release.stderr),
    );
    assert_eq!(default.stdout, release.stdout);
    let release_stderr = String::from_utf8(release.stderr.clone()).unwrap();
    let (_, release_program_stderr) = release_stderr
        .split_once('\n')
        .filter(|(line, _)| line.starts_with("effects: "))
        .expect("release lens must report its compile-time effect summary");
    assert_eq!(
        String::from_utf8_lossy(&default.stderr),
        release_program_stderr
    );
    assert!(
        default.status.success(),
        "default lens failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&default.stdout),
        String::from_utf8_lossy(&default.stderr)
    );
    let stdout = String::from_utf8_lossy(&default.stdout);
    assert!(
        stdout.starts_with("true\ntrue\ntrue\ntrue\ntrue\ncwd="),
        "{stdout}"
    );
    assert!(stdout.contains("|logical=logical-value|set=spec-value|removed=true\n"));
    assert!(stdout.ends_with("process name rejected\nprocess value rejected\n"));

    let _ = fs::remove_dir_all(&dir);
}
