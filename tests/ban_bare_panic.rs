//! W4 (durability, card #453): bans a bare `panic!(` in compiler-crate source
//! outside an explicit, commented allowlist. Every real internal-compiler-bug
//! panic site should go through `jet_foundation::ice!` (the single I2 banner
//! macro, `crates/jet-foundation/src/Diagnostics.rs`) instead of a bare
//! `panic!`, so every ICE reads the same. The allowlist below covers the only
//! legitimate bare-`panic!` classes left:
//!   - the `ice!` macro's own implementation (it must literally say `panic!`)
//!   - `#[cfg(test)]`/`#[test]`-only assertion fixtures (never reached by a
//!     real compile; a test failing loudly with `panic!` is normal Rust)
//!   - the Prelude runtime templates (`Prelude/Core.rs`, `Layout.rs`,
//!     `Scheduler.rs`) — these are `include_str!`-embedded text compiled into
//!     the USER's program, not compiler code; their panics are the `jet_panic`
//!     runtime-panic path (`RUNTIME_PANIC` = 70), a different contract than I2.
//!   - the scheduler host bindings (`SchedulerHost.rs`): `#[cfg(test)]`
//!     assertion fixtures only.
//!
//! A file not on the allowlist may have zero bare `panic!`s. A file on the
//! allowlist may have AT MOST its listed count — so both a brand new bare
//! panic site AND a seeded extra panic on an already-allowlisted file trip
//! this test, per card #453's exit criteria.
//!
//! Run: `cargo test --test ban_bare_panic`

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// (path relative to repo root, max allowed bare `panic!(` occurrences, why).
const ALLOWLIST: &[(&str, usize, &str)] = &[
    (
        "crates/jet-foundation/src/Diagnostics.rs",
        2,
        "the ice! macro's own panic! implementation",
    ),
    (
        "crates/jet-foundation/src/XmlPull.rs",
        32,
        "#[cfg(test)] XML parser/event/tree assertion fixtures; exact audited count",
    ),
    (
        "crates/jet-foundation/src/Terminal.rs",
        1,
        "#[cfg(test)] isolated child-process case assertion fixture",
    ),
    (
        "crates/jet-codegen/src/Prelude/Core.rs",
        4,
        "include_str! runtime template — user-program RUNTIME_PANIC path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/Layout.rs",
        1,
        "include_str! runtime template — user-program RUNTIME_PANIC path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/Scheduler.rs",
        1,
        "include_str! runtime template — user-program RUNTIME_PANIC path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/Mem.rs",
        3,
        "include_str! allocator runtime template — user-program RUNTIME_PANIC path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/LocalCell.rs",
        4,
        "include_str! local-cell runtime template — user-program borrow-conflict panic path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs",
        1,
        "include_str! task runtime template — user-program already-joined-task panic path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs",
        3,
        "include_str! HTTP server runtime template — user-program serving panic path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/App.rs",
        1,
        "include_str! web-app runtime template — user-program serving panic path, not compiler code",
    ),
    (
        "crates/jet-codegen/src/Prelude/TaskGroup.rs",
        4,
        "#[cfg(test)] task-group assertion fixtures; exact audited count",
    ),
    (
        "crates/jet-codegen/src/Prelude/SharedProtocol.rs",
        1,
        "#[cfg(test)] shared-protocol panic waiter fixture",
    ),
    (
        "crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs",
        1,
        "include_str! runtime template — cbor.encode user-program runtime panic, not compiler code",
    ),
    (
        "crates/jet-codegen/src/SchedulerHost.rs",
        5,
        "#[cfg(test)] scheduler fixtures: stream producer failure, cancel result, shield result, yield deadline, and body-panic cleanup",
    ),
    (
        "crates/jet-codegen/src/Codegen/TIR/tests.rs",
        8,
        "test file — every panic! here is inside a #[test] fn",
    ),
    (
        "crates/jet-comptime/src/Comptime/EncodingLite.rs",
        7,
        "#[cfg(test)] XML conversion shape assertion fixtures",
    ),
    (
        "crates/jet-comptime/src/Comptime/Reflect.rs",
        5,
        "#[test]-only assertion fixtures",
    ),
    (
        "crates/jet-parser/src/Parser/mod.rs",
        4,
        "#[cfg(test)] mod s61_tests assertion fixtures; exact audited count",
    ),
    (
        "crates/jet-parser/src/lib.rs",
        5,
        "#[cfg(test)] generic-module parser assertion fixtures",
    ),
    (
        "crates/jet-sema/tests/generic_module_body.rs",
        1,
        "integration-test assertion fixture",
    ),
    (
        "crates/jet-cli/src/Help/mod.rs",
        1,
        "#[test]-only assertion fixture",
    ),
    (
        "crates/jet-cli/src/Help/Render.rs",
        1,
        "#[test]-only help-result shape assertion fixture",
    ),
    (
        "crates/jet-repl/src/lib.rs",
        3,
        "three #[cfg(test)] REPL statement-classifier assertion fixtures",
    ),
    (
        "crates/jet-repl/src/HistoryPlatform.rs",
        1,
        "#[cfg(test)] Unix ABI-table assertion helper",
    ),
    (
        "crates/jet-repl/src/History.rs",
        1,
        "#[test] history lock assertion fixture",
    ),
    (
        "crates/jet-repl/src/Notebook/trust.rs",
        1,
        "#[cfg(test)] notebook render-decision assertion fixture",
    ),
    (
        "Source/BudgetProviders.rs",
        1,
        "#[cfg(test)] deliberately panicking hostile provider fixture for catch_unwind",
    ),
    (
        "crates/jet-cli/src/CLI.rs",
        1,
        "#[cfg(test)] nested-command registry assertion fixture",
    ),
    (
        "Source/Interpreter.rs",
        1,
        "#[test]-only assertion fixture",
    ),
    (
        "Source/LSP/mod.rs",
        2,
        "#[cfg(test)] isolated-project and bundle-presence assertion fixtures",
    ),
    (
        "crates/jet-driver/src/CompilerExtensionHook.rs",
        8,
        "#[cfg(test)] compiler-extension assertion fixtures",
    ),
    (
        "Source/ProveSolver.rs",
        1,
        "#[test]-only solver assertion fixture",
    ),
];

