#[test]
fn exact_invalid_corpus_rejects_in_jet() {
    // FEATURE_CLAIM: claim.native-language / invalid-front-end-boundary
    let filter = case_filter();
    let selected = selected_cases("invalid", filter.as_deref());
    require_lane_selection("invalid", filter.as_deref(), selected.len());
    // Collect every mismatch instead of aborting on the first one: a
    // minimized-fixture corpus this size needs one full pass per edit, not
    // one panic-and-rerun cycle per broken case.
    let mut failures = Vec::new();
    for path in selected {
        replay(&path, "exact_invalid_corpus_rejects_in_jet");
        let expected = expected_code(&path);
        let fixture = fs::read_to_string(&path).unwrap();
        let src = fixture.replace("__NUL__", "\0");
        let materialized = common::unique_tmp("jet_sema_sound_invalid").with_extension("jet");
        fs::write(&materialized, &src).unwrap();
        match jet::compile_with_path(&src, &materialized.to_string_lossy()) {
            Ok(_) => failures.push(format!(
                "{}: sema accepted known-invalid fixture (expected {expected})",
                relative(&path)
            )),
            Err(diags) => {
                if !diags.iter().any(|d| d.code == expected) {
                    failures.push(format!(
                        "{}: expected {expected}, got {:?}",
                        relative(&path),
                        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} invalid-corpus mismatch(es), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
