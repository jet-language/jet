//! Card #1442 — the Core surface ledger must stay true.
//!
//! The ledger feeds the #1398 release gate, and the owner's 2026-08-03 ruling
//! set the bar at every language Jet competes with rather than Python alone.
//! Two things have to hold, and they are different questions:
//!
//! 1. the stored ledger still matches the compiler tables and the recorded
//!    competitor surfaces;
//! 2. the checker that decides (1) still rejects a broken ledger.
//!
//! Only asserting (1) would pass just as happily with every gate deleted, so
//! the hostile fixtures assert (2) by breaking one thing at a time and
//! requiring the matching gate to fire.

mod common;

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(flag: &str) -> (bool, String, String) {
    let output = Command::new("node")
        .arg("scripts/agent/check-core-surface-ledger.mjs")
        .arg(flag)
        .current_dir(root())
        .output()
        .expect("node must run the core-surface-ledger checker");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn source(path: &str) -> String {
    std::fs::read_to_string(root().join(path))
        .unwrap_or_else(|error| panic!("{path} must be readable: {error}"))
}

fn assert_layering_contract_is_taught() {
    let checker_surface = source("crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs");
    for marker in [
        "D-ONCE-LAYER1=B: the typed rung is core.crypto; the raw-byte rung is core.crypto.expert.",
        "D-ONCE-LAYER1=B: the raw-byte rung is core.crypto.expert; the typed rung is core.crypto.",
        "D-ONCE-LAYER1=B: the one-shot rung is core.http; the configurable rung is core.http.client.",
        "D-ONCE-LAYER1=B: the configurable rung is core.http.client; the one-shot rung is core.http.",
    ] {
        assert!(
            checker_surface.contains(marker),
            "Core module surface lost layering marker: {marker}"
        );
    }

    let mem_surface = source("crates/jet-foundation/src/Syntax/core_surface.rs");
    assert!(
        mem_surface.contains(
            "The import gate (`use core.mem`) unlocks every name; the audit gate (`#Unsafe(\"reason\")`) is required only for items marked `Audit`."
        ),
        "core.mem must state both gates at its module gate"
    );

    let laws = source("docs/spec/stdlib-api-laws.md");
    assert!(
        laws.contains("D-ONCE-LAYER1=B")
            && laws.contains("core.crypto.expert")
            && laws.contains("core.http.client"),
        "the ratified layering split must be in the API laws"
    );

    let crypto_example = source("examples/features/crypto/random_api_split.jet");
    assert!(
        crypto_example.contains("D-ONCE-LAYER1=B")
            && crypto_example.contains("use core.crypto as crypto")
            && crypto_example.contains("use core.crypto.expert as expert")
            && crypto_example.contains("crypto rungs agree: {rungs_agree}"),
        "the crypto example must teach both ratified rungs"
    );

    let http_example = source("examples/features/net/http_client.jet");
    assert!(
        http_example.contains("D-ONCE-LAYER1=B")
            && http_example.contains("use core.http as http")
            && http_example.contains("use core.http.client as http_client")
            && http_example.contains("http.get(")
            && http_example.contains("http_client.request("),
        "the HTTP example must teach both ratified rungs"
    );

    let retired_now = concat!("jet", ".time", ".now");
    let retired_format = concat!("jet", ".time", ".format");
    assert_eq!(
        jet::Syntax::rename_target(retired_now),
        Some("core.time.now"),
        "the retired clock door must use Syntax::RETIREMENTS"
    );
    assert_eq!(
        jet::Syntax::rename_target(retired_format),
        Some("DateTime.format_rfc3339()"),
        "the retired time format door must use Syntax::RETIREMENTS"
    );
}

/// Source-surface drift, an unmapped shipped method, a missing competitor
/// member, a duplicate row, a hidden exclusion, a stale owner, and an
/// unratified scope exclusion all fail here.
#[test]
fn core_surface_ledger_matches_its_sources() {
    let (ok, stdout, stderr) = run("--check");
    assert!(
        ok,
        "core surface ledger rejected:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("11 recorded competitor surfaces"),
        "the ledger must compare against all eleven languages the owner named:\n{stdout}"
    );
    assert_layering_contract_is_taught();
}

/// A gate that stops firing fails here rather than going quiet in CI.
#[test]
fn core_surface_ledger_checker_rejects_hostile_fixtures() {
    let (ok, stdout, stderr) = run("--hostile-fixtures");
    assert!(
        ok,
        "a hostile core-surface-ledger fixture was accepted:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for gate in [
        "duplicate row id",
        "fabricated competitor member",
        "unmapped shipped method",
        "hidden exclusion: a language skips a container",
        "a language is dropped from the comparison",
        "stale owner: cluster claims a closed card",
        "unratified scope exclusion",
        "hidden uncompared Core domain",
        "source-surface drift",
        "a competitor member is dropped from the ledger",
        // A capability name that recurs across domains scores differently
        // depending on whether it is one operation or several. Leaving one
        // unclassified silently keeps per-domain scoring, which can hold a real
        // gap at a single witness forever.
        "unclassified repeated capability name",
    ] {
        assert!(
            stdout.contains(gate),
            "the checker no longer proves it rejects `{gate}`:\n{stdout}"
        );
    }
}
