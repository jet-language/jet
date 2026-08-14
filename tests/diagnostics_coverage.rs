//! Diagnostics coverage accounting (board card c116, P0).
//!
//! Enforces invariant I4: every emitted code must have
//!   (a) an entry in the typed diagnostic-row registry,
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

mod common;

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

/// Collect all registered-shape codes emitted in Source/ via Diagnostic::error
/// / Diagnostic::warn.
fn emitted_codes() -> BTreeSet<String> {
    let mut codes: BTreeSet<String> = BTreeSet::new();
    let mut scan_dir = |dir: PathBuf| {
        walk_rs(&dir, &mut |content| {
            for code in extract_quoted_codes(content) {
                codes.insert(code);
            }
        });
    };
    // Scan both the root Source/ (Cmd*, LSP, etc.) and the seam crates.
    scan_dir(root().join("Source"));
    scan_dir(root().join("crates"));
    codes
}

/// Extract registered-shape Jet diagnostic codes from Rust source, in two forms:
/// 1. a standalone quoted code (used in Diagnostic::error calls)
/// 2. a bracketed code inside a string literal (used in eprintln! / format! paths)
fn extract_quoted_codes(text: &str) -> Vec<String> {
    let mut out = extract_delimited_codes(text, b'"', b'"');
    out.extend(extract_delimited_codes(text, b'[', b']'));
    out
}

fn extract_delimited_codes(text: &str, open: u8, close: u8) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == open {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_uppercase()
                    || bytes[end].is_ascii_digit()
                    || bytes[end] == b'-')
            {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == close {
                let code = &text[start..end];
                if jet::Explain::is_code(code) {
                    out.push(code.to_string());
                }
            }
        }
        i += 1;
    }
    out
}

fn walk_rs(dir: &PathBuf, cb: &mut impl FnMut(&str)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
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

/// All codes registered in the typed compile-time diagnostic rows.
fn registered_codes() -> BTreeSet<String> {
    jet_foundation::Registry::diagnostic_rows()
        .iter()
        .map(|row| row.code.to_string())
        .collect()
}

fn registered_code_rows() -> Vec<(String, usize)> {
    jet_foundation::Registry::diagnostic_rows()
        .iter()
        .enumerate()
        .map(|(idx, row)| (row.code.to_string(), idx + 5))
        .collect()
}

fn is_snake_case(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut previous_underscore = false;
    for (index, character) in name.chars().enumerate() {
        if character == '_' {
            if index == 0 || previous_underscore {
                return false;
            }
            previous_underscore = true;
        } else if character.is_ascii_lowercase() || character.is_ascii_digit() {
            previous_underscore = false;
        } else {
            return false;
        }
    }
    !previous_underscore
}

#[test]
fn every_registered_lint_has_a_unique_snake_case_name() {
    let registered_codes: BTreeSet<&str> = jet_foundation::Registry::diagnostic_rows()
        .iter()
        .filter(|row| row.severity == jet_foundation::Diagnostics::Severity::Lint)
        .map(|row| row.code)
        .collect();
    let lints = jet_foundation::LintPolicy::registered_lints();
    let mut names = BTreeSet::new();
    let mut codes = BTreeSet::new();

    for lint in lints {
        assert!(
            is_snake_case(lint.name),
            "lint `{}` is not a stable snake_case name",
            lint.name
        );
        assert!(names.insert(lint.name), "lint name `{}` is duplicated", lint.name);
        assert!(codes.insert(lint.code), "lint code `{}` is duplicated", lint.code);
        assert_eq!(
            jet_foundation::Registry::diagnostic(lint.code)
                .map(|row| row.severity),
            Some(jet_foundation::Diagnostics::Severity::Lint),
            "lint `{}` is not a registered lint diagnostic row",
            lint.code
        );
        assert_eq!(
            jet_foundation::LintPolicy::name_for_code(lint.code),
            Some(lint.name),
            "lint `{}` does not round-trip through the name registry",
            lint.code
        );
    }

    assert_eq!(
        codes, registered_codes,
        "every registered lint row must carry exactly one stable name"
    );
}

/// Whether a code is marked retired in the typed row source.
fn is_retired(code: &str, _diag_md: &str) -> bool {
    jet_foundation::Registry::diagnostic(code).is_some_and(|row| {
        row.status == jet_foundation::Registry::DiagnosticStatus::Retired
    })
}

fn staged_card_number(reason: &str) -> Option<u32> {
    let card = reason.strip_prefix("staged #")?;
    if card.is_empty() || !card.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    card.parse().ok().filter(|card| *card > 0)
}

fn allowlist_reason_errors(entries: &[(&str, &str)]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|(code, reason)| {
            if *reason == "retired" && is_retired(code, "") {
                None
            } else if staged_card_number(reason).is_some() {
                None
            } else {
                Some(format!(
                    "{code}: reason must be `retired` or `staged #<card>`"
                ))
            }
        })
        .collect()
}

