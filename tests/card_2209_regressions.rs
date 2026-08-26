//! Card #2209: every restored sema rejection keeps an independent regression.

const CASES: &[(&str, &str, usize)] = &[
    ("decimal_float_mix", "E0131", 1),
    ("gc_escape_ownership", "E2111", 1),
    ("inline_range_runtime_needs_try", "E0136", 1),
    ("map_key_composite_rejected", "E0502", 2),
    ("must_use_fn_ignored", "E0419", 1),
];

#[test]
fn card_2209_fixtures_keep_their_production_rejections() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (name, code, expected_count) in CASES {
        let path = root.join("tests/ui").join(format!("{name}.jet"));
        let source = std::fs::read_to_string(&path).unwrap();
        let diagnostics = match jet::compile_with_path(&source, path.to_str().unwrap()) {
            Err(diagnostics) => diagnostics,
            Ok(_) => panic!("{name} unexpectedly compiled"),
        };
        let actual_count = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == *code)
            .count();
        assert_eq!(
            actual_count, *expected_count,
            "{name} must emit {expected_count} instance(s) of {code}: {diagnostics:?}"
        );
    }
}
