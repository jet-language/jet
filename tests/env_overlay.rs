mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet::JitBackend::JitBackend;
use jet_jit::CraneliftBackend;

fn compile(name: &str, src: &str) -> (PathBuf, jet::CompileOutput) {
    let dir = std::env::temp_dir().join(format!("jet_env_overlay_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    (dir, out)
}

fn build(dir: &Path, name: &str, out: &jet::CompileOutput) -> PathBuf {
    build_rust(dir, name, &out.rust)
}

fn build_rust(dir: &Path, name: &str, rust: &str) -> PathBuf {
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    bin
}

#[cfg(unix)]
#[test]
fn mutation_enumeration_and_child_snapshot_run() {
    let src = r#"
use core.sys as env
use core.process as process

fn run() {
    env.set("JET_OVERLAY_A", "one")
    env.set("JET_OVERLAY_B", "two")
    print(env.get("JET_OVERLAY_A") ?? "missing")
    names :: env.vars() ?? panic("vars failed")
    print(names.join(",").contains("JET_OVERLAY_A,JET_OVERLAY_B"))
    print(env.unset("JET_OVERLAY_A") ?? false)
    print(env.unset("JET_OVERLAY_A") ?? true)
    inherited :: process.cmd(["/usr/bin/env"]).run() ?? panic("child failed")
    print(inherited.output.contains("JET_OVERLAY_B=two"))
    print(!inherited.output.contains("JET_OVERLAY_A="))
    cleared :: process.cmd(["/usr/bin/env"])
        .env_clear()
        .env("ONLY_THIS", "yes")
        .env_remove("JET_OVERLAY_B")
        .run() ?? panic("clear child failed")
    print(cleared.output == "ONLY_THIS=yes\n")
    removed_wins :: process.cmd(["/usr/bin/env"])
        .env_clear()
        .env("SAME_KEY", "set-first")
        .env_remove("SAME_KEY")
        .run() ?? panic("precedence child failed")
    print(!removed_wins.output.contains("SAME_KEY="))
}
"#;
    let (dir, out) = compile("mutation", src);
    let bin = build(&dir, "mutation", &out);
    // Exceed a typical pipe buffer so process.run must drain the child's
    // captured output while it waits. The logical snapshot inherits this raw
    // host entry, and `/usr/bin/env` writes it before exiting.
    let run = Command::new(bin)
        .current_dir(&dir)
        .env("JET_OVERLAY_PIPE_PRESSURE", "x".repeat(96 * 1024))
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "one\ntrue\ntrue\nfalse\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn codegen_is_raw_locked_and_never_mutates_host_env() {
    let (_, out) = compile(
        "shape",
        r#"
use core.sys as env
use core.process as process

fn remove(name: String) Bool ! env.EnvError {
    return Ok(env.unset(name)?)
}

fn run() {
    env.set("MODE", "test")
    removed :: remove("CI")
    names :: env.vars()
    child :: process.run(["worker"])
}
"#,
    );
    assert!(out.rust.contains("std::env::vars_os()"));
    assert!(out.rust.contains("type JetEnvEntries = Vec<(std::ffi::OsString, std::ffi::OsString)>"));
    assert!(out.rust.contains("RwLock<JetEnvEntries>"));
    assert!(out.rust.contains("fn main() {\n    jet_std_env_init();"));
    assert!(out.rust.contains("command.env_clear()"));
    assert!(out.rust.contains("command.envs(child_env)"));
    assert!(out.rust.contains("std::thread::spawn(move ||"));
    let drain = out
        .rust
        .find("let drains = jet_process_start_output_drain(child);")
        .expect("process wait must start capture drains");
    let wait = out
        .rust
        .find("inner.try_wait()")
        .expect("process wait must poll child status");
    assert!(drain < wait, "capture drains must start before child wait");
    assert!(out.rust.contains("i32::try_from(left.len())"));
    assert!(out.rust.contains("2 => std::cmp::Ordering::Equal"));
    assert!(out.rust.contains("_ => left.cmp(&right)"));
    assert!(!out.rust.contains("std::env::set_var"));
    assert!(!out.rust.contains("std::env::remove_var"));
}

#[cfg(unix)]
#[test]
fn raw_non_unicode_value_survives_child_snapshot_and_vars_fails_whole_snapshot() {
    use std::os::unix::ffi::OsStringExt;

    let src = r#"
use core.sys as env
use core.process as process

fn run() {
    child :: process.run(["./raw_probe"]) ?? panic("raw child failed")
    print(child.output)
    if env.vars() == {
        .Ok(_) -> { print("unexpected vars success") }
        .Err(e) -> { print(e) }
    }
}
"#;
    let (dir, out) = compile("raw", src);
    let probe_rs = dir.join("raw_probe.rs");
    fs::write(
        &probe_rs,
        r#"
use std::os::unix::ffi::OsStrExt;
fn main() {
    let raw_value = std::env::var_os("JET_RAW_VALUE").expect("missing raw value");
    println!("{}", raw_value.as_os_str().as_bytes() == [0x66, 0x80, 0x6f]);
    let raw_name = [b'J', b'E', b'T', b'_', 0x81];
    let found = std::env::vars_os().any(|(name, value)| {
        name.as_os_str().as_bytes() == raw_name
            && value.as_os_str().as_bytes() == [0x76, 0x82, 0x6c]
    });
    println!("{}", found);
}
"#,
    )
    .unwrap();
    let probe_build = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&probe_rs)
        .arg("-o")
        .arg(dir.join("raw_probe"))
        .output()
        .unwrap();
    assert!(probe_build.status.success(), "{}", String::from_utf8_lossy(&probe_build.stderr));
    let bin = build(&dir, "raw", &out);
    let run = Command::new(bin)
        .current_dir(&dir)
        .env(
            "JET_RAW_VALUE",
            std::ffi::OsString::from_vec(vec![0x66, 0x80, 0x6f]),
        )
        .env(
            std::ffi::OsString::from_vec(vec![b'J', b'E', b'T', b'_', 0x81]),
            std::ffi::OsString::from_vec(vec![0x76, 0x82, 0x6c]),
        )
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\ntrue\n\nenvironment contains a name or value that is not valid Unicode\n"
    );
}

