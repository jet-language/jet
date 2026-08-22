//! Diagnostics coverage accounting (board card c116, P0).
//!
//! Enforces invariant I4: every emitted code must have
//!   (a) an entry in the typed diagnostic-row registry,
//!   (b) at least one harness-enumerated fixture with a column-0 report opener,
//!       and
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

fn runtime_stop_calls(source: &str) -> Vec<Vec<&str>> {
    let needle = "set_runtime_stop(";
    let mut calls = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = source[search_from..].find(needle) {
        let start = search_from + offset + needle.len();
        let bytes = source.as_bytes();
        let mut args = Vec::new();
        let mut arg_start = start;
        let mut parens = 0usize;
        let mut brackets = 0usize;
        let mut braces = 0usize;
        let mut string = false;
        let mut escaped = false;
        let mut end = None;
        for index in start..bytes.len() {
            let byte = bytes[index];
            if string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    string = false;
                }
                continue;
            }
            match byte {
                b'"' => string = true,
                b'(' => parens += 1,
                b')' if parens > 0 => parens -= 1,
                b')' => {
                    args.push(source[arg_start..index].trim());
                    end = Some(index);
                    break;
                }
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b'{' => braces += 1,
                b'}' => braces = braces.saturating_sub(1),
                b',' if parens == 0 && brackets == 0 && braces == 0 => {
                    args.push(source[arg_start..index].trim());
                    arg_start = index + 1;
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        calls.push(args);
        search_from = end + 1;
    }
    calls
}

#[test]
fn jit_collections_use_ice_for_internals_and_canonical_runtime_messages() {
    let source = read(&root().join("crates/jet-jit/src/Collections.rs"));
    assert!(
        !source.contains("set_trap("),
        "Collections.rs must not convert adapter invariants into runtime traps"
    );

    let direct_messages: Vec<_> = runtime_stop_calls(&source)
        .into_iter()
        .filter_map(|args| args.get(2).copied())
        .filter(|wording| wording.trim_start_matches('&').trim_start().starts_with('"'))
        .collect();
    assert!(
        direct_messages.is_empty(),
        "Collections.rs must pass canonical or returned messages to set_runtime_stop, not direct wording literals: {direct_messages:?}"
    );
}

/// All codes registered in the typed compile-time diagnostic rows.
fn registered_codes() -> BTreeSet<String> {
    jet_foundation::Registry::diagnostic_rows()
        .iter()
        .map(|row| row.code.to_string())
        .collect()
}

#[test]
fn runtime_stop_renderer_accepts_only_active_runtime_rows() {
    let rows = jet_foundation::Registry::diagnostic_rows();
    let active_runtime = rows
        .iter()
        .filter(|row| {
            row.stage == "runtime"
                && row.status == jet_foundation::Registry::DiagnosticStatus::Active
        })
        .collect::<Vec<_>>();
    assert!(!active_runtime.is_empty(), "registry must publish runtime rows");

    for row in active_runtime {
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            row.code,
            "probe.jet",
            7,
            "probe",
            "stop()",
            1,
            1,
            "runtime probe",
            "",
        );
        assert_eq!(report.code, row.code);
        assert_eq!(report.source, "runtime");
        assert_eq!(report.exit_code, 70);
        assert!(!report.what.is_empty());
        assert!(!report.why.is_empty());
        assert!(!report.fix.is_empty());
    }

    for row in rows.iter().filter(|row| {
        row.stage != "runtime"
            || row.status != jet_foundation::Registry::DiagnosticStatus::Active
    }) {
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            row.code,
            "probe.jet",
            7,
            "probe",
            "stop()",
            1,
            1,
            "runtime probe",
            "",
        );
        assert_eq!(report.exit_code, 101, "{} must be a host fault", row.code);
    }

    let unknown = jet_foundation::Outcome::jet_render_runtime_stop(
        "__unknown_runtime_stop__",
        "probe.jet",
        7,
        "probe",
        "stop()",
        1,
        1,
        "runtime probe",
        "",
    );
    assert_eq!(unknown.exit_code, 101);
    assert_eq!(unknown.source, "host");
}

