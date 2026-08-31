//! E4-JP6A / D-JPK-TRUSTROOT1 — trust primitives + root bootstrap acceptance.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use jetpack::TrustRoot::{
    allow_cache_witness, canonical_root, canonical_snapshot, canonical_targets,
    fixture_threshold_root, is_cache_witness_allowed, sign_root, sign_snapshot, sign_targets,
    sign_timestamp, BoundIdentity, CacheProvenance, CacheReceipt, FixedClock, IdentityKind,
    public_trust_manifest, PublisherIdentity, PublicTrustKey, RootBootstrap, SnapshotMetaEntry,
    SnapshotMetadata, TargetMeta, TargetsMetadata, TimestampMetadata, TrustEngine, TrustError,
    TrustKey, TrustPolicy,
};
use jetpack::SHA256;

#[test]
fn trust_publication_contract_is_owned_pending_and_has_no_fake_root() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let publication = repo.join("site/dist/keys");
    let manifest = fs::read_to_string(publication.join("trust-manifest.json")).unwrap();
    assert_eq!(
        manifest.trim(),
        "{\"schema\":1,\"domain\":\"jet-lang.dev\",\"status\":\"awaiting-key-ceremony\",\"root\":null,\"keys\":[],\"rotation\":\"offline threshold root rotation; publish a new manifest only after the old root verifies the new root\"}"
    );
    assert!(!publication.join("nix-index-v1.ed25519.pub").exists());

    let readme = fs::read_to_string(publication.join("README.md")).unwrap();
    let docs = fs::read_to_string(repo.join("docs/infra/trust-root.md")).unwrap();
    for text in [
        "https://keys.jet-lang.dev/nix-index-v1.ed25519.pub",
        "key-id:base64-public-key",
        "ed25519",
        "issued_unix",
        "expires_unix",
        "manual update",
    ] {
        assert!(
            readme.contains(text) || docs.contains(text),
            "missing contract text: {text}"
        );
    }
    assert!(!manifest.contains("jet-test-index-v1"));
}

#[test]
fn trust_publication_exporter_rejects_the_test_index_key() {
    let value = format!("jet-test-index-v1:{}", "A".repeat(43) + "=");
    let error = PublicTrustKey::from_nix_line("index", &value).unwrap_err();
    assert!(matches!(
        error,
        TrustError::InvalidKey { detail }
            if detail == "test public trust key `jet-test-index-v1` cannot be published"
    ));

    let key = PublicTrustKey::from_nix_line(
        "index",
        "official-index-v1:11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=",
    )
    .unwrap();
    let manifest = public_trust_manifest("jet-lang.dev", None, &[key]).unwrap();
    assert!(manifest.contains("\"key_id\":\"official-index-v1\""));
    assert!(manifest.contains("\"algorithm\":\"ed25519\""));
    assert!(manifest.contains("nix-index-v1.ed25519.pub"));
}

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
                hashes: BTreeMap::from([("sha256".into(), SHA256::sha256_hex(t_canon.as_bytes()))]),
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
    eng.bind_identity(BoundIdentity::registry("registry.jet-lang.dev"))
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

