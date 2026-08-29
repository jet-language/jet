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

/// #2286: generated Core witnesses use the same strict resident-JIT,
/// interpreter, and AOT oracle as the feature corpus. Discovery is a sorted
/// filesystem walk, so adding a witness cannot silently leave it outside the
/// gate. The denominator check is a separate Node command because this wave
/// intentionally lands with uncovered rows for later coverage waves.
#[test]
fn core_conformance_corpus_uses_strict_three_tier_gate() {
    with_jit_test_scope(|| {
        if skip_if_cranelift_host_unsupported() {
            return;
        }
        let entries = core_conformance_corpus_entries();
        assert!(!entries.is_empty(), "Core conformance corpus must have witnesses");
        for (stem, file) in entries {
            assert_cranelift_three_way(&file, &stem);
        }
    });
}

/// #2286 criterion 3: the generator's structural guard must reject the
/// bind-and-discard shape that previously made `uuid.v4` look covered.
#[test]
fn core_conformance_checker_rejects_unconsumed_result() {
    let output = std::process::Command::new("node")
        .arg("scripts/agent/core-conformance.mjs")
        .arg("--hostile-fixtures")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("node must run the Core conformance checker");
    assert!(
        output.status.success(),
        "Core conformance hostile fixture was accepted:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("rejected bind-and-discard result"),
        "hostile fixture output must state the result-consumption guard"
    );
}

