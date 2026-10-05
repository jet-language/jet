//! The strict JIT<->AOT differential example-corpus gate (#2020).
//!
//! parity: guard tests/dev_corpus_gate.rs::example_corpus_strict_jit_aot_differential_gate
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/corpus_gate.rs");

const TOWER_DYN_POLICY_SITE: &str = "dogfood/tower/run.jet";

#[test]
fn tower_dyn_policy_keeps_mechanical_ops_and_named_exceptions() {
    let policy = common::corpus_policy::CorpusPolicy::load().expect("corpus manifest");
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join(TOWER_DYN_POLICY_SITE))
        .expect("Tower recipe source is readable");
    let violations = policy
        .evaluate_source(TOWER_DYN_POLICY_SITE, &source)
        .expect("Tower recipe parses");
    assert!(
        violations.is_empty(),
        "Tower Dyn corpus policy failed:\n{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn tower_dyn_policy_rejects_duplicate_generic_ladders() {
    let policy = common::corpus_policy::CorpusPolicy::load().expect("corpus manifest");
    let violations = policy
        .evaluate_source(
            TOWER_DYN_POLICY_SITE,
            r#"
fn duplicate_equal(left: Dyn, right: Dyn) -> Bool { return true }
fn arrays_semantically_equal(left: [Dyn], right: [Dyn]) -> Bool { return true }
fn truthy(value: Dyn) -> Bool { return true }
fn javascript_truthy_duplicate(value: Dyn) -> Bool { return true }
"#,
        )
        .expect("synthetic Tower source parses");
    assert_eq!(
        violations.len(),
        4,
        "all duplicate generic helpers must be rejected once: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .all(|violation| violation.rule == "dyn-domain-policy"),
        "one occurrence-scoped Dyn rule must own every rejection: {violations:?}"
    );
}

#[test]
fn semantic_corpus_policy_runs_after_dev_domain_checks() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("dev-corpus")
        .expect("dev corpus semantic policy");
}

#[test]
fn semantic_corpus_policy_runs_with_core_conformance() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("conformance")
        .expect("Core conformance semantic policy");
}
