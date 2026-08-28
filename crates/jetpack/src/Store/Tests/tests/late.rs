use super::*;

#[test]
fn ingest_installs_valid_secondary_named_output_with_matching_digest() {
    let (roots, _g) = temp_roots();
    let out = roots.root.join("src-primary");
    let dev = roots.root.join("src-dev");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("payload"), "primary").unwrap();
    fs::write(&dev, "development").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("payload", out.join("payload-link")).unwrap();
    let outputs = BTreeMap::from([("out".to_string(), out), ("dev".to_string(), dev)]);
    let ingested = ingest_tree(
        &roots,
        &IngestRequest {
            name: "named-output".into(),
            version: "1".into(),
            reference: "path:named-output".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    let dev_digest = ingested.entry.named_outputs.get("dev").unwrap();
    let installed = roots.hangar_dir().join("objects").join(dev_digest);
    assert_eq!(fs::read_to_string(&installed).unwrap(), "development");
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(Path::new(&ingested.entry.out).join("payload-link")).unwrap(),
        PathBuf::from("payload")
    );
    let actual =
        super::super::super::super::Envelope::try_output_hash_of(&installed.to_string_lossy())
            .unwrap();
    assert_eq!(ingested.entry.named_outputs.get("dev"), Some(&actual));
    verify_hangar_object(&roots, &ingested.entry).unwrap();
}

