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
//!   - the compiled scheduler runtime module: one runtime-boundary panic and
//!     five `#[cfg(test)]` runtime/assertion fixtures.
//!
//! A file not on the allowlist may have zero bare `panic!`s. A file on the
//! allowlist may have AT MOST its listed count — so both a brand new bare
//! panic site AND a seeded extra panic on an already-allowlisted file trip
//! this test, per card #453's exit criteria.
//!
//! Run: `cargo test --test ban_bare_panic`

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
        "crates/jet-codegen/src/scheduler.rs",
        6,
        "one scheduler runtime unwind boundary plus five #[cfg(test)] fixtures: IOCP completion, deadline, cancel result, shield result, and body-panic cleanup",
    ),
    (
        "crates/jet-codegen/src/Codegen/TIR/tests.rs",
        7,
        "test file — every panic! here is inside a #[test] fn",
    ),
    (
        "crates/jet-comptime/src/Comptime/Reflect.rs",
        5,
        "#[test]-only assertion fixtures",
    ),
    (
        "crates/jet-parser/src/Parser/mod.rs",
        2,
        "#[cfg(test)] mod s61_tests fixtures",
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
