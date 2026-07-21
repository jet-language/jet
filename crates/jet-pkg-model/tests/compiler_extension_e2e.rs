//! Tower #549 C4 — real compiler-extension WASM guest harness.
//!
//! Compiles the include_str host (`Prelude/CompilerExtension.rs`) in-process
//! with the ratified wasmtime Component Model pin, drives fixture components
//! under `fixtures/compiler_extension/`, and proves:
//! - one custom-lint finding analyze → validate → stage → accept round-trip
//! - crash / malformed / incompatible / fuel-exhaust guests fail closed
//!   (Jet `E:` wires, no process abort, no rustc leak, no auto-commit)
//!
//! Compiler post-sema wiring and wall-clock epoch `timeout_ms` remain open.

#![allow(non_snake_case)]

#[path = "../src/Prelude/CompilerExtension.rs"]
mod extension_host;

use extension_host::{
    jet_compiler_extension_analyze, jet_compiler_extension_close, jet_compiler_extension_load,
};
use jet_pkg_model::CompilerExtension::{
    message_exposes_rustc, parse_analyze_result, parse_load_result, AnalyzeResponse, Capability,
    ExtensionSession, Finding, ProtocolError, SessionPhase, SpanFact, SymbolFact, TypedSnapshot,
    TypeFact,
};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/compiler_extension")
        .join(name)
}

fn sample_snapshot() -> TypedSnapshot {
    TypedSnapshot::new(
        Capability::v1_defaults().to_vec(),
        vec![TypeFact {
            id: "t1".into(),
            repr: "Int".into(),
        }],
        vec![SymbolFact {
            id: "s1".into(),
            name: "x".into(),
            kind: "let".into(),
            type_id: "t1".into(),
            span_id: "sp1".into(),
            effects: vec!["pure".into()],
            provenance: "sema".into(),
        }],
        vec![SpanFact {
            id: "sp1".into(),
            file: "main.jet".into(),
            start: 10,
            end: 11,
        }],
    )
    .unwrap()
}

fn assert_jet_owned(err: &ProtocolError) {
    assert!(
        !message_exposes_rustc(&err.message),
        "I2 leak in host message: {}",
        err.message
    );
}

fn load_guest(wasm: &str) -> Result<u64, ProtocolError> {
    let path = fixture(wasm);
    let wire = jet_compiler_extension_load(path.to_str().unwrap());
    let result = parse_load_result(&wire);
    if let Err(ref e) = result {
        assert_jet_owned(e);
    }
    result
}

#[test]
fn custom_lint_analyze_roundtrip_stages_finding() {
    let snap = sample_snapshot();
    let handle = load_guest("lint_no_x.wasm").expect("load lint guest");
    let mut session = ExtensionSession::new();
    session.on_loaded(handle).unwrap();

    let snap_bytes = snap.encode().unwrap();
    let wire = jet_compiler_extension_analyze(handle, &snap_bytes);
    let raw = parse_analyze_result(&wire).expect("analyze must succeed");
    let staged = session.stage_response(&snap, &raw).expect("validate+stage");
    assert_eq!(staged.findings.len(), 1);
    assert_eq!(staged.findings[0].rule, "no-x");
    assert_eq!(staged.findings[0].span_id, "sp1");
    assert_eq!(staged.findings[0].severity, "warning");
    assert!(!session.is_committed());

    let accepted = session.accept_staged().unwrap();
    assert_eq!(
        accepted,
        AnalyzeResponse {
            protocol: 1,
            findings: vec![Finding {
                rule: "no-x".into(),
                span_id: "sp1".into(),
                message: "prefer y".into(),
                severity: "warning".into(),
            }],
            proposed_edits: vec![],
            artifacts: vec![],
        }
    );
    assert!(session.is_committed());

    assert!(session.close(jet_compiler_extension_close));
    assert_eq!(session.phase(), SessionPhase::Closed);
}

#[test]
fn crash_guest_traps_fail_closed_without_commit() {
    let snap = sample_snapshot();
    let handle = load_guest("crash.wasm").expect("load crash guest");
    let mut session = ExtensionSession::new();
    session.on_loaded(handle).unwrap();

    let wire = jet_compiler_extension_analyze(handle, &snap.encode().unwrap());
    let err = parse_analyze_result(&wire).expect_err("crash must fail closed");
    assert!(
        err.message.contains("trapped") || err.message.contains("unreachable"),
        "expected trap wire, got: {}",
        err.message
    );
    assert_jet_owned(&err);
    assert!(session.staged().is_none());
    assert!(!session.is_committed());
    assert!(session.close(jet_compiler_extension_close));
}

#[test]
fn malformed_response_rejected_before_stage() {
    let snap = sample_snapshot();
    let handle = load_guest("malformed.wasm").expect("load malformed guest");
    let mut session = ExtensionSession::new();
    session.on_loaded(handle).unwrap();

    let wire = jet_compiler_extension_analyze(handle, &snap.encode().unwrap());
    let raw = parse_analyze_result(&wire).expect("guest returned bytes");
    let err = session
        .stage_response(&snap, &raw)
        .expect_err("malformed JSON must not stage");
    assert_jet_owned(&err);
    assert!(session.staged().is_none());
    assert!(!session.is_committed());
    assert!(session.close(jet_compiler_extension_close));
}

#[test]
fn incompatible_guest_fails_at_load() {
    let err = load_guest("incompatible.wasm").expect_err("missing analyze export");
    assert!(
        err.message.contains("analyze") || err.message.contains("no exported"),
        "expected missing-analyze message, got: {}",
        err.message
    );
    assert_jet_owned(&err);
    // No live handle — session stays Idle / never commits.
    let mut session = ExtensionSession::new();
    assert_eq!(session.phase(), SessionPhase::Idle);
    assert!(!session.is_committed());
    assert!(!session.close(jet_compiler_extension_close));
}

#[test]
fn fuel_exhaustion_times_out_fail_closed() {
    let snap = sample_snapshot();
    let handle = load_guest("fuel_loop.wasm").expect("load fuel-loop guest");
    let mut session = ExtensionSession::new();
    session.on_loaded(handle).unwrap();

    let wire = jet_compiler_extension_analyze(handle, &snap.encode().unwrap());
    let err = parse_analyze_result(&wire).expect_err("infinite loop must exhaust fuel");
    assert!(
        err.message.contains("trapped")
            || err.message.to_ascii_lowercase().contains("fuel"),
        "expected fuel/trap failure, got: {}",
        err.message
    );
    assert_jet_owned(&err);
    assert!(session.staged().is_none());
    assert!(!session.is_committed());
    assert!(session.close(jet_compiler_extension_close));
}
