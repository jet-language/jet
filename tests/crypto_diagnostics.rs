mod common;

use std::path::PathBuf;
use std::process::Command;

fn scratch() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "jet-e2702-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
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
    assert!(json.starts_with("{\"schema\":\"jet.report/v1\",\"moment\":\"compile\""), "{json}");
    for field in [
        "\"code\":\"E2702\"",
        "\"what\":\"crypto API misuse\"",
        "\"reason\":\"output_length\"",
        "\"operation\":\"hkdf_sha256\"",
        "\"expected\":\"0..8160\"",
        "\"actual\":8161",
        "\"span\":",
        "\"cause\":[]",
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
fn e2702_json_redacts_an_absolute_input_to_its_project_path() {
    let root = scratch();
    let project = root.join("project");
    let source_dir = project.join("src");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("main.jet");
    std::fs::write(
        &source,
        concat!(
            "use core.crypto as crypto\n",
            "fn run() {\n",
            "    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], 8161)\n",
            "}\n",
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("check")
        .arg(&source)
        .arg("--json")
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json = String::from_utf8(output.stderr).unwrap();
    assert!(json.contains("\"file\":\"src/main.jet\""), "{json}");
    assert!(!json.contains(&root.to_string_lossy().into_owned()), "absolute path leaked: {json}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn multiple_e2702_diagnostics_are_independent_json_lines() {
    let root = scratch();
    std::fs::write(
        root.join("main.jet"),
        concat!(
            "use core.crypto as crypto\n",
            "fn first() {\n",
            "    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([1]), [], [], 8161)\n",
            "}\n",
            "fn second() {\n",
            "    _ :: crypto.hkdf_sha256(crypto.Secret.from_bytes([2]), [], [], 8162)\n",
            "}\n",
            "fn run() {}\n",
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
    let lines = json.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{json}");
    for (line, actual) in lines.into_iter().zip([8161, 8162]) {
        assert!(line.starts_with("{\"schema\":\"jet.report/v1\",\"moment\":\"compile\""), "{line}");
        assert!(line.contains("\"code\":\"E2702\""), "{line}");
        assert!(line.contains("\"reason\":\"output_length\""), "{line}");
        assert!(line.contains("\"operation\":\"hkdf_sha256\""), "{line}");
        assert!(line.contains(&format!("\"actual\":{actual}")), "{line}");
    }
    assert!(!root.join(".jet").exists(), "check emitted .jet state");
    assert!(!root.join("build").exists(), "check emitted a build artifact");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn safe_raw_nonce_json_omits_inapplicable_bounds() {
    let root = scratch();
    std::fs::write(
        root.join("main.jet"),
        concat!(
            "use core.crypto as crypto\n",
            "fn run() {\n",
            "    _ :: crypto.seal([], [], [], nonce: [0])\n",
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
    let json = String::from_utf8(output.stderr).unwrap();
    assert!(json.contains("\"reason\":\"raw_nonce\""), "{json}");
    assert!(json.contains("\"operation\":\"seal\""), "{json}");
    assert!(!json.contains("\"expected\":"), "{json}");
    assert!(!json.contains("\"actual\":"), "{json}");
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