#[test]
fn ingest_path_independent_digest_ignores_source_dirname() {
    let (roots, _g) = temp_roots();
    let left = roots.root.join("left-name");
    let right = roots.root.join("right-name");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    fs::write(left.join("f"), "x").unwrap();
    fs::write(right.join("f"), "x").unwrap();
    let mut o1 = BTreeMap::new();
    o1.insert("out".to_string(), left);
    let mut o2 = BTreeMap::new();
    o2.insert("out".to_string(), right);
    let a = ingest_tree(
        &roots,
        &IngestRequest {
            name: "a".into(),
            version: "1".into(),
            reference: "path:left".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: o1,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    let b = ingest_tree(
        &roots,
        &IngestRequest {
            name: "b".into(),
            version: "1".into(),
            reference: "path:right".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: o2,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    assert_eq!(a.entry.envelope.output_hash, b.entry.envelope.output_hash);
    assert!(b.deduplicated);
}

#[test]
fn closure_graph_named_outputs_are_independent_and_queries_are_cycle_safe() {
    let (roots, _g) = temp_roots();
    let base = ingest_fixture(&roots, "base", &[("out", "base")], Vec::new());
    let middle = ingest_fixture(
        &roots,
        "middle",
        &[("out", "middle")],
        vec![base.entry.envelope.output_hash.clone()],
    );
    let consumer = ingest_fixture(
        &roots,
        "consumer",
        &[("out", "consumer-out"), ("dev", "consumer-dev")],
        vec![middle.entry.envelope.output_hash.clone()],
    );
    let primary = consumer.entry.envelope.output_hash.clone();
    let dev = consumer.entry.named_outputs.get("dev").unwrap().clone();

    assert_ne!(primary, dev);
    assert_eq!(
        direct_references_of(&roots, &primary).unwrap(),
        vec![middle.entry.envelope.output_hash.clone()]
    );
    assert_eq!(
        direct_references_of(&roots, &dev).unwrap(),
        vec![middle.entry.envelope.output_hash.clone()]
    );
    assert_eq!(
        transitive_references_of(&roots, &dev).unwrap(),
        vec![
            base.entry.envelope.output_hash.clone(),
            middle.entry.envelope.output_hash.clone(),
        ]
    );
    let mut expected_closure = vec![
        base.entry.envelope.output_hash.clone(),
        dev.clone(),
        middle.entry.envelope.output_hash.clone(),
    ];
    expected_closure.sort();
    assert_eq!(closure_of(&roots, &dev).unwrap(), expected_closure);
    assert_eq!(
        referrers_of(&roots, &middle.entry.envelope.output_hash).unwrap(),
        vec![dev.clone(), primary.clone()]
    );
    let action = entry_action_key(&consumer.entry);
    assert_eq!(
        action_outputs_of(&roots, &action).unwrap().get("dev"),
        Some(&dev)
    );
    assert_eq!(actions_for_output(&roots, &primary).unwrap(), vec![action]);

    ingest_fixture(&roots, "base", &[("out", "base")], vec![primary.clone()]);
    let transitive = transitive_references_of(&roots, &primary).unwrap();
    assert!(transitive.contains(&base.entry.envelope.output_hash));
    assert!(transitive.contains(&middle.entry.envelope.output_hash));
    assert!(!transitive.contains(&primary));
    let mut expected_referrers = vec![
        dev.clone(),
        middle.entry.envelope.output_hash.clone(),
        primary.clone(),
    ];
    expected_referrers.sort();
    assert_eq!(
        transitive_referrers_of(&roots, &base.entry.envelope.output_hash).unwrap(),
        expected_referrers
    );
    let mut expected_reverse_closure = vec![
        base.entry.envelope.output_hash.clone(),
        dev,
        middle.entry.envelope.output_hash.clone(),
        primary,
    ];
    expected_reverse_closure.sort();
    assert_eq!(
        reverse_closure_of(&roots, &base.entry.envelope.output_hash).unwrap(),
        expected_reverse_closure
    );
}

#[test]
fn closure_refresh_and_delete_remove_stale_reverse_edges() {
    let (roots, _g) = temp_roots();
    let left = ingest_fixture(&roots, "left", &[("out", "left")], Vec::new());
    let right = ingest_fixture(&roots, "right", &[("out", "right")], Vec::new());
    let consumer = ingest_fixture(
        &roots,
        "refresh",
        &[("out", "same")],
        vec![left.entry.envelope.output_hash.clone()],
    );
    assert_eq!(
        referrers_of(&roots, &left.entry.envelope.output_hash).unwrap(),
        vec![consumer.entry.envelope.output_hash.clone()]
    );

    let refreshed = ingest_fixture(
        &roots,
        "refresh",
        &[("out", "same")],
        vec![right.entry.envelope.output_hash.clone()],
    );
    assert!(closure_graph(&roots)
        .unwrap()
        .referrers(&left.entry.envelope.output_hash)
        .is_empty());
    assert_eq!(
        closure_graph(&roots)
            .unwrap()
            .referrers(&right.entry.envelope.output_hash),
        vec![refreshed.entry.envelope.output_hash.clone()]
    );
    assert!(remove_closure_record(&roots, &refreshed.entry.id).unwrap());
    let stale_meta = roots
        .hangar_dir()
        .join(&refreshed.entry.id)
        .join("meta.json");
    fs::write(&stale_meta, refreshed.entry.meta_json()).unwrap();
    assert!(closure_graph(&roots)
        .unwrap()
        .referrers(&right.entry.envelope.output_hash)
        .is_empty());
    assert!(!stale_meta.exists());
    assert!(!remove_closure_record(&roots, &refreshed.entry.id).unwrap());
}

#[test]
fn closure_rejects_one_action_mapping_to_different_outputs() {
    let (roots, _g) = temp_roots();
    let make = |name: &str, bytes: &str| {
        let out = roots.root.join(name);
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), bytes).unwrap();
        let envelope = super::super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "path:same-action",
            "action-conflict",
        );
        record_verified(
            &roots,
            name,
            "1",
            "path:same-action",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
    };
    make("first-action-output", "first").unwrap();
    let error = make("second-action-output", "second").unwrap_err();
    assert!(error.to_string().contains("conflicting bytes"));
    assert_eq!(closure_graph(&roots).unwrap().records.len(), 1);
}

#[test]
fn closure_action_key_includes_sorted_reference_digests() {
    let (roots, _g) = temp_roots();
    let first = ingest_fixture(&roots, "key-first", &[("out", "first")], Vec::new());
    let second = ingest_fixture(&roots, "key-second", &[("out", "second")], Vec::new());
    let mut entry = first.entry.clone();
    entry.references = vec![
        second.entry.envelope.output_hash.clone(),
        first.entry.envelope.output_hash.clone(),
    ];
    let ordered = entry_action_key(&entry);
    entry.references.reverse();
    assert_eq!(entry_action_key(&entry), ordered);
    entry.references.pop();
    assert_ne!(entry_action_key(&entry), ordered);

    let mut left = first.entry.clone();
    left.reference = "a\nsource=b".to_string();
    left.cache_identity.source_fingerprint = "c".to_string();
    let mut right = first.entry.clone();
    right.reference = "a".to_string();
    right.cache_identity.source_fingerprint = "b\nsource=c".to_string();
    assert_ne!(entry_action_key(&left), entry_action_key(&right));
}

#[test]
fn closure_action_key_excludes_realized_outputs_but_keeps_action_facts() {
    let (roots, _g) = temp_roots();
    let mut entry =
        ingest_fixture(&roots, "action-projection", &[("out", "bytes")], Vec::new()).entry;
    let original_record = entry.producer_record.clone();
    let original = entry_action_key(&entry);
    let mut producer = ProducerRecord::decode(&entry.producer_record).unwrap();

    let mut replay = producer.plan.facts().clone();
    replay.insert("output.out".to_string(), "sha256-different".to_string());
    producer.plan = crate::Comptime::Build::BuildPlanReplay::from_facts(replay).unwrap();
    entry.producer_record = producer.encode();
    assert_eq!(entry_action_key(&entry), original);

    let mut producer = ProducerRecord::decode(&entry.producer_record).unwrap();
    let mut replay = producer.plan.facts().clone();
    replay.insert("action.recipe".to_string(), "different-recipe".to_string());
    producer.plan = crate::Comptime::Build::BuildPlanReplay::from_facts(replay).unwrap();
    entry.producer_record = producer.encode();
    assert_ne!(entry_action_key(&entry), original);

    let mut producer = ProducerRecord::decode(&original_record).unwrap();
    producer.toolchain_facts.push_str("-different");
    entry.producer_record = producer.encode();
    assert_ne!(entry_action_key(&entry), original);
}

#[test]
fn nix_action_key_is_input_derivation_only() {
    let (roots, _g) = temp_roots();
    let mut first = ingest_fixture(&roots, "nix-action", &[("out", "bytes")], Vec::new()).entry;
    let nix_record = |drv: &str, output: &str, reference: &str| {
        ProducerRecord::new(
            "nix",
            drv,
            crate::SHA256::sha256_hex(drv.as_bytes()),
            crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::from([
                ("nix.drv_path".into(), drv.into()),
                ("nix.reference".into(), reference.into()),
                ("nix.output.out".into(), output.into()),
            ]))
            .unwrap(),
            format!("nix-derivation:{drv}"),
            "policy=test\nplatform=test",
            BTreeMap::from([("nix.output.out".into(), output.into())]),
        )
        .unwrap()
        .encode()
    };
    first.reference = "first@nixpkgs".into();
    first.cache_identity.source_fingerprint = "sha256-first-output".into();
    first.producer_record =
        nix_record("/nix/store/action.drv", "/nix/store/first", "first@nixpkgs");
    let action = entry_action_key(&first);

    let mut second = first.clone();
    second.reference = "alias:second".into();
    second.cache_identity.source_fingerprint = "sha256-second-output".into();
    second.producer_record =
        nix_record("/nix/store/action.drv", "/nix/store/second", "alias:second");
    assert_eq!(entry_action_key(&second), action);

    second.producer_record =
        nix_record("/nix/store/other.drv", "/nix/store/second", "alias:second");
    assert_ne!(entry_action_key(&second), action);
}

#[test]
fn nix_multi_projection_registers_recovers_queries_and_rolls_back_conflict() {
    let (roots, _g) = temp_roots();
    let out = ingest_fixture(
        &roots,
        "projection-out-bytes",
        &[("out", "out")],
        Vec::new(),
    );
    let dev = ingest_fixture(
        &roots,
        "projection-dev-bytes",
        &[("out", "dev")],
        Vec::new(),
    );
    let conflict = ingest_fixture(&roots, "projection-bad-dev", &[("out", "bad")], Vec::new());
    let drv = "/nix/store/multi-projection.drv";
    let project = |mut entry: StoreEntry, id: &str, output_name: &str| {
        let path = entry.out.clone();
        entry.id = id.into();
        entry.name = "multi-projection".into();
        entry.reference = format!("alias:{output_name}");
        entry.named_outputs = BTreeMap::from([
            ("out".into(), entry.envelope.output_hash.clone()),
            (output_name.into(), entry.envelope.output_hash.clone()),
        ]);
        entry.producer_record = ProducerRecord::new(
            "nix",
            drv,
            crate::SHA256::sha256_hex(drv.as_bytes()),
            crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::from([
                ("nix.drv_path".into(), drv.into()),
                ("nix.reference".into(), entry.reference.clone()),
                (format!("nix.output.{output_name}"), path.clone()),
            ]))
            .unwrap(),
            format!("nix-derivation:{drv}"),
            "policy=test\nplatform=test",
            BTreeMap::from([(format!("nix.output.{output_name}"), path)]),
        )
        .unwrap()
        .encode();
        entry
    };
    let out = project(out.entry, "projection-out", "out");
    let dev = project(dev.entry, "projection-dev", "dev");
    let bad = project(conflict.entry, "projection-bad", "dev");
    let action = entry_action_key(&out);
    assert_eq!(entry_action_key(&dev), action);

    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        register_entry_unlocked(&roots, &out)?;
        register_entry_unlocked(&roots, &dev)
    })
    .unwrap();
    fs::remove_file(roots.hangar_dir().join(&dev.id).join("meta.json")).unwrap();
    let outputs = action_outputs_of(&roots, &action).unwrap();
    assert_eq!(outputs.get("out"), Some(&out.envelope.output_hash));
    assert_eq!(outputs.get("dev"), Some(&dev.envelope.output_hash));
    assert!(roots.hangar_dir().join(&dev.id).join("meta.json").is_file());

    let before = closure_graph(&roots).unwrap();
    let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        register_entry_unlocked(&roots, &bad)
    })
    .unwrap_err();
    assert!(error.to_string().contains("conflicting bytes"));
    assert_eq!(closure_graph(&roots).unwrap(), before);
}