#[test]
fn eager_snapshot_and_host_getenv_isolation_run_in_process() {
    let src = r#"
use core.sys as env

fn run() {
    print(env.get("JET_HOST_ISOLATION") ?? "missing")
    env.set("JET_HOST_ISOLATION", "jet-owned")
    print(env.get("JET_HOST_ISOLATION") ?? "missing")
}
"#;
    let (dir, out) = compile("host_isolation", src);
    // Test-only vetted probe: mutate the real host block after main's mandatory
    // snapshot and verify both directions of isolation in this same process.
    let rust = out.rust.replacen(
        "    jet_std_env_init();\n    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n    jet_runtime_boundary(|| __jet_run());",
        "    jet_std_env_init();\n    std::env::set_var(\"JET_HOST_ISOLATION\", \"host-after-snapshot\");\n    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n    jet_runtime_boundary(|| __jet_run());\n    assert_eq!(std::env::var(\"JET_HOST_ISOLATION\").as_deref(), Ok(\"host-after-snapshot\"));",
        1,
    );
    assert_ne!(rust, out.rust, "test probe did not attach after eager init");
    let bin = build_rust(&dir, "host_isolation", &rust);
    let run = Command::new(bin)
        .current_dir(&dir)
        .env("JET_HOST_ISOLATION", "host-before-snapshot")
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "host-before-snapshot\njet-owned\n"
    );
}