const SCAN_ROOTS: &[&str] = &[
    "crates/jet-foundation",
    "crates/jet-lexer",
    "crates/jet-parser",
    "crates/jet-comptime",
    "crates/jet-sema",
    "crates/jet-codegen",
    "crates/jet-driver",
    "crates/jet-semindex",
    "crates/jet-jit",
    "crates/jet-net",
    "crates/jet-queries",
    "crates/jet-rt",
    "crates/jet-impact",
    "crates/jet-repl",
    "crates/jet-debug",
    "crates/jet-cli",
    "Source",
];

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Bare `panic!(` occurrences in `text`, ignoring `//` comment lines (doc
/// comments that merely mention the word, like this file's own header).
fn count_bare_panics(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//") && line.contains("panic!(")
        })
        .count()
}

#[derive(Debug, Clone)]
struct ExamplePanicBudgetEntry {
    path: String,
    max: usize,
    reason: String,
}

const EXAMPLES_PANIC_BUDGET: &str =
    include_str!("fixtures/examples_panic_budget.txt");

fn parse_examples_panic_budget() -> Vec<ExamplePanicBudgetEntry> {
    let mut entries = Vec::new();
    let mut previous_path: Option<String> = None;

    for (line_index, line) in EXAMPLES_PANIC_BUDGET.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.splitn(3, '\t');
        let path = fields
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let max = fields
            .next()
            .unwrap_or_default()
            .trim()
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("invalid panic budget count on line {}", line_index + 1));
        let reason = fields
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();

        assert!(
            path.starts_with("examples/features/"),
            "panic budget path must be a feature-corpus source: {path}"
        );
        assert!(max > 0, "panic budget count must be positive: {path}");
        assert!(
            !reason.is_empty(),
            "panic budget entry needs a teaching reason: {path}"
        );
        if let Some(previous_path) = previous_path {
            assert!(
                previous_path.as_str() < path.as_str(),
                "panic budget paths must be sorted and unique: {path}"
            );
        }
        previous_path = Some(path.clone());
        entries.push(ExamplePanicBudgetEntry { path, max, reason });
    }

    assert!(
        !entries.is_empty(),
        "examples panic budget must contain at least one entry"
    );
    entries
}