#[test]
fn closure_empty_reference_proof_rejects_unknown_provider() {
    let (roots, _g) = temp_roots();
    let mut entry =
        ingest_fixture(&roots, "unknown-proof", &[("out", "bytes")], Vec::new()).entry;
    entry.id = "unknown-proof-record".into();
    let original = ProducerRecord::decode(&entry.producer_record).unwrap();
    entry.producer_record = ProducerRecord::new(
        "unknown-provider",
        original.immutable_source,
        original.source_digest,
        original.plan,
        original.toolchain_facts,
        original.policy_facts,
        BTreeMap::from([("closure.authority".into(), "hangar-cas".into())]),
    )
    .unwrap()
    .encode();
    let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        register_entry_unlocked(&roots, &entry)
    })
    .unwrap_err();
    assert!(error.to_string().contains("store-validated closure proof"));
}

#[test]
fn closure_rejects_named_out_that_disagrees_with_primary() {
    let (roots, _g) = temp_roots();
    let primary = ingest_fixture(
        &roots,
        "named-out-primary",
        &[("out", "primary")],
        Vec::new(),
    );
    let other = ingest_fixture(&roots, "named-out-other", &[("out", "other")], Vec::new());
    let mut conflicting = primary.entry.clone();
    conflicting
        .named_outputs
        .insert("out".to_string(), other.entry.envelope.output_hash);
    let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        register_entry_unlocked(&roots, &conflicting)
    })
    .unwrap_err();
    assert!(error.to_string().contains("names `out`"));
}

