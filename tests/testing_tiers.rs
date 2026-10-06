//! D-REPORT-TEST1/A — testing file helpers keep one meaning on every tier.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::path::Path;

use tir_support::{assert_tiers_agree, have_rustc};

fn jet_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[test]
fn testing_file_helpers_preserve_success_and_failure_values_across_tiers() {
    if !have_rustc() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let golden = jet_string(&root.join("tests/fixtures/testing_failure_reports.golden"));
    let missing = jet_string(&root.join("tests/fixtures/testing_failure_reports.missing"));
    let source = format!(
        r#"use core.testing as testing

fn run() {{
    print(testing.golden("{golden}", "expected\n"))
    print(testing.golden("{missing}", "actual\n"))
    print(testing.golden("{golden}", "actual\n"))
    print(testing.fixture("{missing}").len())
    print(testing.fixture("{golden}").len())
}}
"#,
    );
    assert_tiers_agree(
        "testing_file_helpers_tiers",
        &source,
        "true\nfalse\nfalse\n0\n9\n",
    );
}

#[test]
fn testing_corpus_orders_paths_and_skips_non_text_files_across_tiers() {
    let scratch = common::Scratch::new("testing_corpus_source");
    let fixture = scratch.path.join("corpus");
    std::fs::create_dir(&fixture).expect("create corpus fixture");
    std::fs::write(fixture.join("z.txt"), "last").expect("write last corpus entry");
    std::fs::write(fixture.join("a.txt"), "first").expect("write first corpus entry");
    std::fs::write(fixture.join("b.bin"), [255_u8]).expect("write invalid UTF-8 corpus entry");
    std::fs::create_dir(fixture.join("c.dir")).expect("create ignored directory");
    let source = format!(
        r#"use core.testing as testing

fn run() {{
    values :: testing.corpus("{}")
    print(values.len())
    loop value in values -> print(value)
    print(testing.corpus("{}").len())
}}
"#,
        jet_string(&fixture),
        jet_string(&fixture.join("missing")),
    );
    assert_tiers_agree("testing_corpus_source", &source, "2\nfirst\nlast\n0\n");
}