#[test]
fn jp6b_compromise_rollback_freeze_and_mix_and_match_fail_closed() {
    let now = 1_710_100_000u64;
    // The cache-witness assertions below need a scratch root; follow this
    // file's existing convention. `temp_dir()` honours TMPDIR, which the agent
    // scripts point at disk because /tmp is RAM-backed on this host.
    let dir = std::env::temp_dir().join(format!(
        "jetpack-trust-root-jp6b-{}-{}",
        std::process::id(),
        now
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (root, keyring, keys) = fixture_threshold_root(1, now + 86_400).unwrap();
    let signed_root = sign_root(&root, &[&keys[0], &keys[1]]).unwrap();
    let pin = SHA256::sha256_hex(canonical_root(&root).as_bytes());
    let bootstrap = || {
        TrustEngine::bootstrap(
            &signed_root,
            keyring.clone(),
            TrustPolicy::default(),
            Some(&pin),
            &FixedClock(now),
            None,
        )
        .unwrap()
    };

    // A stolen or unknown root signer cannot replace the threshold.
    let compromised = TrustKey::generate("compromised-root");
    let forged_root = sign_root(&root, &[&keys[0], &compromised]).unwrap();
    assert!(matches!(
        TrustEngine::bootstrap(
            &forged_root,
            keyring.clone(),
            TrustPolicy::default(),
            Some(&pin),
            &FixedClock(now),
            None,
        ),
        Err(TrustError::ThresholdUnmet {
            role: jetpack::TrustRoot::MetadataRole::Root,
            have: 1,
            need: 2
        })
    ));
    let mut replacement = root.clone();
    replacement.version = 2;
    let replacement_root = sign_root(&replacement, &[&keys[0], &keys[1]]).unwrap();
    assert!(matches!(
        TrustEngine::bootstrap(
            &replacement_root,
            keyring.clone(),
            TrustPolicy::default(),
            Some(&pin),
            &FixedClock(now),
            None,
        ),
        Err(TrustError::BootstrapPinMismatch { .. })
    ));

    let mut targets = TargetsMetadata {
        version: 1,
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
    let signed_targets = sign_targets(&targets, &[&keys[3]]).unwrap();
    let mut engine = bootstrap();
    engine
        .verify_targets(&signed_targets, Some("jetsrc/pkg"), &FixedClock(now))
        .unwrap();

    // A signed older version is still a rollback and cannot become usable.
    targets.version = 2;
    let signed_v2 = sign_targets(&targets, &[&keys[3]]).unwrap();
    engine
        .verify_targets(&signed_v2, Some("jetsrc/pkg"), &FixedClock(now))
        .unwrap();
    let signed_old = sign_targets(
        &TargetsMetadata {
            version: 1,
            ..targets.clone()
        },
        &[&keys[3]],
    )
    .unwrap();
    assert!(matches!(
        engine.verify_targets(&signed_old, Some("jetsrc/pkg"), &FixedClock(now)),
        Err(TrustError::Rollback {
            role: jetpack::TrustRoot::MetadataRole::Targets,
            ..
        })
    ));

    // Exact expiry is frozen, not a one-second grace period.
    let frozen = TargetsMetadata {
        version: 3,
        expires_unix: now,
        ..targets.clone()
    };
    assert!(matches!(
        bootstrap().verify_targets(
            &sign_targets(&frozen, &[&keys[3]]).unwrap(),
            Some("jetsrc/pkg"),
            &FixedClock(now),
        ),
        Err(TrustError::Expired {
            role: jetpack::TrustRoot::MetadataRole::Targets,
            ..
        })
    ));

    // Custom and non-SHA metadata are part of the signed target record.
    let mut changed_custom = signed_targets.clone();
    changed_custom
        .signed
        .targets
        .get_mut("jetsrc/pkg")
        .unwrap()
        .custom
        .insert("provenance".into(), "attacker".into());
    changed_custom.signatures = sign_targets(&changed_custom.signed, &[&keys[3]])
        .unwrap()
        .signatures;
    let mut custom_engine = bootstrap();
    custom_engine
        .verify_targets(&signed_targets, Some("jetsrc/pkg"), &FixedClock(now))
        .unwrap();
    assert!(matches!(
        custom_engine.verify_targets(&changed_custom, Some("jetsrc/pkg"), &FixedClock(now)),
        Err(TrustError::ConsistentSnapshotMismatch { .. })
    ));

    // A timestamp may not point at a snapshot with a different byte length,
    // even when its hash and version are otherwise copied from the chain.
    let mut chain = bootstrap();
    chain
        .verify_targets(&signed_targets, Some("jetsrc/pkg"), &FixedClock(now))
        .unwrap();
    let targets_canonical = canonical_targets(&signed_targets.signed);
    let snapshot = SnapshotMetadata {
        version: 1,
        expires_unix: now + 3600,
        meta: BTreeMap::from([(
            "targets".into(),
            SnapshotMetaEntry {
                version: 1,
                length: targets_canonical.len() as u64,
                hashes: BTreeMap::from([(
                    "sha256".into(),
                    SHA256::sha256_hex(targets_canonical.as_bytes()),
                )]),
            },
        )]),
    };
    let signed_snapshot = sign_snapshot(&snapshot, &[&keys[4]]).unwrap();
    chain
        .verify_snapshot(&signed_snapshot, &targets_canonical, 1, &FixedClock(now))
        .unwrap();
    let snapshot_canonical = canonical_snapshot(&snapshot);
    let timestamp = TimestampMetadata {
        version: 1,
        expires_unix: now + 3600,
        snapshot: SnapshotMetaEntry {
            version: 1,
            length: snapshot_canonical.len() as u64 + 1,
            hashes: BTreeMap::from([(
                "sha256".into(),
                SHA256::sha256_hex(snapshot_canonical.as_bytes()),
            )]),
        },
    };
    assert!(matches!(
        chain.verify_timestamp(
            &sign_timestamp(&timestamp, &[&keys[5]]).unwrap(),
            &snapshot_canonical,
            1,
            &FixedClock(now),
        ),
        Err(TrustError::ConsistentSnapshotMismatch { .. })
    ));

    // Cache receipts expose the trust decision and fail closed on tamper or
    // the exact expiry boundary.
    let cache_key = TrustKey::generate("cache-receipt");
    let provenance = CacheProvenance {
        reference: "jetsrc/pkg".into(),
        source: "git:source".into(),
        builder: "cache-builder:trusted".into(),
        action: "sha256:action".into(),
        output: "sha256:output".into(),
        platform: "linux.x86_64".into(),
        sandbox: "sandbox:policy-bound".into(),
        policy: "sha256:policy".into(),
    };
    let receipt = CacheReceipt::issue("public", provenance, 1, now, now + 60, &cache_key).unwrap();
    receipt.verify(&cache_key, &FixedClock(now)).unwrap();
    assert!(!receipt.witness.is_empty());
    allow_cache_witness(&dir, "public", &receipt.witness).unwrap();
    assert!(is_cache_witness_allowed(&dir, "public", &receipt.witness).unwrap());
    let untrusted_witness = if receipt.witness == "stranger" {
        "other-stranger"
    } else {
        "stranger"
    };
    assert!(!is_cache_witness_allowed(&dir, "public", untrusted_witness).unwrap());
    let mut tampered_witness = receipt.clone();
    tampered_witness.witness = untrusted_witness.into();
    assert!(matches!(
        tampered_witness.verify(&cache_key, &FixedClock(now)),
        Err(TrustError::CacheReceiptInvalid { .. })
    ));
    let mut tampered_receipt = receipt.clone();
    tampered_receipt.provenance.output = "sha256:attacker".into();
    assert!(matches!(
        tampered_receipt.verify(&cache_key, &FixedClock(now)),
        Err(TrustError::CacheReceiptInvalid { .. })
    ));
    assert!(matches!(
        receipt.verify(&cache_key, &FixedClock(now + 60)),
        Err(TrustError::CacheReceiptExpired { .. })
    ));
}

#[test]
fn core_build_hook_requires_the_exact_trust_identity() {
    let dir = std::env::temp_dir().join(format!(
        "jetpack-core-build-trust-it-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = dir.join("trust");
    jetpack::Trust::add_grant(
        &store,
        &jetpack::Trust::TrustGrant {
            authority: "build".to_string(),
            subject: "build-sha256:trusted".to_string(),
            scope: "repo".to_string(),
        },
    );
    let hostile_identity = "build-sha256:trusted\n$(touch core-build-pwned)";
    let theme = jetpack::Output::Theme::resolve(true);

    assert!(jetpack::Trust::gate_build_identity(
        &theme,
        &store,
        hostile_identity,
        false,
    )
    .is_err());
    assert!(!dir.join("core-build-pwned").exists());
    std::fs::remove_dir_all(&dir).ok();
}