#[test]
fn closure_rejects_same_action_output_name_with_conflicting_bytes() {
    let (roots, _g) = temp_roots();
    let primary = ingest_fixture(&roots, "same-record", &[("out", "primary")], Vec::new());
    let named = ingest_fixture(&roots, "named-source", &[("out", "named")], Vec::new());
    let action = entry_action_key(&primary.entry);
    let mut conflicting = primary.entry.clone();
    conflicting.id = "conflicting-projection".into();
    conflicting.out = named.entry.out.clone();
    conflicting.envelope.output_hash = named.entry.envelope.output_hash.clone();
    conflicting.named_outputs =
        BTreeMap::from([("out".to_string(), named.entry.envelope.output_hash.clone())]);
    let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        register_entry_unlocked(&roots, &conflicting)
    })
    .unwrap_err();
    assert!(error.to_string().contains("conflicting bytes"));
    assert_eq!(action_outputs_of(&roots, &action).unwrap().len(), 1);
    assert!(!action_outputs_of(&roots, &action)
        .unwrap()
        .contains_key("dev"));
}

#[test]
fn closure_journal_recovers_partial_and_rejects_committed_corruption() {
    use std::io::Write as _;

    let (roots, _g) = temp_roots();
    ingest_fixture(&roots, "journal", &[("out", "journal")], Vec::new());
    let journal = roots.hangar_dir().join("closure-db/journal");
    fs::write(journal.join("999.partial"), "torn").unwrap();
    assert_eq!(recover_closure_journal(&roots).unwrap(), 1);
    assert!(!journal.join("999.partial").exists());

    let transaction = fs::read_dir(&journal)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txn"))
        .unwrap();
    let legacy_referrers = roots.hangar_dir().join("referrers");
    fs::create_dir_all(&legacy_referrers).unwrap();
    fs::write(
        legacy_referrers.join("fake.refs"),
        "fallback-must-not-win\n",
    )
    .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(transaction)
        .unwrap()
        .write_all(b"corrupt")
        .unwrap();
    let error = closure_graph(&roots).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("checksum"));
    assert!(referrers_of(&roots, "fake").is_err());
}

#[test]
fn committed_producer_transaction_recovers_missing_package_projection() {
    let (roots, _g) = temp_roots();
    let ingested = ingest_fixture(
        &roots,
        "producer-recovery",
        &[("out", "primary"), ("dev", "headers")],
        Vec::new(),
    );
    let meta = roots
        .hangar_dir()
        .join(&ingested.entry.id)
        .join("meta.json");
    let expected = fs::read_to_string(&meta).unwrap();
    fs::remove_file(&meta).unwrap();

    assert_eq!(recover_closure_journal(&roots).unwrap(), 1);
    assert!(list_checked(&roots)
        .unwrap()
        .iter()
        .any(|entry| entry.id == ingested.entry.id));
    let graph = closure_graph(&roots).unwrap();
    let record = graph.records.get(&ingested.entry.id).unwrap();
    assert_eq!(
        ProducerRecord::decode(&record.producer_record)
            .unwrap()
            .provider,
        "hangar-ingest"
    );
    assert_eq!(record.outputs.len(), 2);
    assert_eq!(fs::read_to_string(&meta).unwrap(), expected);

    fs::write(&meta, "stale but parseable projection").unwrap();
    assert_eq!(recover_closure_journal(&roots).unwrap(), 1);
    assert_eq!(fs::read_to_string(&meta).unwrap(), expected);

    fs::write(&meta, [0xff, 0xfe, 0xfd]).unwrap();
    assert_eq!(recover_closure_journal(&roots).unwrap(), 1);
    assert_eq!(fs::read_to_string(&meta).unwrap(), expected);

    fs::remove_file(&meta).unwrap();
    let changed = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        register_entry_unlocked(&roots, &ingested.entry)
    })
    .unwrap();
    assert!(!changed);
    assert_eq!(fs::read_to_string(meta).unwrap(), expected);
}

#[test]
fn combined_recovery_sweeps_staging_and_repairs_wal_projection() {
    let (roots, _g) = temp_roots();
    let ingested = ingest_fixture(&roots, "combined-recovery", &[("out", "bytes")], Vec::new());
    let abandoned = roots.hangar_dir().join(".stage/abandoned");
    fs::create_dir_all(&abandoned).unwrap();
    fs::write(abandoned.join("payload"), "partial").unwrap();
    let meta = roots
        .hangar_dir()
        .join(&ingested.entry.id)
        .join("meta.json");
    let expected = ingested.entry.meta_json();
    fs::write(&meta, "stale").unwrap();

    assert_eq!(recover_hangar(&roots).unwrap(), 2);
    assert!(!abandoned.exists());
    assert_eq!(fs::read_to_string(meta).unwrap(), expected);
}

