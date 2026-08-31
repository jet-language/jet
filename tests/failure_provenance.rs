//! D-FAILURE-FOUNDATION1: one provenance-bearing report teaches a repeated
//! failure-domain mismatch and its two reviewable repairs.

const WITNESS: &str = r#"
#Error
enum WrongDomain { One }

#Error
enum CallerDomain { One }

fn wrong_helper() Int !WrongDomain -> {
    return Err(WrongDomain.One)
}

fn caller() Int !CallerDomain -> {
    wrong_helper()
    wrong_helper()
    wrong_helper()
    wrong_helper()
    wrong_helper()
    return Ok(0)
}

fn run() {}
"#;

#[test]
fn repeated_calls_share_one_named_failure_domain_root_and_repair() {
    let diagnostics = jet::compile(WITNESS)
        .expect_err("the witness must have a failure-domain mismatch");
    let reports = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E2404")
        .collect::<Vec<_>>();
    assert_eq!(reports.len(), 1, "one helper must produce one root report: {diagnostics:#?}");

    let report = reports[0];
    assert!(report.what.contains("wrong_helper"), "{report:#?}");
    assert!(report.why.contains("wrong_helper"), "{report:#?}");
    assert!(report.why.contains("input.jet:"), "{report:#?}");
    assert!(
        report.why.contains("explicit !WrongDomain"),
        "{report:#?}"
    );
    assert!(
        report
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("failure-domain:wrong_helper|WrongDomain|CallerDomain")),
        "{report:#?}"
    );

    let caller_edit = report.edit.clone().expect("caller-domain repair");
    assert_eq!(caller_edit.new_text, "Int !WrongDomain");
    let fixed = jet::LSP::apply_edit(WITNESS, &caller_edit);
    assert!(
        jet::compile(&fixed).is_ok(),
        "the named helper repair must clear every call site"
    );

    let fixes = jet::LSP::fixes_from_diagnostics(vec![report.clone()]);
    assert_eq!(fixes.len(), 2, "both E2404 repairs must reach LSP fixes");
    assert!(fixes.iter().all(|fix| {
        fix.applicability == jet_foundation::Diagnostics::FixApplicability::Suggested
            && fix.safety == jet_foundation::Diagnostics::FixSafety::NeedsReview
    }));
    assert!(
        fixes
            .iter()
            .any(|fix| fix.edit.new_text.contains("impl WrongDomain -> CallerDomain")),
        "missing conversion repair: {fixes:#?}"
    );
}
