mod common;
include!("cli_parts/support.rs");
#[path = "cli_parts/bindings.rs"]
mod cli_bindings;

#[test]
fn semantic_corpus_policy_runs_with_cli_binding_fixtures() {
    common::corpus_policy::CorpusPolicy::load()
        .expect("corpus manifest")
        .check_gate("bindgen")
        .expect("CLI binding corpus semantic policy");
}
