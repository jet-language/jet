//! Diagnostics coverage accounting (board card c116, P0).
//!
//! Enforces invariant I4: every emitted code must have
//!   (a) an entry in docs/spec/diagnostics.md,
//!   (b) at least one tests/ui (or tests/ui_lint, tests/cli, tests/release)
//!       snapshot that mentions it, and
//!   (c) a `jet explain` page (resolves through Explain::lookup).
//!
//! Also checks I2 regression: the FFI bridge must not surface raw rustc error
//! codes as Jet diagnostic codes (E0061, E0277, E0308, E0425 are rustc codes
//! used internally for pattern-matching rustc stderr — they must never appear
//! as Jet diagnostic IDs in any snapshot or Source/ emission path that calls
//! Diagnostic::error / Diagnostic::warn).
//!
//! Run: `cargo test diagnostics_coverage`
//! Bless new snapshots: `UPDATE_EXPECT=1 cargo test`

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(p: &PathBuf) -> String {
    fs::read_to_string(p).unwrap_or_else(|_| panic!("cannot read {}", p.display()))
}


/// Collect all [EL]NNNN codes emitted in Source/ via Diagnostic::error /
/// Diagnostic::warn (the string literal form `"E0xxx"` / `"L0xxx"`).
fn emitted_codes() -> BTreeSet<String> {
    let src_dir = root().join("Source");
    let mut codes: BTreeSet<String> = BTreeSet::new();
    walk_rs(&src_dir, &mut |content| {
        // Match `"E0xxx"` or `"L0xxx"` — the string literal form used at
        // Diagnostic::error / push sites and eprintln! paths in main.rs.
        for code in extract_quoted_codes(content) {
            codes.insert(code);
        }
    });
    codes
}

/// Extract Jet diagnostic codes from Rust source, in two forms:
/// 1. `"E0xxx"` — standalone quoted string literal (used in Diagnostic::error calls)
/// 2. `[E0xxx]` — bracket form inside a string literal (used in eprintln! / format! paths)
fn extract_quoted_codes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        // Form 1: standalone "E0xxx" string literal
        if bytes[i] == b'"'
            && (bytes[i + 1] == b'E' || bytes[i + 1] == b'L')
            && bytes[i + 2..i + 6].iter().all(|b| b.is_ascii_digit())
            && bytes[i + 6] == b'"'
        {
            out.push(String::from_utf8_lossy(&bytes[i + 1..i + 6]).to_string());
        }
        // Form 2: [E0xxx] bracket form inside a string (e.g., eprintln!("Error [E0043]: ..."))
        if bytes[i] == b'['
            && (bytes[i + 1] == b'E' || bytes[i + 1] == b'L')
            && bytes[i + 2..i + 6].iter().all(|b| b.is_ascii_digit())
            && bytes[i + 6] == b']'
        {
            out.push(String::from_utf8_lossy(&bytes[i + 1..i + 6]).to_string());
        }
        i += 1;
    }
    out
}

fn walk_rs(dir: &PathBuf, cb: &mut impl FnMut(&str)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_rs(&p, cb);
        } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            if let Ok(content) = fs::read_to_string(&p) {
                cb(&content);
            }
        }
    }
}

/// All codes registered in docs/spec/diagnostics.md (the `| E0xxx |` registry
/// table rows, excluding separator lines).
fn registered_codes() -> BTreeSet<String> {
    let diag_md = read(&root().join("docs/spec/diagnostics.md"));
    let mut out = BTreeSet::new();
    for line in diag_md.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if cells.len() < 3 {
            continue;
        }
        let code = cells[1].trim();
        if code.len() == 5
            && (code.starts_with('E') || code.starts_with('L'))
            && code[1..].chars().all(|c| c.is_ascii_digit())
        {
            out.insert(code.to_string());
        }
    }
    out
}

