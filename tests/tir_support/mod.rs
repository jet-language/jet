#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// Tier-parity scratch programs exercise the language and Core surface, not an
// application manifest. Give every scratch project one explicit test
// authority decision so an authority-floor change cannot turn unrelated
// tests into per-test allowlists.
pub(crate) const TIR_TEST_PACKAGE: &str = "name: \"tir_support\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [Browser, DB, Env, Exec, FFI, FS, GPU, IO, Log, Mem.Alloc, Net, Rand, Secret, Time] } }\n";

fn unique_tmp(prefix: &str) -> PathBuf {
    crate::common::unique_tmp(prefix)
}

pub(crate) fn write_test_package(dir: &Path, source: &str) {
    let temp = std::env::temp_dir();
    let scratch = crate::common::test_scratch_root("scratch");
    assert!(
        dir.parent() == Some(temp.as_path()) || dir.parent() == Some(scratch.as_path()),
        "TIR test package must live in its own scratch child: {}",
        dir.display()
    );
    fs::write(dir.join("package.jet"), source).unwrap();
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
    write_test_package(&dir, TIR_TEST_PACKAGE);
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

/// Compile source that has no repository path. The compiler still needs a
/// real bundle path to resolve its package and module context.
pub fn compile_source(
    name: &str,
    src: &str,
) -> Result<jet::CompileOutput, Vec<jet::Diagnostics::Diagnostic>> {
    let dir = unique_tmp("jet_tir_compile_source");
    fs::create_dir_all(&dir).unwrap();
    write_test_package(&dir, TIR_TEST_PACKAGE);
    let filename = if name.ends_with(".jet") {
        name.to_string()
    } else {
        format!("{name}.jet")
    };
    let path = dir.join(filename);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let result = jet::compile_with_path(src, &shown);
    let _ = fs::remove_dir_all(&dir);
    result
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
    jit_run_with_package(name, src, &[], None)
}

fn jit_run_with_package(
    name: &str,
    src: &str,
    vars: &[(&str, &str)],
    package_source: Option<&str>,
) -> (i32, String, String) {
    let dir = unique_tmp("jet_jit_run");
    fs::create_dir_all(&dir).unwrap();
    let jet_name = format!("{name}.jet");
    let jet_path = dir.join(&jet_name);
    fs::write(&jet_path, src).unwrap();
    write_test_package(&dir, package_source.unwrap_or(TIR_TEST_PACKAGE));
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .arg("run")
        .arg(&jet_path)
        .current_dir(&dir)
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"));
    for (key, value) in vars {
        command.env(key, value);
    }
    let out = command.output().unwrap();
    let stderr = normalize_workspace_root_paths(
        &String::from_utf8_lossy(&out.stderr),
        &dir,
    );
    let _ = fs::remove_dir_all(&dir);
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr,
    )
}

pub fn jit_run_traced(name: &str, src: &str) -> (i32, String, String) {
    let dir = unique_tmp("jet_jit_run_traced");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    write_test_package(&dir, TIR_TEST_PACKAGE);
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", jet_path.to_str().unwrap(), "--trace-tiers"])
        .current_dir(&dir)
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"))
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
pub fn jit_run_with_env(name: &str, src: &str, vars: &[(&str, &str)]) -> (i32, String, String) {
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
    write_test_package(&dir, TIR_TEST_PACKAGE);
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .arg("run")
        .arg(&jet_path)
        .current_dir(&dir)
        // Keep every run out of the shared build cache, which is keyed on the
        // AST hash and would otherwise serve a binary built before this change.
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"));
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
    interpreter_run_with_package(name, src, None)
}

fn interpreter_run_with_package(
    name: &str,
    src: &str,
    package_source: Option<&str>,
) -> (i32, String, String) {
    let dir = unique_tmp("jet_interpreter_run");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    write_test_package(&dir, package_source.unwrap_or(TIR_TEST_PACKAGE));
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg("--interpret")
        .arg(&path)
        .current_dir(&dir)
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"))
        .output()
        .unwrap();
    let stderr = normalize_workspace_root_paths(
        &String::from_utf8_lossy(&out.stderr),
        &dir,
    );
    let _ = fs::remove_dir_all(&dir);
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr,
    )
}

