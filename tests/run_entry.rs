mod common;

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
    let source = "message :: \"script entry\"\nprint(message)\nprint(helper())\nfn helper() => Int { return 42 }\n";
    let output = jet::compile(source)
        .expect("script statements should lower through the normal entry path");
    assert!(
        output.rust.contains("pub fn __jet_run() -> JetOutcome<(), JetErr>"),
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
fn script_helper_below_call_and_mutual_recursion_keep_statement_order() {
    let source = r#"
print("start")
limit :: 2
print(limit)
print(even(limit + 2))
print("end")

fn even(n: Int) => Bool {
    return if n == 0 -> true else -> odd(n - 1)
}

fn odd(n: Int) => Bool {
    return if n == 0 -> false else -> even(n - 1)
}
"#;
    let expected = "start\n2\ntrue\nend\n";
    let dir = common::unique_tmp("jet_script_order");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(&file, source).unwrap();
    let path = file.to_str().unwrap();

    let outcomes = [
        ("jit", jet::Interpreter::run_jit_once(path)),
        (
            "interpreter",
            jet::Interpreter::run_interpreter_once_with_args(path, &[]),
        ),
        ("dev-jit", jet::Interpreter::dev_iteration(path, false, false)),
        (
            "dev-interpreter",
            jet::Interpreter::dev_iteration(path, false, true),
        ),
    ];
    for (tier, outcome) in outcomes {
        let jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } = outcome
        else {
            panic!("{tier} did not run the script: {outcome:?}");
        };
        assert_eq!(stdout, expected, "{tier} stdout");
        assert!(stderr.is_empty(), "{tier} stderr: {stderr}");
        assert_eq!(exit_code, 0, "{tier} exit code");
    }
    let _ = std::fs::remove_dir_all(&dir);

    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_script_order_aot", "main", source);
        assert_eq!(code, 0, "AOT exit code: {stderr}");
        assert_eq!(stdout, expected, "AOT stdout");
        assert!(stderr.is_empty(), "AOT stderr: {stderr}");
    }
}

#[test]
fn script_bindings_are_ordered_locals_not_file_wide_declarations() {
    let before_binding = "print(later)\nlater :: 1\n";
    let diagnostics = jet::compile(before_binding)
        .expect_err("a loose binding must not be visible before its statement");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0107"),
        "expected an unknown-name diagnostic before the binding, got {diagnostics:?}"
    );

    let inside_declaration = "later :: 1\nfn helper() => Int { return later }\n";
    let diagnostics = jet::compile(inside_declaration)
        .expect_err("a loose binding must stay local to the implicit run body");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0107"),
        "expected an unknown-name diagnostic inside the helper, got {diagnostics:?}"
    );
}

