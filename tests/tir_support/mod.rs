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
    let jet_path = dir.join(format!("{name}.jet"));
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

/// The same snippet on both tiers, asserting they agree. This is the shape I9
/// actually asks for: not "AOT prints X", but "every tier prints the same X".
pub fn assert_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let (jit_code, jit_out, jit_err) = jit_run(name, src);
    assert_eq!(jit_code, 0, "`jet run` failed:\n{jit_err}");
    assert_eq!(
        jit_out, expected_stdout,
        "`jet run` (Cranelift/interpreter) disagreed:\n{jit_err}"
    );
    if have_rustc() {
        let (aot_code, aot_out) = build_and_run(name, src);
        assert_eq!(aot_code, 0, "AOT run failed:\n{aot_out}");
        assert_eq!(
            aot_out, jit_out,
            "AOT and `jet run` disagree — one tier re-encoded the rule (I9)"
        );
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
