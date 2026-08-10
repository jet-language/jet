#[test]
fn bare_run_stays_valid() {
    jet::compile("fn run() {}\n").expect("bare beginner entrypoint must compile");
}

#[test]
fn script_top_level_recovery_always_consumes_a_token() {
    // A stray `}` is not a statement, and statement recovery stops *before* a
    // `}` at brace depth 0. The top-level script loop must still move the
    // cursor, or it re-reports the same token until memory runs out.
    let diagnostics = jet::compile("    }\n").expect_err("a stray `}` is not a program");
    assert!(
        diagnostics.len() < 10,
        "top-level recovery must report the stray brace once, not in a loop: {diagnostics:?}"
    );
}

#[test]
fn script_statements_use_one_fallible_run_and_keep_declarations_legal() {
    let source = "message :: \"script entry\"\nprint(message)\nfn helper() => Int { return 42 }\n";
    let output = jet::compile(source)
        .expect("script statements should lower through the normal entry path");
    assert!(
        output.rust.contains("pub fn __jet_run() -> Result<(), JetErr>"),
        "implicit script entry must use the fallible unit boundary:\n{}",
        output.rust
    );
    assert!(
        output.rust.contains("script entry"),
        "script body must reach generated code:\n{}",
        output.rust
    );
    assert!(
        output.rust.contains("__jet_helper"),
        "ordinary declarations must remain legal in a script:\n{}",
        output.rust
    );
}

#[test]
fn explicit_run_conflict_has_a_compilable_auto_wrap_fix() {
    let source = "print(\"before\")\nfn run() { print(\"middle\") }\nprint(\"after\")\n";
    let diagnostics = jet::compile(source)
        .expect_err("loose statements and explicit run must conflict");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0621")
        .expect("script conflict diagnostic");
    let edit = diagnostic.edit.as_ref().expect("script conflict auto-fix");
    let fixed = edit.new_text.clone();
    assert!(
        !fixed.contains("print(\"before\")\nfn run"),
        "loose statement stayed outside run:\n{fixed}"
    );
    jet::compile(&fixed)
        .expect("the explicit-run auto-wrap must compile");
}

