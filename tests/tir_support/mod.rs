#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_tmp(prefix: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), n))
}

pub fn have_rustc() -> bool {
    let present = Command::new("rustc").arg("--version").output().is_ok();
    if !present && std::env::var("JET_REQUIRE_RUSTC").as_deref() == Ok("1") {
        panic!(
            "JET_REQUIRE_RUSTC=1 but rustc not found on PATH — refusing to \
             silently skip I2 (rustc-must-accept) coverage. Fix the CI \
             environment; do not unset JET_REQUIRE_RUSTC to paper over this."
        );
    }
    present
}

pub fn build_and_run(name: &str, src: &str) -> (i32, String) {
    let (code, stdout, _stderr) = build_and_run_full("jet_tir_test", name, src);
    (code, stdout)
}

pub fn compile(name: &str, src: &str) -> String {
    let dir = unique_tmp("jet_tir_compile");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected:\n{}",
                jet::render_diagnostics(&shown, src, &diags)
            )
        })
        .rust
}

pub fn build_and_run_full(prefix: &str, name: &str, src: &str) -> (i32, String, String) {
    build_and_run_full_inner(prefix, name, src, None)
}

/// Run a snippet the way `jet run` does — through the Cranelift host, with the
/// interpreter picking up whatever the host deopts on. `build_and_run*` above
/// only ever proves AOT, so a rule re-encoded in an engine would pass every one
/// of those and still be wrong (I9). Needs no rustc.
///
/// Returns `(exit code, stdout, stderr)`.
pub fn jit_run(name: &str, src: &str) -> (i32, String, String) {
    jit_run_with_env(name, src, &[])
}

pub fn jit_run_traced(name: &str, src: &str) -> (i32, String, String) {
    let dir = unique_tmp("jet_jit_run_traced");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", jet_path.to_str().unwrap(), "--trace-tiers"])
        .current_dir(&dir)
        .env("JET_CACHE_DIR", dir.join("cache"))
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `jit_run` with environment variables the program can read back.
///
/// A trap test needs an operand the comptime evaluator cannot see. It folds
/// literals, calls, and loops over literal lists, so a value that only exists
/// in the process environment is the smallest thing that reaches the Cranelift
/// host instead of stopping the build with a comptime diagnostic.
pub fn jit_run_with_env(
    name: &str,
    src: &str,
    vars: &[(&str, &str)],
) -> (i32, String, String) {
    jit_run_with_env_args(name, src, vars, &[])
}

/// `jit_run` with process env and program argv after `--`.
pub fn jit_run_with_env_args(
    name: &str,
    src: &str,
    vars: &[(&str, &str)],
    program_args: &[&str],
) -> (i32, String, String) {
    let dir = unique_tmp("jet_jit_run");
    fs::create_dir_all(&dir).unwrap();
    let jet_name = format!("{name}.jet");
    let jet_path = dir.join(&jet_name);
    fs::write(&jet_path, src).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .arg("run")
        .arg(&jet_path)
        .current_dir(&dir)
        // Keep every run out of the shared build cache, which is keyed on the
        // AST hash and would otherwise serve a binary built before this change.
        .env("JET_CACHE_DIR", dir.join("cache"));
    for (key, value) in vars {
        command.env(key, value);
    }
    if !program_args.is_empty() {
        command.arg("--");
        for arg in program_args {
            command.arg(arg);
        }
    }
    let out = command.output().unwrap();
    let _ = fs::remove_dir_all(&dir);
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

pub fn interpreter_run(name: &str, src: &str) -> (i32, String, String) {
    let dir = unique_tmp("jet_interpreter_run");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let outcome = jet::Interpreter::dev_iteration(&shown, false, true);
    let _ = fs::remove_dir_all(&dir);
    match outcome {
        jet::Interpreter::RunOutcome::Ran {
            exit_code,
            stdout,
            stderr,
        } => (exit_code, stdout, stderr),
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("interpreter rejected the tier-comparison source: {diagnostics:?}")
        }
    }
}

/// The same snippet on both tiers, asserting they agree. This is the shape I9
/// actually asks for: not "AOT prints X", but "every tier prints the same X".
pub fn assert_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let (jit_code, jit_out, jit_err) = jit_run(name, src);
    assert_eq!(jit_code, 0, "`jet run` failed:\n{jit_err}");
    assert_eq!(
        jit_out, expected_stdout,
        "`jet run` (Cranelift/interpreter) disagreed:\n{jit_err}"
    );
    let (interpreter_code, interpreter_out, interpreter_err) = interpreter_run(name, src);
    assert_eq!(
        interpreter_code, jit_code,
        "forced interpreter and default JIT exit codes disagree:\ninterpreter stderr: {interpreter_err}\nJIT stderr: {jit_err}"
    );
    assert_eq!(
        interpreter_out, jit_out,
        "forced interpreter and default JIT stdout disagree:\ninterpreter stderr: {interpreter_err}\nJIT stderr: {jit_err}"
    );
    assert_eq!(
        interpreter_err, jit_err,
        "forced interpreter and default JIT stderr disagree"
    );
    if have_rustc() {
        let (aot_code, aot_out, aot_err) = build_and_run_full("jet_tir_test", name, src);
        assert_eq!(aot_code, jit_code, "AOT and `jet run` exit codes disagree:\n{aot_err}");
        assert_eq!(
            aot_out, jit_out,
            "AOT and `jet run` disagree — one tier re-encoded the rule (I9)"
        );
        assert_eq!(
            aot_err, jit_err,
            "AOT and `jet run` stderr disagree — one tier re-encoded the rule (I9)"
        );
        assert_eq!(
            aot_code, interpreter_code,
            "AOT and forced interpreter exit codes disagree"
        );
        assert_eq!(
            aot_out, interpreter_out,
            "AOT and forced interpreter stdout disagree"
        );
        assert_eq!(
            aot_err, interpreter_err,
            "AOT and forced interpreter stderr disagree"
        );
    }
}

