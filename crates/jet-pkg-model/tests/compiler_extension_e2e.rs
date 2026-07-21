//! Tower #549 C4/C5 — real compiler-extension WASM guest harness.
//!
//! Drives fixture components under `fixtures/compiler_extension/` through the
//! compiled `CompilerExtensionHost` (same wasmtime substrate the driver uses)
//! and proves:
//! - one custom-lint finding analyze → validate → stage → accept round-trip
//! - crash / malformed / incompatible / fuel-exhaust guests fail closed
//!   (Jet `E:` wires, no process abort, no rustc leak, no auto-commit)
//! - wall-clock epoch `timeout_ms` interrupts a looping guest fail-closed
//! - WASI-random import guests fail closed at load (deterministic sandbox)
//! - pure guest re-analyze is byte-identical for the same snapshot
//!
//! Driver post-sema wire + AOT/dev fact-parity proofs live in
//! `jet-driver::CompilerExtensionHook`. C5 still requires independent Sol
//! review and full `scripts/agent/verify-full.sh` (not claimed by this crate).

#![allow(non_snake_case)]

use jet_pkg_model::CompilerExtension::{
    message_exposes_rustc, parse_analyze_result, parse_load_result, AnalyzeResponse, Capability,
    ExtensionSession, Finding, ProtocolError, SessionPhase, SpanFact, SymbolFact, TypedSnapshot,
    TypeFact,
};
use jet_pkg_model::CompilerExtensionHost::{
    jet_compiler_extension_analyze, jet_compiler_extension_analyze_with_limits,
    jet_compiler_extension_close, jet_compiler_extension_load,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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

#[test]
fn wall_clock_timeout_ms_epoch_interrupt_fail_closed() {
    // Huge fuel so the wall-clock epoch path wins over fuel exhaustion.
    // Short timeout keeps the live proof fast while still observing sleep.
    const HUGE_FUEL: u64 = u64::MAX / 4;
    const TIMEOUT_MS: u64 = 50;

    let snap = sample_snapshot();
    let handle = load_guest("fuel_loop.wasm").expect("load loop guest");
    let mut session = ExtensionSession::new();
    session.on_loaded(handle).unwrap();

    let start = Instant::now();
    let wire = jet_compiler_extension_analyze_with_limits(
        handle,
        &snap.encode().unwrap(),
        HUGE_FUEL,
        TIMEOUT_MS,
    );
    let elapsed = start.elapsed();
    let err = parse_analyze_result(&wire).expect_err("loop must hit wall-clock timeout");
    let lower = err.message.to_ascii_lowercase();
    assert!(
        lower.contains("interrupt") || lower.contains("epoch") || lower.contains("trapped"),
        "expected epoch interrupt trap, got: {}",
        err.message
    );
    assert!(
        !lower.contains("fuel"),
        "timeout proof must not be a fuel miss: {}",
        err.message
    );
    assert!(
        elapsed >= Duration::from_millis(TIMEOUT_MS.saturating_sub(10)),
        "epoch interrupt fired too early ({elapsed:?}); expected ~{TIMEOUT_MS}ms wall budget"
    );
    assert!(
        elapsed < Duration::from_millis(2_000),
        "epoch interrupt took too long ({elapsed:?}); wall path should not wait full v1 default"
    );
    assert_jet_owned(&err);
    assert!(session.staged().is_none());
    assert!(!session.is_committed());
    assert!(session.close(jet_compiler_extension_close));
}

#[test]
fn wasi_random_import_guest_fails_closed_at_load() {
    // Deterministic sandbox law: ambient entropy imports are denied.
    let err = load_guest("imports_random.wasm").expect_err("WASI random must fail load");
    let lower = err.message.to_ascii_lowercase();
    assert!(
        lower.contains("import")
            || lower.contains("instantiate")
            || lower.contains("random")
            || lower.contains("wasi"),
        "expected import/instantiate denial, got: {}",
        err.message
    );
    assert!(
        lower.contains("no clock")
            || lower.contains("no host import")
            || lower.contains("admits no host"),
        "message must name the deterministic sandbox denial, got: {}",
        err.message
    );
    assert_jet_owned(&err);
    let mut session = ExtensionSession::new();
    assert_eq!(session.phase(), SessionPhase::Idle);
    assert!(!session.is_committed());
    assert!(!session.close(jet_compiler_extension_close));
}

#[test]
fn pure_guest_reanalyze_is_byte_identical() {
    let snap = sample_snapshot();
    let snap_bytes = snap.encode().unwrap();
    let handle = load_guest("lint_no_x.wasm").expect("load pure lint guest");
    let mut session = ExtensionSession::new();
    session.on_loaded(handle).unwrap();

    let wire_a = jet_compiler_extension_analyze(handle, &snap_bytes);
    let wire_b = jet_compiler_extension_analyze(handle, &snap_bytes);
    let raw_a = parse_analyze_result(&wire_a).expect("first analyze");
    let raw_b = parse_analyze_result(&wire_b).expect("second analyze");
    assert_eq!(
        raw_a, raw_b,
        "deterministic sandbox: same snapshot must yield identical analyze bytes"
    );
    assert!(session.staged().is_none());
    assert!(!session.is_committed());
    assert!(session.close(jet_compiler_extension_close));
}

#[test]
fn analyze_wasm_component_helper_accepts_lint() {
    let snap = sample_snapshot();
    let path = fixture("lint_no_x.wasm");
    let accepted =
        jet_pkg_model::CompilerExtension::analyze_wasm_component(path.to_str().unwrap(), &snap)
            .expect("helper round-trip");
    assert_eq!(accepted.findings.len(), 1);
    assert_eq!(accepted.findings[0].rule, "no-x");
}
