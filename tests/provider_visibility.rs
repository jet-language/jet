use std::fs;
use std::process::Command;

#[test]
fn raw_provider_backends_are_not_external_api() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!(
        "jet-provider-visibility-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(scratch.join("src")).unwrap();
    fs::write(
        scratch.join("Cargo.toml"),
        format!(
            "[package]\nname = \"provider-visibility\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\njetpack = {{ path = {:?} }}\n",
            root.join("crates/jetpack")
        ),
    )
    .unwrap();
    fs::write(
        scratch.join("src/main.rs"),
        "use jetpack::Provider::{provider_for, CoreProvider, NixProvider, Provider};\nfn main() {}\n",
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["check", "--offline"])
        .current_dir(&scratch)
        .env("CARGO_TARGET_DIR", root.join("target/provider-visibility"))
        .output()
        .unwrap();
    assert!(!output.status.success(), "raw provider API unexpectedly compiled");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for symbol in ["provider_for", "CoreProvider", "NixProvider", "Provider"] {
        assert!(stderr.contains(symbol) && stderr.contains("private"), "stderr: {stderr}");
    }
    let _ = fs::remove_dir_all(scratch);
}
