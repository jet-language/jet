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

#[test]
fn testing_snap_declares_env_and_obeys_updates_across_tiers() {
    assert_tiers_agree(
        "testing_snap_source",
        r#"use core.testing as testing
use core.sys as sys

fn run() {
    sys.set("JET_UPDATE_SNAPSHOTS", "1")
    print(testing.snap("source snap é", "first\n"))
    sys.set("JET_UPDATE_SNAPSHOTS", "0")
    print(testing.snap("source snap é", "second\n"))
    sys.set("JET_UPDATE_SNAPSHOTS", "1")
    print(testing.snap("source snap é", "second\n"))
    sys.set("JET_UPDATE_SNAPSHOTS", "0")
    print(testing.snap("source snap é", "second\n"))
}
"#,
        "true\nfalse\ntrue\ntrue\n",
    );
}

#[test]
fn source_testing_diff_accepts_audited_utf8_storage_helpers() {
    let scratch = common::Scratch::new("testing_audited_utf8_diff");
    let golden = scratch.path.join("utf8.golden");
    std::fs::write(&golden, "expected é界\n").expect("write UTF-8 golden");
    let source = format!(
        r#"use core.testing as testing
fn run() {{
    print(testing.golden("{}", "actual é界\n"))
}}
"#,
        jet_string(&golden),
    );
    // Mismatched golden text executes byte scanning and owned diff text
    // transfer in the real Core module, not a generated user-origin helper.
    assert_tiers_agree("testing_audited_utf8_diff", &source, "false\n");
}

#[test]
fn user_unsafe_does_not_admit_private_testing_text_helpers() {
    let scratch = common::Scratch::new("user_private_testing_text");
    let entry = scratch.path.join("main.jet");
    std::fs::write(&entry, "use core.encoding as encoding\nfn run() {}\n")
        .expect("write user source closure");
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).expect("load encoding source");
    let source = r#"use core.encoding as encoding
fn run() {
    #Unsafe("user audit is not a checked Core source identity") {
        _ :: encoding.__string_byte("x", U64{0})
        _ :: encoding.__bytes_into_text(^[U8]{120})
    }
}
"#;
    // Bypass the reserved-name lexical gate only, retaining user source origin.
    let (tokens, lexical) = jet::Lexer::lex_generated(source);
    assert!(lexical.is_empty(), "{lexical:?}");
    let (program, _) = jet::Parser::parse_for_check_with_source(&tokens, source)
        .expect("parse generated private helper calls");
    bundle.modules[bundle.entry].items = program.items;
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    for member in ["__string_byte", "__bytes_into_text"] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E0510"
                && diagnostic.what.contains(member)),
            "{member} escaped source-origin checking: {diagnostics:?}",
        );
    }
}