#[test]
fn unannotated_run_is_fallible_by_default_and_reports_at_the_edge() {
    let source = r#"
fn step() => Int ? {
    return Err("boom")
}

fn run() {
    value :: step()?
    print(value)
}
"#;
    let out = jet::compile(source).expect("an unannotated run may use ?");
    assert!(
        out.rust.contains("pub fn __jet_run() -> JetOutcome<(), JetErr>"),
        "unannotated run should default to a fallible unit boundary:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("jet_runtime_boundary(|| {")
            && out.rust.contains("jet_entry_error_exit("),
        "the default entry must report an unhandled error:\n{}",
        out.rust
    );

    let dir = common::unique_tmp("jet_unannotated_entry_error");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.jet"), source).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("boom"), "entry report should contain the source error: {stderr}");
    assert!(stderr.contains("Error"), "entry report should contain its error frame: {stderr}");
}

#[test]
fn script_entry_uses_the_same_front_end_for_check_and_build() {
    let dir = common::unique_tmp("jet_script_check_build");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(&file, "print(\"script entry\")\n").unwrap();
    let path = file.to_str().unwrap();

    let diagnostics = jet::check_with_path(path);
    assert!(diagnostics.is_empty(), "check rejected the script: {diagnostics:?}");
    let output = jet::compile_programmable_build(path, &[])
        .expect("build should use the same implicit run entry");
    assert!(
        output.rust.contains("script entry"),
        "build dropped the script body:\n{}",
        output.rust
    );

    let _ = std::fs::remove_dir_all(&dir);
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
fn script_dev_verb_uses_the_single_file_implicit_run_entry() {
    let dir = common::unique_tmp("jet_script_dev_verb");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.jet"),
        "print(helper())\nfn helper() => String { return \"dev script\" }\n",
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", "main.jet", "--watch=off"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("jet dev should run a single script file");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "jet dev rejected the script:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "dev script\n",
        "jet dev must execute the implicit run body exactly once"
    );
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
        .args(["test", "--show-default", path, "--serial"])
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
fn run() ? {
    return Err("boom")
}
"#;
    let out = jet::compile(src).expect("fallible unit run should compile");
    assert!(
        out.rust.contains("pub fn __jet_run() -> JetOutcome<(), JetErr>"),
        "unit-fallible run should lower to JetOutcome<(), JetErr>:\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("jet_runtime_boundary(|| {")
            && out.rust.contains("jet_entry_error_exit("),
        "fallible run wrapper must handle returned errors:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("jet_runtime_explicit_exit(1)"),
        "fallible run wrapper must use the shared exit boundary:\n{}",
        out.rust
    );
}

#[test]
fn declared_crypto_error_uses_the_generic_runtime_boundary() {
    let src = r#"
use core.crypto as crypto

fn run() ? CryptoError {
    length :: 0
    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], length)?
}
"#;
    let out = jet::compile(src).expect("CryptoError entrypoint should compile");
    let ffi = out
        .ffi
        .as_ref()
        .expect("core.crypto must prepare its hidden bridge");
    let return_type = format!("JetOutcome<(), {}::JetCryptoError>", ffi.crate_name);
    assert!(
        out.rust.contains(&format!("pub fn __jet_run() -> {return_type}")),
        "CryptoError run should retain its error type:\n{}",
        out.rust
    );
    let main = out.rust.rfind("\nfn main()").expect("generated main");
    let entry = &out.rust[main..];
    assert!(
        entry.contains("jet_runtime_boundary(|| {")
            && entry.contains("jet_entry_error_text(&")
            && !entry.contains("jet_render_e3001_crypto")
            && !entry.contains("jet_abort_diagnostic"),
        "CryptoError run must use the generic reported-error boundary:\n{entry}"
    );
}

#[test]
fn unhandled_crypto_error_exits_1_with_the_generic_entry_report() {
    let src = r#"
use core.crypto as crypto

fn dynamic_length(value: Int) => Int {
    return value
}

fn run() ? CryptoError {
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
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(stdout.is_empty(), "stdout must stay empty: {stdout:?}");
    assert!(stderr.contains("hkdf_sha256"), "stderr should carry the error report: {stderr}");
    assert!(!stderr.contains("unhandled cryptographic error"), "stderr:\n{stderr}");
}

#[test]
fn user_crypto_error_is_a_normal_declared_entry_error() {
    let src = r#"
enum CryptoError {
    Internal
}

fn run() ? CryptoError {
    return Err(CryptoError.Internal)
}
"#;
    jet::compile(src).expect("declared error families are valid at the entry");
}

#[test]
fn typed_entry_error_pins_the_declared_family() {
    let src = r#"
enum StoreErr {
    Missing
}

fn run() ? StoreErr {
    return Err(StoreErr.Missing)
}
"#;
    let out = jet::compile(src).expect("typed fallible entries must compile");
    assert!(
        out.rust.contains("JetOutcome<(), __jet_StoreErr>"),
        "the entry must keep StoreErr as its error family:\n{}",
        out.rust
    );

    let dir = common::unique_tmp("jet_typed_entry_error");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.jet"), src).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--quiet", "main.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "Missing\n");

    if common::have_rustc() {
        let (code, stdout, stderr) = common::build_and_run("jet_typed_entry_aot", "main", src);
        assert_eq!(code, 1, "AOT exit code: {stderr}");
        assert!(stdout.is_empty(), "AOT stdout: {stdout:?}");
        assert_eq!(stderr, "Missing\n");
    }

    let interpreter_dir = common::unique_tmp("jet_typed_entry_interpreter");
    std::fs::create_dir_all(&interpreter_dir).unwrap();
    let interpreter_path = interpreter_dir.join("main.jet");
    std::fs::write(&interpreter_path, src).unwrap();
    let interpreter = jet::Interpreter::dev_iteration(
        interpreter_path.to_str().unwrap(),
        false,
        true,
    );
    let _ = std::fs::remove_dir_all(&interpreter_dir);
    match interpreter {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 1);
            assert!(stdout.is_empty());
            assert_eq!(stderr, "Missing\n");
        }
        other => panic!("interpreter did not run typed entry: {other:?}"),
    }
}

#[test]
fn internal_crypto_error_uses_the_reported_entry_exit() {
    let src = r#"
use core.crypto as crypto

fn run() ? CryptoError {
    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], 0)?
}
"#;
    let out = jet::compile(src).expect("CryptoError entrypoint should compile");
    let ffi = out.ffi.as_ref().expect("core.crypto must emit its bridge");
    let main = out.rust.rfind("\nfn main()").expect("generated main");
    let start = out.rust[main..]
        .find("jet_runtime_boundary(|| {")
        .map(|offset| main + offset)
        .expect("generated crypto error boundary");
    let rest = &out.rust[start..];
    let end = rest.find("\n    });").expect("generated boundary close") + "\n    });".len();
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
fn jet_entry_error_text<E: std::fmt::Display>(error: &E) -> String {{ error.to_string() }}
fn jet_entry_report(error: String) -> String {{ format!("{{}}\n", error) }}
fn jet_runtime_explicit_exit(code: i64) -> ! {{ std::process::exit(code as i32) }}
fn jet_entry_error_exit(error: String) -> ! {{ eprint!("{{}}", jet_entry_report(error)); jet_runtime_explicit_exit(1) }}
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
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "internal test-17\n"
    );
}