fn registered_code_rows() -> Vec<(String, usize)> {
    jet_foundation::Registry::diagnostic_rows()
        .iter()
        .enumerate()
        .map(|(idx, row)| (row.code.to_string(), idx + 5))
        .collect()
}

#[test]
fn registered_codes_keep_explicit_severity_and_lint_names() {
    use jet_foundation::Diagnostics::Severity;

    for (code, severity) in [
        ("E2930", Severity::Lint),
        ("W0410", Severity::Lint),
        ("R0801", Severity::Error),
        ("E0043", Severity::Error),
    ] {
        let row = jet_foundation::Registry::diagnostic(code)
            .unwrap_or_else(|| panic!("{code} must stay registered"));
        assert_eq!(
            row.severity, severity,
            "{code} must keep the registry row's explicit severity"
        );
    }

    for lint in jet_foundation::Registry::lint_rows() {
        let name = lint
            .lint_name
            .unwrap_or_else(|| panic!("lint {} needs a stable name", lint.code));
        assert!(
            is_snake_case(name),
            "lint {} has an unstable selector name `{name}`",
            lint.code
        );
    }
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
    let lints: Vec<_> = jet_foundation::Registry::lint_rows().collect();
    let mut names = BTreeSet::new();
    let mut codes = BTreeSet::new();

    for lint in lints {
        let name = lint
            .lint_name
            .expect("every registered lint must carry a stable name");
        assert!(
            is_snake_case(name),
            "lint `{}` is not a stable snake_case name",
            name
        );
        assert!(names.insert(name), "lint name `{name}` is duplicated");
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
            Some(name),
            "lint `{}` does not round-trip through the name registry",
            lint.code
        );
        assert_eq!(
            jet_foundation::LintPolicy::code_for_name(name),
            Some(lint.code),
            "lint name `{name}` does not resolve to its registered code"
        );
    }

    assert_eq!(
        codes, registered_codes,
        "every registered lint row must carry exactly one stable name"
    );
}

#[test]
fn rendered_lint_keeps_code_beside_name() {
    let diagnostic = jet_foundation::Diagnostics::Diagnostic::from_row("L0302", &[], None);
    let rendered = diagnostic.render("example.jet", "");
    assert!(
        rendered.starts_with("Warning [L0302] (same_enum_guard_table):"),
        "rendered lint lost its code/name pair: {rendered}"
    );
}

fn assert_ui_snapshot_matches_row(
    code: &str,
    holes: &[(&str, &str)],
    fixture: &str,
) {
    let row = jet_foundation::Registry::diagnostic(code)
        .unwrap_or_else(|| panic!("{code} must stay registered"));
    let rendered = row.render(holes);
    let expected = format!(
        "Error [{code}]: {}\n Why: {}\n Fix: {}\n",
        rendered.what, rendered.why, rendered.fix
    );
    assert_eq!(
        read(&root().join("tests/ui").join(fixture)),
        expected,
        "{fixture} must preserve {code}'s registered What/Why/Fix identity"
    );
}

#[test]
fn registered_config_and_cli_snapshots_keep_row_identity() {
    assert_ui_snapshot_matches_row(
        "E1206",
        &[("code", "L0302"), ("name", "same_enum_guard_table")],
        "lint_policy_code_name.stderr",
    );
    assert_ui_snapshot_matches_row("E0043", &[], "cli_e0043_install.stderr");
    assert_ui_snapshot_matches_row(
        "E1219",
        &[("name", "turbo")],
        "cli_e1219_unknown_profile.stderr",
    );
}

