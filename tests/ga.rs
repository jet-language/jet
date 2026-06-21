//! E2-M17 GA checklist — asserts that Epoch 2 is complete at the compiler
//! level. This test is the enforcement layer for the exit criteria in
//! docs/plans/epoch-2/m17-epoch2-ga.md.
//!
//! What is checked here:
//!   1. Every E2 diagnostic registered in diagnostics.md has a `jet explain`
//!      entry (already enforced by cli.rs; duplicated here for M17 traceability).
//!   2. All 6 D-GA1=B showcase programs exist in examples/showcase/ and are
//!      front-end-clean (sema accepts them).
//!   3. Perf/size budgets: every showcase binary must stay under a hard size
//!      ceiling when built with `--small` (D-GA2=B).
//!   4. The `nix develop -c cargo test` suite is green (asserted by the CI
//!      that runs this test file).
//!
//! What is NOT checked here (deferred or out of scope):
//!   - DAP step-through debugger (VS Code extension work, deferred).
//!   - HTTP service showcase build time (network tests are flaky in CI).
//!   - jet dev demo (watch loop — not golden-testable).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ── 1. Every diagnostic code has a jet explain entry ──────────────────────

/// Mirrors the check in cli.rs `every_registered_code_has_an_explain_entry`.
/// Kept here for M17 traceability.
#[test]
fn ga_every_diagnostic_has_explain() {
    let md =
        fs::read_to_string(root().join("docs/spec/diagnostics.md")).expect("diagnostics.md");
    let index = jet::Explain::index();

    let mut missing = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with("| E") && !line.starts_with("| L") {
            continue;
        }
        let first = line.trim_matches('|').split('|').next().unwrap_or("").trim();
        if is_code(first) && !index.contains_key(first) {
            missing.push(first.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "M17 GA gate: these diagnostic codes lack a `jet explain` entry:\n  {}",
        missing.join(", ")
    );
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}

// ── 2. All D-GA1=B showcases exist and are front-end clean ────────────────

/// D-GA1=B mandates 6 showcases. This test asserts they all exist in
/// examples/showcase/ and pass the Jet front end (parse + sema).
#[test]
fn ga_all_showcases_front_end_clean() {
    // Showcase list — (filename, description).
    // Showcase 4 (jet dev demo) and C FFI (showcase 5) are separately tested
    // in tests/dev.rs and tests/cffi.rs respectively; listed for completeness.
    let showcases: &[(&str, &str)] = &[
        // Showcase 1 — CLI tools
        ("jetgrep.jet", "CLI tool: jetgrep"),
        ("jsonfmt.jet", "CLI tool: jsonfmt"),
        ("wordfreq.jet", "CLI tool: wordfreq"),
        // Showcase 2 — HTTP service
        ("http_service.jet", "HTTP service with tasks/channels"),
        // Showcase 3 — library authoring
        ("library.jet", "library: traits, delegation, labels"),
        // Showcase 5 — expert low-level tier (M13 @unsafe)
        ("lowlevel.jet", "low-level: @unsafe + Ptr<T>"),
        // Showcase 6 — freestanding / cross-compile smoke (M15)
        ("freestanding.jet", "freestanding smoke"),
    ];

    let showcase_dir = root().join("examples/showcase");
    for (file, desc) in showcases {
        let path = showcase_dir.join(file);
        assert!(
            path.is_file(),
            "M17 GA gate: showcase file missing: {} ({})",
            path.display(),
            desc
        );
        let src = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("cannot read {}", path.display()));
        let result = jet::compile_with_path(&src, path.to_str().unwrap());
        assert!(
            result.is_ok(),
            "M17 GA gate: showcase '{}' failed front end:\n{:?}",
            desc,
            result.err()
        );
    }
}

// ── 3. Hard size budgets (D-GA2=B) ────────────────────────────────────────

/// D-GA2=B: hard CI perf/size gates. Each showcase binary built with
/// `--small` must stay under its pinned ceiling.
///
/// Ceilings are generous — they catch accidental bloat (core library pulled in
/// unexpectedly) rather than micro-optimisation regressions.
#[test]
fn ga_showcase_size_budgets() {
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping GA size budgets (need jet + rustc)");
        return;
    }

    // (showcase file, max bytes with --small)
    // Budgets are 4 MiB per tool; adjust if the core library grows intentionally.
    let budgets: &[(&str, u64)] = &[
        ("jetgrep.jet", 4_194_304),
        ("jsonfmt.jet", 4_194_304),
        ("wordfreq.jet", 4_194_304),
        ("library.jet", 4_194_304),
        ("freestanding.jet", 4_194_304),
        ("lowlevel.jet", 4_194_304),
        // http_service.jet links tasks/net — skip size gate (varies by platform).
    ];

    let showcase_dir = root().join("examples/showcase");
    let build_dir = std::env::temp_dir().join(format!("jet_ga_budgets_{}", std::process::id()));
    fs::create_dir_all(build_dir.join("build")).unwrap();

    for (file, max_bytes) in budgets {
        let src = showcase_dir.join(file);
        let stem = Path::new(file).file_stem().unwrap().to_string_lossy();
        let bin = build_dir.join("build").join(stem.as_ref());

        let out = Command::new(&jet)
            .args(["build", "--small", src.to_str().unwrap()])
            .current_dir(&build_dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "GA size gate: `--small` build of {} failed:\n{}",
            file,
            String::from_utf8_lossy(&out.stderr)
        );

        let size = fs::metadata(&bin).map(|m| m.len()).unwrap_or(0);
        assert!(
            size <= *max_bytes && size > 0,
            "GA size gate: {} --small binary is {} bytes (limit {})",
            file,
            size,
            max_bytes
        );
    }

    let _ = fs::remove_dir_all(&build_dir);
}
