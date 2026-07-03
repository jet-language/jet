//! D-SEMINDEX1 integration tests for the stable semantic-index API.

use jet_semindex::{open, SCHEMA_VERSION};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features")
        .join(name)
}

#[test]
fn semindex_schema_version() {
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn semindex_hello_json_shape() {
    let idx = open(&fixture("basics/hello.jet")).expect("hello indexes");
    let json = idx.to_json();
    assert!(json.starts_with('{'));
    assert!(json.contains("\"schema_version\":1"));
    assert!(json.contains("\"definitions\""));
    assert!(json.contains("\"run\""));
}

#[test]
fn semindex_effects_and_calls() {
    let idx = open(&fixture("effects/effects.jet")).expect("effects indexes");
    assert!(idx.lookup("report").is_some());
    assert!(!idx.call_edges().is_empty());
    let report_effects = idx.effect_of("report").expect("report has effects");
    assert!(!report_effects.inferred.is_empty() || !report_effects.direct.is_empty());
}

#[test]
fn semindex_references() {
    let idx = open(&fixture("basics/hello.jet")).expect("hello indexes");
    assert!(!idx.references_to("print").is_empty());
}

#[test]
fn jet_semindex_cli_json_smoke() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let path = fixture("basics/hello.jet");
    let out = std::process::Command::new(bin)
        .args(["semindex", path.to_str().unwrap(), "--json"])
        .output()
        .expect("jet semindex");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"schema_version\":1"));
}