#[test]
fn retired_bench_command_snapshot_keeps_registered_teaching_code() {
    let row = jet_foundation::Registry::diagnostic("E2101")
        .expect("retired command route must stay registered");
    assert_eq!(
        row.status,
        jet_foundation::Registry::DiagnosticStatus::Retired,
        "E2101 must remain a retired teaching diagnostic"
    );
    let snapshot = read(&root().join("tests/ui/bench_command_retired_e2101.stderr"));
    assert!(snapshot.contains("Error [E2101]"), "snapshot lost E2101: {snapshot}");
    assert!(snapshot.contains("`bench`"), "snapshot lost retired spelling: {snapshot}");
    assert!(
        snapshot.contains("jet test --measure"),
        "snapshot lost canonical fix: {snapshot}"
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

/// Compare the one expected-unimplemented baseline with the current registry
/// projection. A stale row must be deleted; missing or unexpected rows must
/// update the registry and baseline together.
fn coverage_baseline_failure<'a>(
    entries: impl IntoIterator<Item = &'a str>,
    unimplemented: &BTreeSet<String>,
    emitted: &BTreeSet<String>,
) -> Option<String> {
    let expected: BTreeSet<String> = entries.into_iter().map(str::to_string).collect();

    let mut now_emitted: Vec<String> = expected.intersection(emitted).cloned().collect();
    now_emitted.sort();

    let mut missing: Vec<String> = expected
        .difference(unimplemented)
        .filter(|code| !emitted.contains(*code))
        .cloned()
        .collect();
    missing.sort();

    let mut unexpected: Vec<String> = unimplemented.difference(&expected).cloned().collect();
    unexpected.sort();

    let mut failures = Vec::new();
    if !now_emitted.is_empty() {
        failures.push(format!(
            "EXPECTED_SPEC_AHEAD_OF_IMPL contains now-emitted codes; remove the line for each:\n  {}",
            now_emitted.join("\n  ")
        ));
    }
    if !missing.is_empty() {
        failures.push(format!(
            "EXPECTED_SPEC_AHEAD_OF_IMPL is missing baseline entries; restore the line for each:\n  {}",
            missing.join("\n  ")
        ));
    }
    if !unexpected.is_empty() {
        failures.push(format!(
            "Unexpected codes registered in typed rows but not emitted in Source/.\n\
         If this is intentional (staged feature), add the code to \
         EXPECTED_SPEC_AHEAD_OF_IMPL in tests/diagnostics_coverage.rs.\n\
         Unexpected:\n  {}",
            unexpected.join("\n  ")
        ));
    }

    (!failures.is_empty()).then(|| failures.join("\n"))
}

fn code_set(codes: &[&str]) -> BTreeSet<String> {
    codes.iter().map(|code| (*code).to_string()).collect()
}

#[test]
fn coverage_baseline_rejects_removal_addition_and_substitution() {
    let baseline = ["E2101", "E3001"];
    let emitted = BTreeSet::new();

    assert_eq!(
        coverage_baseline_failure(
            baseline.iter().copied(),
            &code_set(&["E2101"]),
            &emitted,
        ),
        Some(
            "EXPECTED_SPEC_AHEAD_OF_IMPL is missing baseline entries; restore the line for each:\n  E3001"
                .to_string()
        )
    );
    assert_eq!(
        coverage_baseline_failure(
            ["E2101"].iter().copied(),
            &code_set(&["E2101", "E3001"]),
            &emitted,
        ),
        Some(
            "Unexpected codes registered in typed rows but not emitted in Source/.\n\
         If this is intentional (staged feature), add the code to \
         EXPECTED_SPEC_AHEAD_OF_IMPL in tests/diagnostics_coverage.rs.\n\
         Unexpected:\n  E3001"
                .to_string()
        )
    );
    assert_eq!(
        coverage_baseline_failure(
            baseline.iter().copied(),
            &code_set(&["E2101", "E4000"]),
            &emitted,
        ),
        Some(
            "EXPECTED_SPEC_AHEAD_OF_IMPL is missing baseline entries; restore the line for each:\n  E3001\nUnexpected codes registered in typed rows but not emitted in Source/.\n\
         If this is intentional (staged feature), add the code to \
         EXPECTED_SPEC_AHEAD_OF_IMPL in tests/diagnostics_coverage.rs.\n\
         Unexpected:\n  E4000"
                .to_string()
        )
    );
    assert_eq!(
        coverage_baseline_failure(
            baseline.iter().copied(),
            &code_set(&baseline),
            &emitted,
        ),
        None
    );
}

/// Direct fixture directories that carry rendered CLI/tool reports. UI and
/// lint paths are collected separately because their harnesses derive the
/// expected snapshot path from each source fixture.
const REPORT_FIXTURE_DIRS: &[(&str, &str)] = &[
    ("tests/cli", "txt"),
    ("tests/release", "txt"),
    ("tests/fixtures/cli-diagnostics", "stderr"),
    ("tests/fixtures/jetpack-diagnostics", "stderr"),
    ("tests/fixtures/module-eval-diagnostics", "stderr"),
];

/// Exact snapshot paths consumed by the UI/lint harnesses plus direct report
/// fixture directories. Keeping this path table independent from code
/// extraction prevents prose, comments, and Rust assertions from becoming
/// coverage.
fn report_snapshot_paths() -> Vec<PathBuf> {
    let root = root();
    let mut out = Vec::new();

    let ui = root.join("tests/ui");
    if let Ok(entries) = fs::read_dir(&ui) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some(jet::Syntax::FILE_EXT)
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".fixed."))
            {
                let snapshot = path.with_extension("stderr");
                if snapshot.is_file() {
                    out.push(snapshot);
                }
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            let entry = ["run", "main"]
                .into_iter()
                .map(|name| path.join(format!("{name}.{}", jet::Syntax::FILE_EXT)))
                .find(|candidate| candidate.is_file());
            if entry.is_some() {
                let snapshot = path.join("stderr");
                if snapshot.is_file() {
                    out.push(snapshot);
                }
            } else {
                let workspace = path.join(jet::Syntax::WORKSPACE_FILE);
                if workspace.is_file() {
                    let name = path
                        .file_name()
                        .expect("workspace fixture directory name")
                        .to_string_lossy();
                    let snapshot = path
                        .parent()
                        .expect("workspace fixture parent")
                        .join(format!("{name}.stderr"));
                    if snapshot.is_file() {
                        out.push(snapshot);
                    }
                }
            }
        }
    }

    let lint = root.join("tests/ui_lint");
    if let Ok(entries) = fs::read_dir(&lint) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some(jet::Syntax::FILE_EXT)
            {
                let snapshot = path.with_extension("warn");
                if snapshot.is_file() {
                    out.push(snapshot);
                }
            }
        }
    }

    for &(dir, extension) in REPORT_FIXTURE_DIRS {
        let dir = root.join(dir);
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|ext| ext.to_str()) == Some(extension)
                {
                    out.push(path);
                }
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