#[test]
fn admission_failure_points_hide_partial_metadata_and_recover_idempotently() {
    for (index, point) in [
        AdmissionFailurePoint::AfterObjectPublication,
        AdmissionFailurePoint::AfterReceiptPublication,
        AdmissionFailurePoint::AfterClosureRegistration,
    ]
    .into_iter()
    .enumerate()
    {
        let (roots, _g) = temp_roots();
        let source = roots.root.join(format!("failure-source-{index}"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("payload"), format!("failure-{index}")).unwrap();
        let request = IngestRequest {
            name: format!("failure-{index}"),
            version: "1".into(),
            reference: format!("path:failure-{index}"),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: BTreeMap::from([("out".into(), source)]),
            signature: String::new(),
            provenance: "transaction-test".into(),
            platform_artifact_kind: String::new(),
        };
        let result = with_admission_failure(point, || ingest_tree(&roots, &request));
        assert!(result.is_err(), "failure point {point:?} did not fire");

        if point != AdmissionFailurePoint::AfterClosureRegistration {
            recover_hangar(&roots).unwrap();
            recover_hangar(&roots).unwrap();
            assert!(list_checked(&roots).unwrap().is_empty());
            assert_eq!(
                fs::read_dir(roots.hangar_dir().join(OBJECTS_DIR))
                    .unwrap()
                    .count(),
                0
            );
        } else {
            let committed = list_checked(&roots).unwrap();
            assert_eq!(committed.len(), 1);
            let meta = roots
                .hangar_dir()
                .join(&committed[0].id)
                .join("meta.json");
            fs::remove_file(&meta).unwrap();
            recover_hangar(&roots).unwrap();
            let recovered = list_checked(&roots).unwrap();
            recover_hangar(&roots).unwrap();
            assert_eq!(list_checked(&roots).unwrap(), recovered);
            assert_eq!(recovered, committed);
        }
    }
}

#[test]
fn closure_legacy_migration_is_idempotent() {
    let (roots, _g) = temp_roots();
    let mut first =
        ingest_fixture(&roots, "legacy-first", &[("out", "first")], Vec::new()).entry;
    let mut second =
        ingest_fixture(&roots, "legacy-second", &[("out", "second")], Vec::new()).entry;
    first.references = vec![second.envelope.output_hash.clone()];
    second.references = vec![first.envelope.output_hash.clone()];
    fs::write(
        roots.hangar_dir().join(&first.id).join("meta.json"),
        first.meta_json(),
    )
    .unwrap();
    fs::write(
        roots.hangar_dir().join(&second.id).join("meta.json"),
        second.meta_json(),
    )
    .unwrap();
    fs::remove_dir_all(roots.hangar_dir().join("closure-db")).unwrap();

    assert_eq!(migrate_closure_graph(&roots).unwrap(), 2);
    assert_eq!(migrate_closure_graph(&roots).unwrap(), 0);
    let graph = closure_graph(&roots).unwrap();
    assert!(graph.records.contains_key(&first.id));
    assert!(graph.records.contains_key(&second.id));
    assert_eq!(
        graph.direct_references(&first.envelope.output_hash),
        first.references
    );
    assert_eq!(
        graph.direct_references(&second.envelope.output_hash),
        second.references
    );
}

#[test]
fn closure_legacy_migration_rejects_atomically() {
    let (roots, _g) = temp_roots();
    let mut entry =
        ingest_fixture(&roots, "legacy-invalid", &[("out", "invalid")], Vec::new()).entry;
    entry.references = vec!["sha256-missing".to_string()];
    fs::write(
        roots.hangar_dir().join(&entry.id).join("meta.json"),
        entry.meta_json(),
    )
    .unwrap();
    fs::remove_dir_all(roots.hangar_dir().join("closure-db")).unwrap();

    let error = migrate_closure_graph(&roots).unwrap_err();
    assert!(error.to_string().contains("references missing object"));
    let journal = roots.hangar_dir().join("closure-db/journal");
    let transactions = fs::read_dir(journal)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("txn"))
        .count();
    assert_eq!(transactions, 0);
}

#[test]
fn legacy_migration_fails_closed_without_immutable_producer_facts() {
    let (roots, _g) = temp_roots();
    let mut entry = ingest_fixture(
        &roots,
        "legacy-missing-producer",
        &[("out", "legacy")],
        Vec::new(),
    )
    .entry;
    entry.producer_record.clear();
    entry.cache_identity = CacheIdentity::default();
    fs::write(
        roots.hangar_dir().join(&entry.id).join("meta.json"),
        entry.meta_json(),
    )
    .unwrap();
    fs::remove_dir_all(roots.hangar_dir().join("closure-db")).unwrap();

    let error = migrate_closure_graph(&roots).unwrap_err();
    assert!(error.to_string().contains("lacks immutable producer facts"));
}

#[test]
fn provider_registration_moves_local_output_into_canonical_objects() {
    let (roots, _g) = temp_roots();
    let out = roots.hangar_dir().join("provider-output");
    fs::create_dir_all(out.join("bin")).unwrap();
    fs::write(out.join("bin/tool"), "provider").unwrap();
    seal_local_output(&out).unwrap();
    let envelope = super::super::super::super::Envelope::Envelope::for_output(
        &out.to_string_lossy(),
        "path:provider",
        "provider",
    );
    let (canonical_out, canonical_bin, canonical_rlib) = canonicalize_local_output_unlocked(
        &roots,
        &out.to_string_lossy(),
        &out.join("bin").to_string_lossy(),
        "",
        &envelope.output_hash,
    )
    .unwrap();
    let entry = record_verified_mode(
        &roots,
        "provider",
        "1",
        "path:provider",
        &canonical_out,
        &canonical_bin,
        &canonical_rlib,
        &envelope,
        &test_identity(),
        false,
    )
    .unwrap();
    assert_eq!(
        Path::new(&entry.out),
        roots
            .hangar_dir()
            .join("objects")
            .join(&entry.envelope.output_hash)
    );
    assert!(!out.exists());
    verify_hangar_object(&roots, &entry).unwrap();
    snapshot_lease(&roots, &entry).unwrap();
}