#[cfg(unix)]
#[test]
fn concurrent_mutation_and_real_child_spawns_take_untorn_snapshots() {
    let (dir, out) = compile(
        "atomic_spawn",
        "use core.sys as env\nfn run() { print(env.get(\"JET_PAIR_UNUSED\") ?? \"\") }\n",
    );
    let pair_probe_rs = dir.join("pair_probe.rs");
    fs::write(
        &pair_probe_rs,
        r#"
use std::os::unix::ffi::OsStrExt;

fn main() {
    let left_name = [b'J', b'E', b'T', b'_', b'P', b'A', b'I', b'R', b'_', 0x81];
    let right_name = [b'J', b'E', b'T', b'_', b'P', b'A', b'I', b'R', b'_', 0x82];
    let mut left = None;
    let mut right = None;
    for (name, value) in std::env::vars_os() {
        if name.as_os_str().as_bytes() == left_name {
            left = Some(value);
        } else if name.as_os_str().as_bytes() == right_name {
            right = Some(value);
        }
    }
    let state = match (left, right) {
        (Some(left), Some(right))
            if left.as_os_str().as_bytes() == [b'A', 0x91]
                && right.as_os_str().as_bytes() == [b'A', 0x91] => "A",
        (Some(left), Some(right))
            if left.as_os_str().as_bytes() == [b'B', 0x92]
                && right.as_os_str().as_bytes() == [b'B', 0x92] => "B",
        (Some(_), Some(_)) => "MIXED",
        _ => "MISSING",
    };
    println!("{state}");
}
"#,
    )
    .unwrap();
    let pair_probe_build = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&pair_probe_rs)
        .arg("-o")
        .arg(dir.join("pair_probe"))
        .output()
        .unwrap();
    assert!(
        pair_probe_build.status.success(),
        "{}",
        String::from_utf8_lossy(&pair_probe_build.stderr)
    );

    // Test-only hook pauses the first child exactly as it enters the logical
    // snapshot, forcing the writer to commit a new pair during that launch.
    // No hook or synchronization state ships in generated production code.
    let hooked_snapshot = out.rust.replacen(
        "fn jet_std_env_snapshot_raw() -> JetEnvEntries {\n    jet_env_read().clone()\n}",
        r#"fn jet_std_env_snapshot_raw() -> JetEnvEntries {
    jet_test_snapshot_entry();
    jet_env_read().clone()
}

static JET_TEST_SNAPSHOT_ENTERED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static JET_TEST_FIRST_COMMIT_DONE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static JET_TEST_SNAPSHOT_HOOK_USED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn jet_test_snapshot_entry() {
    use std::sync::atomic::Ordering;
    if JET_TEST_SNAPSHOT_HOOK_USED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        JET_TEST_SNAPSHOT_ENTERED.store(true, Ordering::Release);
        while !JET_TEST_FIRST_COMMIT_DONE.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }
}"#,
        1,
    );
    assert_ne!(
        hooked_snapshot, out.rust,
        "snapshot-entry test hook did not attach"
    );

    let probe = r#"    jet_std_env_init();
    use std::os::unix::ffi::OsStringExt;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let left_name = std::ffi::OsString::from_vec(vec![b'J', b'E', b'T', b'_', b'P', b'A', b'I', b'R', b'_', 0x81]);
    let right_name = std::ffi::OsString::from_vec(vec![b'J', b'E', b'T', b'_', b'P', b'A', b'I', b'R', b'_', 0x82]);
    {
        let mut entries = jet_env_write();
        entries.retain(|(name, _)| {
            !jet_env_key_eq(name.as_os_str(), left_name.as_os_str())
                && !jet_env_key_eq(name.as_os_str(), right_name.as_os_str())
        });
        entries.push((left_name.clone(), std::ffi::OsString::from_vec(vec![b'A', 0x91])));
        entries.push((right_name.clone(), std::ffi::OsString::from_vec(vec![b'A', 0x91])));
    }
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let generation = std::sync::Arc::new(AtomicU64::new(0));
    let writer_stop = stop.clone();
    let writer_generation = generation.clone();
    let writer_left = left_name.clone();
    let writer_right = right_name.clone();
    let writer = std::thread::spawn(move || {
        while !JET_TEST_SNAPSHOT_ENTERED.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let mut use_b = true;
        while !writer_stop.load(Ordering::Acquire) {
            let value = if use_b {
                std::ffi::OsString::from_vec(vec![b'B', 0x92])
            } else {
                std::ffi::OsString::from_vec(vec![b'A', 0x91])
            };
            {
                let mut entries = jet_env_write();
                entries.retain(|(name, _)| {
                    !jet_env_key_eq(name.as_os_str(), writer_left.as_os_str())
                        && !jet_env_key_eq(name.as_os_str(), writer_right.as_os_str())
                });
                entries.push((writer_left.clone(), value.clone()));
                entries.push((writer_right.clone(), value));
            }
            writer_generation.fetch_add(1, Ordering::AcqRel);
            JET_TEST_FIRST_COMMIT_DONE.store(true, Ordering::Release);
            use_b = !use_b;
            std::thread::yield_now();
        }
    });
    let mut advanced_during_launch = false;
    let mut observations = Vec::new();
    let mut launch_error = None;
    for _ in 0..16 {
        let before = generation.load(Ordering::Acquire);
        let result = match jet_std_process_run(&vec!["./pair_probe".to_string()]) {
            Ok(result) => result,
            Err(error) => {
                launch_error = Some(format!("{error:?}"));
                break;
            }
        };
        let after = generation.load(Ordering::Acquire);
        advanced_during_launch |= after > before;
        observations.push(result.output.trim().to_string());
    }
    stop.store(true, Ordering::Release);
    writer.join().unwrap();
    assert!(launch_error.is_none(), "child launch failed: {launch_error:?}");
    assert_eq!(observations.len(), 16, "not every real child probe completed");
    for state in observations {
        assert!(state == "A" || state == "B", "child observed {state} raw pair");
    }
    assert!(advanced_during_launch, "writer generation never advanced across a child launch");
    jet_runtime_boundary(|| __jet_run());"#;
    let rust = hooked_snapshot.replacen(
        "    jet_std_env_init();\n    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n    jet_runtime_boundary(|| __jet_run());",
        probe,
        1,
    );
    assert_ne!(rust, hooked_snapshot, "atomicity probe did not attach");
    let bin = build_rust(&dir, "atomic_spawn", &rust);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
}

