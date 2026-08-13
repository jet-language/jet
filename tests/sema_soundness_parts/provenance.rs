#[test]
fn valid_corpus_reaches_rustc() {
    // CAPABILITY_CLAIM: claim.native-language / valid-native-execution
    let filter = case_filter();
    let selected = selected_cases("valid", filter.as_deref());
    require_lane_selection("valid", filter.as_deref(), selected.len());
    require_rustc();
    let failures = run_all_collecting_failures(selected, |path| {
        replay(path, "valid_corpus_reaches_rustc");
        let src = fs::read_to_string(path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy();
        let (code, _, stderr) = build_and_run("jet_sema_sound_valid", &name, &src);
        if code == 0 {
            Ok(())
        } else {
            Err(format!("{}: failed (exit {code}):\n{stderr}", relative(path)))
        }
    });
    assert!(
        failures.is_empty(),
        "{} valid-corpus failure(s), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
/// Crit #3: safe generated user bodies contain no unaudited `unsafe`,
/// classified structurally by provenance. Every `valid`/`differential`
/// fixture's generated Rust must be free of bare `unsafe` once the vetted
/// prelude modules (FFI/mem/term/os/atomic/gtk bridges) are stripped —
/// except the handful of `examples/features/{lowlevel,memory,effects,
/// crypto}` stems that intentionally exercise the audited `#Unsafe` gate
/// (`GATED_UNSAFE_STEMS`), which may contain only *gated* `unsafe { … }` /
/// `unsafe fn` forms, never an ungated one. Mirrors golden.rs's per-example
/// I1 check, applied across the whole soundness corpus.
#[test]
fn generated_rust_has_no_unaudited_unsafe() {
    // CAPABILITY_CLAIM: claim.native-language / safe-codegen-boundary
    let filter = case_filter();
    let mut selected = selected_cases("valid", filter.as_deref());
    selected.extend(selected_cases("differential", filter.as_deref()));
    require_lane_selection("valid or differential", filter.as_deref(), selected.len());
    let failures = run_all_collecting_failures(selected, |path| {
        replay(path, "generated_rust_has_no_unaudited_unsafe");
        let src = fs::read_to_string(path).unwrap();
        // NB: keep this prefix free of the substring "unsafe" — codegen
        // embeds the source path in a `jet:source-map` comment, and a tmp
        // dir name containing "unsafe" would false-positive every check
        // below.
        let materialized = common::unique_tmp("jet_sema_sound_prov").with_extension("jet");
        fs::write(&materialized, &src).unwrap();
        let compiled = match jet::compile_with_path(&src, &materialized.to_string_lossy()) {
            Ok(c) => c,
            Err(diags) => {
                return Err(format!("{}: front end rejected: {diags:?}", relative(path)))
            }
        };
        let user_code = strip_vetted_prelude_modules(&compiled.rust);
        let stem = path.file_stem().unwrap().to_string_lossy();
        let gated = original_example_stem(&stem)
            .map(|orig| GATED_UNSAFE_STEMS.contains(&orig.as_str()))
            .unwrap_or(false);
        if gated {
            for (i, line) in user_code.lines().enumerate() {
                if let Some(col) = line.find("unsafe") {
                    let after = line[col..].trim_start_matches("unsafe").trim_start();
                    if !(after.starts_with('{') || after.starts_with("fn ")) {
                        return Err(format!(
                            "{}: ungated `unsafe` at line {}: {}",
                            relative(path),
                            i + 1,
                            line.trim()
                        ));
                    }
                }
            }
        } else if user_code.contains("unsafe") {
            return Err(format!(
                "{}: generated Rust contains `unsafe` outside the vetted prelude/gated #Unsafe tier",
                relative(path)
            ));
        }
        Ok(())
    });
    assert!(
        failures.is_empty(),
        "{} unsafe-provenance failure(s), each replayable via SEMA_SOUNDNESS_CASE=<path>:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
