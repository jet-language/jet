//! Program-meaning regressions on the run tiers.
//!
//! Each case here was first found as a native-tier miscompile: a map that
//! read Float values as exact-Int pointers, `text += piece` lowered as integer
//! addition of two heap handles (#4572), a word-sized struct call that lost
//! bits (#4383), Shared owners released early. The program's observable
//! output is the contract, so every case runs through both run tiers — the
//! default `jet run` (build + execute) and `jet run --release` (the release
//! profile) — and the two must agree byte for byte with the oracle (I9).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `(tier, exit code, stdout, stderr)` for one `jet run` invocation.
fn run_tier(file: &Path, release: bool) -> (i32, String, String) {
    let cache = common::Scratch::new(if release {
        "jet_run_regression_release"
    } else {
        "jet_run_regression_default"
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.arg("run");
    if release {
        command.arg("--release");
    }
    let output = command
        .arg(file)
        .current_dir(repo_root())
        .env("JET_STORE_DIR", cache.path.join("cache"))
        .env("JETPACK_ROOT", cache.path.join("jetpack"))
        .env("JETPACK_ENV", "1")
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn jet run");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Both run tiers exit 0 with exactly `expected` on stdout and nothing on
/// stderr.
fn assert_run_tiers(file: &Path, expected: &str) {
    for (tier, release) in [("default jet run", false), ("jet run --release", true)] {
        let (code, stdout, stderr) = run_tier(file, release);
        assert_eq!(code, 0, "{tier} failed for {}:\n{stderr}", file.display());
        assert_eq!(stdout, expected, "{tier} stdout drifted for {}", file.display());
        assert!(stderr.is_empty(), "{tier} wrote stderr for {}:\n{stderr}", file.display());
    }
}

fn read_golden(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("read golden {relative}: {error}"))
}

fn scratch_program(name: &str, source: &str) -> (common::Scratch, PathBuf) {
    let scratch = common::Scratch::new(name);
    let file = scratch.path.join(format!("{name}.jet"));
    fs::write(&file, source).expect("write scratch program");
    (scratch, file)
}

/// Map values that are Floats are raw words. Every positive Float >= 2.0 has
/// the exact-Int pointer tag in its top bits, so a map that routed them as
/// integer cells dereferenced the mantissa (SIGSEGV in scaling-data).
#[test]
fn map_float_values_are_never_exact_int_pointers() {
    assert_run_tiers(
        &repo_root().join("tests/fixtures/map_float_values.jet"),
        "index 16.0 add 1000.125\nliteral 2.5 3.75\ncopy 3 6.5 source 2\n\
         filter 1 1000.125\na 16.0\nb 1000.125\n",
    );
}

/// #4576: fixed-width 64-bit map keys and values are word cells, never
/// exact-Int pointers.
#[test]
fn map_word_keys_and_values_are_never_exact_int_pointers() {
    assert_run_tiers(
        &repo_root().join("tests/fixtures/map_word_cells.jet"),
        "absent true\npresent 1 2\n4611686018427387912 2\n5846639821254430312 1\n\
         18446744073709551615 3\nsigned 4\nvalue 5846639821254430312\nliteral\n",
    );
}

/// #4572: `text += piece` on locals, fields, list-element fields and
/// `&String` parameters is text append, never handle arithmetic.
#[test]
fn string_append_matches_golden() {
    assert_run_tiers(
        &repo_root().join("Examples/features/basics/string_append.jet"),
        &read_golden("Examples/features/expected/basics/string_append.out"),
    );
}

/// #4383: a struct-returning word call preserves every bit.
#[test]
fn struct_call_word_preserves_bits() {
    assert_run_tiers(
        &repo_root().join("tests/fixtures/struct_call_word_original.jet"),
        "wrapped: 7299585414513572552\n",
    );
    assert_run_tiers(
        &repo_root().join("Examples/features/basics/struct_call_word.jet"),
        &read_golden("Examples/features/expected/basics/struct_call_word.out"),
    );
}

/// #4383: a wrong expectation over the word-call result is an assertion
/// failure (E3001, exit 70) on every tier, not a silent pass.
#[test]
fn struct_call_word_wrong_expectation_is_an_assertion_failure() {
    let file = repo_root().join("tests/fixtures/struct_call_word_failure.jet");
    for (tier, release) in [("default jet run", false), ("jet run --release", true)] {
        let (code, _stdout, stderr) = run_tier(&file, release);
        assert_eq!(code, 70, "{tier}: {stderr}");
        assert!(stderr.contains("E3001"), "{tier}: {stderr}");
    }
}

/// Named arguments keep their observable evaluation order.
#[test]
fn named_args_example_matches_golden() {
    assert_run_tiers(
        &repo_root().join("Examples/features/basics/named_args.jet"),
        &read_golden("Examples/features/expected/basics/named_args.out"),
    );
}

/// `#Root` receiver calls and the direct-call spelling reach the same
/// function.
#[test]
fn root_receiver_calls_share_the_direct_call_path() {
    let (_scratch, file) = scratch_program(
        "root_calls",
        r#"fn scale(#Root value: Int, factor: Int) Int -[]> {
    return value * factor
}

fn add_half(#Root value: Float) Float -[]> {
    return value + 0.5
}

fn run() {
    total :: 3
    print(total.scale(4))
    print(scale(total, 4))
    print(total.add_half())
    total.print()
}
"#,
    );
    assert_run_tiers(&file, "12\n12\n3.5\n3\n");
}

const SHARED_OWNERSHIP_SOURCE: &str = include_str!("fixtures/shared_owner_topology.jet");

/// Shared owners moved into aggregates and closures keep exactly the
/// expected strong counts, and nothing is released early or leaked.
#[test]
fn shared_and_closure_owner_topology_is_preserved() {
    let (_scratch, file) = scratch_program("shared_ownership", SHARED_OWNERSHIP_SOURCE);
    assert_run_tiers(
        &file,
        "1\n2\n2\n1\n1\n7\n8\ntrue\n8\n102\n202\n302\n2\n1\n1\n13\n1\ntrue\n11\n1\ntrue\ntrue\ntrue\n",
    );
}
