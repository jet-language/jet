//! c109 Phase 1: the typed-IR (TIR) path. These programs are squarely inside
//! the Phase-1 subset (scalar/String params, arithmetic, helper calls, an
//! if-expression, bindings, returns, print), so codegen routes them through
//! `Codegen/TIR.rs`. The asserts prove they compile (rustc accepts the output —
//! I2) and run with the right output. Golden parity (`tests/golden.rs`) covers
//! byte-equivalence with the old emitter baselines for the example suite.

use std::fs;
use std::process::Command;

mod common;
use common::have_rustc;

fn build_and_run(name: &str, src: &str) -> (i32, String) {
    let (code, stdout, _stderr) = common::build_and_run("jet_tir_test", name, src);
    (code, stdout)
}

// Shared multi-file harness used by several feature-family modules.
/// Build + run a multi-file program: write each `(relative path, source)` pair into a
/// fresh temp dir, compile the entry, then rustc + run. Used by the Phase-14
/// cross-module tests, which need sibling module files on disk.
fn build_and_run_multi(name: &str, entry: &str, files: &[(&str, &str)]) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!("jet_tir_multi_{}_{}", name, std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let shown = entry_path.to_string_lossy().into_owned();
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let out = jet::compile_with_path(&entry_src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, &entry_src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
    )
}

#[path = "tir/control_and_data.rs"]
mod control_and_data;
#[path = "tir/collections_and_methods.rs"]
mod collections_and_methods;
#[path = "tir/core_and_closures.rs"]
mod core_and_closures;
#[path = "tir/modules_and_enums.rs"]
mod modules_and_enums;
#[path = "tir/unsafe_and_runtime.rs"]
mod unsafe_and_runtime;
#[path = "tir/language_features.rs"]
mod language_features;
#[path = "tir/io_and_ownership.rs"]
mod io_and_ownership;
#[path = "tir/patterns_and_fields.rs"]
mod patterns_and_fields;
#[path = "tir/data_math_reactive.rs"]
mod data_math_reactive;

#[test]
fn tir_integration_target_stays_split_by_feature_family() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let orchestrator = fs::read_to_string(root.join("tests/tir.rs")).unwrap();
    assert!(
        orchestrator.lines().count() <= 150,
        "tests/tir.rs must remain a thin shared harness and module registry"
    );
    for module in [
        "collections_and_methods",
        "control_and_data",
        "core_and_closures",
        "data_math_reactive",
        "io_and_ownership",
        "language_features",
        "modules_and_enums",
        "patterns_and_fields",
        "unsafe_and_runtime",
    ] {
        let source = fs::read_to_string(root.join(format!("tests/tir/{module}.rs"))).unwrap();
        assert!(
            source.lines().count() <= 800,
            "tests/tir/{module}.rs grew past its feature-family boundary"
        );
    }
}
