use std::path::PathBuf;
use std::process::Command;

fn scratch() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "jet-e2702-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn e2702_json_exits_one_and_creates_no_artifact() {
    let root = scratch();
    std::fs::write(
        root.join("main.jet"),
        concat!(
            "use core.crypto as crypto\n",
            "fn run() {\n",
            "    safe :: crypto.hkdf_sha256(\n",
            "        crypto.Secret.from_bytes([1]), [], [], 8161\n",
            "    )\n",
            "}\n",
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", "main.jet", "--json"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let json = String::from_utf8(output.stderr).unwrap();
    assert!(json.starts_with("{\"schema\":\"jet.diagnostic/v1\",\"code\":\"E2702\""), "{json}");
    for field in [
        "\"class\":\"user\"",
        "\"phase\":\"sema\"",
        "\"what\":\"crypto API misuse\"",
        "\"reason\":\"output_length\"",
        "\"operation\":\"hkdf_sha256\"",
        "\"expected\":\"0..8160\"",
        "\"actual\":8161",
        "\"primarySpan\":",
        "\"relatedSpans\":[]",
    ] {
        assert!(json.contains(field), "missing {field}: {json}");
    }
    for forbidden in ["password", "plaintext", "ciphertext", "backend", "rustc", "dependency"] {
        assert!(!json.contains(forbidden), "leaked `{forbidden}`: {json}");
    }
    assert!(!root.join(".jet").exists(), "check emitted .jet state");
    assert!(!root.join("build").exists(), "check emitted a build artifact");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn explain_e2702_teaches_precedence_and_dynamic_values() {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["explain", "E2702"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    for teaching in ["syntax, effects, and types", "compiler-known", "Dynamic values"] {
        assert!(text.contains(teaching), "missing `{teaching}`:\n{text}");
    }
}