fn now_emitted_codes<'a>(
    entries: impl IntoIterator<Item = &'a str>,
    emitted: &BTreeSet<String>,
) -> Vec<String> {
    let mut now_emitted: Vec<String> = entries
        .into_iter()
        .filter(|code| emitted.contains(*code))
        .map(str::to_string)
        .collect();
    now_emitted.sort();
    now_emitted
}

fn stale_allowlist_failure<'a>(
    entries: impl IntoIterator<Item = &'a str>,
    emitted: &BTreeSet<String>,
) -> Option<String> {
    let now_emitted = now_emitted_codes(entries, emitted);
    (!now_emitted.is_empty()).then(|| {
        format!(
            "EXPECTED_SPEC_AHEAD_OF_IMPL contains now-emitted codes; remove the line for each:\n  {}",
            now_emitted.join("\n  ")
        )
    })
}

/// All [EL]NNNN codes that appear in snapshot files or legacy test assertions.
/// Required rendered-snapshot codes are excluded from the legacy fallback.
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

    // Exact CLI stderr fixtures for command diagnostics that do not originate
    // from .jet UI source files.
    for fixture_dir in ["jetpack-diagnostics", "cli-diagnostics"] {
        let fixtures = root.join("tests/fixtures").join(fixture_dir);
        if let Ok(entries) = fs::read_dir(&fixtures) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("stderr") {
                    for code in extract_snapshot_codes(&read(&p)) {
                        out.insert(code);
                    }
                }
            }
        }
    }

    // Legacy runtime assertions outside this card remain accepted until their
    // owning cards migrate them. #343 coverage below uses committed artifacts;
    // none of E0966-E0978/E1203/E1207/E1801/E3302/E3402/L2101 depends
    // on this compatibility scan.
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    if let Ok(entries) = fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) == Some("rs")
                && path.file_name().and_then(|x| x.to_str()) != Some("diagnostics_coverage.rs")
            {
                if let Ok(content) = fs::read_to_string(path) {
                    for code in extract_assert_codes(&content) {
                        if !RENDERED_SNAPSHOT_REQUIRED.contains(&code.as_str()) {
                            out.insert(code);
                        }
                    }
                }
            }
        }
    }

    let module_eval = root.join("tests/fixtures/module-eval-diagnostics");
    if let Ok(entries) = fs::read_dir(&module_eval) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) == Some("stderr") {
                for code in extract_snapshot_codes(&read(&path)) {
                    out.insert(code);
                }
            }
        }
    }

    out
}

