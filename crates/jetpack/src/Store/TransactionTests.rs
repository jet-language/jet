use super::*;
use crate::Store::{
    closure_graph, list_checked, recover_hangar, CacheIdentity, IngestRequest, Roots,
};
use std::collections::BTreeMap;
use std::fs;

struct RootGuard {
    roots: Roots,
}

impl RootGuard {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jet-admission-{tag}-{}-{}",
            std::process::id(),
            TRANSACTION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self {
            roots: Roots {
                root: path,
                dev_mode: true,
            },
        }
    }
}

impl Drop for RootGuard {
    fn drop(&mut self) {
        let _ = make_tree_writable_for_removal(&self.roots.root);
        let _ = fs::remove_dir_all(&self.roots.root);
    }
}

fn identity() -> CacheIdentity {
    CacheIdentity {
        source_fingerprint: "transaction-source".into(),
        recipe_fingerprint: "transaction-recipe".into(),
        policy_fingerprint: "transaction-policy".into(),
        platform: crate::Envelope::host_platform(),
    }
}

fn request(roots: &Roots, name: &str) -> IngestRequest {
    let source = roots.root.join(format!("source-{name}"));
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), name.as_bytes()).unwrap();
    IngestRequest {
        name: name.into(),
        version: "1".into(),
        reference: format!("path:{name}"),
        cache_identity: identity(),
        references: Vec::new(),
        outputs: BTreeMap::from([(String::from("out"), source)]),
        signature: String::new(),
        provenance: "admission-transaction-test".into(),
        platform_artifact_kind: String::new(),
    }
}

fn assert_no_committed_package(roots: &Roots) {
    assert!(list_checked(roots).unwrap().is_empty());
    assert!(closure_graph(roots).unwrap().records.is_empty());
    assert_eq!(
        fs::read_dir(roots.hangar_dir().join(OBJECTS_DIR))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn admission_failure_after_object_publication_recovers_without_live_orphans() {
    let guard = RootGuard::new("object");
    let request = request(&guard.roots, "object");
    let result = with_admission_failure(AdmissionFailurePoint::AfterObjectPublication, || {
        crate::Store::ingest_tree(&guard.roots, &request)
    });
    assert!(result.is_err());

    recover_hangar(&guard.roots).unwrap();
    recover_hangar(&guard.roots).unwrap();
    assert_no_committed_package(&guard.roots);
}

#[test]
fn admission_failure_after_receipt_publication_recovers_without_half_registration() {
    let guard = RootGuard::new("receipt");
    let request = request(&guard.roots, "receipt");
    let result = with_admission_failure(AdmissionFailurePoint::AfterReceiptPublication, || {
        crate::Store::ingest_tree(&guard.roots, &request)
    });
    assert!(result.is_err());

    recover_hangar(&guard.roots).unwrap();
    recover_hangar(&guard.roots).unwrap();
    assert_no_committed_package(&guard.roots);
}

#[test]
fn admission_failure_after_closure_registration_recovers_projection_idempotently() {
    let guard = RootGuard::new("closure");
    let request = request(&guard.roots, "closure");
    let result = with_admission_failure(AdmissionFailurePoint::AfterClosureRegistration, || {
        crate::Store::ingest_tree(&guard.roots, &request)
    });
    assert!(result.is_err());

    let committed = list_checked(&guard.roots).unwrap();
    assert_eq!(committed.len(), 1);
    let meta = guard
        .roots
        .hangar_dir()
        .join(&committed[0].id)
        .join("meta.json");
    fs::remove_file(&meta).unwrap();

    recover_hangar(&guard.roots).unwrap();
    let recovered = list_checked(&guard.roots).unwrap();
    assert_eq!(recovered, committed);
    recover_hangar(&guard.roots).unwrap();
    assert_eq!(list_checked(&guard.roots).unwrap(), recovered);
    assert_eq!(closure_graph(&guard.roots).unwrap().records.len(), 1);
}
