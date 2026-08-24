use jet_foundation::MemSentry::{jet_sentry_check, jet_sentry_reset, jet_sentry_scope};

#[test]
fn dev_sentry_faults_feed_the_shared_memory_ledger_without_engine_policy() {
    let root = std::env::temp_dir().join(format!("jet_sentry_ledger_{}", std::process::id()));
    let ledger = root.join("ledger.jsonl");
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("JET_MEMORY_LEDGER", &ledger);

    jet_sentry_reset();
    let gate = jet_sentry_scope(true, "unsafe.jet", 12, "reads raw storage");
    let fault = jet_sentry_check(1, 8, 1, "read", "live allocation").unwrap();
    assert_eq!(fault.code, "R0801");
    assert_eq!(fault.file, "unsafe.jet");
    drop(gate);
    jet_sentry_reset();
    std::env::remove_var("JET_MEMORY_LEDGER");

    let witness = std::fs::read_to_string(&ledger).unwrap();
    assert!(witness.contains("\"schema\":\"jet.memory.ledger\""));
    assert!(witness.contains("\"kind\":\"sentry\""));
    assert!(witness.contains("\"code\":\"R0801\""));
    assert!(witness.contains("\"source\":\"unsafe.jet\""));
    assert!(witness.contains("\"provenance\":\"source #Unsafe gate\""));
    assert!(witness.contains("derive the pointer from live storage inside this gate"));

    let _ = std::fs::remove_dir_all(root);
}