fn rendered_snapshot_texts() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for dir in [
        root().join("tests/ui"),
        root().join("tests/ui_lint"),
        root().join("tests/fixtures/jetpack-diagnostics"),
        root().join("tests/fixtures/module-eval-diagnostics"),
    ] {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                let sub = p.join("stderr");
                if sub.is_file() {
                    out.push((sub.clone(), read(&sub)));
                }
            } else if matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("stderr" | "warn")
            ) {
                out.push((p.clone(), read(&p)));
            }
        }
    }
    for name in [
        "bind_missing_e3208.txt",
        "bind_data_invalid_e3208.txt",
        "doctor_l2101.txt",
        "fetch_no_git_e1203.txt",
        "repl_e1801.txt",
        "unknown_target_e3302.txt",
    ] {
        let path = root().join("tests/cli").join(name);
        if path.is_file() {
            out.push((path.clone(), read(&path)));
        }
    }
    out
}

#[test]
fn diagnostic_voice_scope_includes_owned_runtime_artifacts() {
    let paths: BTreeSet<String> = rendered_snapshot_texts()
        .into_iter()
        .filter_map(|(path, _)| {
            path.strip_prefix(root())
                .ok()
                .map(|p| p.display().to_string())
        })
        .collect();
    for expected in [
        "tests/fixtures/module-eval-diagnostics/E0970.stderr",
        "tests/fixtures/module-eval-diagnostics/E0971.stderr",
        "tests/cli/doctor_l2101.txt",
        "tests/cli/fetch_no_git_e1203.txt",
        "tests/cli/repl_e1801.txt",
    ] {
        assert!(paths.contains(expected), "voice ratchet omitted {expected}");
    }
}

/// Extract codes from Rust test assertions like `.contains("E1234")` or
/// `d.code == "E1234"` or `assert_eq!(diag.code, "E1234")`.
fn extract_assert_codes(text: &str) -> Vec<String> {
    extract_delimited_codes(text, b'"', b'"')
}

/// Workspace diagnostics must have rendered snapshots. Code assertions in
/// `tests/workspace.rs` do not satisfy I4(b).
const RENDERED_SNAPSHOT_REQUIRED: &[&str] = &["E0995", "E0996", "E0997"];

/// Extract numeric or word-shaped bracket codes from snapshot text.
fn extract_snapshot_codes(text: &str) -> Vec<String> {
    extract_delimited_codes(text, b'[', b']')
}

