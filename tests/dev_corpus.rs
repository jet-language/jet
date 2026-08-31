//! The shared `collect_jit_coverage` corpus observation and its ratchets (#2020).
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/corpus.rs");

#[test]
fn semantic_corpus_policy_runs_with_the_dev_corpus_gate() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("dev-corpus")
        .expect("dev corpus semantic policy");
}
