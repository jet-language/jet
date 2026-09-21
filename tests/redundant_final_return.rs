//! L0530 proves the implicit-tail return lint and its safe source fix.

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

const SOURCE: &str = include_str!("ui_lint/redundant_final_return.jet");
const EXPECTED_STDOUT: &str = "42\n";

#[test]
fn final_return_fix_removes_keyword() {
    let output = tir_support::compile_source("redundant_final_return.jet", SOURCE)
        .expect("final return fixture must compile");
    let lint = output
        .lints
        .iter()
        .find(|diagnostic| diagnostic.code == "L0530")
        .expect("a final explicit return must be linted");
    assert_eq!(
        lint.applicability,
        Some(jet::Diagnostics::FixApplicability::Safe)
    );
    assert_eq!(lint.safety, Some(jet::Diagnostics::FixSafety::Formatting));
    let edits = lint.all_edits();
    assert_eq!(edits.len(), 1, "the fix removes the redundant keyword");

    let fixed = jet::FixEngine::apply_edits(SOURCE, &edits)
        .expect("the final-return edit must apply without overlap");
    assert!(!fixed.contains("return 42"), "final return survived: {fixed}");
    assert!(fixed.contains("    42\n"), "implicit tail was not formed: {fixed}");
    let formatted = jet::format_source(&fixed).expect("fixed source must format");
    assert!(formatted.contains("42"), "formatted tail lost its value: {formatted}");

    let fixed_output = tir_support::compile_source("redundant_final_return_fixed.jet", &formatted)
        .expect("fixed final-return fixture must compile");
    assert!(!fixed_output
        .lints
        .iter()
        .any(|diagnostic| diagnostic.code == "L0530"));
    tir_support::assert_tiers_agree("redundant_final_return", SOURCE, EXPECTED_STDOUT);
    tir_support::assert_tiers_agree("redundant_final_return_fixed", &formatted, EXPECTED_STDOUT);
}

