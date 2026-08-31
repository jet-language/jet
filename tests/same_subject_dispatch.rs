//! L0514: the adjacent-guard rewrite keeps parser-style dispatch behavior.

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

const SOURCE: &str = include_str!("fixtures/adjacent_subject_dispatch_witness.jet");
const EXPECTED_STDOUT: &str = "revision\n";
const BRACED_SOURCE: &str = r#"fn run() {
    key :: "rev"
    if key == "revision" {
        print("revision")
    }
    if key == "rev" {
        print("revision")
    }
    if key == "path" {
        print("path")
    }
}
"#;

#[test]
fn rewrite_compiles_formats_and_preserves_parser_dispatch_behavior() {
    let output = tir_support::compile_source("adjacent_subject_dispatch_witness.jet", SOURCE)
        .expect("the non-Jetpack dispatch witness should compile");
    let lint = output
        .lints
        .iter()
        .find(|diagnostic| diagnostic.code == "L0514")
        .expect("three adjacent categorical guards should be linted");
    assert_eq!(
        lint.applicability,
        Some(jet::Diagnostics::FixApplicability::Suggested)
    );
    assert_eq!(lint.safety, Some(jet::Diagnostics::FixSafety::NeedsReview));

    let edit = lint.edit.clone().expect("L0514 should carry a source edit");
    let fixed = jet::FixEngine::apply_edits(SOURCE, std::slice::from_ref(&edit))
        .expect("the structured L0514 edit should apply");
    assert!(
        fixed.contains("if key == {\n        \"revision\" | \"rev\" -> print(\"revision\")"),
        "fixed source did not group aliases in an ordered table:\n{fixed}"
    );
    assert_eq!(
        jet::format_source(&fixed).expect("the rewrite should be formatter-readable"),
        fixed,
        "the L0514 action should produce formatted source"
    );
    let fixed_output =
        tir_support::compile_source("adjacent_subject_dispatch_witness_fixed.jet", &fixed)
            .expect("the suggested rewrite should compile");
    assert!(
        !fixed_output
            .lints
            .iter()
            .any(|diagnostic| diagnostic.code == "L0514"),
        "the rewrite should remove the adjacent guard run"
    );

    tir_support::assert_tiers_agree(
        "adjacent_subject_dispatch_original",
        SOURCE,
        EXPECTED_STDOUT,
    );
    tir_support::assert_tiers_agree("adjacent_subject_dispatch_rewrite", &fixed, EXPECTED_STDOUT);
}

#[test]
fn braced_rewrite_is_formatter_stable_and_compiles() {
    let output = tir_support::compile_source(
        "adjacent_subject_dispatch_braced.jet",
        BRACED_SOURCE,
    )
    .expect("the braced dispatch witness should compile");
    let lint = output
        .lints
        .iter()
        .find(|diagnostic| diagnostic.code == "L0514")
        .expect("three adjacent braced guards should be linted");
    let edit = lint.edit.clone().expect("L0514 should carry a source edit");
    let fixed = jet::FixEngine::apply_edits(BRACED_SOURCE, std::slice::from_ref(&edit))
        .expect("the braced L0514 edit should apply");

    assert_eq!(
        jet::format_source(&fixed).expect("the braced rewrite should format"),
        fixed,
        "the LSP refactor action should produce formatter-stable source"
    );
    tir_support::compile_source("adjacent_subject_dispatch_braced_fixed.jet", &fixed)
        .expect("the formatted braced rewrite should compile");
}
