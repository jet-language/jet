// The strict JIT<->AOT differential corpus gate (#2020).
//
// Own target: it classifies all ~500 stems with an AOT build each, and
// `tools/ci/jit-aot-parity.sh` already runs it as its own invocation.

/// c727 C1–C4: discover every top-level example, classify it, and ratchet the
/// manifest. AOT-oracle examples (exit 0) must resident-JIT or deopt-interp
/// with backend attribution — never silent fallback. Each AOT-oracle case
/// compares pure-interpreter, default tiered, and optimized AOT
/// stdout/stderr/exit byte-for-byte (D-ONECORE1=A). AOT-green examples that
/// fail default tiered run land in shrink-only `run_tier_broken`
/// (D-VERDICT-1254-1 / D-LENS-RUN1).
/// parity: guard tests/dev_corpus_gate.rs::example_corpus_strict_jit_aot_differential_gate
///
/// c730: CI runs this via `tools/ci/jit-aot-parity.sh` on every supported
/// native x86_64 host (Linux/macOS/Windows). Set `JET_CORPUS_GATE_REPORT_DIR`
/// to write the canonical report bundle.
#[test]
fn example_corpus_strict_jit_aot_differential_gate() {
    let started = std::time::Instant::now();
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let filter = std::env::var("JET_CORPUS_GATE_FILTER").ok();
    let records = collect_corpus_gate_records();
    if std::env::var("JET_DUMP_CORPUS_GATE").as_deref() == Ok("1") {
        print_corpus_gate_manifest(&records);
        if std::env::var("JET_WRITE_CORPUS_GATE").as_deref() == Ok("1") {
            let manifest = corpus_gate_manifest_from_records(&records);
            fs::write(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/jit_corpus_gate.txt"),
                manifest,
            )
            .expect("write tests/jit_corpus_gate.txt");
        }
        eprintln!(
            "c727 corpus gate: {} examples ({} resident JIT, {} deopt-interp, {} run-tier-broken)",
            records.len(),
            records
                .iter()
                .filter(|r| r.class == CorpusGateClass::ResidentJit)
                .count(),
            records
                .iter()
                .filter(|r| r.class == CorpusGateClass::DeoptInterp)
                .count(),
            records
                .iter()
                .filter(|r| r.class == CorpusGateClass::RunTierBroken)
                .count(),
        );
        return;
    }
    let mut expected = parse_corpus_gate_manifest();
    if let Some(filter) = filter {
        expected.retain(|record| record.stem.contains(&filter));
    } else {
        assert_eq!(
            records.len(),
            all_example_stems().len(),
            "corpus gate must classify every discovered example"
        );
        // #1998: the blob compare below reports the first diverging record, so a
        // manifest row naming an example that does not exist reads exactly like
        // an example whose class changed. That is how the nonexistent stem
        // `tooling/data_line` held a battery down for ten days. Name the two
        // failures apart, and name them before the blob compare runs.
        let discovered: std::collections::HashSet<String> =
            all_example_stems().into_iter().collect();
        let ghosts: Vec<&str> = expected
            .iter()
            .filter(|record| !discovered.contains(&record.stem))
            .map(|record| record.stem.as_str())
            .collect();
        assert!(
            ghosts.is_empty(),
            "tests/jit_corpus_gate.txt names {} stem(s) with no \
             examples/features/<topic>/<name>.jet file: {ghosts:?}. A nonexistent stem is a \
             stale row to delete, never a classification that failed.",
            ghosts.len()
        );
        let listed: std::collections::HashSet<&str> =
            expected.iter().map(|record| record.stem.as_str()).collect();
        let unlisted: Vec<&str> = records
            .iter()
            .map(|record| record.stem.as_str())
            .filter(|stem| !listed.contains(stem))
            .collect();
        assert!(
            unlisted.is_empty(),
            "{} example(s) appear in NO section of tests/jit_corpus_gate.txt, breaking that \
             file's own invariant that every top-level example appears in exactly one: \
             {unlisted:?}. Regenerate the manifest; a missing row is not a class change.",
            unlisted.len()
        );
    }
    // Hard floor: run_tier_broken must stay empty. A regression that moves an
    // AOT-green example into that class must fail even if the manifest is
    // regenerated (D-VERDICT-1254-1 / D-LENS-RUN1).
    let broken: Vec<&str> = records
        .iter()
        .filter(|r| r.class == CorpusGateClass::RunTierBroken)
        .map(|r| r.stem.as_str())
        .collect();
    assert!(
        broken.is_empty(),
        "JIT/AOT run-tier parity regression: AOT-green example(s) fail under default \
         `jet run` (run_tier_broken must stay empty): {}",
        broken.join(", ")
    );
    assert_eq!(
        records, expected,
        "corpus gate manifest drifted; update tests/jit_corpus_gate.txt only for an intentional \
         ratchet move (D-VERDICT-1254-1: run_tier_broken may only shrink). Refresh with \
         JET_DUMP_CORPUS_GATE=1 JET_WRITE_CORPUS_GATE=1 cargo test --test dev \
         example_corpus_strict_jit_aot_differential_gate -- --exact --nocapture"
    );
    let aot_oracle: Vec<_> = records
        .iter()
        .filter(|r| {
            matches!(
                r.class,
                CorpusGateClass::ResidentJit | CorpusGateClass::DeoptInterp
            )
        })
        .collect();
    eprintln!(
        "c727 corpus gate: {} classified, {} AOT-oracle ({} resident JIT, {} deopt-interp), {} run-tier-broken",
        records.len(),
        aot_oracle.len(),
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::ResidentJit)
            .count(),
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::DeoptInterp)
            .count(),
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::RunTierBroken)
            .count(),
    );
    write_corpus_gate_report(&records, started.elapsed());
}