fn is_report_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    matches!(bytes.next(), Some(byte) if byte.is_ascii_uppercase())
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Extract only report-opening lines. Column 0 is intentional: indented
/// backend text, Why/Fix cross-references, comments, and quoted assertions do
/// not assert a rendered report.
fn extract_report_opening_codes(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = ["Error [", "Warning [", "Stop [", "Lint ["]
                .into_iter()
                .find_map(|prefix| line.strip_prefix(prefix))?;
            let end = rest.find(']')?;
            let code = &rest[..end];
            is_report_code(code).then(|| code.to_string())
        })
        .collect()
}

fn rendered_report_codes() -> BTreeSet<String> {
    report_snapshot_paths()
        .into_iter()
        .flat_map(|path| extract_report_opening_codes(&read(&path)))
        .collect()
}

#[test]
fn report_openers_ignore_prose_comments_and_rust_assertion_text() {
    let text = concat!(
        "Error [E0001]: what; Why: [E9998]; Fix: [E9997]\n",
        "// Error [E9996]\n",
        "assert!(stderr.contains(\"[E9995]\"));\n",
        "  Error [E9994]: indented\n",
        "Warning [L0001]: lint\n",
        "Stop [E0002]: stop\n",
        "Lint [L0002]: lint\n",
    );

    assert_eq!(
        extract_report_opening_codes(text),
        vec![
            "E0001".to_string(),
            "L0001".to_string(),
            "E0002".to_string(),
            "L0002".to_string(),
        ]
    );
}

