// The strict JIT<->AOT differential corpus gate (#2020).
//
// Own target: it classifies all ~500 stems with an AOT build each, and
// `tools/ci/jit-aot-parity.sh` already runs it as its own invocation.

/// #2013: the ledger accounts for the whole corpus — checked with no run at all.
///
/// Deliberately its own test, ahead of the gate below. The gate needs a Cranelift
/// host and an AOT build per stem and returns green early where the host is
/// unsupported, so `tests/jit_corpus_gate.txt` could — and did — drift to 374
/// rows against a 496-stem corpus without any check firing. This one parses the
/// ledger and walks `examples/features/<topic>/`; it runs everywhere, in
/// milliseconds, and says nothing about which tier runs a stem.
#[test]
fn corpus_gate_manifest_accounts_for_every_example() {
    assert_corpus_gate_manifest_covers_corpus();
}

/// #2013 negative control: the completeness law actually fires.
///
/// The check above passes today only because 122 unclassified stems sit under a
/// declared ceiling, so "it passed" says nothing about whether it can fail. Hand
/// `audit_corpus_gate_ledger` a corpus with a stem no section names, a stem two
/// sections claim, a row with no file, and a held-out row with no reason, and
/// require it to name each one. Without this, the pins would rest on an assertion
/// nobody ever watched fail.
#[test]
fn corpus_gate_ledger_audit_fires_on_a_missing_row() {
    let manifest = vec![
        CorpusGateRecord {
            stem: "basics/hello".to_string(),
            class: CorpusGateClass::ResidentJit,
            detail: String::new(),
        },
        CorpusGateRecord {
            stem: "basics/twice".to_string(),
            class: CorpusGateClass::ResidentJit,
            detail: String::new(),
        },
        CorpusGateRecord {
            stem: "basics/twice".to_string(),
            class: CorpusGateClass::DeoptInterp,
            detail: String::new(),
        },
        CorpusGateRecord {
            stem: "tooling/data_line".to_string(),
            class: CorpusGateClass::ResidentJit,
            detail: String::new(),
        },
        CorpusGateRecord {
            stem: "net/http_server".to_string(),
            class: CorpusGateClass::GateExcluded,
            detail: String::new(),
        },
    ];
    let corpus = vec![
        "basics/hello".to_string(),
        "basics/twice".to_string(),
        "net/http_server".to_string(),
        "tooling/data_plot".to_string(),
    ];

    let audit = audit_corpus_gate_ledger(&manifest, &corpus);

    assert_eq!(
        audit.unclassified,
        vec!["tooling/data_plot".to_string()],
        "a stem in no section must be named, not counted away"
    );
    assert_eq!(
        audit.ghosts,
        vec!["tooling/data_line".to_string()],
        "a row with no file must read as a stale row, never as a class change"
    );
    assert_eq!(audit.duplicated.len(), 1, "{:?}", audit.duplicated);
    assert!(
        audit.duplicated[0].contains("basics/twice")
            && audit.duplicated[0].contains("resident_jit")
            && audit.duplicated[0].contains("deopt_interp"),
        "the duplicate must name both claiming sections: {:?}",
        audit.duplicated
    );
    assert_eq!(audit.reasonless.len(), 1, "{:?}", audit.reasonless);
    assert!(
        audit.reasonless[0].contains("net/http_server")
            && audit.reasonless[0].contains("gate_excluded"),
        "a held-out row with no reason must be named with its section: {:?}",
        audit.reasonless
    );
    // Ghost rows are excluded from the classified count on purpose: a row naming
    // no file classifies nothing, so counting it would let three stale rows pay
    // for three stems that fell out of the ledger.
    assert_eq!(
        audit.classified, 3,
        "only stems that exist count toward the floor"
    );
    assert_eq!(
        audit.classified + audit.unclassified.len(),
        corpus.len(),
        "the identity the two pins rest on must hold on synthetic input too"
    );
    assert_eq!(audit.excluded, 1);
}

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
        // #1998 + #2013: ghosts, duplicates and unlisted stems are ONE law, and
        // it lives beside the pins that make it a ratchet — not a second copy
        // here (AGENTS.md I8). It runs before the blob compare because that
        // compare reports the first diverging record, so a missing or
        // nonexistent row reads exactly like a classification that changed.
        assert_corpus_gate_manifest_covers_corpus();
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
         JET_DUMP_CORPUS_GATE=1 JET_WRITE_CORPUS_GATE=1 cargo test --test dev_corpus_gate \
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