#[test]
fn fallible_set_runtime_hook_is_typed_for_next_edition() {
    let (dir, out) = compile(
        "fallible_set_hook",
        "use core.sys as env\nfn run() { print(env.get(\"JET_ENV_UNUSED\") ?? \"\") }\n",
    );
    let probe = r#"    jet_std_env_init();
    let invalid_name: Result<(), jet_std::EnvError> =
        jet_std_env_set(&"".to_string(), &"value".to_string());
    assert!(matches!(invalid_name, Err(jet_std::EnvError::InvalidName)));
    let invalid_value: Result<(), jet_std::EnvError> =
        jet_std_env_set(&"name".to_string(), &"bad\0value".to_string());
    assert!(matches!(invalid_value, Err(jet_std::EnvError::InvalidValue)));
    jet_runtime_boundary(|| __jet_run());"#;
    let rust = out.rust.replacen(
        "    jet_std_env_init();\n    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n    jet_runtime_boundary(|| __jet_run());",
        probe,
        1,
    );
    assert_ne!(rust, out.rust, "fallible-set probe did not attach");
    let bin = build_rust(&dir, "fallible_set_hook", &rust);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
}

#[cfg(windows)]
#[test]
fn windows_casefold_last_spelling_and_child_inheritance_run_natively() {
    let src = r#"
use core.sys as env
use core.process as process

fn run() {
    env.set("Jet_Win_Case", "first")
    print(env.get("jET_wIN_cASE") ?? "missing")
    env.set("JET_WIN_CASE", "last-✓")
    print(env.get("jet_win_case") ?? "missing")
    names :: env.vars() ?? panic("vars failed")
    print(names.join(",").contains("JET_WIN_CASE"))
    child :: process.run(["cmd.exe", "/C", "echo %jet_win_case%"])
        ?? panic("child failed")
    print(child.output.contains("last-✓"))
}
"#;
    let (dir, out) = compile("windows_native", src);
    let bin = build(&dir, "windows_native", &out);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "first\nlast-✓\ntrue\ntrue\n");
}

#[test]
fn old_edition_invalid_set_uses_exact_runtime_error() {
    let src = r#"
use core.sys as env

fn run() {
    env.set("", "value")
}
"#;
    let (dir, out) = compile("invalid", src);
    let bin = build(&dir, "invalid", &out);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(run.status.code(), Some(70));
    assert!(stderr.contains("panic: core.sys.set: invalid environment variable name"));
    assert!(stderr.contains("invalid.jet:5 in run"), "missing call-site frame: {stderr}");
}

