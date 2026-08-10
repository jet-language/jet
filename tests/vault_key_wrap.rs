mod common;

#[test]
fn actual_vault_key_wrap_bridge_fixture_compiles_and_runs() {
    let root = std::env::current_dir().unwrap();
    let dir = std::env::temp_dir().join(format!("jet-vault-key-wrap-proof-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let jet_codegen = root.join("crates/jet-codegen");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(r#"[package]
name = "jet-vault-key-wrap-proof"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
aes-gcm = "0.10"
age = "0.10"
argon2 = {{ version = "=0.5.3", default-features = false, features = ["alloc", "password-hash"] }}
blake3 = "1"
chacha20poly1305 = "0.10"
ed25519-dalek = "2"
hkdf = "0.12"
sha2 = "0.10"
subtle = "2"
x25519-dalek = "2"
jet-codegen = {{ path = "{}" }}
"#,jet_codegen.display()),
    )
    .unwrap();
    let fixture = root.join("tests/fixtures/vault_key_wrap_runtime.rs");
    std::fs::write(dir.join("src/lib.rs"), format!("include!({fixture:?});\n")).unwrap();
    let output = std::process::Command::new("cargo")
        .args(["test", "--offline", "--quiet", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .args(["--", "--test-threads=1"])
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "vault key-wrap proof failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