/// c727 C1–C4: discover every top-level example, classify it, and ratchet the
/// manifest. AOT-oracle examples (exit 0) must resident-JIT or deopt-interp
/// with backend attribution — never silent fallback. Each AOT-oracle case
/// compares default tiered against optimized AOT stdout/stderr/exit
/// byte-for-byte, and compares the pure interpreter too WHEN the TIR evaluator
/// runs the program. A pure-interpreter refusal carrying E2201/E0956 is accepted
/// and records `interpreter_refused: CODE` on the observed backend row. A
/// `resident_jit` row proves AOT==tiered-JIT plus resident Cranelift execution.
/// It proves interpreter parity only without that marker. Every accepted refusal
/// is TIR coverage owed against D-ONECORE1=A/I9, not a settled boundary. Do not
/// cite a marked `resident_jit` row as three-tier parity. An AOT-green example whose
/// default tiered run REFUSES to run it lands in `run_tier_broken`, which must
/// hold exactly `RUN_TIER_BROKEN_HELD_OUT`; one that runs but disagrees with the
/// oracle lands in `tier_divergent`, which must stay empty. Recording both facts
/// in one section made the run-tier message false for the divergences
/// (D-VERDICT-1254-1 / D-LENS-RUN1). An example whose AOT oracle failed to build
/// or exited non-zero gets no comparison at all and lands in `aot_broken`, which
/// must hold exactly `AOT_BROKEN_HELD_OUT`: that fact used to be filed under
/// `expected_exit`, so seven stems with a broken oracle sat in a benign section
/// and were dropped from the differential (#2016).
/// parity: guard tests/dev_corpus_gate.rs::example_corpus_strict_jit_aot_differential_gate
///
/// c730: CI runs this via `tools/ci/jit-aot-parity.sh` on every supported
/// native x86_64 host (Linux/macOS/Windows). Set the shard index/count to run
/// one weighted corpus slice; each slice writes its report, and
/// `tools/ci/compose-corpus-gate.sh` composes the ledger. Set
/// `JET_CORPUS_GATE_REPORT_DIR` to write the report bundle.
#[test]
fn example_corpus_strict_jit_aot_differential_gate() {
    let started = std::time::Instant::now();
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    let shard = corpus_gate_shard_config();
    let filter = std::env::var("JET_CORPUS_GATE_FILTER").ok();
    let selected_stems = corpus_gate_selected_stems();
    let records = collect_corpus_gate_records();
    let covered_stems: Vec<String> = records.iter().map(|record| record.stem.clone()).collect();
    assert_eq!(
        covered_stems, selected_stems,
        "corpus gate shard did not classify its exact selected stem set"
    );
    if std::env::var("JET_DUMP_CORPUS_GATE").as_deref() == Ok("1") {
        print_corpus_gate_manifest(&records);
        if shard.is_some() {
            assert!(
                std::env::var_os("JET_CORPUS_GATE_REPORT_DIR").is_some(),
                "a sharded corpus dump needs JET_CORPUS_GATE_REPORT_DIR; compose shard reports instead of overwriting the canonical ledger"
            );
        } else if std::env::var("JET_WRITE_CORPUS_GATE").as_deref() == Ok("1") {
            let manifest = corpus_gate_manifest_from_records(&records);
            fs::write(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/jit_corpus_gate.txt"),
                manifest,
            )
            .expect("write tests/jit_corpus_gate.txt");
        }
        write_corpus_gate_report(&records, started.elapsed());
        eprintln!(
            "c727 corpus gate: shard {}/{} classified {} examples ({} resident JIT, {} deopt-interp, {} run-tier-broken, {} tier-divergent)",
            shard.map_or(0, |(index, _)| index),
            shard.map_or(1, |(_, count)| count),
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
            records
                .iter()
                .filter(|r| r.class == CorpusGateClass::TierDivergent)
                .count(),
        );
        return;
    }
    let mut expected = parse_corpus_gate_manifest();
    if filter.is_some() || shard.is_some() {
        let selected: std::collections::HashSet<&str> =
            selected_stems.iter().map(String::as_str).collect();
        expected.retain(|record| selected.contains(record.stem.as_str()));
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
    // Hard floor, independent of the ledger: the run-tier class holds exactly the
    // stems named in `RUN_TIER_BROKEN_HELD_OUT` and nothing else, and the
    // broken-oracle class holds exactly `AOT_BROKEN_HELD_OUT`. A regression that
    // moves an AOT-green example into the run-tier class, or breaks another
    // stem's oracle, fails here even if the manifest is regenerated, and a fixed
    // hold-out fails too, so neither list can outlive its defect
    // (D-VERDICT-1254-1 / D-LENS-RUN1, #2016).
    let broken: Vec<(&str, &str)> = records
        .iter()
        .filter(|r| r.class == CorpusGateClass::RunTierBroken)
        .map(|r| (r.stem.as_str(), r.detail.as_str()))
        .collect();
    // One filter for both compares: a hold-out this run never put an oracle
    // behind is not evidence of a fix. `JET_CORPUS_GATE_FILTER` narrows which
    // stems were classified at all, and a host without rustc classifies every
    // stem `oracle_unavailable` without building anything.
    let oracle_reached: std::collections::HashSet<&str> = records
        .iter()
        .filter(|r| r.class != CorpusGateClass::OracleUnavailable)
        .map(|r| r.stem.as_str())
        .collect();
    let held_out: Vec<(&str, &str)> = RUN_TIER_BROKEN_HELD_OUT
        .iter()
        .filter(|(stem, _, _)| oracle_reached.contains(stem))
        .map(|(stem, codes, _)| (*stem, *codes))
        .collect();
    assert_eq!(
        broken,
        held_out,
        "JIT/AOT run-tier regression: the AOT-green example(s) whose default `jet run` REFUSES \
         to run them are not the held-out set. New entries are the defect; a vanished entry is \
         fixed and must leave RUN_TIER_BROKEN_HELD_OUT in the same diff. Held out: {}",
        RUN_TIER_BROKEN_HELD_OUT
            .iter()
            .map(|(stem, codes, why)| format!("{stem} ({codes}) — {why}"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    // #2016: the section that used to hide inside `expected_exit`. A stem whose
    // AOT oracle failed to build or exited non-zero gets NO tier comparison at
    // all, so every row here is a hole in the differential and the set may only
    // shrink. A build failure is also an I2 internal compiler error: rustc
    // rejecting generated code is never a user-facing outcome.
    let aot_broken: Vec<(&str, &str)> = records
        .iter()
        .filter(|r| r.class == CorpusGateClass::AotBroken)
        .map(|r| (r.stem.as_str(), r.detail.as_str()))
        .collect();
    let aot_held_out: Vec<(&str, &str)> = AOT_BROKEN_HELD_OUT
        .iter()
        .filter(|(stem, _, _)| oracle_reached.contains(stem))
        .map(|(stem, detail, _)| (*stem, *detail))
        .collect();
    assert_eq!(
        aot_broken,
        aot_held_out,
        "AOT oracle regression: the example(s) whose optimized AOT build or run FAILED are not \
         the held-out set, so the three-tier differential silently covers a different set than \
         the one this gate claims. New entries are the defect — a failed AOT build of generated \
         code is an I2 internal compiler error; a vanished entry is fixed and must leave \
         AOT_BROKEN_HELD_OUT in the same diff. Held out: {}",
        AOT_BROKEN_HELD_OUT
            .iter()
            .map(|(stem, detail, why)| format!("{stem} ({detail}) — {why}"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    // The other half of the same law, and the reason it used to read false: a
    // stem that RAN under both tiers with different observables is a divergence,
    // not a refusal. It stays empty — default tiered output must equal optimized
    // AOT output byte for byte (D-ONECORE1=A), the same law
    // `jit_coverage_audit` states and enforces.
    let divergent: Vec<String> = records
        .iter()
        .filter(|r| r.class == CorpusGateClass::TierDivergent)
        .map(|r| format!("{} ({})", r.stem, r.detail))
        .collect();
    assert!(
        divergent.is_empty(),
        "JIT/AOT tier divergence: AOT-green example(s) also ran under default `jet run`, but the \
         named stream(s) disagree with the optimized AOT oracle (tier_divergent must stay \
         empty): {}",
        divergent.join(", ")
    );
    assert_eq!(
        records, expected,
        "corpus gate manifest drifted; update tests/jit_corpus_gate.txt only for an intentional \
         ratchet move (D-VERDICT-1254-1: run_tier_broken may only shrink). Refresh with \
         JET_CORPUS_GATE_REPORT_DIR=jit-aot-parity-report JET_WRITE_CORPUS_GATE=1 \
         bash tools/ci/jit-aot-parity.sh"
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
        "c727 corpus gate: {} classified, {} AOT-oracle ({} resident JIT, {} deopt-interp), {} run-tier-broken, {} tier-divergent, {} aot-broken",
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
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::TierDivergent)
            .count(),
        records
            .iter()
            .filter(|r| r.class == CorpusGateClass::AotBroken)
            .count(),
    );
    write_corpus_gate_report(&records, started.elapsed());
}
