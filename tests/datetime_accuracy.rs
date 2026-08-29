//! Card #2288: permanent differential coverage for the core.time kernel.
//!
//! The TSV is the committed Python-oracle result.  This test only executes
//! Jet: it validates the table shape, checks each checked-in golden against its
//! rows, and runs every bounded witness through default and release.

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VECTOR_TABLE: &str = include_str!("fixtures/datetime_accuracy.tsv");

const BATCHES: [(&str, &str, &str); 3] = [
    (
        "epoch_parse",
        "examples/features/time/datetime_accuracy_epoch_parse.jet",
        "examples/features/expected/time/datetime_accuracy_epoch_parse.out",
    ),
    (
        "civil_arithmetic",
        "examples/features/time/datetime_accuracy_civil_arithmetic.jet",
        "examples/features/expected/time/datetime_accuracy_civil_arithmetic.out",
    ),
    (
        "zones",
        "examples/features/time/datetime_accuracy_zones.jet",
        "examples/features/expected/time/datetime_accuracy_zones.out",
    ),
];

#[derive(Debug)]
struct Vector {
    batch: String,
    family: String,
    ident: String,
    operation: String,
    metadata: String,
    oracle: String,
}

fn vectors() -> Vec<Vector> {
    assert!(VECTOR_TABLE.starts_with("# datetime-accuracy-v1\n"));
    let mut rows = Vec::new();
    let mut keys = HashSet::new();
    for (line_index, line) in VECTOR_TABLE.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            6,
            "datetime accuracy vector line {} must have six TSV fields: {line:?}",
            line_index + 1
        );
        assert!(
            BATCHES.iter().any(|(batch, _, _)| *batch == fields[0]),
            "unknown datetime accuracy batch on line {}: {}",
            line_index + 1,
            fields[0]
        );
        for field in &fields {
            assert!(!field.is_empty(), "empty vector field on line {}", line_index + 1);
        }
        let key = (fields[0], fields[1], fields[2], fields[3]);
        assert!(keys.insert(key), "duplicate datetime accuracy vector key: {key:?}");
        rows.push(Vector {
            batch: fields[0].to_string(),
            family: fields[1].to_string(),
            ident: fields[2].to_string(),
            operation: fields[3].to_string(),
            metadata: fields[4].to_string(),
            oracle: fields[5].to_string(),
        });
    }
    assert!(!rows.is_empty(), "datetime accuracy vector table is empty");
    rows
}

fn assert_corpus_shape(rows: &[Vector]) {
    for (batch, _, _) in BATCHES {
        assert!(
            rows.iter().any(|row| row.batch == batch),
            "datetime accuracy batch has no vectors: {batch}"
        );
    }
    for family in ["CAL", "F1", "F2", "F3", "F4", "F5", "F6", "PROP"] {
        assert!(
            rows.iter().any(|row| row.family == family),
            "datetime accuracy family has no vectors: {family}"
        );
    }
    for id in ["f2_000", "f2_001", "f2_002", "f2_003"] {
        assert!(
            rows.iter().any(|row| row.ident == id),
            "missing century/leap edge vector: {id}"
        );
    }
    for operation in [
        "parse_format_round_trip",
        "diff_days_antisymmetry",
        "add_days_composition",
        "add_months_clamping",
    ] {
        assert!(
            rows.iter().any(|row| row.operation == operation),
            "missing datetime property law: {operation}"
        );
    }
    for id in [
        "f5_malformed_leap_second",
        "f5_malformed_hour_24",
        "f5_malformed_offset_plus_24",
    ] {
        assert!(
            rows.iter().any(|row| row.ident == id),
            "missing malformed RFC3339 vector: {id}"
        );
    }
    for zone in [
        "Australia/Lord_Howe",
        "Asia/Kathmandu",
        "Pacific/Apia",
        "Pacific/Chatham",
        "Africa/Casablanca",
        "America/Sao_Paulo",
        "Asia/Tehran",
        "Europe/London",
        "America/New_York",
        "UTC",
    ] {
        assert!(
            rows.iter()
                .any(|row| row.metadata.contains(&format!("zone={zone}"))),
            "missing hostile-DST zone vector: {zone}"
        );
    }
}

fn expected_output(rows: &[Vector], batch: &str) -> Vec<u8> {
    let mut output = String::new();
    for row in rows.iter().filter(|row| row.batch == batch) {
        output.push_str("CASE\t");
        output.push_str(&row.ident);
        output.push('\t');
        output.push_str(&row.operation);
        output.push('\t');
        output.push_str(&row.oracle);
        output.push('\n');
    }
    output.into_bytes()
}

fn run_witness(
    root: &Path,
    scratch: &Path,
    source: &Path,
    release: bool,
) -> std::process::Output {
    let mode = if release { "release" } else { "default" };
    let cache = scratch.join(mode);
    fs::create_dir_all(&cache).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.arg("run");
    if release {
        command.arg("--release");
    }
    command
        .arg(source)
        .current_dir(root)
        .env("JET_CACHE_DIR", cache.join("build"))
        .env("JET_RUN_CACHE_DIR", cache.join("run"))
        .env("JET_TZDB_DIR", root.join("corelib/tzdb"))
        .env("JETPACK_ENV", "1")
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("spawn datetime accuracy {mode} witness: {error}"))
}

#[test]
fn datetime_accuracy_differential_and_properties() {
    let rows = vectors();
    assert_corpus_shape(&rows);

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch =
        common::test_scratch_root(&format!("datetime_accuracy_{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();

    for (batch, source_name, golden_name) in BATCHES {
        let source = root.join(source_name);
        let golden = root.join(golden_name);
        assert!(source.is_file(), "missing datetime accuracy witness: {source_name}");
        let expected = expected_output(&rows, batch);
        assert_eq!(
            fs::read(&golden).unwrap_or_else(|error| panic!("read {golden_name}: {error}")),
            expected,
            "datetime accuracy golden disagrees with committed oracle rows for {batch}"
        );

        let default = run_witness(&root, &scratch.join(batch), &source, false);
        assert!(
            default.status.success(),
            "default datetime accuracy witness failed for {batch}:\n{}",
            String::from_utf8_lossy(&default.stderr)
        );
        assert_eq!(
            default.stdout, expected,
            "default datetime accuracy witness disagrees with oracle for {batch}"
        );

        let release = run_witness(&root, &scratch.join(batch), &source, true);
        assert!(
            release.status.success(),
            "release datetime accuracy witness failed for {batch}:\n{}",
            String::from_utf8_lossy(&release.stderr)
        );
        assert_eq!(
            release.stdout, expected,
            "release datetime accuracy witness disagrees with oracle for {batch}"
        );
        assert_eq!(
            default.stdout, release.stdout,
            "default and release datetime accuracy output differ for {batch}"
        );
    }

    let _ = fs::remove_dir_all(&scratch);
}