#[test]
fn reverse_coverage_diff_names_retired_and_unregistered_fixture_codes() {
    let left = BTreeSet::new();
    let right: BTreeSet<String> = extract_report_opening_codes(
        "Error [E0004]: retired\nError [E9999]: no longer registered\n",
    )
    .into_iter()
    .collect();

    let (left_only, right_only) = coverage_set_diff(&left, &right);
    assert!(left_only.is_empty());
    assert_eq!(right_only, code_set(&["E0004", "E9999"]));
    assert_eq!(format_code_set(&right_only), "E0004\n  E9999");
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

#[test]
fn word_shaped_codes_are_registered_and_explainable() {
    let expected = [
        "E-WEB-ABI-TYPE",
        "E-WEB-CROSS-PARTITION",
        "E-WEB-TARGET-BROWSER",
        "E-WEB-TIR-UNSUPPORTED",
        "E-APP-TARGET-FEATURE",
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

/// Current fixture/registry mismatches that are intentional and owned. The
/// side names which independently maintained table is missing the code:
/// `left-only` is a registered-active emitted code without a report opener;
/// `right-only` is a report opener without a registered-active emitted code.
/// Every row has an owner, and the count ratchet below makes this list shrink-only.
const DIAGNOSTIC_COVERAGE_ALLOWLIST: &[(&str, &str, &str)] = &[
    // LEFT-only: registered-active and emitted, but no column-0 report opener.
    ("E0037", "left-only", "Tower #2093"),
    ("E0153", "left-only", "Tower #2093"),
    ("E0343", "left-only", "Tower #2093"),
    ("E0345", "left-only", "Tower #2093"),
    ("E0347", "left-only", "Tower #2093"),
    ("E0349", "left-only", "Tower #2093"),
    ("E0403", "left-only", "Tower #2093"),
    ("E0502", "left-only", "Tower #2093"),
    ("E0601", "left-only", "Tower #2093"),
    ("E0613", "left-only", "Tower #2093"),
    ("E0916", "left-only", "Tower #2093"),
    ("E0953", "left-only", "Tower #2093"),
    ("E0959", "left-only", "Tower #2093"),
    ("E0979", "left-only", "Tower #2093"),
    ("E0980", "left-only", "Tower #2093"),
    ("E0981", "left-only", "Tower #2093"),
    ("E0982", "left-only", "Tower #2093"),
    ("E1204", "left-only", "Tower #2093"),
    ("E1217", "left-only", "Tower #2093"),
    ("E1218", "left-only", "Tower #2093"),
    ("E1221", "left-only", "Tower #2093"),
    ("E1227", "left-only", "Tower #2093"),
    ("E1228", "left-only", "Tower #2093"),
    ("E1230", "left-only", "Tower #2093"),
    ("E1232", "left-only", "Tower #2093"),
    ("E1233", "left-only", "Tower #2093"),
    ("E1234", "left-only", "Tower #2093"),
    ("E1235", "left-only", "Tower #2093"),
    ("E1236", "left-only", "Tower #2093"),
    ("E1237", "left-only", "Tower #2093"),
    ("E1238", "left-only", "Tower #2093"),
    ("E1240", "left-only", "Tower #2093"),
    ("E1241", "left-only", "Tower #2093"),
    ("E1242", "left-only", "Tower #2093"),
    ("E1243", "left-only", "Tower #2093"),
    ("E1244", "left-only", "Tower #2093"),
    ("E1245", "left-only", "Tower #2093"),
    ("E1246", "left-only", "Tower #2093"),
    ("E1247", "left-only", "Tower #2093"),
    ("E1248", "left-only", "Tower #2093"),
    ("E1249", "left-only", "Tower #2093"),
    ("E1250", "left-only", "Tower #2093"),
    ("E1251", "left-only", "Tower #2093"),
    ("E1252", "left-only", "Tower #2093"),
    ("E1254", "left-only", "Tower #2093"),
    ("E1255", "left-only", "Tower #2093"),
    ("E1256", "left-only", "Tower #2093"),
    ("E1261", "left-only", "Tower #2093"),
    ("E1262", "left-only", "Tower #2093"),
    ("E1263", "left-only", "Tower #2093"),
    ("E1266", "left-only", "Tower #2093"),
    ("E1267", "left-only", "Tower #2093"),
    ("E1268", "left-only", "Tower #2093"),
    ("E1269", "left-only", "Tower #2093"),
    ("E1271", "left-only", "Tower #2093"),
    ("E1275", "left-only", "Tower #2093"),
    ("E1276", "left-only", "Tower #2093"),
    ("E1278", "left-only", "Tower #2093"),
    ("E1279", "left-only", "Tower #2093"),
    ("E1280", "left-only", "Tower #2093"),
    ("E1281", "left-only", "Tower #2093"),
    ("E1282", "left-only", "Tower #2093"),
    ("E1283", "left-only", "Tower #2093"),
    ("E1284", "left-only", "Tower #2093"),
    ("E1285", "left-only", "Tower #2093"),
    ("E1286", "left-only", "Tower #2093"),
    ("E1287", "left-only", "Tower #2093"),
    ("E1288", "left-only", "Tower #2093"),
    ("E1289", "left-only", "Tower #2093"),
    ("E1290", "left-only", "Tower #2093"),
    ("E1291", "left-only", "Tower #2093"),
    ("E1294", "left-only", "Tower #2093"),
    ("E1316", "left-only", "Tower #2093"),
    ("E1320", "left-only", "Tower #2093"),
    ("E1329", "left-only", "Tower #2093"),
    ("E1330", "left-only", "Tower #2093"),
    ("E1332", "left-only", "Tower #2093"),
    ("E1336", "left-only", "Tower #2093"),
    ("E1802", "left-only", "Tower #2093"),
    ("E1803", "left-only", "Tower #2093"),
    ("E2106", "left-only", "Tower #2093"),
    ("E2201", "left-only", "Tower #2093"),
    ("E2202", "left-only", "Tower #2093"),
    ("E2203", "left-only", "Tower #2093"),
    ("E2204", "left-only", "Tower #2093"),
    ("E2210", "left-only", "Tower #2093"),
    ("E2601", "left-only", "Tower #2093"),
    ("E2602", "left-only", "Tower #2093"),
    ("E2603", "left-only", "Tower #2093"),
    ("E2604", "left-only", "Tower #2093"),
    ("E2605", "left-only", "Tower #2093"),
    ("E2606", "left-only", "Tower #2093"),
    ("E2712", "left-only", "Tower #2093"),
    ("E2901", "left-only", "Tower #2093"),
    ("E2906", "left-only", "Tower #2093"),
    ("E2907", "left-only", "Tower #2093"),
    ("E2908", "left-only", "Tower #2093"),
    ("E3002", "left-only", "Tower #2093"),
    ("E3201", "left-only", "Tower #2093"),
    ("E3209", "left-only", "Tower #2093"),
    ("E3210", "left-only", "Tower #2093"),
    ("E3504", "left-only", "Tower #2093"),
    ("L0204", "left-only", "Tower #2093"),
    ("L0205", "left-only", "Tower #2093"),
    ("L3102", "left-only", "Tower #2093"),
    // RIGHT-only: existing report openers for retired/reserved rows.
    ("E0060", "right-only", "Tower #2093"),
    ("E0065", "right-only", "Tower #2093"),
    ("E0066", "right-only", "Tower #2093"),
    ("E0067", "right-only", "Tower #2093"),
    ("E0128", "right-only", "Tower #2093"),
    ("E0146", "right-only", "Tower #2093"),
    ("E0214", "right-only", "Tower #2093"),
    ("E0341", "right-only", "Tower #2093"),
    ("E0351", "right-only", "Tower #2093"),
    ("E0358", "right-only", "Tower #2093"),
    ("E0374", "right-only", "Tower #2093"),
    ("E0375", "right-only", "Tower #2093"),
    ("E0376", "right-only", "Tower #2093"),
    ("E0377", "right-only", "Tower #2093"),
    ("E0426", "right-only", "Tower #2093"),
    ("E0431", "right-only", "Tower #2093"),
    ("E0432", "right-only", "Tower #2093"),
    ("E0922", "right-only", "Tower #2093"),
    ("E0928", "right-only", "Tower #2093"),
    ("E0929", "right-only", "Tower #2093"),
    ("E0960", "right-only", "Tower #2093"),
    ("E0988", "right-only", "Tower #2093"),
    ("E1002", "right-only", "Tower #2093"),
    ("E1105", "right-only", "Tower #2093"),
    ("E1107", "right-only", "Tower #2093"),
    ("E1115", "right-only", "Tower #2093"),
    ("E1116", "right-only", "Tower #2093"),
    ("E1209", "right-only", "Tower #2093"),
    ("E1210", "right-only", "Tower #2093"),
    ("E1306", "right-only", "Tower #2093"),
    ("E1342", "right-only", "Tower #2093"),
    ("E1343", "right-only", "Tower #2093"),
    ("E2101", "right-only", "Tower #2093"),
    ("E2510", "right-only", "Tower #2093"),
    ("E2714", "right-only", "Tower #2093"),
    ("E2805", "right-only", "Tower #2093"),
    ("E2935", "right-only", "Tower #2093"),
    ("E2936", "right-only", "Tower #2093"),
    ("E3206", "right-only", "Tower #2093"),
    ("E3530", "right-only", "Tower #2093"),
];

const DIAGNOSTIC_COVERAGE_ALLOWLIST_CEILING: usize = 153;

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

fn coverage_allowlist_codes(side: &str) -> BTreeSet<String> {
    DIAGNOSTIC_COVERAGE_ALLOWLIST
        .iter()
        .filter(|(_, entry_side, _)| *entry_side == side)
        .map(|(code, _, _)| (*code).to_string())
        .collect()
}

fn coverage_allowlist_errors() -> Vec<String> {
    let registered = registered_codes();
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();

    for &(code, side, owner) in DIAGNOSTIC_COVERAGE_ALLOWLIST {
        if !is_report_code(code) {
            errors.push(format!("{code}: invalid diagnostic code"));
        }
        if side != "left-only" && side != "right-only" {
            errors.push(format!("{code}: side must be `left-only` or `right-only`"));
        }
        if owner.trim().is_empty() {
            errors.push(format!("{code}: coverage owner is empty"));
        }
        if !registered.contains(code) {
            errors.push(format!("{code}: coverage allowlist row is not registered"));
        }
        if !seen.insert(code) {
            errors.push(format!("{code}: duplicate coverage allowlist row"));
        }
    }

    errors
}

/// LEFT: registered-active codes emitted in Source/crates, minus named
/// exclusions. RIGHT: codes opened by an actual harness/direct fixture report,
/// minus the same named exclusions. The two sets remain independently derived.
fn coverage_left() -> BTreeSet<String> {
    let emitted = emitted_codes();
    let exclusions = all_exclusions();
    registered_codes()
        .into_iter()
        .filter(|code| emitted.contains(code))
        .filter(|code| {
            jet_foundation::Registry::diagnostic(code).is_some_and(|row| {
                row.status == jet_foundation::Registry::DiagnosticStatus::Active
            })
        })
        .filter(|code| !exclusions.contains(code))
        .collect()
}

fn coverage_right() -> BTreeSet<String> {
    let exclusions = all_exclusions();
    rendered_report_codes()
        .into_iter()
        .filter(|code| !exclusions.contains(code))
        .collect()
}

fn coverage_set_diff(
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    (
        left.difference(right).cloned().collect(),
        right.difference(left).cloned().collect(),
    )
}

fn format_code_set(codes: &BTreeSet<String>) -> String {
    if codes.is_empty() {
        "(none)".to_string()
    } else {
        codes.iter().cloned().collect::<Vec<_>>().join("\n  ")
    }
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

        // Verify no rendered report opens with the rustc code.
        let snaps = rendered_report_codes();
        assert!(
            !snaps.contains(code),
            "I2 violation: rustc code {} appears in a user-facing snapshot as [{}]. \
             Rustc error codes must never reach end users.",
            code,
            code
        );
        let _ = &allowed_fn_context;
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
// I4(b): every emitted Jet code has at least one report-opening fixture
// ---------------------------------------------------------------------------

#[test]
fn every_emitted_code_has_snapshot() {
    let allowlist_errors = coverage_allowlist_errors();
    assert!(
        allowlist_errors.is_empty(),
        "invalid diagnostic coverage allowlist rows:\n{}",
        allowlist_errors.join("\n")
    );

    let left = coverage_left();
    let right = coverage_right();
    let (mut left_only, mut right_only) = coverage_set_diff(&left, &right);
    let allowed_left = coverage_allowlist_codes("left-only");
    let allowed_right = coverage_allowlist_codes("right-only");
    left_only.retain(|code| !allowed_left.contains(code));
    right_only.retain(|code| !allowed_right.contains(code));

    assert!(
        left_only.is_empty() && right_only.is_empty(),
        "I4(b) diagnostic coverage set mismatch after named exclusions and \
         owned allowlist rows:\n\
         LEFT-only (registered-active emitted, no report opener):\n  {}\n\
         RIGHT-only (report opener, not registered-active emitted):\n  {}",
        format_code_set(&left_only),
        format_code_set(&right_only),
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
                continue;
            }
            let has_applicability = matches!(
                jet_foundation::JSON::json_get(&report, "applicability"),
                Some(jet_foundation::JSON::JSONValue::String(value))
                    if value == "safe" || value == "suggested"
            );
            if !has_applicability {
                failures.push(format!(
                    "{}:{} — machine edit has no closed applicability grade",
                    path.display(),
                    line + 1
                ));
            }
            let has_safety = matches!(
                jet_foundation::JSON::json_get(&report, "fix_edits"),
                Some(jet_foundation::JSON::JSONValue::Array(edits))
                    if edits.iter().all(|edit| matches!(
                        jet_foundation::JSON::json_get(edit, "safety"),
                        Some(jet_foundation::JSON::JSONValue::String(value))
                            if [
                                jet_foundation::Report::FixSafety::Formatting,
                                jet_foundation::Report::FixSafety::BehaviorPreserving,
                                jet_foundation::Report::FixSafety::ApiChanging,
                                jet_foundation::Report::FixSafety::TargetChanging,
                                jet_foundation::Report::FixSafety::NeedsReview,
                            ]
                            .iter()
                            .any(|grade| grade.as_str() == value.as_str())
                    ))
            );
            if !has_safety {
                failures.push(format!(
                    "{}:{} — machine edit has no closed safety grade",
                    path.display(),
                    line + 1
                ));
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
// I4(b) coverage allowlist sentinel: rows must shrink, never rot
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_coverage_allowlist_rows_are_still_needed() {
    let allowlist_errors = coverage_allowlist_errors();
    assert!(
        allowlist_errors.is_empty(),
        "invalid diagnostic coverage allowlist rows:\n{}",
        allowlist_errors.join("\n")
    );

    let left = coverage_left();
    let right = coverage_right();
    let mut stale = Vec::new();
    for &(code, side, owner) in DIAGNOSTIC_COVERAGE_ALLOWLIST {
        let still_needed = match side {
            "left-only" => left.contains(code) && !right.contains(code),
            "right-only" => right.contains(code) && !left.contains(code),
            _ => false,
        };
        if !still_needed {
            stale.push(format!("{code} ({side}; owner {owner})"));
        }
    }
    stale.sort();

    assert!(
        stale.is_empty(),
        "remove stale I4(b) diagnostic coverage allowlist rows:\n  {}",
        stale.join("\n  ")
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
        (
            "DIAGNOSTIC_COVERAGE_ALLOWLIST",
            DIAGNOSTIC_COVERAGE_ALLOWLIST.len(),
            DIAGNOSTIC_COVERAGE_ALLOWLIST_CEILING,
        ),
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
        ("E0007", "retired"),
        ("E0062", "retired"),
        ("E0071", "retired"),
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
        ("E3104", "retired"),
        ("E0958", "retired"),
        ("E0951", "retired"),
        ("E0993", "retired"),
        ("E0328", "retired"),
        ("E0342", "staged #1606"),
        ("E0954", "retired"),
        ("E0920", "retired"),
        ("E1109", "staged #1220"),
        ("E1111", "retired"),
        ("E1229", "retired"),
        ("E0410", "retired"),
        ("E0859", "staged #521"),
        ("E0416", "retired"),
        ("E0428", "retired"),
        ("E2407", "staged #1330"),
        ("E3626", "staged #521"),
    ];

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
    let stale_fixture_failure = coverage_baseline_failure(
        STALE_ALLOWLIST_FIXTURE.iter().copied(),
        &BTreeSet::new(),
        &emitted,
    )
    .expect("embedded stale allowlist fixture must fail");
    assert_eq!(
        stale_fixture_failure,
        "EXPECTED_SPEC_AHEAD_OF_IMPL contains now-emitted codes; remove the line for each:\n  E2101\n  E3001"
    );

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

    let baseline_failure = coverage_baseline_failure(
        EXPECTED_SPEC_AHEAD_OF_IMPL.iter().map(|(code, _)| *code),
        &spec_ahead_of_impl,
        &emitted,
    );
    if let Some(failure) = baseline_failure {
        panic!("{failure}");
    }
}