fn collect_feature_example_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read feature example directory") {
        let path = entry.expect("read feature example entry").path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("expected") {
                continue;
            }
            collect_feature_example_files(&path, out);
        } else if path.file_name().and_then(|name| name.to_str()) != Some("package.jet")
            && path.extension().is_some_and(|extension| extension == "jet")
        {
            out.push(path);
        }
    }
}

/// The panic ratchet scans every non-manifest `.jet` source under the feature
/// corpus. Expected output and package manifests are data, not example source.
fn collect_feature_example_sources(root: &Path) -> Vec<PathBuf> {
    let ex_dir = root.join("examples/features");
    let mut files = Vec::new();
    collect_feature_example_files(&ex_dir, &mut files);
    files.sort();
    files
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_jet_panic_call(bytes: &[u8], index: usize) -> bool {
    if index + 5 > bytes.len()
        || (index > 0 && is_identifier_byte(bytes[index - 1]))
        || !bytes[index..].starts_with(b"panic")
    {
        return false;
    }

    let mut after_name = index + 5;
    while after_name < bytes.len() && bytes[after_name].is_ascii_whitespace() {
        after_name += 1;
    }
    after_name < bytes.len() && bytes[after_name] == b'('
}

/// Counts non-comment `panic(` calls in Jet source. The scanner ignores line
/// and block comments only; it remains conservative around strings because
/// interpolation can contain executable expressions, and false positives are
/// safer than allowing a new panic call through the ratchet.
fn count_jet_panic_calls(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut index = 0;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        if in_block_comment {
            if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                in_block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if bytes[index] == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if bytes[index] == b'"' {
                in_string = false;
                index += 1;
                continue;
            }
            if is_jet_panic_call(bytes, index) {
                count += 1;
                index += 5;
            } else {
                index += 1;
            }
            continue;
        }

        if bytes[index] == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            in_block_comment = true;
            index += 2;
            continue;
        }
        if is_jet_panic_call(bytes, index) {
            count += 1;
            index += 5;
        } else {
            index += 1;
        }
    }

    count
}

fn example_panic_counts(root: &Path) -> Vec<(String, usize)> {
    collect_feature_example_sources(root)
        .into_iter()
        .map(|file| {
            let path = file
                .strip_prefix(root)
                .expect("example must be below repository root")
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&file).expect("read feature example source");
            let count = count_jet_panic_calls(&text);
            (path, count)
        })
        .collect()
}

fn example_panic_budget_violations(
    current: &[(String, usize)],
    budget: &[ExamplePanicBudgetEntry],
) -> Vec<String> {
    let mut allowed = BTreeMap::new();
    let mut maximum_total = 0;
    for entry in budget {
        maximum_total += entry.max;
        allowed.insert(entry.path.clone(), entry.max);
    }

    let mut current_paths = BTreeSet::new();
    let mut current_total = 0;
    let mut violations = Vec::new();
    for (path, count) in current {
        current_paths.insert(path.clone());
        current_total += *count;
        match allowed.get(path) {
            None if *count > 0 => violations.push(format!(
                "{path}: {count} panic( calls are unbudgeted; add a real teaching reason before keeping this path"
            )),
            Some(max) if *count > *max => violations.push(format!(
                "{path}: {count} panic( calls grew past its budget of {max} ({}); migrate the failure instead",
                budget
                    .iter()
                    .find(|entry| entry.path.as_str() == path.as_str())
                    .map(|entry| entry.reason.as_str())
                    .unwrap_or("missing reason")
            )),
            _ => {}
        }
    }

    for entry in budget {
        if !current_paths.contains(&entry.path) {
            violations.push(format!(
                "{}: budget path no longer exists in the feature corpus; remove the stale entry",
                entry.path
            ));
        }
    }
    if current_total > maximum_total {
        violations.push(format!(
            "feature corpus total grew to {current_total} panic( calls; budget is {maximum_total}"
        ));
    }
    violations
}

