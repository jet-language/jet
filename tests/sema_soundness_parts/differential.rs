#[test]
fn executable_corpus_matches_aot_and_default_dev() {
    // FEATURE_CLAIM: claim.native-language / accepted-native-semantics
    let filter = case_filter();
    let all = selected_cases("differential", filter.as_deref());
    let selected = if filter.is_some() {
        all
    } else {
        all.into_iter()
            .enumerate()
            .filter(|(index, _)| index % DIFFERENTIAL_PARTITIONS == DIFFERENTIAL_PARTITION)
            .map(|(_, path)| path)
            .collect()
    };
    require_lane_selection("differential", filter.as_deref(), selected.len());
    require_rustc();
    let failures = run_all_collecting_failures(selected, |path| {
        replay(path, "executable_corpus_matches_aot_and_default_dev");
        let src = fs::read_to_string(path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy();
        let expected = fs::read_to_string(path.with_extension("out"))
            .unwrap_or_else(|e| panic!("{} needs .out: {e}", relative(path)));
        let aot = build_and_run("jet_sema_sound_diff", &name, &src);
        let dev = match default_dev_with_deadline(path) {
            Ok(result) => result,
            Err(error) => return Err(format!("{}: {error}", relative(path))),
        };
        if aot != dev {
            return Err(format!("{}: AOT/default-dev divergence: {aot:?} vs {dev:?}", relative(path)));
        }
        let want = (0, expected, String::new());
        if aot != want {
            return Err(format!("{}: output drift: got {aot:?}, want {want:?}", relative(path)));
        }
        Ok(())
    });
    assert!(
        failures.is_empty(),
        "{} differential-corpus failure(s), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
