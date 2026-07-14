//! E4-JP6A / D-JPK-TRUSTROOT1 — trust primitives + root bootstrap acceptance.

use std::collections::BTreeMap;
use std::time::Duration;

use jetpack::SHA256;
use jetpack::TrustRoot::{
    canonical_root, canonical_snapshot, canonical_targets, fixture_threshold_root, sign_root,
    sign_snapshot, sign_targets, sign_timestamp, BoundIdentity, FixedClock, IdentityKind,
    PublisherIdentity, RootBootstrap, SnapshotMetaEntry, SnapshotMetadata, TargetMeta,
    TargetsMetadata, TimestampMetadata, TrustEngine, TrustError, TrustPolicy,
};

#[test]
fn jp6a_bootstrap_threshold_delegation_snapshot_and_identities() {
    let now = 1_710_000_000u64;
    let dir = std::env::temp_dir().join(format!(
        "jetpack-trust-root-it-{}-{}",
        std::process::id(),
        now
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let (root, keyring, keys) = fixture_threshold_root(1, now + 86_400).unwrap();
    let signed_root = sign_root(&root, &[&keys[0], &keys[1]]).unwrap();
    let pin = SHA256::sha256_hex(canonical_root(&root).as_bytes());

    let mut eng = TrustEngine::bootstrap(
        &signed_root,
        keyring,
        TrustPolicy {
            trusted_unix: Some(now),
            max_clock_skew: Duration::from_secs(60),
            ..TrustPolicy::default()
        },
        Some(&pin),
        &FixedClock(now),
        Some(&dir),
    )
    .unwrap();

    let loaded = RootBootstrap::load(&dir).unwrap();
    assert_eq!(loaded.pin_digest, pin);
    assert!(loaded.consistent_snapshot);

    // Signature strip
    let mut stripped = signed_root.clone();
    stripped.signatures.clear();
    assert!(matches!(
        TrustEngine::bootstrap(
            &stripped,
            eng.keyring.clone(),
            TrustPolicy::default(),
            Some(&pin),
            &FixedClock(now),
            None,
        ),
        Err(TrustError::SignatureStripped { .. })
    ));

    // Threshold-minus-one recovery drill
    let mut rotated = root.clone();
    rotated.version = 2;
    eng.recovery_drill_threshold_minus_one(&rotated, &[&keys[0]], &FixedClock(now))
        .unwrap();

    // Real rotation with 2-of-3
    rotated.roles = eng.root.roles.clone();
    rotated.public_key_ids = eng.root.public_key_ids.clone();
    rotated.delegations = eng.root.delegations.clone();
    let signed_rot = sign_root(&rotated, &[&keys[1], &keys[2]]).unwrap();
    eng.update_root(&signed_rot, &FixedClock(now)).unwrap();

    // Targets + delegation + consistent snapshot chain
    let targets = TargetsMetadata {
        version: 3,
        expires_unix: now + 3600,
        targets: BTreeMap::from([(
            "jetsrc/pkg".into(),
            TargetMeta {
                length: 3,
                hashes: BTreeMap::from([("sha256".into(), SHA256::sha256_hex(b"pkg"))]),
                custom: BTreeMap::new(),
            },
        )]),
    };
    let signed_t = sign_targets(&targets, &[&keys[3]]).unwrap();
    eng.verify_targets(&signed_t, Some("jetsrc/pkg"), &FixedClock(now))
        .unwrap();
    assert!(matches!(
        eng.verify_targets(&signed_t, Some("other/pkg"), &FixedClock(now)),
        Err(TrustError::DelegationDenied { .. })
    ));

    let t_canon = canonical_targets(&targets);
    let snap = SnapshotMetadata {
        version: 4,
        expires_unix: now + 3600,
        meta: BTreeMap::from([(
            "targets".into(),
            SnapshotMetaEntry {
                version: 3,
                length: t_canon.len() as u64,
                hashes: BTreeMap::from([(
                    "sha256".into(),
                    SHA256::sha256_hex(t_canon.as_bytes()),
                )]),
            },
        )]),
    };
    let signed_s = sign_snapshot(&snap, &[&keys[4]]).unwrap();
    eng.verify_snapshot(&signed_s, &t_canon, 3, &FixedClock(now))
        .unwrap();
    let s_canon = canonical_snapshot(&snap);
    let ts = TimestampMetadata {
        version: 5,
        expires_unix: now + 3600,
        snapshot: SnapshotMetaEntry {
            version: 4,
            length: s_canon.len() as u64,
            hashes: BTreeMap::from([("sha256".into(), SHA256::sha256_hex(s_canon.as_bytes()))]),
        },
    };
    eng.verify_timestamp(
        &sign_timestamp(&ts, &[&keys[5]]).unwrap(),
        &s_canon,
        4,
        &FixedClock(now),
    )
    .unwrap();

    // Distinct identity domains + hybrid publisher proofs
    eng.bind_identity(BoundIdentity::registry("registry.jet.dev"))
        .unwrap();
    eng.bind_identity(BoundIdentity::cache_builder("builder.ci"))
        .unwrap();
    eng.bind_identity(BoundIdentity::remote_executor("exec.1"))
        .unwrap();
    eng.bind_identity(BoundIdentity::publisher(
        "pub.alice",
        PublisherIdentity::OfflineEd25519 {
            public_key_hex: "ab".repeat(32),
            key_id: "alice-ed25519".into(),
        },
    ))
    .unwrap();
    assert_eq!(
        eng.identities.get(&IdentityKind::Publisher).unwrap().name,
        "pub.alice"
    );
    assert!(matches!(
        eng.bind_identity(BoundIdentity::registry("other.registry")),
        Err(TrustError::IdentityKindMismatch { .. })
    ));

    let _ = std::fs::remove_dir_all(&dir);
}
