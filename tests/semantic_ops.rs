//! D-DEVR-SEMID1=A: semantic operation producers and blame consumers.

use jet_semindex::{open, semantic_blame, semantic_rename_ops, SemanticOp, SemanticOpTarget};
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("jet_semantic_ops_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    (root.clone(), root.join("run.jet"))
}

#[test]
fn tool_fix_pairing_records_rename_and_blame_reads_the_receipt() {
    let (root, path) = fixture("rename");
    let before = "fn report() Int -[]> { return 1 }\nfn run() { print(report()) }\n";
    let after = "fn summarize() Int -[]> { return 1 }\nfn run() { print(summarize()) }\n";
    fs::write(&path, before).unwrap();
    let before_index = open(&path).expect("before index");
    fs::write(&path, after).unwrap();
    let after_index = open(&path).expect("after index");

    let produced = semantic_rename_ops(&before_index, &after_index);
    assert_eq!(produced.len(), 1);
    assert_eq!(produced[0].kind, "rename");
    assert_eq!(produced[0].from.as_deref(), Some("report"));
    assert_eq!(produced[0].to.as_deref(), Some("summarize"));
    assert!(!produced[0].targets[0].stable_id.is_empty());

    let rows = semantic_blame(&after_index, &produced);
    let renamed = rows
        .iter()
        .find(|row| row.identity.ends_with("summarize"))
        .expect("renamed definition");
    assert_eq!(
        renamed.operation.as_ref().and_then(|op| op.from.as_deref()),
        Some("report")
    );

    let hand_edit_rows = semantic_blame(&after_index, &[]);
    assert!(hand_edit_rows.iter().all(|row| row.operation.is_none()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn blame_matches_recorded_targets_without_text_inference() {
    let (root, path) = fixture("receipt");
    let source = "fn summarize() Int -[]> { return 1 }\nfn run() { print(summarize()) }\n";
    fs::write(&path, source).unwrap();
    let index = open(&path).expect("index");
    let op = SemanticOp {
        kind: "rename".to_string(),
        rule_id: Some("test".to_string()),
        from: Some("report".to_string()),
        to: Some("summarize".to_string()),
        node: None,
        match_template: None,
        replace_template: None,
        targets: vec![SemanticOpTarget {
            stable_id: "def:before".to_string(),
            before: "report".to_string(),
            after: "summarize".to_string(),
            kind: "function".to_string(),
            module_path: "run".to_string(),
        }],
        files: Vec::new(),
    };
    let rows = semantic_blame(&index, &[op]);
    assert!(rows
        .iter()
        .find(|row| row.identity.ends_with("summarize"))
        .and_then(|row| row.operation.as_ref())
        .is_some_and(|operation| operation.kind == "rename"));
    let _ = fs::remove_dir_all(root);
}