/// The same snippet on both tiers, asserting they agree. This is the shape I9
/// actually asks for: not "AOT prints X", but "every tier prints the same X".
pub fn assert_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    assert_tiers_agree_with_package(name, src, expected_stdout, None);
}

/// Tier parity with a temporary package policy. Effectful snippets need an
/// application authority decision before the real JIT and interpreter paths
/// will execute them; the source remains unchanged and the policy is test-only.
pub fn assert_tiers_agree_with_application_policy(
    name: &str,
    src: &str,
    expected_stdout: &str,
    package_source: &str,
) {
    assert_tiers_agree_with_package(name, src, expected_stdout, Some(package_source));
}

fn assert_tiers_agree_with_package(
    name: &str,
    src: &str,
    expected_stdout: &str,
    package_source: Option<&str>,
) {
    let (jit_code, jit_out, jit_err_raw) = jit_run_with_package(name, src, &[], package_source);
    let jit_err = jit_err_raw;
    assert_eq!(jit_code, 0, "`jet run` failed:\n{jit_err}");
    assert_eq!(
        jit_out, expected_stdout,
        "`jet run` (Cranelift/interpreter) disagreed:\n{jit_err}"
    );
    let (interpreter_code, interpreter_out, interpreter_err_raw) =
        interpreter_run_with_package(name, src, package_source);
    let interpreter_err = interpreter_err_raw;
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
        let (aot_code, aot_out, aot_err_raw) = build_and_run_full("jet_tir_test", name, src);
        let aot_err = aot_err_raw;
        assert_eq!(
            aot_code, jit_code,
            "AOT and `jet run` exit codes disagree:\n{aot_err}"
        );
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
    assert_example_cli_tiers_agree_with(stem, |actual| {
        assert_eq!(actual, expected_stdout);
    });
}

pub fn assert_example_cli_tiers_agree_with<F>(stem: &str, check_stdout: F)
where
    F: Fn(&str),
{
    assert_example_cli_tiers_agree_with_package(stem, None, check_stdout);
}