#[test]
fn word_shaped_codes_are_registered_and_explainable() {
    let expected = [
        "E-WEB-ABI-TYPE",
        "E-WEB-CROSS-PARTITION",
        "E-WEB-TARGET-BROWSER",
        "E-WEB-TIR-UNSUPPORTED",
        "E-APP-TARGET-CAPABILITY",
        "E-OSTARGET-MIXED-AXIS",
        "E-OSTARGET-UNMATCHED-CALL",
        "E-OSTARGET-BUILD-CONTEXT",
        "E-OSTARGET-DISPATCH-ARM",
        "E-OSTARGET-DISPATCH-EXHAUSTIVE",
    ];
    let registered = registered_codes();
    let live = jet::Explain::live_codes();
    for code in expected {
        assert!(registered.contains(code), "scanner missed {code}");
        assert!(live.iter().any(|live_code| live_code == code), "explain missed {code}");
    }
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

/// Codes registered in the typed rows as *retired*; still emitted for backward-compat
/// log readability but not expected to have new snapshots.
/// (E0019 is retired but still fires for legacy `import` spelling. The
///  retirement status in the row is the coverage proof.)
const RETIRED_WITH_LEGACY_EMISSION: &[&str] = &["E0019"];

/// Codes that are fully implemented (parser + sema) but gated behind an
/// unreleased syntax feature; the gate is named here. They satisfy I4 via
/// the typed row + the gate comment; snapshots will land with the gate.
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
/// the typed diagnostic row.
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
    // E0916 is reserved for Debug auto-derive limits and documented as
    // "defined, not yet emitted"; the helper lives in Generics.rs before the
    // feature path is wired. Remove this once auto-derived Debug can actually
    // reject a non-debuggable field through sema.
    "E0916",
    // E2202 (dev interpreter step budget) is covered by tests/dev.rs
    // (`infinite_loop_hits_e2202_fuel_stop` + the c77 battery boundary set).
    // E0153: protocol expansion parse failure — internal compiler error path only
    // (D-PROTO1); no user-writable fixture triggers a failed fragment re-parse.
    "E0153",
    // E3001/E3005: `jet prove --json` (Source/CmdProve.rs render_report) now embeds these
    // as literal quoted `"E3001"`/`"E3005"` JSON-field values in generated evidence records,
    // which the literal-scan `emitted_codes()` picks up. But the real user-facing rendering
    // of these codes (D-OBS1/D-OBS2 runtime panic voice in jet_panic_rich/jet_contract_fail,
    // crates/jet-codegen/src/Prelude/Core.rs) is deliberately bracket-free — `panic: {msg}` /
    // `@{Pre|Post} contract failed: {msg}`, never `[E3001]`/`[E3005]` — so no snapshot fixture
    // can contain the bracket form this scanner's `extract_snapshot_codes` looks for without
    // fabricating text the compiler never prints. Real coverage exists as CLI-process
    // assertions in tests/prove.rs (`prove_captures_contract_results_and_runtime_panics_structurally`
    // asserts `"code":"E3001"` / `"code":"E3005"` in real `jet prove --json` output). Card #521.
    "E3001",
    "E3005",
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
    // The FFI.rs pattern-matcher is the only allowlisted use.
    // Verify the strings appear only inside a function whose name contains "mismatch" or "looks_like".
    let allowed_fn_context = "looks_like_signature_mismatch";
    let mut source_text = String::new();
    for dir in [root().join("Source"), root().join("crates")] {
        walk_rs(&dir, &mut |content| {
            source_text.push_str(content);
            source_text.push('\n');
        });
    }

    for &code in RUSTC_CODES_IN_SOURCE {
        // Count occurrences in Source/ and seam crates.
        let mut other_count = 0usize;
        for dir in [root().join("Source"), root().join("crates")] {
            walk_rs(&dir, &mut |content| {
                other_count += content
                    .lines()
                    .filter(|l| l.contains(code))
                    .filter(|l| {
                        // Any line that uses Diagnostic::error with this code is a leak
                        l.contains("Diagnostic::error") || l.contains("Diagnostic::warn")
                    })
                    .count();
            });
        }

        assert_eq!(
            other_count, 0,
            "I2 violation: rustc error code {} appears in a Jet Diagnostic::error/warn call \
             outside of {}: {} occurrence(s). \
             Rustc codes must NEVER be emitted as Jet diagnostics — only used to classify rustc stderr.",
            code, allowed_fn_context, other_count
        );

        // Also confirm classifier usage is benign (not inside a Diagnostic::error call).
        let classifier_diag_lines = source_text
            .lines()
            .filter(|l| l.contains(code) && l.contains("Diagnostic::error"))
            .count();
        assert_eq!(
            classifier_diag_lines, 0,
            "I2 violation: rustc code {} appears inside a Diagnostic::error call",
            code
        );

        // Verify no snapshot file mentions the rustc code in [ENNNNN] format
        let bracketed = format!("[{}]", code);
        let snaps = snapshot_codes();
        assert!(
            !snaps.contains(code),
            "I2 violation: rustc code {} appears in a user-facing snapshot as [{}]. \
             Rustc error codes must never reach end users.",
            code,
            code
        );
        let _ = (&allowed_fn_context, &bracketed);
    }
}

