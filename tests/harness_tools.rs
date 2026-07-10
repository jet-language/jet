mod common;

use common::{
    unified_diff, UNIFIED_DIFF_MAX_INPUT_BYTES, UNIFIED_DIFF_MAX_INPUT_LINES,
    UNIFIED_DIFF_MAX_OUTPUT_BYTES,
};

#[test]
fn unified_diff_keeps_small_diffs_complete() {
    let diff = unified_diff("expected", "actual", "same\nold\n", "same\nnew\n");
    assert!(diff.starts_with("--- expected\n+++ actual\n@@"));
    assert!(diff.contains(" same\n"));
    assert!(diff.contains("-old\n"));
    assert!(diff.contains("+new\n"));
    assert!(!diff.contains("truncated"));
}

#[test]
fn unified_diff_bounds_hostile_output_and_names_every_truncation() {
    let expected_line = format!("{}\n", "e".repeat(1024));
    let actual_line = format!("{}\n", "a".repeat(1024));
    let expected = expected_line.repeat(UNIFIED_DIFF_MAX_INPUT_LINES * 2);
    let actual = actual_line.repeat(UNIFIED_DIFF_MAX_INPUT_LINES * 2);

    let diff = unified_diff("expected", "actual", &expected, &actual);

    assert!(diff.starts_with("--- expected\n+++ actual\n"));
    assert!(diff.contains("@@"));
    assert!(
        diff.contains("diff input truncated: expected"),
        "expected input truncation must be explicit"
    );
    assert!(
        diff.contains("diff input truncated: actual"),
        "actual input truncation must be explicit"
    );
    assert!(
        diff.contains("diff output truncated; remaining compared edits omitted"),
        "render truncation must be explicit"
    );
    assert!(diff.len() <= UNIFIED_DIFF_MAX_OUTPUT_BYTES);
    assert!(expected.len() > UNIFIED_DIFF_MAX_INPUT_BYTES);
    assert!(actual.len() > UNIFIED_DIFF_MAX_INPUT_BYTES);
}

#[test]
fn unified_diff_line_limit_is_reported_without_scanning_all_lines() {
    let expected = "old\n".repeat(UNIFIED_DIFF_MAX_INPUT_LINES * 4);
    let actual = "new\n".repeat(UNIFIED_DIFF_MAX_INPUT_LINES * 4);
    let diff = unified_diff("expected", "actual", &expected, &actual);

    assert!(diff.contains(&format!(
        "limits are {} bytes/{} lines",
        UNIFIED_DIFF_MAX_INPUT_BYTES, UNIFIED_DIFF_MAX_INPUT_LINES
    )));
    assert!(diff.len() <= UNIFIED_DIFF_MAX_OUTPUT_BYTES);
}