/// Run an executable example through the three hosted lenses named by I9:
/// release/AOT, default `jet run`, and forced TIR interpretation.
pub fn assert_example_cli_tiers_agree(stem: &str, expected_stdout: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/features").join(format!("{stem}.jet"));
    assert!(
        source.is_file(),
        "missing executable allocator example: {}",
        source.display()
    );

    let modes = [
        ("release", true, false),
        ("default", false, false),
        ("interpret", false, true),
    ];
    let mut baseline = None;
    for (mode, release, interpret) in modes {
        let cache = unique_tmp(&format!("jet_example_{mode}"));
        fs::create_dir_all(&cache).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        if interpret {
            command.arg("--interpret");
        }
        command
            .arg(&source)
            .current_dir(&root)
            .env("JET_CACHE_DIR", cache.join("cache"))
            .env("NO_COLOR", "1");
        let output = command.output().unwrap();
        let result = (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        let _ = fs::remove_dir_all(&cache);
        assert_eq!(
            result.0, 0,
            "{mode} run failed for {stem}:\n{}",
            result.2
        );
        assert_eq!(
            result.1, expected_stdout,
            "{mode} output disagreed for {stem}:\n{}",
            result.2
        );
        if let Some((baseline_mode, baseline_code, baseline_stdout)) = &baseline {
            assert_eq!(
                result.0, *baseline_code,
                "{mode} exit code disagreed with {baseline_mode} for {stem}"
            );
            assert_eq!(
                &result.1, baseline_stdout,
                "{mode} output disagreed with {baseline_mode} for {stem}"
            );
        } else {
            baseline = Some((mode, result.0, result.1));
        }
    }
}

pub fn build_and_run_full_with_cfg(
    prefix: &str,
    name: &str,
    src: &str,
    rustc_cfg: &str,
) -> (i32, String, String) {
    build_and_run_full_inner(prefix, name, src, Some(rustc_cfg))
}

fn build_and_run_full_inner(
    prefix: &str,
    name: &str,
    src: &str,
    rustc_cfg: Option<&str>,
) -> (i32, String, String) {
    let dir = unique_tmp(prefix);
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        "--crate-name",
    ]);
    rustc_cmd.arg(jet::Syntax::sanitize_crate_name(
        rs.file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("out"),
    ));
    rustc_cmd.args([
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(cfg) = rustc_cfg {
        rustc_cmd.args(["--cfg", cfg]);
    }
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        stderr,
    )
}

/// Build and run a multi-file program from a fresh temporary directory.
#[allow(dead_code)]
pub fn build_and_run_multi(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!(
        "jet_tir_multi_{}_{}",
        name,
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let shown = entry_path.to_string_lossy().into_owned();
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let out = jet::compile_with_path(&entry_src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, &entry_src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--crate-name",
        ])
        .arg(jet::Syntax::sanitize_crate_name(
            rs.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("out"),
        ))
        .args([
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
    )
}

/// Run a multi-file program through the default `jet run` lens.
pub fn run_default_multi(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    let dir = unique_tmp(&format!("jet_jit_multi_{name}"));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", entry, "--trace-tiers"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    (
        run.status.code().unwrap_or(1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

pub fn strip_vetted_prelude_modules(rust_code: &str) -> String {
    crate::common::strip_vetted_prelude_modules(rust_code)
}

#[test]
fn watcher_process_probe_is_vetted_without_hiding_user_unsafe() {
    let generated = "// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe\nunsafe { ffi() }\n// JET_VETTED_UNSAFE_END: jet_watch_process_probe\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}