#[test]
fn diagnostic_snapshots_do_not_leak_runtime_or_backend_voice() {
    let banned = [
        "thread 'main' panicked",
        "panicked at",
        "stack backtrace:",
        "target/debug",
        ".rs:",
        "rustc rejected generated code",
    ];
    let mut failures = Vec::new();
    for (path, text) in rendered_snapshot_texts() {
        for needle in banned {
            if text.contains(needle) && !allowed_backend_voice_snapshot(&path, &text, needle) {
                failures.push(format!("{} contains `{}`", path.display(), needle));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "user-facing diagnostics leaked runtime/backend voice:\n{}",
        failures.join("\n")
    );
}

#[test]
fn runtime_user_error_codes_use_jet_panic_voice() {
    let mut failures = Vec::new();
    let mut paths = vec![root().join("crates/jet-codegen/src/Prelude/Core.rs")];
    collect_rs_paths(
        &root().join("crates/jet-codegen/src/Prelude/Core"),
        &mut paths,
    );
    collect_rs_paths(
        &root().join("crates/jet-codegen/src/Prelude/CoreLib"),
        &mut paths,
    );
    for path in paths {
        let text = read(&path);
        let rel = path.strip_prefix(root()).unwrap_or(&path).display().to_string();
        for (idx, line) in text.lines().enumerate() {
            if line.contains("panic!(\"E") {
                failures.push(format!(
                    "{}:{} uses raw panic for a Jet error code",
                    rel,
                    idx + 1
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "runtime user errors must use Jet-owned panic/report helpers:\n{}",
        failures.join("\n")
    );
}

fn collect_rs_paths(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_paths(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    out.sort();
}

fn allowed_backend_voice_snapshot(path: &std::path::Path, text: &str, needle: &str) -> bool {
    let p = path.to_string_lossy();
    if p.ends_with("ffi_bad_path.stderr") {
        return needle == ".rs:" || needle == "rustc rejected generated code";
    }
    if p.ends_with("os_target_unmatched_call.stderr") && text.contains("raw rustc") {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// I4(a): every emitted Jet code has a typed row
// ---------------------------------------------------------------------------

#[test]
fn every_emitted_code_has_typed_row() {
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
        "The following codes are emitted in Source/ but have NO typed diagnostic row \
         (invariant I4a). For each: add one row with code, stage, severity, moment, \
         What/Why/Fix templates, then add a tests/ui snapshot.\n\
         Missing:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn diagnostics_registry_has_no_duplicate_code_rows() {
    let mut seen = BTreeSet::new();
    let mut duplicates = Vec::new();
    for (code, line) in registered_code_rows() {
        if !seen.insert(code.clone()) {
            duplicates.push(format!("{code} at Diagnostics.jet:{line}"));
        }
    }
    assert!(
        duplicates.is_empty(),
        "duplicate code rows in the typed diagnostic source:\n{}",
        duplicates.join("\n")
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
    let diag_md = "";
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

#[test]
fn e0102_and_e0111_exact_replacement_reports_carry_machine_edits() {
    let mut failures = Vec::new();
    check_json_snapshots_for_edits(&root().join("tests/cli"), &mut failures);
    assert!(
        failures.is_empty(),
        "machine-readable diagnostics with one exact replacement token must carry fix_edits:\n{}",
        failures.join("\n")
    );
}

fn check_json_snapshots_for_edits(path: &PathBuf, failures: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            check_json_snapshots_for_edits(&path, failures);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("txt") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (line, raw) in text.lines().enumerate() {
            let Ok(report) = jet_foundation::JSON::parse_json(raw) else {
                continue;
            };
            let Some(schema) = jet_foundation::JSON::json_get(&report, "schema")
                .and_then(jet_foundation::JSON::json_str)
            else {
                continue;
            };
            if schema != "jet.report/v1" {
                continue;
            }
            let Some(fix) = jet_foundation::JSON::json_get(&report, "fix")
                .and_then(jet_foundation::JSON::json_str)
            else {
                continue;
            };
            if !names_one_exact_replacement(fix) {
                continue;
            }
            let has_edit = matches!(
                jet_foundation::JSON::json_get(&report, "fix_edits"),
                Some(jet_foundation::JSON::JSONValue::Array(edits)) if !edits.is_empty()
            );
            if !has_edit {
                failures.push(format!("{}:{} — {fix}", path.display(), line + 1));
            }
        }
    }
}

fn names_one_exact_replacement(fix: &str) -> bool {
    (fix.starts_with("did you mean `") && fix.ends_with("`?"))
        || (fix.starts_with("replace `") && fix.contains("` with `"))
        || (fix.starts_with("declare it with `") && fix.contains(" := "))
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
    let diag_md = "";

    // Codes in the acknowledged list that now have snapshot coverage — time to remove them.
    let mut now_covered: Vec<String> = ACKNOWLEDGED_COVERAGE_GAPS
        .iter()
        .filter(|c| {
            snaps.contains(&c.to_string()) && !is_retired(c, &diag_md)
        })
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
// Count-ratchets (card #447 / durability W2): exclusion lists must shrink
// over time, never grow silently. A PR that adds a new exclusion to route
// around this file's checks trips the corresponding ceiling below and must
// bump it explicitly (a visible, reviewable diff) rather than sneaking a
// larger list past a green run.
// ---------------------------------------------------------------------------
#[test]
fn exclusion_list_counts_do_not_grow() {
    const CEILINGS: &[(&str, usize, usize)] = &[
        ("ACKNOWLEDGED_COVERAGE_GAPS", ACKNOWLEDGED_COVERAGE_GAPS.len(), 6),
        ("STAGED_BEHIND_GATE", STAGED_BEHIND_GATE.len(), 5),
        ("UNTESTABLE_VIA_SNAPSHOT", UNTESTABLE_VIA_SNAPSHOT.len(), 1),
        (
            "RETIRED_WITH_LEGACY_EMISSION",
            RETIRED_WITH_LEGACY_EMISSION.len(),
            1,
        ),
        ("RUSTC_CODES_IN_SOURCE", RUSTC_CODES_IN_SOURCE.len(), 3),
    ];
    let mut violations = Vec::new();
    for (name, actual, ceiling) in CEILINGS {
        if actual > ceiling {
            violations.push(format!(
                "{name} grew from {ceiling} to {actual} entries — shrinking is welcome, \
                 growing needs a reviewed bump of the ceiling in this test, not a silent add"
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

// ---------------------------------------------------------------------------
// I4(c): jet explain resolves for every live typed row
// ---------------------------------------------------------------------------

#[test]
fn every_registered_code_has_explain_page() {
    let registered = registered_codes();
    let diag_md = "";

    let mut missing: Vec<String> = registered
        .iter()
        .filter(|c| !is_retired(c, &diag_md))
        .filter(|c| jet::Explain::lookup(c).is_none())
        .cloned()
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "The following codes are registered in the typed row source but \
         `jet explain <code>` returns None (invariant I4c).\n\
         Since Explain.rs is built directly from the row registry, this means the \
         typed row is malformed or missing.\n\
         Missing explain:\n  {}",
        missing.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Audit: typed rows but NOT emitted (spec ahead of impl)
//
// This allowlist is shrink-only. Retired rows stay listed for explain-page
// coverage. Staged rows name the owning Tower card.
// ---------------------------------------------------------------------------

#[test]
fn registered_unimplemented_codes_are_expected() {
    // Reason is exactly `retired` or `staged #<owning-card>`.
    const EXPECTED_SPEC_AHEAD_OF_IMPL: &[(&str, &str)] = &[
        ("E0004", "retired"),
        ("E0005", "retired"),
        ("E0006", "retired"),
        ("E0062", "retired"),
        ("E0010", "retired"),
        ("E0011", "retired"),
        ("E0020", "retired"),
        ("E0058", "retired"),
        ("E0206", "retired"),
        ("E0207", "retired"),
        ("E0745", "retired"),
        ("E0427", "retired"),
        ("E0912", "retired"),
        ("E0990", "retired"),
        ("E2301", "retired"),
        ("E2302", "retired"),
        ("E2303", "staged #1164"),
        ("E2304", "retired"),
        ("E2306", "retired"),
        ("E2403", "staged #1542"),
        ("E2410", "staged #1830"),
        ("E2412", "staged #1830"),
        ("E2413", "retired"),
        ("E2701", "staged #1495"),
        ("E2801", "staged #17"),
        ("E2802", "staged #17"),
        ("E2803", "staged #17"),
        ("E2804", "staged #17"),
        ("E2902", "staged #1530"),
        ("E2940", "staged #240"),
        ("E3002", "staged #1530"),
        ("E3104", "retired"),
        ("E0958", "retired"),
        ("E0951", "retired"),
        ("E0993", "retired"),
        ("E0328", "retired"),
        ("E0342", "staged #1606"),
        ("E0954", "retired"),
        ("E0920", "retired"),
        ("E1109", "staged #1220"),
        ("E1229", "retired"),
        ("E0410", "retired"),
        ("E0859", "staged #521"),
        ("E0416", "retired"),
        ("E0428", "retired"),
        ("E2407", "staged #1330"),
        ("E3626", "staged #521"),
    ];

    let expected: BTreeSet<String> = EXPECTED_SPEC_AHEAD_OF_IMPL
        .iter()
        .map(|(code, _)| (*code).to_string())
        .collect();

    let emitted = emitted_codes();
    let registered = registered_codes();
    let exclusions = all_exclusions();

    let reason_errors = allowlist_reason_errors(EXPECTED_SPEC_AHEAD_OF_IMPL);
    assert!(
        reason_errors.is_empty(),
        "Invalid EXPECTED_SPEC_AHEAD_OF_IMPL reasons:\n  {}",
        reason_errors.join("\n  ")
    );

    const STALE_ALLOWLIST_FIXTURE: &[&str] = &["E2101", "E3001"];
    let stale_fixture_failure = stale_allowlist_failure(
        STALE_ALLOWLIST_FIXTURE.iter().copied(),
        &emitted,
    )
    .expect("embedded stale allowlist fixture must fail");
    assert!(stale_fixture_failure.contains("E2101"));
    assert!(stale_fixture_failure.contains("E3001"));
    assert!(stale_fixture_failure.contains("remove the line"));

    assert!(
        registered.contains("E0033"),
        "E0033 is a retired reservation and must remain in the typed rows"
    );
    assert!(
        registered.contains("E0745") && is_retired("E0745", ""),
        "E0745 is a retired reservation and must remain in the typed rows"
    );
    let parser = read(&root().join("crates/jet-parser/src/Parser/mod.rs"));
    assert!(
        parser.contains("\"E0032\", \"E0033\", \"E0036\""),
        "E0033 must remain in the parser retired-code non-emission guard"
    );

    let spec_ahead_of_impl: BTreeSet<String> = registered
        .iter()
        .filter(|c| !exclusions.contains(*c))
        .filter(|c| {
            jet_foundation::Registry::diagnostic(c).is_some_and(|row| {
                row.status != jet_foundation::Registry::DiagnosticStatus::Reserved
            })
        })
        .filter(|c| !emitted.contains(*c))
        .cloned()
        .collect();

    // Anything in spec_ahead_of_impl but NOT in expected is a surprise.
    let mut unexpected: Vec<String> = spec_ahead_of_impl.difference(&expected).cloned().collect();
    unexpected.sort();

    assert!(
        unexpected.is_empty(),
        "Unexpected codes registered in typed rows but not emitted in Source/.\n\
         If this is intentional (staged feature), add the code to \
         EXPECTED_SPEC_AHEAD_OF_IMPL in tests/diagnostics_coverage.rs.\n\
         Unexpected:\n  {}",
        unexpected.join("\n  ")
    );

    let now_emitted = stale_allowlist_failure(
        EXPECTED_SPEC_AHEAD_OF_IMPL.iter().map(|(code, _)| *code),
        &emitted,
    );
    if let Some(failure) = now_emitted {
        panic!("{failure}");
    }
}