#[test]
fn script_entry_matches_default_jit_and_forced_interpreter() {
    let dir = std::env::temp_dir().join(format!("jet_script_entry_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(&file, "print(\"script entry\")\n").unwrap();
    let path = file.to_str().unwrap();
    let jit = jet::Interpreter::run_jit_once(path);
    let interpreter = jet::Interpreter::run_interpreter_once_with_args(path, &[]);
    let dev_jit = jet::Interpreter::dev_iteration(path, false, false);
    let dev_interpreter = jet::Interpreter::dev_iteration(path, false, true);
    let _ = std::fs::remove_dir_all(&dir);
    for (tier, outcome) in [
        ("jit", jit),
        ("interpreter", interpreter),
        ("dev-jit", dev_jit),
        ("dev-interpreter", dev_interpreter),
    ] {
        let jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } = outcome
        else {
            panic!("{tier} did not run the script: {outcome:?}");
        };
        assert_eq!(stdout, "script entry\n", "{tier} stdout");
        assert!(stderr.is_empty(), "{tier} stderr: {stderr}");
        assert_eq!(exit_code, 0, "{tier} exit code");
    }
}

#[test]
fn script_test_verb_keeps_test_blocks_and_does_not_run_script_body() {
    let dir = std::env::temp_dir().join(format!(
        "jet_script_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "print(\"script body\")\n#Test(\"script test\") { require(true) }\n",
    )
    .unwrap();
    let path = file.to_str().unwrap();
    let (harness, _) = jet::compile_tests_with_path("", path)
        .expect("jet test should accept a script with a #Test block");
    assert!(harness.contains("script test"), "test name missing from harness");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["test", path, "--serial"])
        .output()
        .expect("jet test should run");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "jet test failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("script test: pass"), "stdout: {stdout}");
    assert!(!stdout.contains("script body"), "jet test ran the script body: {stdout}");
}

#[test]
fn script_dev_entry_can_select_declared_dev_without_running_implicit_run() {
    let dir = std::env::temp_dir().join(format!(
        "jet_script_dev_entry_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(&file, "print(\"script body\")\nfn dev() { print(\"dev body\") }\n").unwrap();
    let output = jet::compile_with_entry(file.to_str().unwrap(), "dev")
        .expect("jet dev entry swap should accept scripts");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.rust.contains("dev body"), "dev body missing from AOT output");
    assert!(output.rust.contains("script body"), "implicit run should remain a normal function");
}

#[test]
fn imported_scripts_are_rejected_before_their_body_can_run() {
    let dir = std::env::temp_dir().join(format!(
        "jet_imported_script_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tools = dir.join("tools.jet");
    let entry = dir.join("main.jet");
    std::fs::write(&tools, "print(\"must not run\")\n").unwrap();
    std::fs::write(&entry, "use \"./tools\"\nprint(\"entry\")\n").unwrap();
    let diagnostics = jet::compile_with_path("", entry.to_str().unwrap())
        .expect_err("imported scripts must be rejected");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0620"),
        "expected E0620, got {diagnostics:?}"
    );
}

#[test]
fn fallible_unit_run_is_the_only_fallible_entrypoint() {
    let src = r#"
fn run() => () ? {
    return Err("boom")
}
"#;
    let out = jet::compile(src).expect("fallible unit run should compile");
    assert!(
        out.rust.contains("pub fn __jet_run() -> Result<(), JetErr>"),
        "() ? run should lower to Result<(), JetErr>:\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("if let Err(__jet_err) = jet_runtime_boundary(|| __jet_run())"),
        "fallible run wrapper must handle returned errors:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("std::process::exit(1);"),
        "fallible run wrapper must exit nonzero on error:\n{}",
        out.rust
    );
}

#[test]
fn crypto_fallible_unit_run_uses_the_e3001_runtime_boundary() {
    let src = r#"
use core.crypto as crypto

fn run() => () ? CryptoError {
    length :: 0
    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], length)?
}
"#;
    let out = jet::compile(src).expect("CryptoError entrypoint should compile");
    let ffi = out
        .ffi
        .as_ref()
        .expect("core.crypto must prepare its hidden bridge");
    let return_type = format!("Result<(), {}::JetCryptoError>", ffi.crate_name);
    assert!(
        out.rust.contains(&format!("pub fn __jet_run() -> {return_type}")),
        "CryptoError run should retain its error type:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("Error [E3001]: unhandled cryptographic error")
            && out.rust.contains("std::process::exit(if __jet_internal { 101 } else { 70 });"),
        "CryptoError run must use the E3001 boundary:\n{}",
        out.rust
    );
}

#[test]
fn unhandled_crypto_error_exits_70_with_a_redacted_e3001_frame() {
    let src = r#"
use core.crypto as crypto

fn dynamic_length(value: Int) => Int {
    return value
}

fn run() => () ? CryptoError {
    length :: dynamic_length(8161)
    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], length)?
}
"#;
    let dir = std::env::temp_dir().join(format!(
        "jet_crypto_entry_error_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("unhandled_crypto_error.jet"), src).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "unhandled_crypto_error.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let code = output.status.code().unwrap_or(0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(code, 70, "stderr:\n{stderr}");
    assert!(stdout.is_empty(), "stdout must stay empty: {stdout:?}");
    assert!(
        stderr.ends_with(concat!(
            "Error [E3001]: unhandled cryptographic error\n",
            " Why: hkdf_sha256: output length must be 0..8160; got 8161\n",
            " Fix: handle the CryptoError in fn run\n",
        )),
        "stderr:\n{stderr}"
    );
}

#[test]
fn user_crypto_error_is_not_the_core_entry_error() {
    let src = r#"
enum CryptoError {
    Internal
}

fn run() => () ? CryptoError {
    return Err(.Internal)
}
"#;
    let diagnostics = jet::compile(src).expect_err("user CryptoError must not gain core behavior");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0122"),
        "expected entrypoint E0122, got {diagnostics:?}"
    );
}

#[test]
fn internal_crypto_error_exits_101_in_the_generated_wrapper() {
    let src = r#"
use core.crypto as crypto

fn run() => () ? CryptoError {
    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], 0)?
}
"#;
    let out = jet::compile(src).expect("CryptoError entrypoint should compile");
    let ffi = out.ffi.as_ref().expect("core.crypto must emit its bridge");
    let marker = "if let Err(__jet_err) = jet_runtime_boundary(|| __jet_run()) {";
    let start = out.rust.find(marker).expect("generated crypto error boundary");
    let rest = &out.rust[start..];
    let end = rest.find("\n    }\n").expect("generated boundary close") + "\n    }".len();
    let invocation = &rest[..end];
    let rust = format!(r#"
mod {ffi_name} {{
    #[derive(Debug)]
    pub enum JetCryptoError {{ Internal {{ incident_id: &'static str }} }}
    impl std::fmt::Display for JetCryptoError {{
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
            match self {{ Self::Internal {{ incident_id }} => write!(f, "internal {{incident_id}}") }}
        }}
    }}
}}
fn jet_runtime_boundary<T>(f: impl FnOnce() -> T) -> T {{ f() }}
fn __jet_run() -> Result<(), {ffi_name}::JetCryptoError> {{
    Err({ffi_name}::JetCryptoError::Internal {{ incident_id: "test-17" }})
}}
fn main() {{
    {invocation}
}}
"#, ffi_name = ffi.crate_name);
    let dir = std::env::temp_dir().join(format!("jet_crypto_internal_entry_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    let binary = dir.join("main");
    std::fs::write(&source, rust).unwrap();
    let built = std::process::Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(built.status.success(), "rustc stderr: {}", String::from_utf8_lossy(&built.stderr));
    let output = std::process::Command::new(&binary).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(output.status.code(), Some(101));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        concat!(
            "Error [E3001]: unhandled cryptographic error\n",
            " Why: internal test-17\n",
            " Fix: handle the CryptoError in fn run\n",
        )
    );
}

#[test]
fn fallible_unit_run_can_finish_normally_after_try() {
    let src = r#"
fn step() => Int ? {
    return Ok(1)
}

fn run() => () ? {
    n :: step()?
    print(n)
}
"#;
    let out = jet::compile(src).expect("fallible unit run should allow normal completion");
    assert!(
        out.rust.contains("Ok(())"),
        "fallible unit run should synthesize success at the end:\n{}",
        out.rust
    );
}

#[test]
fn unit_fallible_run_is_accepted() {
    let src = r#"
fn run() => () ? {
    return Err("boom")
}
"#;
    jet::compile(src).expect("() ? is the canonical fallible run type");
}

#[test]
fn fallible_unit_fallthrough_is_entrypoint_only() {
    let src = r#"
fn helper() => () ? {
}

fn run() {
    print("hi")
}
"#;
    let diags = jet::compile(src).expect_err("non-run () ? fallthrough needs return");
    assert!(
        diags.iter().any(|d| d.code == "E0114"),
        "expected E0114, got: {diags:?}"
    );
}

#[test]
fn retired_void_type_reports_the_migration_diagnostic() {
    let src = "fn run() => Void ? { return Err(\"boom\") }\n";
    let diagnostics = jet::compile(src).expect_err("Void must not remain a source type");
    assert!(
        diagnostics.iter().any(|d| d.code == "E0431"),
        "expected E0431, got: {diagnostics:?}"
    );
}

#[test]
fn classic_if_without_else_does_not_satisfy_missing_return_check() {
    let src = r#"
fn maybe(flag: Bool) => Int {
    if flag { return 1 }
}

fn run() {
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("a conditional return still needs an else path");
    assert!(
        diags.iter().any(|d| d.code == "E0114"),
        "expected E0114, got: {diags:?}"
    );
}