/// Run an executable example from an isolated scratch package. This keeps a
/// narrowly scoped authority grant out of the shared examples tree.
pub fn assert_example_cli_tiers_agree_with_package<F>(
    stem: &str,
    package_source: Option<&str>,
    check_stdout: F,
) where
    F: Fn(&str),
{
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stem_path = root.join("examples/features").join(stem);
    let source = if stem_path.is_dir() {
        stem_path.join("run.jet")
    } else {
        stem_path.with_extension("jet")
    };
    assert!(
        source.is_file(),
        "missing executable allocator example: {}",
        source.display()
    );
    let (scratch, run_source) = if let Some(package_source) = package_source {
        let scratch = unique_tmp("jet_example_policy");
        fs::create_dir_all(&scratch).unwrap();
        let run_source = scratch.join(source.file_name().expect("example source has a filename"));
        fs::copy(&source, &run_source).unwrap();
        write_test_package(&scratch, package_source);
        (Some(scratch), run_source)
    } else {
        (None, source.clone())
    };

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
            .arg(&run_source)
            .current_dir(&root)
            .env("JET_STORE_DIR", cache.join("cache"))
            .env("JETPACK_ROOT", cache.join("jetpack"))
            .env("NO_COLOR", "1")
            // The test itself already runs inside scripts/agent/jet-env. Mark
            // the child as active there so the project-env gate does not turn
            // this tier-parity proof into an E1355 test.
            .env("JETPACK_ENV", "1");
        let output = command.output().unwrap();
        let result = (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        let _ = fs::remove_dir_all(&cache);
        assert_eq!(result.0, 0, "{mode} run failed for {stem}:\n{}", result.2);
        check_stdout(&result.1);
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
    if let Some(scratch) = scratch {
        let _ = fs::remove_dir_all(scratch);
    }
}

/// Replace only the exact generated source/scratch-root prefix. The oracle
/// keeps project-relative paths, while a tier may embed its unique temp root.
fn normalize_workspace_root_paths(stderr: &str, root: &std::path::Path) -> String {
    let root = root.display().to_string();
    stderr
        .replace(&format!("{root}/"), "")
        .replace(&format!("{root}\\"), "")
}

/// Render compile-time Jet lints exactly as the non-interactive CLI does.
fn render_compile_lints(
    file: &str,
    src: &str,
    lints: &[jet::Diagnostics::Diagnostic],
) -> String {
    if lints.is_empty() {
        return String::new();
    }
    let mut rendered = jet::render_all_linked(file, src, lints, false, false);
    rendered.push_str(&format!(
        "\n{} problem{} found\n",
        lints.len(),
        if lints.len() == 1 { "" } else { "s" }
    ));
    if let Some(first) = lints.first() {
        rendered.push_str(&format!(
            "{}\n",
            jet::Explain::pointer_line(&first.code, false)
        ));
    }
    rendered
}

/// Run an executable error example through debug/release AOT, default jet run,
/// and the forced interpreter. The .err.out file is one byte oracle.
pub fn assert_example_cli_error_tiers_agree(
    stem: &str,
    expected_exit_code: i32,
    expected_stderr: &str,
) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/features").join(format!("{stem}.jet"));
    assert!(
        source.is_file(),
        "missing executable error example: {}",
        source.display()
    );
    let relative = format!("examples/features/{stem}.jet");
    let modes = [
        ("debug", Some("--profile=debug"), false),
        ("release", Some("--release"), false),
        ("default", None, false),
        ("interpret", None, true),
    ];
    let mut baseline = None;
    for (mode, profile, interpret) in modes {
        let cache = unique_tmp(&format!("jet_example_error_{mode}"));
        fs::create_dir_all(&cache).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if let Some(profile) = profile {
            command.arg(profile);
        }
        if interpret {
            command.arg("--interpret");
        }
        command
            .arg(&relative)
            .current_dir(&root)
            .env("JET_STORE_DIR", cache.join("cache"))
            .env("JETPACK_ROOT", cache.join("jetpack"))
            .env("NO_COLOR", "1");
        let output = command.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let result = (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            normalize_workspace_root_paths(&stderr, &root),
        );
        let _ = fs::remove_dir_all(&cache);
        assert_eq!(result.0, expected_exit_code, "{mode} exit code for {stem}");
        assert!(
            result.1.is_empty(),
            "{mode} stdout for {stem}: {:?}",
            result.1
        );
        assert_eq!(
            result.2, expected_stderr,
            "{mode} stderr disagreed with the .err.out oracle for {stem}"
        );
        if let Some((baseline_mode, baseline_result)) = &baseline {
            assert_eq!(
                &result, baseline_result,
                "{mode} result disagreed with {baseline_mode} for {stem}"
            );
        } else {
            baseline = Some((mode, result));
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
    write_test_package(&dir, TIR_TEST_PACKAGE);
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let compile_stderr = render_compile_lints(&shown, src, &out.lints);
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args(["--edition", "2021", "--crate-name"]);
    rustc_cmd.arg(jet::Syntax::sanitize_crate_name(
        rs.file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("out"),
    ));
    rustc_cmd.args([rs.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
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
    let run_stderr = String::from_utf8_lossy(&run.stderr);
    let stderr = normalize_workspace_root_paths(
        &format!("{compile_stderr}{run_stderr}"),
        &dir,
    );
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        stderr,
    )
}

/// Build and run a multi-file program from a fresh temporary directory.
#[allow(dead_code)]
pub fn build_and_run_multi(name: &str, entry: &str, files: &[(&str, &str)]) -> (i32, String) {
    let dir = unique_tmp(&format!("jet_tir_multi_{name}"));
    fs::create_dir_all(&dir).unwrap();
    write_test_package(&dir, TIR_TEST_PACKAGE);
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
        .args(["--edition", "2021", "--crate-name"])
        .arg(jet::Syntax::sanitize_crate_name(
            rs.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("out"),
        ))
        .args([rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
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

/// Build a multi-file program through the public release-profile AOT path,
/// then execute the emitted artifact.
pub fn build_release_and_run_multi(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    let dir = unique_tmp(&format!("jet_release_multi_{name}"));
    fs::create_dir_all(&dir).unwrap();
    write_test_package(&dir, TIR_TEST_PACKAGE);
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let build = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "--release", entry])
        .current_dir(&dir)
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    if !build.status.success() {
        return (
            build.status.code().unwrap_or(1),
            String::new(),
            String::from_utf8_lossy(&build.stderr).into_owned(),
        );
    }
    let artifact = dir.join("build").join(
        std::path::Path::new(entry)
            .file_stem()
            .expect("release entry has a file stem"),
    );
    let run = Command::new(artifact).current_dir(&dir).output().unwrap();
    (
        run.status.code().unwrap_or(1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

/// Run a scratch program through release AOT, default resident JIT, and the
/// forced interpreter.
pub fn assert_release_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let files = [("main.jet", src)];
    let runs = [
        build_release_and_run_multi(name, "main.jet", &files),
        run_default_multi(name, "main.jet", &files),
        run_interpret_multi(name, "main.jet", &files),
    ];
    let baseline = &runs[0];
    for (mode, result) in ["release AOT", "default resident JIT", "forced interpreter"]
        .into_iter()
        .zip(runs.iter())
    {
        assert_eq!(result.0, 0, "{mode} failed:\n{}", result.2);
        assert_eq!(result.1, expected_stdout, "{mode} output disagreed");
        assert_eq!(result.0, baseline.0, "{mode} exit code disagreed");
        assert_eq!(result.1, baseline.1, "{mode} stdout disagreed");
    }
}
/// Run a bare single-file source through release AOT, resident JIT, and the
/// forced interpreter. Unlike `assert_tiers_agree`, this intentionally writes
/// no `package.jet`, so the source exercises the driver's standalone path.
pub fn assert_bare_release_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let dir = unique_tmp(&format!("jet_bare_tiers_{name}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let path = path.to_string_lossy().into_owned();
    let modes = [
        ("release AOT", vec!["run", "--release", path.as_str()]),
        ("default resident JIT", vec!["run", path.as_str()]),
        (
            "forced interpreter",
            vec!["run", "--interpret", path.as_str()],
        ),
    ];
    let mut baseline = None;
    for (mode, args) in modes {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(args)
            .current_dir(&dir)
            .env("JET_STORE_DIR", dir.join(format!("cache-{mode}")))
            .env("JETPACK_ROOT", dir.join(format!("jetpack-{mode}")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let result = (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        assert_eq!(result.0, 0, "{mode} failed:\n{}", result.2);
        assert_eq!(result.1, expected_stdout, "{mode} output disagreed");
        if let Some((baseline_mode, baseline_result)) = &baseline {
            assert_eq!(
                &result, baseline_result,
                "{mode} disagreed with {baseline_mode}"
            );
        } else {
            baseline = Some((mode, result));
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Assert one authority-denied generic call has the same diagnostic on every
/// execution tier. The package grants the ordinary default roots but omits
/// the effect under test, so no tier receives that authority.
pub fn assert_release_tier_error_with_application_policy(
    name: &str,
    src: &str,
    package_source: &str,
    expected_code: &str,
) {
    let dir = unique_tmp(&format!("jet_policy_tiers_{name}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    fs::write(dir.join("package.jet"), package_source).unwrap();
    let path = path.to_string_lossy().into_owned();
    let modes = [
        ("release AOT", vec!["run", "--release", path.as_str()]),
        ("default resident JIT", vec!["run", path.as_str()]),
        (
            "forced interpreter",
            vec!["run", "--interpret", path.as_str()],
        ),
    ];
    let mut baseline = None;
    for (mode, args) in modes {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(args)
            .current_dir(&dir)
            .env("JET_STORE_DIR", dir.join(format!("cache-{mode}")))
            .env("JETPACK_ROOT", dir.join(format!("jetpack-{mode}")))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let result = (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        assert_ne!(result.0, 0, "{mode} unexpectedly succeeded");
        assert!(result.1.is_empty(), "{mode} wrote stdout: {:?}", result.1);
        assert!(
            result.2.contains(expected_code),
            "{mode} did not report {expected_code}:\n{}",
            result.2
        );
        if let Some((baseline_mode, baseline_result)) = &baseline {
            assert_eq!(
                &result, baseline_result,
                "{mode} disagreed with {baseline_mode}"
            );
        } else {
            baseline = Some((mode, result));
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Assert the stable E3010 code/message across release AOT, resident JIT, and
/// forced interpretation while ignoring adapter-specific source locations.
pub fn assert_release_error_tiers_agree(name: &str, src: &str, expected_message: &str) {
    let files = [("main.jet", src)];
    let runs = [
        build_release_and_run_multi(name, "main.jet", &files),
        run_default_multi(name, "main.jet", &files),
        run_interpret_multi(name, "main.jet", &files),
    ];
    let expected_prefix = format!("Stop [E3010]: `{expected_message}`");
    let baseline_code = runs[0].0;
    let mut baseline_prefix = None;
    for (mode, result) in ["release AOT", "default resident JIT", "forced interpreter"]
        .into_iter()
        .zip(runs.iter())
    {
        assert_ne!(result.0, 0, "{mode} unexpectedly succeeded");
        assert_eq!(result.0, baseline_code, "{mode} exit code disagreed");
        assert!(result.1.is_empty(), "{mode} wrote stdout: {:?}", result.1);
        let prefix = result
            .2
            .lines()
            .find(|line| line.contains("Stop [E3010]:"))
            .and_then(|line| line.split(" — ").next())
            .unwrap_or_else(|| panic!("{mode} did not report E3010:\n{}", result.2));
        assert_eq!(prefix, expected_prefix.as_str(), "{mode} message disagreed");
        if let Some(baseline_prefix) = baseline_prefix {
            assert_eq!(prefix, baseline_prefix, "{mode} detail disagreed");
        } else {
            baseline_prefix = Some(prefix);
        }
    }
}

/// Run a multi-file program through the default `jet run` lens.
pub fn run_default_multi(name: &str, entry: &str, files: &[(&str, &str)]) -> (i32, String, String) {
    let dir = unique_tmp(&format!("jet_jit_multi_{name}"));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    write_test_package(&dir, TIR_TEST_PACKAGE);
    let run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", entry, "--trace-tiers"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"))
        .output()
        .unwrap();
    let result = (
        run.status.code().unwrap_or(1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    );
    let _ = fs::remove_dir_all(&dir);
    result
}

/// Run a multi-file program through the forced TIR interpreter (`--interpret`).
/// I9: the default lens can hide a rule that only one engine knows, so a
/// cross-module fact has to answer on this lens too, with no Cranelift host to
/// fall back on.
#[allow(dead_code)]
pub fn run_interpret_multi(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    let dir = unique_tmp(&format!("jet_interp_multi_{name}"));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    write_test_package(&dir, TIR_TEST_PACKAGE);
    let run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--interpret", entry, "--trace-tiers"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"))
        .output()
        .unwrap();
    let result = (
        run.status.code().unwrap_or(1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    );
    let _ = fs::remove_dir_all(&dir);
    result
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