#[test]
fn windows_directory_sync_contract_requests_flushable_handle() {
    let contract = windows_directory_sync_contract();
    assert!(contract.read);
    assert!(contract.write, "FlushFileBuffers requires GENERIC_WRITE");
    assert_eq!(contract.share_mode, 0x0000_0001 | 0x0000_0002 | 0x0000_0004);
    assert_eq!(contract.custom_flags, 0x0200_0000 | 0x0020_0000);
}

#[cfg(windows)]
#[test]
fn windows_store_tree_sync_flushes_directory_handle() {
    let (roots, _g) = temp_roots();
    let tree = roots.root.join("windows-sync-tree");
    fs::create_dir_all(tree.join("nested")).unwrap();
    fs::write(tree.join("nested/payload"), "durable").unwrap();
    fsync_tree(&tree).unwrap();
}

#[test]
fn closure_registration_serializes_concurrent_writers() {
    let (roots, _g) = temp_roots();
    let root = roots.root.clone();
    let threads = (0..8)
        .map(|index| {
            let root = root.clone();
            std::thread::spawn(move || {
                let roots = Roots {
                    root,
                    dev_mode: true,
                };
                ingest_fixture(
                    &roots,
                    &format!("concurrent-{index}"),
                    &[("out", &format!("bytes-{index}"))],
                    Vec::new(),
                );
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(closure_graph(&roots).unwrap().records.len(), 8);
}

#[test]
fn closure_journal_compacts_without_changing_graph() {
    let (roots, _g) = temp_roots();
    for index in 0..70 {
        ingest_fixture(
            &roots,
            &format!("compact-{index}"),
            &[("out", &format!("compact-bytes-{index}"))],
            Vec::new(),
        );
    }
    let graph = closure_graph(&roots).unwrap();
    assert_eq!(graph.records.len(), 70);
    let transactions = fs::read_dir(roots.hangar_dir().join("closure-db/journal"))
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("txn"))
        .count();
    assert!(transactions < 10, "journal did not compact: {transactions}");
}

#[test]
fn closure_readers_are_serialized_with_registration_and_compaction() {
    let (roots, _g) = temp_roots();
    let writer_root = roots.root.clone();
    let writer = std::thread::spawn(move || {
        let roots = Roots {
            root: writer_root,
            dev_mode: true,
        };
        for index in 0..70 {
            ingest_fixture(
                &roots,
                &format!("reader-compact-{index}"),
                &[("out", &format!("reader-compact-bytes-{index}"))],
                Vec::new(),
            );
        }
    });
    let reader_root = roots.root.clone();
    let reader = std::thread::spawn(move || {
        let roots = Roots {
            root: reader_root,
            dev_mode: true,
        };
        let mut previous = 0;
        for _ in 0..100 {
            let count = closure_graph(&roots).unwrap().records.len();
            assert!(count >= previous, "reader observed a stale graph");
            previous = count;
        }
    });
    writer.join().unwrap();
    reader.join().unwrap();
    assert_eq!(closure_graph(&roots).unwrap().records.len(), 70);
}

#[cfg(unix)]
#[test]
fn cas_pool_hardlink_preserves_cache_verification_and_rejects_outside_peers() {
    use std::os::unix::fs::MetadataExt as _;

    let (roots, _g) = temp_roots();
    // Two distinct object digests that share an identical file payload.
    let src_c = roots.root.join("cas-c");
    fs::create_dir_all(&src_c).unwrap();
    fs::write(src_c.join("payload"), "shared-cas-bytes").unwrap();
    fs::write(src_c.join("unique"), "c-only").unwrap();
    let src_d = roots.root.join("cas-d");
    fs::create_dir_all(&src_d).unwrap();
    fs::write(src_d.join("payload"), "shared-cas-bytes").unwrap();
    fs::write(src_d.join("unique"), "d-only").unwrap();

    let mut outs_c = BTreeMap::new();
    outs_c.insert("out".to_string(), src_c);
    let third = ingest_tree(
        &roots,
        &IngestRequest {
            name: "cas-c".into(),
            version: "1".into(),
            reference: "path:cas-c".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: outs_c,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    let mut outs_d = BTreeMap::new();
    outs_d.insert("out".to_string(), src_d);
    let fourth = ingest_tree(
        &roots,
        &IngestRequest {
            name: "cas-d".into(),
            version: "1".into(),
            reference: "path:cas-d".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: outs_d,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    assert_ne!(
        third.entry.envelope.output_hash,
        fourth.entry.envelope.output_hash
    );

    // Registration shares payload bytes with the machine CAS pool.
    let pay_c = Path::new(&third.entry.out).join("payload");
    assert!(fs::metadata(&pay_c).unwrap().nlink() >= 2);

    // Registration already populated the shared pool; cleanup is idempotent
    // and must not create a second sharing mechanism.
    let report = optimize_cas_pool(&roots).unwrap();
    assert_eq!(report.optimized_files, 0, "{report:?}");
    assert!(roots.shared_cas_dir().is_dir());
    assert!(fs::metadata(&pay_c).unwrap().nlink() >= 2);

    // Hangar-internal cas peers: verify still green; digest stable.
    verify_hangar_object(&roots, &third.entry).unwrap();
    verify_hangar_object(&roots, &fourth.entry).unwrap();
    let expectation = test_expectation(Path::new(&third.entry.out));
    let proof = verify_cache_entry(&roots, &third.entry, &third.entry.reference, &expectation);
    assert!(proof.output_digest, "{proof:?}");
    assert!(proof.trusted(), "{proof:?}");
    find_verified_by_reference(&roots, &third.entry.reference, &expectation)
        .unwrap()
        .unwrap()
        .lease
        .validate()
        .unwrap();

    // A pool-backed inode remains trusted even with a foreign peer.
    let outside = roots.root.join("outside-peer");
    fs::hard_link(&pay_c, &outside).unwrap();
    let in_hangar = super::super::super::super::Envelope::try_output_hash_of_in_hangar(
        &third.entry.out,
        &roots.hangar_dir(),
        false,
    );
    assert_eq!(in_hangar.unwrap(), third.entry.envelope.output_hash);
    let proof = verify_cache_entry(&roots, &third.entry, &third.entry.reference, &expectation);
    assert!(proof.output_digest, "{proof:?}");
    assert!(proof.trusted(), "{proof:?}");
    find_verified_by_reference(&roots, &third.entry.reference, &expectation)
        .unwrap()
        .unwrap()
        .lease
        .validate()
        .unwrap();
    fs::remove_file(outside).ok();

    // A foreign peer without the object's exact Hangar CAS backing remains
    // untrusted.
    let non_pool = roots.hangar_dir().join("non-pool-output");
    fs::create_dir_all(&non_pool).unwrap();
    let non_pool_payload = non_pool.join("payload");
    fs::write(&non_pool_payload, "unpooled").unwrap();
    let non_pool_peer = roots.root.join("outside-non-pool-peer");
    fs::hard_link(&non_pool_payload, &non_pool_peer).unwrap();
    let non_pool_result =
        super::super::super::super::Envelope::try_output_hash_of_in_hangar(
            &non_pool.to_string_lossy(),
            &roots.hangar_dir(),
            false,
        );
    assert!(non_pool_result.is_err(), "{non_pool_result:?}");
    fs::remove_file(non_pool_peer).ok();
    fs::remove_dir_all(non_pool).ok();
}

#[cfg(unix)]
#[test]
fn optimizer_rejects_symlinked_object_pool_without_touching_outside_data() {
    let (roots, _g) = temp_roots();
    let hangar = roots.hangar_dir();
    let outside = roots.root.join("optimizer-outside");
    fs::create_dir_all(&hangar).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("live"), "must survive").unwrap();
    std::os::unix::fs::symlink(&outside, hangar.join("objects")).unwrap();

    let error = optimize_cas_pool(&roots).unwrap_err();
    assert!(error.to_string().contains("object pool"), "{error}");
    assert_eq!(
        fs::read_to_string(outside.join("live")).unwrap(),
        "must survive"
    );
    assert!(!hangar.join("cas").exists());
}

#[cfg(unix)]
#[test]
fn clean_plan_rejects_symlinked_build_scratch_without_following_it() {
    let (roots, _g) = temp_roots();
    let hangar = roots.hangar_dir();
    let outside = roots.root.join("scratch-outside");
    fs::create_dir_all(&hangar).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("live"), "must survive").unwrap();
    std::os::unix::fs::symlink(&outside, hangar.join("build-scratch")).unwrap();

    let error = clean_plan(&roots).unwrap_err();
    assert!(error.to_string().contains("build scratch"), "{error}");
    assert_eq!(
        fs::read_to_string(outside.join("live")).unwrap(),
        "must survive"
    );
}
#[cfg(target_os = "linux")]
#[test]
fn ingest_rejects_semantic_xattr_without_platform_artifact_kind() {
    let (roots, _g) = temp_roots();
    let src = roots.root.join("xattr-src");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("payload");
    fs::write(&file, "xattr-bytes").unwrap();
    set_user_xattr(&file, "user.jet.test", b"keep");
    let mut outputs = BTreeMap::new();
    outputs.insert("out".to_string(), src);
    let err = ingest_tree(
        &roots,
        &IngestRequest {
            name: "xattr".into(),
            version: "1".into(),
            reference: "path:xattr".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "E1315");
    assert!(
        err.what().contains("semantic xattr") || err.why().contains("semantic xattr"),
        "{err:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn ingest_keeps_semantic_xattr_with_platform_artifact_kind() {
    let (roots, _g) = temp_roots();
    let src = roots.root.join("xattr-ok");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("payload");
    fs::write(&file, "xattr-bytes").unwrap();
    set_user_xattr(&src, "user.jet.directory", b"directory");
    set_user_xattr(&file, "user.jet.test", b"keep");
    let mut outputs = BTreeMap::new();
    outputs.insert("out".to_string(), src.clone());
    let ingested = ingest_tree(
        &roots,
        &IngestRequest {
            name: "xattr-ok".into(),
            version: "1".into(),
            reference: "path:xattr-ok".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: "macos-app".into(),
        },
    )
    .unwrap();
    let first_hash = ingested.entry.envelope.output_hash.clone();
    assert_eq!(ingested.entry.platform_artifact_kind, "macos-app");
    verify_hangar_object(&roots, &ingested.entry).unwrap();
    let sealed = Path::new(&ingested.entry.out).join("payload");
    let names = super::super::super::super::Envelope::list_xattr_names(&sealed).unwrap();
    assert!(
        names.iter().any(|n| n == "user.jet.test"),
        "semantic xattr must be preserved on sealed object: {names:?}"
    );
    let root_names =
        super::super::super::super::Envelope::list_xattr_names(Path::new(&ingested.entry.out))
            .unwrap();
    assert!(root_names.iter().any(|name| name == "user.jet.directory"));
    set_user_xattr(&src, "user.jet.directory", b"changed");
    let changed_hash = super::super::super::super::Envelope::try_output_hash_of_with_policy(
        &src.to_string_lossy(),
        true,
        &mut |_, _| {},
    )
    .unwrap();
    assert_ne!(first_hash, changed_hash);
}

#[cfg(target_os = "linux")]
#[test]
fn ingest_rejects_semantic_directory_xattr_without_platform_kind() {
    let (roots, _g) = temp_roots();
    let src = roots.root.join("xattr-directory-reject");
    fs::create_dir_all(&src).unwrap();
    set_user_xattr(&src, "user.jet.directory", b"reject");
    let error = ingest_tree(
        &roots,
        &IngestRequest {
            name: "xattr-directory".into(),
            version: "1".into(),
            reference: "path:xattr-directory".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: BTreeMap::from([("out".into(), src)]),
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap_err();
    assert!(error.what().contains("semantic xattr") || error.why().contains("semantic xattr"));
}

#[cfg(target_os = "macos")]
#[test]
fn ingest_symlink_xattr_is_nofollow_rejected_digested_and_copied() {
    use std::os::unix::fs::symlink;
    let (roots, _g) = temp_roots();
    let src = roots.root.join("xattr-symlink");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("target"), "bytes").unwrap();
    symlink("target", src.join("link")).unwrap();
    set_apple_xattr(&src.join("link"), "user.jet.symlink", b"first");
    let request = |kind: &str| IngestRequest {
        name: "xattr-symlink".into(),
        version: "1".into(),
        reference: "path:xattr-symlink".into(),
        cache_identity: test_identity(),
        references: Vec::new(),
        outputs: BTreeMap::from([("out".into(), src.clone())]),
        signature: String::new(),
        provenance: String::new(),
        platform_artifact_kind: kind.into(),
    };
    assert!(ingest_tree(&roots, &request("")).is_err());
    let ingested = ingest_tree(&roots, &request("macos-tree")).unwrap();
    let sealed = Path::new(&ingested.entry.out).join("link");
    assert!(
        super::super::super::super::Envelope::list_xattr_names(&sealed)
            .unwrap()
            .iter()
            .any(|name| name == "user.jet.symlink")
    );
    let first = ingested.entry.envelope.output_hash;
    set_apple_xattr(&src.join("link"), "user.jet.symlink", b"second");
    let second = super::super::super::super::Envelope::try_output_hash_of_with_policy(
        &src.to_string_lossy(),
        true,
        &mut |_, _| {},
    )
    .unwrap();
    assert_ne!(first, second);
}

#[cfg(target_os = "linux")]
fn set_user_xattr(path: &Path, name: &str, value: &[u8]) {
    use std::os::unix::ffi::OsStrExt as _;
    type LibcChar = i8;
    #[link(name = "c")]
    extern "C" {
        fn lsetxattr(
            path: *const LibcChar,
            name: *const LibcChar,
            value: *const u8,
            size: usize,
            flags: i32,
        ) -> i32;
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let c_name = std::ffi::CString::new(name).unwrap();
    let rc = unsafe {
        lsetxattr(
            c_path.as_ptr(),
            c_name.as_ptr(),
            value.as_ptr(),
            value.len(),
            0,
        )
    };
    assert_eq!(
        rc,
        0,
        "lsetxattr failed: {}",
        std::io::Error::last_os_error()
    );
}

#[cfg(target_os = "macos")]
fn set_apple_xattr(path: &Path, name: &str, value: &[u8]) {
    use std::os::unix::ffi::OsStrExt as _;
    unsafe extern "C" {
        fn setxattr(
            path: *const i8,
            name: *const i8,
            value: *const u8,
            size: usize,
            position: u32,
            options: i32,
        ) -> i32;
    }
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = std::ffi::CString::new(name).unwrap();
    let rc = unsafe {
        setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr(),
            value.len(),
            0,
            0x0001,
        )
    };
    assert_eq!(
        rc,
        0,
        "setxattr failed: {}",
        std::io::Error::last_os_error()
    );
}
