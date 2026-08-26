//! Card #2209: every restored sema rejection keeps an independent regression.

fn assert_fixture_rejects(name: &str, code: &str, expected_count: usize) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("tests/ui").join(format!("{name}.jet"));
    let source = std::fs::read_to_string(&path).unwrap();
    let diagnostics = match jet::compile_with_path(&source, path.to_str().unwrap()) {
        Err(diagnostics) => diagnostics,
        Ok(_) => panic!("{name} unexpectedly compiled"),
    };
    let actual_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .count();
    assert_eq!(
        actual_count, expected_count,
        "{name} must emit {expected_count} instance(s) of {code}: {diagnostics:?}"
    );
}

#[test]
fn decimal_float_mix_emits_e0131() {
    assert_fixture_rejects("decimal_float_mix", "E0131", 1);
}

#[test]
fn gc_escape_ownership_emits_e2111() {
    assert_fixture_rejects("gc_escape_ownership", "E2111", 1);
}

#[test]
fn inline_range_runtime_needs_try_emits_e0136() {
    assert_fixture_rejects("inline_range_runtime_needs_try", "E0136", 1);
}

#[test]
fn map_key_composite_rejected_emits_both_e0502_diagnostics() {
    assert_fixture_rejects("map_key_composite_rejected", "E0502", 2);
}

#[test]
fn must_use_fn_ignored_emits_e0419() {
    assert_fixture_rejects("must_use_fn_ignored", "E0419", 1);
}
