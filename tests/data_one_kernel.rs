//! #1657 (I9): `core.data` statistics live in one place.
//!
//! `crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs` is the kernel.
//! AOT embeds it with `include_str!`, the Cranelift JIT host and the
//! comptime/interpreter tier `include!` the same file, and every other tier is
//! a marshalling adapter. A second implementation — compensated or naive —
//! makes one tier answer a question differently from the rest, which is the
//! exact I9 drift this card removed.
//!
//! This test fails the build on a second copy. It looks for two things:
//!   1. a function that takes a float sequence and carries a statistics name
//!      (`sum`, `mean`, `variance`, …) outside the kernel;
//!   2. the kernel's own arithmetic fingerprints — the Neumaier compensation
//!      step and the Welford update — outside the kernel.
//!
//! Run: `cargo test --test data_one_kernel`

use std::fs;
use std::path::{Path, PathBuf};

const KERNEL: &str = "crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs";

/// Statistics names that must not head a second float-sequence function.
const STAT_WORDS: &[&str] = &[
    "sum",
    "mean",
    "min",
    "max",
    "median",
    "variance",
    "stddev",
    "quantile",
    "rolling",
    "describe",
];

/// Arithmetic that only the kernel may contain.
const FINGERPRINTS: &[(&str, &str)] = &[
    ("compensation +=", "Neumaier compensated sum"),
    ("m2 += delta * delta2", "Welford variance update"),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

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

fn source_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs_files(&root().join("crates"), &mut out);
    collect_rs_files(&root().join("Source"), &mut out);
    out.sort();
    out
}

/// A one-line function head that reads a float sequence, answers with a float
/// or a float list, and names a statistic. A marshalling adapter answers with a
/// value type instead, so it never matches.
fn stats_function_names(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if !(line.contains("&[f64]") || line.contains("&Vec<f64>")) {
            continue;
        }
        let Some((_, after_arrow)) = line.split_once("-> ") else {
            continue;
        };
        if !after_arrow.contains("f64") {
            continue;
        }
        let Some(rest) = line.split_once("fn ") else {
            continue;
        };
        let name: String = rest
            .1
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let lower = name.to_ascii_lowercase();
        if lower
            .split('_')
            .any(|part| STAT_WORDS.contains(&part))
        {
            found.push(name);
        }
    }
    found
}

#[test]
fn only_one_core_data_statistics_implementation() {
    let kernel = root().join(KERNEL);
    assert!(
        kernel.is_file(),
        "the one core.data statistics kernel is missing: {KERNEL}"
    );

    let mut offenders = Vec::new();
    for path in source_files() {
        if path == kernel {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root())
            .unwrap_or(&path)
            .display()
            .to_string();
        for name in stats_function_names(&text) {
            offenders.push(format!("{relative}: fn {name}(… &[f64] …)"));
        }
        for (needle, what) in FINGERPRINTS {
            if text.contains(needle) {
                offenders.push(format!("{relative}: {what} (`{needle}`)"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a second core.data statistics implementation exists (I9). Every tier must call \
         the one kernel at {KERNEL} instead:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn every_tier_includes_the_one_kernel() {
    let kernel_name = "CoreLib/Top/DataStats.rs";
    for (path, how) in [
        (
            "crates/jet-codegen/src/Codegen/mod.rs",
            "AOT embeds the kernel",
        ),
        (
            "crates/jet-jit/src/Data.rs",
            "the Cranelift JIT host includes the kernel",
        ),
        (
            "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
            "comptime and the interpreter include the kernel",
        ),
    ] {
        let text = fs::read_to_string(root().join(path))
            .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
        assert!(
            text.contains(kernel_name),
            "{how}: {path} no longer names {kernel_name}"
        );
    }
}