#[test]
fn fallible_unit_run_can_finish_normally_after_try() {
    let src = r#"
fn step() => Int ? {
    return Ok(1)
}

fn run() ? {
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
fn run() ? {
    return Err("boom")
}
"#;
    jet::compile(src).expect("unit-fallible syntax is the canonical fallible run type");
}

#[test]
fn unit_fallible_signatures_lower_with_value_fallible_returns() {
    let src = r#"
struct Config { value: Int }

fn save(path: String) ? IOError {
    return .Err(IOError.InvalidInput(IOContext.{
        operation: .Read,
        resource: None,
        os_code: None,
        cause: Val("not implemented"),
    }))
}

fn sync() ? {
    return Err("not implemented")
}

fn load() => Config ? IOError {
    return Ok(Config.{ value: 1 })
}

fn run() {}
"#;
    let out = jet::compile(src).expect("unit and value fallible signatures should compile");
    for function in ["__jet_save", "__jet_sync", "__jet_load"] {
        assert!(
            out.rust.contains(&format!("pub fn {function}")),
            "lowered output omitted {function}:\n{}",
            out.rust
        );
    }
    assert!(
        out.rust.contains("pub fn __jet_load")
            && out.rust.contains("JetOutcome<__jet_Config"),
        "value-returning fallible signature lost its value type:\n{}",
        out.rust
    );
}

#[test]
fn fallible_unit_fallthrough_is_entrypoint_only() {
    let src = r#"
fn helper() ? {
}

fn run() {
    print("hi")
}
"#;
    let diags = jet::compile(src).expect_err("non-run unit-fallible fallthrough needs return");
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