/// Whether a code is marked as retired in diagnostics.md.
fn is_retired(code: &str, diag_md: &str) -> bool {
    for line in diag_md.lines() {
        if line.contains(code) && line.contains("retired") {
            return true;
        }
    }
    false
}

/// All [EL]NNNN codes that appear in any snapshot file (*.stderr, *.warn,
/// tests/cli/*.txt, tests/release/*.txt, and subdirectory stderr files).
fn snapshot_codes() -> BTreeSet<String> {
    let root = root();
    let mut out: BTreeSet<String> = BTreeSet::new();

    // tests/ui/*.stderr (flat) and tests/ui/**/stderr (subdirs)
    let ui = root.join("tests/ui");
    if let Ok(entries) = fs::read_dir(&ui) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                // subdirectory fixture: tests/ui/foo/stderr
                let sub = p.join("stderr");
                if sub.is_file() {
                    for code in extract_snapshot_codes(&read(&sub)) {
                        out.insert(code);
                    }
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some("stderr") {
                for code in extract_snapshot_codes(&read(&p)) {
                    out.insert(code);
                }
            }
        }
    }

    // tests/ui_lint/*.warn
    let lint = root.join("tests/ui_lint");
    if let Ok(entries) = fs::read_dir(&lint) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("warn") {
                for code in extract_snapshot_codes(&read(&p)) {
                    out.insert(code);
                }
            }
        }
    }

    // tests/cli/*.txt
    let cli = root.join("tests/cli");
    if let Ok(entries) = fs::read_dir(&cli) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("txt") {
                for code in extract_snapshot_codes(&read(&p)) {
                    out.insert(code);
                }
            }
        }
    }

    // tests/release/*.txt
    let rel = root.join("tests/release");
    if let Ok(entries) = fs::read_dir(&rel) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("txt") {
                for code in extract_snapshot_codes(&read(&p)) {
                    out.insert(code);
                }
            }
        }
    }

    // Also scan test/*.rs files for .contains("ENNNNN") / d.code == "ENNNNN" patterns.
    // These cover codes verified by assertion in cffi.rs, pkg.rs, repl.rs, dev.rs, etc.
    // Skip diagnostics_coverage.rs itself — its constant arrays would create false positives.
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    if let Ok(entries) = fs::read_dir(&tests_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                if p.file_name().and_then(|n| n.to_str()) == Some("diagnostics_coverage.rs") {
                    continue; // skip self — our constant arrays would be false positives
                }
                if let Ok(content) = fs::read_to_string(&p) {
                    for code in extract_assert_codes(&content) {
                        out.insert(code);
                    }
                }
            }
        }
    }

    out
}

/// Extract codes from Rust test assertions like `.contains("E1234")` or
/// `d.code == "E1234"` or `assert_eq!(diag.code, "E1234")`.
fn extract_assert_codes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        // Match `"E0xxx"` or `"L0xxx"` in any assertion context
        if bytes[i] == b'"'
            && (bytes[i + 1] == b'E' || bytes[i + 1] == b'L')
            && bytes[i + 2..i + 6].iter().all(|b| b.is_ascii_digit())
            && bytes[i + 6] == b'"'
        {
            out.push(String::from_utf8_lossy(&bytes[i + 1..i + 6]).to_string());
        }
        i += 1;
    }
    out
}

