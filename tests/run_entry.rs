#[test]
fn bare_run_stays_valid() {
    jet::compile("fn run() {}\n").expect("bare beginner entrypoint must compile");
}

#[test]
fn fallible_void_run_is_the_only_fallible_entrypoint() {
    let src = r#"
fn run() => Void ? {
    return Err("boom")
}
"#;
    let out = jet::compile(src).expect("fallible Void run should compile");
    assert!(
        out.rust.contains("pub fn user_run() -> Result<(), String>"),
        "Void ? run should lower to Result<(), String>:\n{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("if let Err(__jet_err) = jet_runtime_boundary(|| user_run())"),
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
fn crypto_fallible_void_run_uses_the_e3001_runtime_boundary() {
    let src = r#"
use core.crypto as crypto

fn run() => Void ? CryptoError {
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
        out.rust.contains(&format!("pub fn user_run() -> {return_type}")),
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

fn run() => Void ? CryptoError {
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

fn run() => Void ? CryptoError {
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

fn run() => Void ? CryptoError {
    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], 0)?
}
"#;
    let out = jet::compile(src).expect("CryptoError entrypoint should compile");
    let ffi = out.ffi.as_ref().expect("core.crypto must emit its bridge");
    let marker = "if let Err(__jet_err) = jet_runtime_boundary(|| user_run()) {";
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
fn user_run() -> Result<(), {ffi_name}::JetCryptoError> {{
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
fn fallible_void_run_can_finish_normally_after_try() {
    let src = r#"
fn step() => Int ? {
    return Ok(1)
}

fn run() => Void ? {
    n :: step()?
    print(n)
}
"#;
    let out = jet::compile(src).expect("fallible Void run should allow normal completion");
    assert!(
        out.rust.contains("Ok(())"),
        "fallible Void run should synthesize success at the end:\n{}",
        out.rust
    );
}

#[test]
fn unit_fallible_run_stays_rejected() {
    let src = r#"
fn run() => Unit ? {
    return Err("boom")
}
"#;
    let diags = jet::compile(src).expect_err("Unit ? run should not be accepted");
    assert!(
        diags.iter().any(|d| d.code == "E0122"),
        "expected E0122, got: {diags:?}"
    );
}

#[test]
fn fallible_void_fallthrough_is_entrypoint_only() {
    let src = r#"
fn helper() => Void ? {
}

fn run() {
    print("hi")
}
"#;
    let diags = jet::compile(src).expect_err("non-run Void ? fallthrough needs return");
    assert!(
        diags.iter().any(|d| d.code == "E0114"),
        "expected E0114, got: {diags:?}"
    );
}
