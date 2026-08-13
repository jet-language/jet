#[test]
fn selector_routes_exactly_one_fixture_category() {
    let filter = case_filter();
    let counts: Vec<_> = ["valid", "invalid", "differential"]
        .into_iter()
        .map(|kind| (kind, selected_cases(kind, filter.as_deref()).len()))
        .collect();
    if let Some(filter) = filter {
        assert_eq!(
            counts.iter().map(|(_, count)| count).sum::<usize>(),
            1,
            "SEMA_SOUNDNESS_CASE must select exactly one fixture: {filter}; routes: {counts:?}"
        );
        assert_eq!(
            counts.iter().filter(|(_, count)| *count == 1).count(),
            1,
            "selector must route to exactly one category: {counts:?}"
        );
    } else {
        assert!(
            counts.iter().all(|(_, count)| *count > 0),
            "full corpus must keep every category non-vacuous: {counts:?}"
        );
    }
}
/// Crit #2: the required CI run is at least 250 fixed cases, zero silent
/// skips. The count always covers the complete fixed corpus; a replay selector
/// cannot weaken this acceptance lane.
#[test]
fn full_corpus_meets_minimum_case_count() {
    // FEATURE_CLAIM: claim.native-language / corpus-size-floor
    if case_filter().is_some() {
        selector_routes_exactly_one_fixture_category();
    }
    let total: usize = ["valid", "invalid", "differential"]
        .iter()
        .map(|kind| cases(kind).len())
        .sum();
    assert!(
        total >= 250,
        "sema soundness corpus has {total} fixed cases across valid+invalid+differential; \
         crit #2 requires at least 250"
    );
}