/// Card #2003 criterion 3 — one env owner, proved as a CROSS-TIER
/// DIFFERENTIAL.
///
/// PROVES: a `env.set` performed by the running program is visible, with the
/// same value, to BOTH the decoding read (`env.get`) and the raw-by-name read
/// behind an env-derived terminal fact (`io.terminal_width()` reads `COLUMNS`,
/// `io.terminal_height()` reads `LINES`), on AOT, on the resident JIT and on
/// the interpreter. Each tier is asserted to have actually executed as that
/// tier: the AOT tier is a rustc-built binary, the JIT tier asserts
/// `jit_executed_for_test()` with no deopt and no fallback, and the
/// interpreter tier asserts that NO JIT execution happened while it ran — a
/// silent deopt would otherwise hand back the right answer from the wrong
/// tier.
///
/// WHY BOTH READS: an `env.set` + `env.get` pair alone is a false green. Two
/// owners agree with themselves; the divergence only appears when the write
/// lands in one owner and a second read consults the other. Before #2003 the
/// interpreter wrote `env.set` into the compiler's own `std::env` while
/// `io.terminal_width()` read Jet's logical table, so `env.get` returned the
/// new value and the width did not.
#[test]
fn env_set_is_visible_identically_on_aot_jit_and_interpreter() {
    if !common::have_rustc() || !jet_jit::cranelift_host_supported() {
        eprintln!("note: rustc or the cranelift host is unavailable; skipping tier differential");
        return;
    }
    std::thread::Builder::new()
        .name("env-owner-tier-parity".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(env_set_is_visible_identically_on_aot_jit_and_interpreter_inner)
        .expect("spawn env tier-parity worker")
        .join()
        .expect("env tier-parity worker must not panic");
}

fn env_set_is_visible_identically_on_aot_jit_and_interpreter_inner() {
    let src = r#"
use core.sys as env
use core.term as io

fn run() {
    env.set("JET_ENV_TIER_OWNER", "shared-owner")
    env.set("COLUMNS", "137")
    env.set("LINES", "41")
    print(env.get("JET_ENV_TIER_OWNER") ?? "missing")
    print(io.terminal_width())
    print(io.terminal_height())
}
"#;
    const EXPECTED: &str = "shared-owner\n137\n41\n";

    let (dir, out) = compile("tier_owner", src);
    let entry = dir.join("tier_owner.jet");
    let shown = entry.to_string_lossy().into_owned();

    // Tier 1 — AOT: rustc builds the generated program and the OS runs it.
    let bin = build(&dir, "tier_owner", &out);
    let aot = Command::new(&bin).current_dir(&dir).output().unwrap();
    assert_eq!(
        aot.status.code(),
        Some(0),
        "AOT run failed: {}",
        String::from_utf8_lossy(&aot.stderr)
    );
    let aot_stdout = String::from_utf8_lossy(&aot.stdout).into_owned();

    // Tier 2 — resident JIT: compiled natively in this process, no deopt.
    let mut bundle = jet::Loader::load_entry(&shown).expect("tier fixture must load");
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diag| matches!(diag.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "tier fixture must type-check: {errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "env fixture must be resident-JIT safe: {:?}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit_stdout = match backend.run(&bundle, false) {
        RunOutcome::Ran {
            stdout, exit_code, ..
        } => {
            assert_eq!(exit_code, 0, "resident JIT exited non-zero");
            stdout
        }
        RunOutcome::Problems(diags) => panic!("resident JIT did not run the fixture: {diags:?}"),
    };
    assert!(
        jet_jit::jit_executed_for_test(),
        "fixture did not execute in the resident JIT"
    );
    assert!(
        !jet_jit::deopt_invoked_for_test() && !jet_jit::fallback_invoked_for_test(),
        "resident JIT tier deopted or fell back, so it proves nothing about the JIT"
    );

    // Tier 3 — interpreter: same process, evaluator only.
    jet_jit::reset_jit_trace_for_test();
    let interpreted_stdout = match dev_iteration(&shown, false, true) {
        RunOutcome::Ran {
            stdout, exit_code, ..
        } => {
            assert_eq!(exit_code, 0, "interpreter exited non-zero");
            stdout
        }
        RunOutcome::Problems(diags) => panic!("interpreter did not run the fixture: {diags:?}"),
    };
    assert!(
        !jet_jit::jit_executed_for_test(),
        "the interpreter tier silently executed native JIT code"
    );

    assert_eq!(aot_stdout, EXPECTED, "AOT drifted from the pinned observable");
    assert_eq!(jit_stdout, aot_stdout, "resident JIT diverged from AOT");
    assert_eq!(
        interpreted_stdout, aot_stdout,
        "interpreter diverged from AOT — `env.set` reached a different owner"
    );
    let _ = fs::remove_dir_all(&dir);
}