#[test]
fn examples_panic_budget_only_shrinks() {
    let root = root();
    let budget = parse_examples_panic_budget();
    let current = example_panic_counts(&root);
    let violations = example_panic_budget_violations(&current, &budget);

    assert!(
        violations.is_empty(),
        "feature example panic budget grew or became stale:\n{}",
        violations.join("\n")
    );
}

#[test]
fn examples_panic_budget_trips_on_seeded_growth() {
    let budget = parse_examples_panic_budget();
    let seed = budget
        .first()
        .expect("panic budget needs a seeded growth entry");
    let seeded_source = "panic(\"seeded\")\n".repeat(seed.max + 1);
    let seeded_count = count_jet_panic_calls(&seeded_source);
    assert_eq!(seeded_count, seed.max + 1);

    let current = vec![(seed.path.clone(), seeded_count)];
    let violations = example_panic_budget_violations(&current, &budget);
    assert!(
        violations
            .iter()
            .any(|violation| violation.starts_with(seed.path.as_str())),
        "seeded panic growth did not trip the path budget: {violations:?}"
    );
}

#[test]
fn compiler_crates_ban_bare_panic_outside_allowlist() {
    let root = root();
    let mut violations = Vec::new();

    for scan_root in SCAN_ROOTS {
        let mut files = Vec::new();
        collect_rs_files(&root.join(scan_root), &mut files);
        for file in files {
            let rel = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&file).unwrap_or_default();
            let count = count_bare_panics(&text);
            if count == 0 {
                continue;
            }
            let allowed = ALLOWLIST
                .iter()
                .find(|(path, ..)| *path == rel)
                .map(|(_, max, _)| *max)
                .unwrap_or(0);
            if count > allowed {
                violations.push(format!(
                    "{rel}: {count} bare panic!( (allowlisted: {allowed}) — use jet_foundation::ice!(span, \"…\") or add to ALLOWLIST with a reason"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "bare panic! outside the ice!/vetted allowlist (I2, card #453):\n{}",
        violations.join("\n")
    );
}

/// Proves the ban actually trips: an allowlisted file's count going UP (a
/// seeded extra bare panic) must fail against the allowlist ceiling. This
/// doesn't touch real source — it re-runs the same counting logic the real
/// test uses against a synthetic seeded string, so the assertion logic itself
/// is covered without needing to add-then-revert a real bare panic! in
/// production code.
#[test]
fn ban_logic_trips_on_a_seeded_bare_panic() {
    let seeded = r#"
fn already_vetted_test_fixture() {
    panic!("the allowlist permits this one vetted fixture");
}

fn seeded_leak() {
    panic!("bare panic! slipped past ice! adoption");
}
"#;
    let count = count_bare_panics(seeded);
    let allowlisted_ceiling_before_seed = 1;
    assert_eq!(count, 2, "fixture must contain one vetted and one seeded panic");
    assert!(
        count > allowlisted_ceiling_before_seed,
        "ban logic failed to reject growth past an existing allowlist ceiling"
    );
    // This is the exact comparison compiler_crates_ban_bare_panic_outside_allowlist
    // makes per-file: an already-allowlisted file permits its vetted count,
    // so one seeded leak must exceed that ceiling and be reported.
    assert!(
        count > allowlisted_ceiling_before_seed,
        "seeded bare panic! did not trip the allowlisted-file ratchet"
    );
}