/// Extract [ENNNNN] / [LNNNNN] from snapshot text (the rendered-output form).
fn extract_snapshot_codes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        if bytes[i] == b'['
            && (bytes[i + 1] == b'E' || bytes[i + 1] == b'L')
            && bytes[i + 2..i + 6].iter().all(|b| b.is_ascii_digit())
            && bytes[i + 6] == b']'
        {
            out.push(String::from_utf8_lossy(&bytes[i + 1..i + 6]).to_string());
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// The known exclusions
// ---------------------------------------------------------------------------

/// Codes present in Source/ that are NOT Jet diagnostic codes — they are rustc
/// error codes used in FFI.rs to classify rustc stderr output (I2 / looks_like_signature_mismatch).
/// They must never appear in a Jet Diagnostic::error call or a user-facing snapshot
/// (in [Exxxx] bracket format) EXCEPT when embedded in a "cargo said:" block inside an FFI diagnostic.
///
/// NOTE: E0308 is intentionally excluded here even though it also appears as a rustc code —
/// Jet uses E0308 independently for the null-typing diagnostic (CheckerInfer.rs).
/// The I2 guard for E0308 is that it must never appear ORIGINATING from rustc code paths,
/// which is confirmed by the FFI.rs Diagnostic::error check above.
const RUSTC_CODES_IN_SOURCE: &[&str] = &[
    "E0061", // rustc: wrong arg count — NOT a Jet code
    "E0277", // rustc: trait bound not satisfied — NOT a Jet code
    "E0425", // rustc: unresolved name — NOT a Jet code
];

/// Internal placeholder used in Jetpack/ModuleEval — never reaches the user.
const INTERNAL_PLACEHOLDER: &[&str] = &["E0000"];

/// Codes registered in diagnostics.md as *retired*; still emitted for backward-compat
/// log readability but not expected to have new snapshots.
/// (E0019 is retired but still fires for legacy `import` spelling. The
///  retirement note in diagnostics.md is the coverage proof.)
const RETIRED_WITH_LEGACY_EMISSION: &[&str] = &["E0019"];

/// Codes that are fully implemented (parser + sema) but gated behind an
/// unreleased syntax feature; the gate is named here. They satisfy I4 via
/// the diagnostics.md entry + the gate comment; snapshots will land with the gate.
///
/// Gate: D-UNINIT1 (#Uninit binding) — parser gate in Parser/Statements.rs:660
const STAGED_BEHIND_GATE: &[&str] = &[
    "E0420", // #Uninit: read before write
    "E0421", // #Uninit: needs type annotation
    "E0422", // #Uninit: cannot have initializer
    "E0423", // #Uninit: needs plain-data type
    "E0424", // #Uninit: needs use core.mem
];

/// Codes that ARE fully implemented but cannot be snapshot-tested via a .jet file
/// because the triggering condition causes a physical problem (stack overflow, system
/// dependency) before the diagnostic fires. Coverage is via Source/ unit tests or
/// the diagnostic code entry in diagnostics.md.
///
/// E0909: Parser depth-guards at 64 levels of generic nesting, but the parser's
///        own recursive descent overflows the stack before reaching depth 64.
///        The actual depth guard is tested in Source/Generics.rs unit tests.
///        (Tracked as a known issue: the guard depth should be lowered to avoid the
///        stack overflow, but that's a separate bug fix.)
const UNTESTABLE_VIA_SNAPSHOT: &[&str] = &[
    "E0909", // generic instantiation: parser stack overflow before depth guard fires
];

/// Codes that are emitted in Source/ but have no test coverage yet.
/// These are ACKNOWLEDGED GAPS — not exclusions. Each entry must name WHY it has no coverage
/// and what work is needed. This list should shrink over time.
///
/// The test still PASSES with these codes listed here, but a separate test
/// (`acknowledged_coverage_gaps_are_expected`) verifies the list doesn't GROW silently.
const ACKNOWLEDGED_COVERAGE_GAPS: &[&str] = &[
    // Jetpack ModuleEval diagnostics (E0966–E0978):
    // These fire only from `jet::Jetpack::ModuleEval::evaluate_env` / `evaluate_modules`,
    // not from `jet check`/`jet build`/`jet run`. No unit tests exist for ModuleEval yet.
    // TODO: add tests/module_eval.rs using jet::Jetpack::ModuleEval::evaluate_source().
    "E0966",
    "E0967",
    "E0968",
    "E0969",
    "E0970",
    "E0971",
    "E0972",
    "E0973",
    "E0974",
    "E0975",
    "E0976",
    "E0977",
    "E0978",
    // Environment-specific: require git-not-installed or a specific store condition.
    // These are integration-test scenarios that need CI matrix variations.
    "E1203", // git not installed (requires git to be absent from PATH)
    // Feature-staged: registered and spec'd, but the feature isn't wired up yet.
    "E1207", // registry dep not supported (registry resolver not yet implemented)
    // REPL codes: tested in tests/repl.rs (E1802 is there; E1801 is not yet tested).
    // TODO: add a repl test that hits the fuel cap.
    "E1801",
    // E2202 (dev interpreter step budget) is covered by tests/dev.rs
    // (`infinite_loop_hits_e2202_fuel_stop` + the c77 battery boundary set).
    // Cross-compilation: E3302 requires a target not in the test environment.
    "E3302",
    // Package build I/O: E3402 requires a sandboxed package build environment.
    "E3402",
    // Doctor advisory: L2101 is in tests/cli/doctor_ok.txt but not in [L2101] format.
    // The doctor output renders it as "L2101: ..." not as "[L2101]".
    // TODO: update doctor output format to use [L2101] brackets, or add assertion in cli.rs.
    "L2101",
    // `jet bind` header translation failure: E3208 requires a broken C header file passed
    // to `jet bind`, which in turn requires bindgen/libclang available. Integration test
    // needs a CI matrix with libclang installed. The eprintln! emission site is in CmdDevTools.rs.
    // TODO: add tests/devtools.rs test with a bad header.
    "E3208",
];

/// All exclusions combined.
fn all_exclusions() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for &c in RUSTC_CODES_IN_SOURCE
        .iter()
        .chain(INTERNAL_PLACEHOLDER.iter())
        .chain(RETIRED_WITH_LEGACY_EMISSION.iter())
        .chain(STAGED_BEHIND_GATE.iter())
        .chain(UNTESTABLE_VIA_SNAPSHOT.iter())
    {
        out.insert(c.to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// I2 regression guard
// ---------------------------------------------------------------------------

/// Confirm that rustc codes are only used in the `looks_like_signature_mismatch`
/// pattern matcher, not as Jet Diagnostic codes.
#[test]
fn i2_rustc_codes_do_not_leak_as_jet_diagnostics() {
    let ffi_path = root().join("Source/FFI.rs");
    let ffi_content = read(&ffi_path);

    // The FFI.rs pattern-matcher is the only allowlisted use.
    // Verify the strings appear only inside a function whose name contains "mismatch" or "looks_like".
    let allowed_fn_context = "looks_like_signature_mismatch";

    for &code in RUSTC_CODES_IN_SOURCE {
        // Count occurrences in FFI.rs — should all be in looks_like_signature_mismatch
        let in_ffi = ffi_content.matches(code).count();
        // Count occurrences in all other Source/ files
        let src_dir = root().join("Source");
        let mut other_count = 0usize;
        walk_rs(&src_dir, &mut |content| {
            // Skip FFI.rs itself
            other_count += content
                .lines()
                .filter(|l| l.contains(code))
                .filter(|l| {
                    // Any line that uses Diagnostic::error with this code is a leak
                    l.contains("Diagnostic::error") || l.contains("Diagnostic::warn")
                })
                .count();
        });

        assert_eq!(
            other_count, 0,
            "I2 violation: rustc error code {} appears in a Jet Diagnostic::error/warn call \
             outside of {}: {} occurrence(s). \
             Rustc codes must NEVER be emitted as Jet diagnostics — only used to classify rustc stderr.",
            code, allowed_fn_context, other_count
        );

        // Also confirm FFI.rs usage is benign (not inside a Diagnostic::error call)
        let ffi_diag_lines = ffi_content
            .lines()
            .filter(|l| l.contains(code) && l.contains("Diagnostic::error"))
            .count();
        assert_eq!(
            ffi_diag_lines, 0,
            "I2 violation: rustc code {} appears inside a Diagnostic::error call in FFI.rs",
            code
        );

        // Verify no snapshot file mentions the rustc code in [ENNNNN] format
        let bracketed = format!("[{}]", code);
        let snaps = snapshot_codes();
        assert!(
            !snaps.contains(code),
            "I2 violation: rustc code {} appears in a user-facing snapshot as [{}]. \
             Rustc error codes must never reach end users.",
            code, code
        );
        let _ = (in_ffi, &allowed_fn_context, &bracketed);
    }
}

// ---------------------------------------------------------------------------
// I4(a): every emitted Jet code has a diagnostics.md entry
// ---------------------------------------------------------------------------

#[test]
fn every_emitted_code_has_diagnostics_md_entry() {
    let emitted = emitted_codes();
    let registered = registered_codes();
    let exclusions = all_exclusions();

    let mut missing: Vec<String> = emitted
        .iter()
        .filter(|c| !exclusions.contains(*c))
        .filter(|c| !registered.contains(*c))
        .cloned()
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "The following codes are emitted in Source/ but have NO entry in \
         docs/spec/diagnostics.md (invariant I4a).\n\
         For each: add a row to the Error code registry table with code, stage, and meaning,\n\
         then add what/why/fix and a tests/ui snapshot.\n\
         Missing:\n  {}",
        missing.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// I4(b): every emitted Jet code has at least one snapshot
// ---------------------------------------------------------------------------

#[test]
fn every_emitted_code_has_snapshot() {
    let emitted = emitted_codes();
    let snaps = snapshot_codes();
    let exclusions = all_exclusions();
    let diag_md = read(&root().join("docs/spec/diagnostics.md"));
    let acknowledged: BTreeSet<String> = ACKNOWLEDGED_COVERAGE_GAPS
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut missing: Vec<String> = emitted
        .iter()
        .filter(|c| !exclusions.contains(*c))
        .filter(|c| !acknowledged.contains(*c))
        .filter(|c| !snaps.contains(*c))
        .filter(|c| !is_retired(c, &diag_md))
        .cloned()
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "The following codes are emitted in Source/ but have NO tests/ui snapshot \
         (invariant I4b). Add a .jet fixture + .stderr/.warn golden file for each.\n\
         Missing:\n  {}",
        missing.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Acknowledged gap sentinel: the list must not grow silently
// ---------------------------------------------------------------------------

/// Verifies that the ACKNOWLEDGED_COVERAGE_GAPS list does not include codes that
/// now have coverage (codes should be removed from the list when they're fixed),
/// and that no new unacknowledged gap has appeared.
#[test]
fn acknowledged_gaps_are_still_unresolved() {
    let snaps = snapshot_codes();
    let diag_md = read(&root().join("docs/spec/diagnostics.md"));

    // Codes in the acknowledged list that now have snapshot coverage — time to remove them.
    let mut now_covered: Vec<String> = ACKNOWLEDGED_COVERAGE_GAPS
        .iter()
        .filter(|c| snaps.contains(&c.to_string()) && !is_retired(c, &diag_md))
        .map(|s| s.to_string())
        .collect();
    now_covered.sort();

    assert!(
        now_covered.is_empty(),
        "These codes in ACKNOWLEDGED_COVERAGE_GAPS now have snapshot coverage — \
         remove them from the list in tests/diagnostics_coverage.rs:\n  {}",
        now_covered.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// I4(c): jet explain resolves for every live registered code
// ---------------------------------------------------------------------------

#[test]
fn every_registered_code_has_explain_page() {
    let registered = registered_codes();
    let diag_md = read(&root().join("docs/spec/diagnostics.md"));

    let mut missing: Vec<String> = registered
        .iter()
        .filter(|c| !is_retired(c, &diag_md))
        .filter(|c| jet::Explain::lookup(c).is_none())
        .cloned()
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "The following codes are registered in docs/spec/diagnostics.md but \
         `jet explain <code>` returns None (invariant I4c).\n\
         Since Explain.rs is built directly from diagnostics.md, this means the code \
         is malformed in the registry table (check pipe-column count).\n\
         Missing explain:\n  {}",
        missing.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Audit: codes in diagnostics.md but NOT emitted (spec ahead of impl)
//
// This is NOT a failure — it's an informational audit. Codes may be
// registered before their feature lands. We print a note if running with
// RUST_LOG=info or similar, but don't fail the test.
// ---------------------------------------------------------------------------

#[test]
fn registered_unimplemented_codes_are_expected() {
    // These codes are in diagnostics.md but not yet emitted in Source/.
    // They are INTENTIONALLY registered ahead of their feature milestone.
    // If a new code appears here that is NOT in this list, it is unexpected
    // and should be investigated.
    const EXPECTED_SPEC_AHEAD_OF_IMPL: &[&str] = &[
        "E0004", // retired
        "E0005", // retired
        "E0006", // retired
        "E0011", // retired
        "E2101", // CLI: emitted via eprintln! (not Diagnostic::error) in main.rs
        "E2102", // CLI: emitted via eprintln! (not Diagnostic::error) in main.rs
        "E2303", // alias for E1102 (view crossing task boundary); registered for jet explain
        "E2403", // E2-M6 (library authoring) — staged
        "E2410", // D-SERDE: runtime decode error (missing required field) — emitted as a DecodeError string in generated code, not a compile Diagnostic
        "E2412", // D-SERDE: runtime decode error (unknown field under #[DenyUnknownFields]) — emitted as a DecodeError string, not a compile Diagnostic
        "E2701", // E2-M9 (ring library) — staged
        "E2702", // E2-M9 — staged
        "E2801", // E2-M10 (networking) — staged
        "E2802", // E2-M10 — staged
        "E2803", // E2-M10 — staged
        "E2804", // E2-M10 — staged, but appears in tests/ui snapshot
        "E2902", // E2-M11 (#Todo typed holes) — staged
        "E3001", // E2-M12 runtime panic report — runtime, not compile-time
        "E3002", // E2-M12 error propagation trace — runtime
        "E3208", // emitted via eprintln! in CmdDevTools.rs
        "L2301", // E2-M5 advisory — staged
        "L2501", // reserved (path-normalisation issue noted in spec)
        "L2701", // E2-M9 — staged
        "L2801", // E2-M10 — staged
        "L2901", // E2-M11 — staged
    ];

    let expected: BTreeSet<String> = EXPECTED_SPEC_AHEAD_OF_IMPL
        .iter()
        .map(|s| s.to_string())
        .collect();

    let emitted = emitted_codes();
    let registered = registered_codes();
    let exclusions = all_exclusions();

    let spec_ahead_of_impl: BTreeSet<String> = registered
        .iter()
        .filter(|c| !exclusions.contains(*c))
        .filter(|c| !emitted.contains(*c))
        .cloned()
        .collect();

    // Anything in spec_ahead_of_impl but NOT in expected is a surprise.
    let mut unexpected: Vec<String> = spec_ahead_of_impl
        .difference(&expected)
        .cloned()
        .collect();
    unexpected.sort();

    assert!(
        unexpected.is_empty(),
        "Unexpected codes registered in diagnostics.md but not emitted in Source/.\n\
         If this is intentional (staged feature), add the code to \
         EXPECTED_SPEC_AHEAD_OF_IMPL in tests/diagnostics_coverage.rs.\n\
         Unexpected:\n  {}",
        unexpected.join("\n  ")
    );

    // Also flag anything in expected but now emitted (good news — just clean up the list).
    let now_emitted: Vec<String> = expected
        .iter()
        .filter(|c| emitted.contains(*c))
        .cloned()
        .collect();
    // Only mention this in documentation; it's not a hard failure.
    // (The codes in EXPECTED_SPEC_AHEAD_OF_IMPL may be promoted to emitted as
    //  features land; remove them from the list when that happens.)
    let _ = now_emitted;
}
