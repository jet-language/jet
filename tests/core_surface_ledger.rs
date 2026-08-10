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
