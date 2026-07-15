use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_roots() -> (Roots, tempdir::Guard) {
        let g = tempdir::Guard::new("jpk-store");
        let roots = Roots {
            root: g.path.clone(),
            dev_mode: true,
        };
        (roots, g)
    }

    fn test_identity() -> CacheIdentity {
        CacheIdentity {
            source_fingerprint: "source-v1".to_string(),
            recipe_fingerprint: "recipe-v1".to_string(),
            policy_fingerprint: "policy-v1".to_string(),
            platform: super::super::super::Envelope::host_platform(),
        }
    }

    fn test_expectation(out: &Path) -> CacheExpectation {
        CacheExpectation {
            identity: test_identity(),
            owned_output: Some(out.to_path_buf()),
            allow_unsigned_local: true,
        }
    }

    fn ingest_fixture(
        roots: &Roots,
        name: &str,
        outputs: &[(&str, &str)],
        references: Vec<String>,
    ) -> IngestedObject {
        let mut paths = BTreeMap::new();
        for (output, bytes) in outputs {
            let path = roots.root.join(format!("fixture-{name}-{output}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("payload"), bytes).unwrap();
            paths.insert((*output).to_string(), path);
        }
        ingest_tree(
            roots,
            &IngestRequest {
                name: name.to_string(),
                version: "1".to_string(),
                reference: format!("path:{name}"),
                cache_identity: test_identity(),
                references,
                outputs: paths,
                signature: String::new(),
                provenance: "closure-test".to_string(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap()
    }

    fn verified(roots: &Roots, reference: &str, expectation: &CacheExpectation) -> bool {
        find_verified_by_reference(roots, reference, expectation)
            .unwrap()
            .is_some()
    }

    #[test]
    fn required_child_pipe_preserves_value_and_reports_missing_pipe() {
        assert_eq!(required_child_pipe(Some(41), "unused").unwrap(), 41);

        let stdin = required_child_pipe(None::<()>, "piped lease keeper stdin").unwrap_err();
        assert_eq!(stdin.kind(), std::io::ErrorKind::Other);
        assert_eq!(stdin.to_string(), "piped lease keeper stdin");

        let stdout = required_child_pipe(None::<()>, "piped lease keeper stdout").unwrap_err();
        assert_eq!(stdout.kind(), std::io::ErrorKind::Other);
        assert_eq!(stdout.to_string(), "piped lease keeper stdout");
    }

    #[test]
    fn open_snapshot_files_rejects_paths_outside_root_before_inspection() {
        let (roots, _g) = temp_roots();
        let snapshot_root = roots.root.join("snapshot");
        let outside = roots.root.join("outside/payload");
        let outside_dir = roots.root.join("outside-empty-dir");
        fs::create_dir_all(&snapshot_root).unwrap();
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        fs::write(&outside, "outside").unwrap();
        let mut files = Vec::new();

        let mut assert_rejected = |path: &Path| {
            let error = open_snapshot_files(&snapshot_root, path, &mut files).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
            assert_eq!(
                error.to_string(),
                format!(
                    "snapshot path `{}` is outside snapshot root `{}`",
                    path.display(),
                    snapshot_root.display()
                )
            );
            assert!(files.is_empty());
        };

        assert_rejected(&outside);
        assert_rejected(&outside_dir);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let outside_link = roots.root.join("outside-link");
            symlink(&outside, &outside_link).unwrap();
            assert_rejected(&outside_link);
            assert_eq!(fs::read_link(&outside_link).unwrap(), outside);
            assert_eq!(fs::read_to_string(&outside).unwrap(), "outside");
        }
    }

    #[test]
    fn record_and_list_roundtrip() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("fastfetch-output");
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/fastfetch"), "fixture").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "nixpkgs:fastfetch",
            "nix",
        );
        let mut e = record_verified(
            &roots,
            "fastfetch",
            "2.1.0",
            "nixpkgs:fastfetch",
            &out.to_string_lossy(),
            &out.join("bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        e.named_outputs
            .insert("out".to_string(), envelope.output_hash.clone());
        // Name-and-version first, fingerprint last (D-PM1).
        assert!(e.id.starts_with("fastfetch-2.1.0-"));
        let listed = list(&roots);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], e);
    }

    #[test]
    fn clean_keeps_fresh_entries() {
        let (roots, _g) = temp_roots();
        for name in ["a", "b"] {
            let reference = format!("nixpkgs:{name}");
            let out = roots.root.join(format!("{name}-output"));
            fs::create_dir_all(out.join("bin")).unwrap();
            fs::write(out.join("bin").join(name), name).unwrap();
            let envelope = super::super::super::Envelope::Envelope::for_output(
                &out.to_string_lossy(),
                &reference,
                "nix",
            );
            record_verified(
                &roots,
                name,
                "1.0",
                &reference,
                &out.to_string_lossy(),
                &out.join("bin").to_string_lossy(),
                "",
                &envelope,
                &test_identity(),
            )
            .unwrap();
        }
        let report = clean(&roots).unwrap();
        assert_eq!(report.removed_objects, 0);
        assert_eq!(list(&roots).len(), 2);
    }

    #[test]
    fn committed_profile_generation_survives_lease_teardown_and_clean() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("profile-root-output");
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/tool"), "profile bytes").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "path:profile-tool",
            "path",
        );
        let mut entry = record_verified(
            &roots,
            "profile-tool",
            "1",
            "path:profile-tool",
            &out.to_string_lossy(),
            &out.join("bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let prepared = prepare_profile_generation_root(
            &roots,
            "user",
            "tools",
            1,
            &format!("sha256-{}", "f".repeat(64)),
            vec![entry.envelope.output_hash.clone()],
            1,
        )
        .unwrap();
        commit_profile_generation_root(&roots, &prepared, 2).unwrap();

        entry.last_used_at = 1;
        let record = roots.hangar_dir().join(&entry.id).join("meta.json");
        fs::write(&record, entry.meta_json()).unwrap();
        assert_eq!(clean_plan(&roots).unwrap().removed_objects, 0);
        assert_eq!(clean(&roots).unwrap().removed_objects, 0);
        assert!(record.is_file());

        let snapshot = Lifecycle::snapshot(&roots).unwrap();
        assert_eq!(snapshot.protected_targets.len(), 1);
        let root = snapshot.roots.values().next().unwrap();
        assert_eq!(root.identity.kind, Lifecycle::RootKind::ProfileGeneration);
        assert_eq!(root.identity.id.as_str(), "profile-generation:user:tools:1");
        assert_eq!(root.phase, Lifecycle::RootPhase::Committed);
    }

    #[test]
    fn profile_generation_root_keeps_transitive_closure_cycle_safely() {
        let (roots, _g) = temp_roots();
        let base = ingest_fixture(&roots, "profile-base", &[("out", "base")], Vec::new());
        let middle = ingest_fixture(
            &roots,
            "profile-middle",
            &[("out", "middle")],
            vec![base.entry.envelope.output_hash.clone()],
        );
        let consumer = ingest_fixture(
            &roots,
            "profile-consumer",
            &[("out", "consumer")],
            vec![middle.entry.envelope.output_hash.clone()],
        );
        let prepared = prepare_profile_generation_root(
            &roots,
            "user",
            "tools",
            8,
            &format!("sha256-{}", "8".repeat(64)),
            vec![consumer.entry.envelope.output_hash.clone()],
            1,
        )
        .unwrap();
        commit_profile_generation_root(&roots, &prepared, 2).unwrap();

        for mut entry in [base.entry, middle.entry, consumer.entry] {
            entry.last_used_at = 1;
            fs::write(
                roots.hangar_dir().join(&entry.id).join("meta.json"),
                entry.meta_json(),
            )
            .unwrap();
        }
        assert_eq!(clean_plan(&roots).unwrap().removed_objects, 0);
        assert_eq!(clean(&roots).unwrap().removed_objects, 0);
        assert_eq!(list_checked(&roots).unwrap().len(), 3);
    }

    #[test]
    fn external_consumer_root_resumes_exactly_and_never_rebinds() {
        let (roots, _g) = temp_roots();
        let first = ingest_fixture(&roots, "external-first", &[("out", "first")], Vec::new());
        let second = ingest_fixture(&roots, "external-second", &[("out", "second")], Vec::new());
        let first_hash = first.entry.envelope.output_hash.clone();
        let second_hash = second.entry.envelope.output_hash.clone();
        let witness = format!("sha256-{}", "1".repeat(64));

        let prepared = reconcile_external_consumer_root(
            &roots,
            "jetos-generation",
            "host\0generation",
            &witness,
            vec![first_hash.clone()],
            1,
        )
        .unwrap()
        .unwrap();
        let resumed = reconcile_external_consumer_root(
            &roots,
            "jetos-generation",
            "host\0generation",
            &witness,
            vec![first_hash.clone()],
            9,
        )
        .unwrap()
        .unwrap();
        commit_external_consumer_root(&roots, &resumed, 10).unwrap();
        assert!(reconcile_external_consumer_root(
            &roots,
            "jetos-generation",
            "host\0generation",
            &witness,
            vec![first_hash.clone()],
            11,
        )
        .unwrap()
        .is_none());
        drop(prepared);

        assert!(reconcile_external_consumer_root(
            &roots,
            "jetos-generation",
            "host\0generation",
            &format!("sha256-{}", "2".repeat(64)),
            vec![second_hash.clone()],
            12,
        )
        .is_err());
        assert!(reconcile_external_consumer_root(
            &roots,
            "jetos-generation",
            "other-host\0generation",
            &witness,
            vec!["/nix/store/not-a-hangar-object".to_string()],
            12,
        )
        .is_err());
        assert!(reconcile_external_consumer_root(
            &roots,
            "jetos-generation",
            "host\0generation",
            &witness,
            vec![second_hash],
            12,
        )
        .is_err());
        let snapshot = Lifecycle::snapshot(&roots).unwrap();
        let root = snapshot.roots.values().next().unwrap();
        assert_eq!(root.identity.kind, Lifecycle::RootKind::ExternalConsumer);
        assert_eq!(root.identity.id.as_str(), format!(
            "external-consumer:jetos-generation:{}",
            SHA256::sha256_hex(b"host\0generation")
        ));
        assert_eq!(root.identity.producer.as_str(), "jetos-generation");
        assert_eq!(root.identity.incarnation.get(), 1);
        assert_eq!(root.identity.witness.as_str(), witness);
        assert_eq!(root.targets, BTreeSet::from([first_hash.clone()]));
        assert_eq!(root.protected_targets, BTreeSet::from([first_hash]));
        assert_eq!(root.phase, Lifecycle::RootPhase::Committed);
    }

    #[test]
    fn ids_differ_by_ref() {
        let a = entry_id("x", "1.0", "nixpkgs:x", "/o");
        let b = entry_id("x", "1.0", "github:o/x", "/o");
        assert_ne!(a, b);
    }

    #[test]
    fn id_omits_empty_version() {
        // Unknown version falls back to `<name>-<fp>`, no dangling segment.
        let id = entry_id("x", "", "nixpkgs:x", "/o");
        assert!(id.starts_with("x-"));
        assert!(!id.starts_with("x--"));
    }

    #[test]
    fn verified_cache_rejects_deleted_and_tampered_outputs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:demo",
            "core-source",
        );
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let expectation = test_expectation(&out);
        let entry = find_by_reference(&roots, "mine:demo").unwrap();
        let proof = verify_cache_entry(&roots, &entry, "mine:demo", &expectation);
        assert!(!proof.signature_verified);
        assert!(proof.unsigned_local_allowed);
        assert!(verified(&roots, "mine:demo", &expectation));

        fs::write(out.join("payload"), "tampered").unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));

        fs::remove_dir_all(&out).unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));
    }

    #[test]
    fn verified_cache_rejects_wrong_platform_and_incomplete_proof() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let mut envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:demo",
            "core-source",
        );
        envelope.platform = "not-this-host".to_string();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let mut expectation = test_expectation(&out);
        assert!(!verified(&roots, "mine:demo", &expectation));

        envelope.platform = super::super::super::Envelope::host_platform();
        envelope.provenance.clear();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));

        envelope.provenance = "mine:demo via core-source".to_string();
        envelope.signature = "unverified-signature-text".to_string();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(!verified(&roots, "mine:demo", &expectation));

        envelope.signature.clear();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "mine:demo",
            &out.to_string_lossy(),
            &out.join("missing-bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        expectation.owned_output = Some(out.clone());
        assert!(!verified(&roots, "mine:demo", &expectation));
    }

    #[test]
    fn closure_rejects_parent_traversal_to_sibling() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        let sibling = roots.root.join("other");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("tool"), "outside").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:escape",
            "core-source",
        );
        let mut entry = record_verified(
            &roots,
            "escape",
            "1",
            "mine:escape",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        entry.bin = out.join("../other").to_string_lossy().into_owned();
        let proof = verify_cache_entry(
            &roots,
            &entry,
            "mine:escape",
            &test_expectation(&out),
        );
        assert!(!proof.closure);
        assert!(!proof.trusted());
    }

    #[cfg(unix)]
    #[test]
    fn nix_compat_output_gets_durable_gc_root() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let entry = roots.root.join("entry");
        let out = roots.root.join("fake-nix-output");
        let helper = roots.root.join("fake-nix-store");
        fs::create_dir_all(&entry).unwrap();
        fs::create_dir_all(&out).unwrap();
        fs::write(&helper, "#!/bin/sh\nln -s \"$5\" \"$2\"\n").unwrap();
        let mut perms = fs::metadata(&helper).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).unwrap();

        pin_nix_gc_root_with(&entry, &out, &helper).unwrap();
        assert_eq!(fs::read_link(entry.join(NIX_GC_ROOT)).unwrap(), out);
    }

    #[cfg(unix)]
    #[test]
    fn startup_migration_roots_existing_real_paths() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let prefix = roots.root.join("nix/store");
        let out = prefix.join("abc-demo");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "demo").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "nixpkgs:demo",
            "nix",
        );
        let entry = record(
            &roots,
            "demo",
            "1.0",
            "nixpkgs:demo",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
        )
        .unwrap();
        let helper = roots.root.join("fake-nix-store-migrate");
        fs::write(&helper, "#!/bin/sh\nln -s \"$5\" \"$2\"\n").unwrap();
        let mut perms = fs::metadata(&helper).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).unwrap();

        assert_eq!(migrate_nix_gc_roots_with(&roots, &prefix, &helper).unwrap(), 1);
        let root = roots.hangar_dir().join(entry.id).join(NIX_GC_ROOT);
        assert_eq!(fs::canonicalize(root).unwrap(), fs::canonicalize(out).unwrap());
    }

    #[test]
    fn hostile_out_pointer_never_quarantines_another_object() {
        let (roots, _g) = temp_roots();
        let survivor = roots.hangar_dir().join("survivor-output");
        let expected = roots.hangar_dir().join("expected-output");
        fs::create_dir_all(&survivor).unwrap();
        fs::create_dir_all(&expected).unwrap();
        fs::write(survivor.join("keep"), "survivor").unwrap();
        fs::write(expected.join("bad"), "candidate").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &survivor.to_string_lossy(),
            "mine:hostile",
            "core-source",
        );
        let entry = record_verified(
            &roots,
            "hostile",
            "1.0",
            "mine:hostile",
            &survivor.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: Some(expected.clone()),
            allow_unsigned_local: true,
        };

        quarantine_invalid_entry(&roots, &entry, &expectation).unwrap();
        assert_eq!(fs::read_to_string(survivor.join("keep")).unwrap(), "survivor");
        assert!(!expected.exists());
    }

    #[test]
    fn quarantine_moves_sealed_owned_output() {
        let (roots, _g) = temp_roots();
        let ingested = ingest_fixture(&roots, "sealed", &[("out", "tampered")], Vec::new());
        let entry = ingested.entry;
        let out = PathBuf::from(&entry.out);

        quarantine_invalid_entry(&roots, &entry, &test_expectation(&out)).unwrap();

        assert!(!out.exists());
        assert!(find_by_reference(&roots, "path:sealed").is_none());
        assert!(fs::read_dir(roots.hangar_dir().join("quarantine"))
            .unwrap()
            .flatten()
            .any(|item| item.file_name().to_string_lossy().starts_with(&format!(
                "output-{}-",
                entry.envelope.output_hash
            ))));
        ingest_fixture(&roots, "sealed", &[("out", "tampered")], Vec::new());
    }

    #[test]
    fn cache_lease_is_private_snapshot_without_long_object_lock() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("leased-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:leased",
            "core-source",
        );
        record_verified(
            &roots,
            "leased",
            "1.0",
            "mine:leased",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let hit = find_verified_by_reference(
            &roots,
            "mine:leased",
            &test_expectation(&out),
        )
        .unwrap()
        .unwrap();
        fs::write(out.join("payload"), "mutated outside cooperative lock").unwrap();
        hit.lease.validate().unwrap();
        let stable = hit
            .lease
            .stable_path(&out.join("payload").to_string_lossy())
            .unwrap();
        assert_eq!(fs::read_to_string(stable).unwrap(), "trusted");
    }

    #[test]
    fn realization_type_distinguishes_fresh_cached_and_missing_outputs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("typed-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:typed",
            "core-source",
        );
        let entry = record_verified(
            &roots,
            "typed",
            "1",
            "mine:typed",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();

        let fresh = VerifiedRealization {
            lease: snapshot_lease(&roots, &entry).unwrap(),
            entry: entry.clone(),
            source_state: super::super::super::Provider::SourceState::Built,
        };
        assert_eq!(fresh.consumption_status(), &ConsumptionStatus::Consumable);
        assert!(fresh
            .lease
            .stable_path(&out.join("payload").to_string_lossy())
            .is_ok());

        let hit = find_verified_by_reference(&roots, "mine:typed", &test_expectation(&out))
            .unwrap()
            .unwrap();
        let cached = VerifiedRealization {
            entry: hit.entry,
            source_state: super::super::super::Provider::SourceState::Cached,
            lease: hit.lease,
        };
        assert_eq!(cached.consumption_status(), &ConsumptionStatus::Consumable);

        let missing_out = roots.root.join("missing-output");
        let missing_entry = StoreEntry {
            id: "missing-1-test".to_string(),
            name: "missing".to_string(),
            version: "1".to_string(),
            reference: "mine:missing".to_string(),
            out: missing_out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope: super::super::super::Envelope::Envelope::default(),
            cache_identity: test_identity(),
            references: Vec::new(),
            named_outputs: BTreeMap::new(),
            platform_artifact_kind: String::new(),
            producer_record: String::new(),
            realized_at: 0,
            last_used_at: 0,
        };
        let missing = VerifiedRealization {
            lease: snapshot_lease(&roots, &missing_entry).unwrap(),
            entry: missing_entry,
            source_state: super::super::super::Provider::SourceState::Built,
        };
        assert!(matches!(
            missing.consumption_status(),
            ConsumptionStatus::NonConsumable { .. }
        ));
        assert!(missing.lease.stable_path(&missing_out.to_string_lossy()).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn leased_fd_view_executes_original_after_rename_symlink_swap() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};

        let (roots, _g) = temp_roots();
        let out = roots.root.join("fd-view-output");
        let tool = out.join("bin/tool");
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(out.join("bin/tool-real"), "#!/bin/sh\nprintf trusted").unwrap();
        fs::set_permissions(
            out.join("bin/tool-real"),
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        symlink("tool-real", &tool).unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "mine:fd-view",
            "core-source",
        );
        record_verified(
            &roots,
            "fd-view",
            "1",
            "mine:fd-view",
            &out.to_string_lossy(),
            &out.join("bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let hit = find_verified_by_reference(
            &roots,
            "mine:fd-view",
            &test_expectation(&out),
        )
        .unwrap()
        .unwrap();
        let stable_tool = hit.lease.stable_path(&tool.to_string_lossy()).unwrap();

        let moved = roots.root.join("fd-view-original");
        let attacker = roots.root.join("fd-view-attacker");
        fs::rename(&out, &moved).unwrap();
        fs::create_dir_all(attacker.join("bin")).unwrap();
        fs::write(attacker.join("bin/tool"), "#!/bin/sh\nprintf attacker").unwrap();
        fs::set_permissions(
            attacker.join("bin/tool"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        symlink(&attacker, &out).unwrap();

        hit.lease.validate().unwrap();
        let wrapper = hit.lease.wrapper_dir().unwrap();
        assert!(fs::set_permissions(wrapper, fs::Permissions::from_mode(0o755)).is_err());
        assert!(fs::remove_file(wrapper.join("tool")).is_err());
        hit.lease.validate().unwrap();
        make_tree_writable_for_removal(&hit.lease.snapshot_root).unwrap();
        let snapshot_tool = hit.lease.snapshot_root.join("bin/tool");
        fs::remove_file(&snapshot_tool).unwrap();
        fs::write(&snapshot_tool, "#!/bin/sh\nprintf snapshot-attacker").unwrap();
        fs::set_permissions(&snapshot_tool, fs::Permissions::from_mode(0o755)).unwrap();

        let output = Command::new(&stable_tool).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "trusted");
        let nested = Command::new("/bin/sh")
            .args(["-c", "exec tool"])
            .env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin",
                    hit.lease.wrapper_dir().unwrap().display()
                ),
            )
            .output()
            .unwrap();
        assert!(nested.status.success());
        assert_eq!(String::from_utf8(nested.stdout).unwrap(), "trusted");
        assert_eq!(fs::read_to_string(out.join("bin/tool")).unwrap(), "#!/bin/sh\nprintf attacker");
    }

    #[test]
    fn configured_key_is_required_before_signature_verifier_runs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("signed-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "signed").unwrap();
        let mut envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "cache:demo",
            "remote-cache",
        );
        envelope.signature = "ed25519:abcd".to_string();
        let entry = StoreEntry {
            id: entry_id("demo", "1", "cache:demo", &out.to_string_lossy()),
            name: "demo".to_string(),
            version: "1".to_string(),
            reference: "cache:demo".to_string(),
            out: out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope,
            cache_identity: test_identity(),
            references: Vec::new(),
            named_outputs: BTreeMap::new(),
            platform_artifact_kind: String::new(),
            producer_record: String::new(),
            realized_at: 0,
            last_used_at: 0,
        };
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: None,
            allow_unsigned_local: false,
        };
        let mut called = false;
        assert!(!verify_configured_signature_with(
            &roots,
            &entry,
            &expectation,
            |_, _, _| {
                called = true;
                true
            }
        ));
        assert!(!called);

        fs::create_dir_all(roots.root.join("trust")).unwrap();
        fs::write(roots.root.join("trust/cache.ed25519.pub"), "public-key").unwrap();
        assert!(verify_configured_signature_with(
            &roots,
            &entry,
            &expectation,
            |key, message, signature| {
                key == "public-key"
                    && message.contains("source=source-v1")
                    && signature == "abcd"
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn mutable_replaced_crypto_helper_is_never_a_trust_root() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let out = roots.root.join("signed-output-hostile");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "signed").unwrap();
        fs::create_dir_all(roots.root.join("trust")).unwrap();
        fs::write(roots.root.join("trust/cache.ed25519.pub"), "attacker-key").unwrap();
        let marker = roots.root.join("helper-ran");
        let helper = roots.root.join("trust/jet-crypto-helper");
        fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        let mut envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "cache:hostile",
            "remote-cache",
        );
        envelope.signature = "ed25519:forged".to_string();
        let entry = StoreEntry {
            id: entry_id("hostile", "1", "cache:hostile", &out.to_string_lossy()),
            name: "hostile".to_string(),
            version: "1".to_string(),
            reference: "cache:hostile".to_string(),
            out: out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope,
            cache_identity: test_identity(),
            references: Vec::new(),
            named_outputs: BTreeMap::new(),
            platform_artifact_kind: String::new(),
            producer_record: String::new(),
            realized_at: 0,
            last_used_at: 0,
        };
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: None,
            allow_unsigned_local: false,
        };
        let proof = verify_cache_entry(&roots, &entry, "cache:hostile", &expectation);
        assert!(!proof.signature_verified);
        assert!(!proof.trusted());
        assert!(!marker.exists(), "mutable helper must never execute");
    }

    #[test]
    fn e2604_reason_snapshot_is_exact() {
        let failure = IntegrityFailure {
            package: "demo".to_string(),
            version: "1.2.3".to_string(),
            expected: "sha256-good".to_string(),
            actual: "sha256-bad".to_string(),
            reason: "content digest verification".to_string(),
            disposition: "Jetpack quarantined it instead of using or silently repairing it."
                .to_string(),
            fix: "Re-run `jet store fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
                .to_string(),
        };
        assert_eq!(
            failure.what(),
            "Integrity check failed for `demo` `1.2.3` — expected `sha256-good`, got `sha256-bad`."
        );
        assert_eq!(
            failure.why(),
            "The cached artifact failed content digest verification. Jetpack quarantined it instead of using or silently repairing it."
        );
        assert_eq!(
            failure.fix(),
            "Re-run `jet store fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
        );
    }

    #[test]
    fn ingest_atomic_dedupes_same_bytes_and_records_referrers() {
        let (roots, _g) = temp_roots();
        let src_a = roots.root.join("src-a");
        let src_b = roots.root.join("src-b");
        let src_c = roots.root.join("src-c");
        fs::create_dir_all(&src_a).unwrap();
        fs::create_dir_all(&src_b).unwrap();
        fs::create_dir_all(&src_c).unwrap();
        fs::write(src_a.join("payload"), "same-bytes").unwrap();
        fs::write(src_b.join("payload"), "same-bytes").unwrap();
        fs::write(src_c.join("payload"), "different").unwrap();

        let mut outs_a = BTreeMap::new();
        outs_a.insert("out".to_string(), src_a);
        let first = ingest_tree(
            &roots,
            &IngestRequest {
                name: "alpha".into(),
                version: "1".into(),
                reference: "path:a".into(),
                cache_identity: test_identity(),
                references: Vec::new(),
                outputs: outs_a,
                signature: String::new(),
                provenance: "test via hangar-ingest".into(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap();
        assert!(!first.deduplicated);
        assert!(first.entry.envelope.output_hash.starts_with("sha256-"));

        let mut outs_b = BTreeMap::new();
        outs_b.insert("out".to_string(), src_b);
        let second = ingest_tree(
            &roots,
            &IngestRequest {
                name: "beta".into(),
                version: "1".into(),
                reference: "path:b".into(),
                cache_identity: test_identity(),
                references: Vec::new(),
                outputs: outs_b,
                signature: String::new(),
                provenance: "test via hangar-ingest".into(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap();
        assert!(second.deduplicated);
        assert_eq!(
            first.entry.envelope.output_hash,
            second.entry.envelope.output_hash
        );

        let mut outs_c = BTreeMap::new();
        outs_c.insert("out".to_string(), src_c);
        let third = ingest_tree(
            &roots,
            &IngestRequest {
                name: "gamma".into(),
                version: "1".into(),
                reference: "path:c".into(),
                cache_identity: test_identity(),
                references: vec![first.entry.envelope.output_hash.clone()],
                outputs: outs_c,
                signature: String::new(),
                provenance: "test via hangar-ingest".into(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap();
        assert!(!third.deduplicated);
        assert_ne!(
            first.entry.envelope.output_hash,
            third.entry.envelope.output_hash
        );
        assert_eq!(
            referrers_of(&roots, &first.entry.envelope.output_hash).unwrap(),
            vec![third.entry.envelope.output_hash.clone()]
        );
        assert_eq!(recover_hangar_staging(&roots).unwrap(), 0);
    }

    #[test]
    fn ingest_retry_after_object_publish_seals_before_metadata() {
        let (roots, _g) = temp_roots();
        let first = ingest_fixture(&roots, "retry-crash", &[("out", "retry")], Vec::new());
        let object = PathBuf::from(&first.entry.out);
        make_tree_writable_for_removal(&object).unwrap();
        fs::remove_dir_all(roots.hangar_dir().join(&first.entry.id)).unwrap();
        fs::remove_dir_all(roots.hangar_dir().join("closure-db")).unwrap();

        let retry = ingest_fixture(&roots, "retry-crash", &[("out", "retry")], Vec::new());
        assert!(retry.deduplicated);
        assert!(fs::metadata(&object).unwrap().permissions().readonly());
        assert!(fs::metadata(object.join("payload"))
            .unwrap()
            .permissions()
            .readonly());
        assert!(roots
            .hangar_dir()
            .join(&retry.entry.id)
            .join("meta.json")
            .is_file());
        assert!(closure_graph(&roots)
            .unwrap()
            .records
            .contains_key(&retry.entry.id));
    }

    #[test]
    fn ingest_rejects_path_law_reserved_name() {
        let (roots, _g) = temp_roots();
        let src = roots.root.join("src-bad");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("CON"), "nope").unwrap();
        let mut outputs = BTreeMap::new();
        outputs.insert("out".to_string(), src);
        let err = ingest_tree(
            &roots,
            &IngestRequest {
                name: "bad".into(),
                version: "1".into(),
                reference: "path:bad".into(),
                cache_identity: test_identity(),
                references: Vec::new(),
                outputs,
                signature: String::new(),
                provenance: String::new(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "E1299");
        assert!(err.what().contains("store path") || err.why().contains("reserved") || err.why().contains("CON") || format!("{err:?}").contains("reserved"), "{err:?}");
    }

    #[test]
    fn ingest_rejects_output_names_that_escape_staging() {
        for alias in [
            "../escaped-output",
            "..\\escaped-output",
            ".",
            "out/",
            "out//",
            "out/.",
            "out/./",
            "out\\",
            "out\\.",
        ] {
            let (roots, _g) = temp_roots();
            let src = roots.root.join("src-output-name");
            fs::create_dir_all(&src).unwrap();
            fs::write(src.join("payload"), "bytes").unwrap();
            let escaped = roots.root.join("escaped-output");
            let mut outputs = BTreeMap::new();
            outputs.insert("out".to_string(), src.clone());
            outputs.insert(alias.to_string(), src);
            let err = ingest_tree(
                &roots,
                &IngestRequest {
                    name: "bad-output-name".into(),
                    version: "1".into(),
                    reference: "path:bad-output-name".into(),
                    cache_identity: test_identity(),
                    references: Vec::new(),
                    outputs,
                    signature: String::new(),
                    provenance: String::new(),
                    platform_artifact_kind: String::new(),
                },
            )
            .unwrap_err();
            assert_eq!(err.code(), "E1315", "alias {alias:?}: {err:?}");
            assert!(
                err.what().contains("one path component"),
                "alias {alias:?}: {err:?}"
            );
            assert!(!roots.hangar_dir().exists(), "alias {alias:?}");
            assert!(!escaped.exists(), "alias {alias:?}");
        }
    }

    #[test]
    fn ingest_installs_valid_secondary_named_output_with_matching_digest() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("src-primary");
        let dev = roots.root.join("src-dev");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&dev).unwrap();
        fs::write(out.join("payload"), "primary").unwrap();
        fs::write(dev.join("payload"), "development").unwrap();
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
        assert_eq!(
            fs::read_to_string(installed.join("payload")).unwrap(),
            "development"
        );
        let actual = super::super::super::Envelope::try_output_hash_of(&installed.to_string_lossy())
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
        assert_eq!(action_outputs_of(&roots, &action).unwrap().get("dev"), Some(&dev));
        assert_eq!(actions_for_output(&roots, &primary).unwrap(), vec![action]);

        ingest_fixture(
            &roots,
            "base",
            &[("out", "base")],
            vec![primary.clone()],
        );
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
            let envelope = super::super::super::Envelope::Envelope::for_output(
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
        let mut entry = ingest_fixture(
            &roots,
            "action-projection",
            &[("out", "bytes")],
            Vec::new(),
        )
        .entry;
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
                ])).unwrap(),
                format!("nix-derivation:{drv}"),
                "policy=test\nplatform=test",
                BTreeMap::from([
                    ("nix.output.out".into(), output.into()),
                ]),
            ).unwrap().encode()
        };
        first.reference = "nixpkgs:first".into();
        first.cache_identity.source_fingerprint = "sha256-first-output".into();
        first.producer_record = nix_record("/nix/store/action.drv", "/nix/store/first", "nixpkgs:first");
        let action = entry_action_key(&first);

        let mut second = first.clone();
        second.reference = "alias:second".into();
        second.cache_identity.source_fingerprint = "sha256-second-output".into();
        second.producer_record = nix_record("/nix/store/action.drv", "/nix/store/second", "alias:second");
        assert_eq!(entry_action_key(&second), action);

        second.producer_record = nix_record("/nix/store/other.drv", "/nix/store/second", "alias:second");
        assert_ne!(entry_action_key(&second), action);
    }

    #[test]
    fn nix_multi_projection_registers_recovers_queries_and_rolls_back_conflict() {
        let (roots, _g) = temp_roots();
        let out = ingest_fixture(&roots, "projection-out-bytes", &[("out", "out")], Vec::new());
        let dev = ingest_fixture(&roots, "projection-dev-bytes", &[("out", "dev")], Vec::new());
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
                ])).unwrap(),
                format!("nix-derivation:{drv}"),
                "policy=test\nplatform=test",
                BTreeMap::from([(format!("nix.output.{output_name}"), path)]),
            ).unwrap().encode();
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
        }).unwrap();
        fs::remove_file(roots.hangar_dir().join(&dev.id).join("meta.json")).unwrap();
        let outputs = action_outputs_of(&roots, &action).unwrap();
        assert_eq!(outputs.get("out"), Some(&out.envelope.output_hash));
        assert_eq!(outputs.get("dev"), Some(&dev.envelope.output_hash));
        assert!(roots.hangar_dir().join(&dev.id).join("meta.json").is_file());

        let before = closure_graph(&roots).unwrap();
        let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked(&roots, &bad)
        }).unwrap_err();
        assert!(error.to_string().contains("conflicting bytes"));
        assert_eq!(closure_graph(&roots).unwrap(), before);
    }

    #[test]
    fn closure_empty_reference_proof_rejects_unknown_provider() {
        let (roots, _g) = temp_roots();
        let mut entry = ingest_fixture(&roots, "unknown-proof", &[("out", "bytes")], Vec::new()).entry;
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
        ).unwrap().encode();
        let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked(&roots, &entry)
        }).unwrap_err();
        assert!(error.to_string().contains("store-validated closure proof"));
    }

    #[test]
    fn closure_rejects_named_out_that_disagrees_with_primary() {
        let (roots, _g) = temp_roots();
        let primary = ingest_fixture(&roots, "named-out-primary", &[("out", "primary")], Vec::new());
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
        conflicting.named_outputs = BTreeMap::from([(
            "out".to_string(),
            named.entry.envelope.output_hash.clone(),
        )]);
        let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked(&roots, &conflicting)
        })
        .unwrap_err();
        assert!(error.to_string().contains("conflicting bytes"));
        assert_eq!(action_outputs_of(&roots, &action).unwrap().len(), 1);
        assert!(!action_outputs_of(&roots, &action).unwrap().contains_key("dev"));
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
        fs::write(legacy_referrers.join("fake.refs"), "fallback-must-not-win\n").unwrap();
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
        let meta = roots.hangar_dir().join(&ingested.entry.id).join("meta.json");
        let expected = ingested.entry.meta_json();
        fs::write(&meta, "stale").unwrap();

        assert_eq!(recover_hangar(&roots).unwrap(), 2);
        assert!(!abandoned.exists());
        assert_eq!(fs::read_to_string(meta).unwrap(), expected);
    }

    #[test]
    fn closure_legacy_migration_is_idempotent() {
        let (roots, _g) = temp_roots();
        let mut first = ingest_fixture(&roots, "legacy-first", &[("out", "first")], Vec::new()).entry;
        let mut second = ingest_fixture(&roots, "legacy-second", &[("out", "second")], Vec::new()).entry;
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
        assert_eq!(graph.direct_references(&first.envelope.output_hash), first.references);
        assert_eq!(graph.direct_references(&second.envelope.output_hash), second.references);
    }

    #[test]
    fn closure_legacy_migration_rejects_atomically() {
        let (roots, _g) = temp_roots();
        let mut entry = ingest_fixture(&roots, "legacy-invalid", &[("out", "invalid")], Vec::new()).entry;
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
        let envelope = super::super::super::Envelope::Envelope::for_output(
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
        let names = super::super::super::Envelope::list_xattr_names(&sealed).unwrap();
        assert!(
            names.iter().any(|n| n == "user.jet.test"),
            "semantic xattr must be preserved on sealed object: {names:?}"
        );
        let root_names = super::super::super::Envelope::list_xattr_names(
            Path::new(&ingested.entry.out),
        )
        .unwrap();
        assert!(root_names.iter().any(|name| name == "user.jet.directory"));
        set_user_xattr(&src, "user.jet.directory", b"changed");
        let changed_hash = super::super::super::Envelope::try_output_hash_of_with_policy(
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
        assert!(super::super::super::Envelope::list_xattr_names(&sealed)
            .unwrap()
            .iter()
            .any(|name| name == "user.jet.symlink"));
        let first = ingested.entry.envelope.output_hash;
        set_apple_xattr(&src.join("link"), "user.jet.symlink", b"second");
        let second = super::super::super::Envelope::try_output_hash_of_with_policy(
            &src.to_string_lossy(),
            true,
            &mut |_, _| {},
        )
        .unwrap();
        assert_ne!(first, second);
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

        // Ingest leaves nlink=1 (no cas peers yet).
        let pay_c = Path::new(&third.entry.out).join("payload");
        assert_eq!(fs::metadata(&pay_c).unwrap().nlink(), 1);

        let report = optimize_cas_pool(&roots).unwrap();
        assert!(report.optimized_files >= 2, "{report:?}");
        assert!(roots.hangar_dir().join("cas").is_dir());
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

        // Outside-hangar peer still rejected.
        let outside = roots.root.join("outside-peer");
        fs::hard_link(&pay_c, &outside).unwrap();
        let bare = super::super::super::Envelope::try_output_hash_of(&third.entry.out);
        assert!(bare.is_err(), "{bare:?}");
        let in_hangar = super::super::super::Envelope::try_output_hash_of_in_hangar(
            &third.entry.out,
            &roots.hangar_dir(),
            false,
        );
        assert!(in_hangar.is_err(), "{in_hangar:?}");
        let proof = verify_cache_entry(&roots, &third.entry, &third.entry.reference, &expectation);
        assert!(!proof.output_digest, "{proof:?}");
        assert!(!proof.trusted(), "{proof:?}");
        fs::remove_file(outside).ok();
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
        assert_eq!(rc, 0, "lsetxattr failed: {}", std::io::Error::last_os_error());
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
        assert_eq!(rc, 0, "setxattr failed: {}", std::io::Error::last_os_error());
    }
}

/// Minimal scoped tempdir for tests (std-only; auto-removes on drop).
#[cfg(test)]
mod tempdir {
    use std::path::PathBuf;

    pub struct Guard {
        pub path: PathBuf,
    }

    impl Guard {
        pub fn new(tag: &str) -> Guard {
            let mut path = std::env::temp_dir();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            path.push(format!("{tag}-{nanos}-{:?}", std::thread::current().id()));
            std::fs::create_dir_all(&path).unwrap();
            Guard { path }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[test]
fn store_stays_split_along_existing_phases() {
    const MAX_MODULE_LINES: usize = 2500;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |relative: &str| std::fs::read_to_string(root.join(relative)).unwrap();
    let store = read("src/Store.rs");
    let ingest = read("src/Store/Ingest.rs");
    let closure = read("src/Store/Closure.rs");
    let tests = read("src/Store/Tests.rs");
    let tests_production = tests
        .split("#[test]\nfn store_stays_split_along_existing_phases")
        .next()
        .unwrap();

    for (relative, source) in [
        ("src/Store.rs", store.as_str()),
        ("src/Store/Ingest.rs", ingest.as_str()),
        ("src/Store/Closure.rs", closure.as_str()),
        ("src/Store/Tests.rs", tests_production),
    ] {
        assert!(
            source.lines().count() < MAX_MODULE_LINES,
            "{relative} must stay below the card #510 module boundary"
        );
        assert!(!source.contains("include!("));
        assert!(!source.contains("#[path"));
    }
    assert!(store.contains("\nmod Ingest;\npub use Ingest::*;"));
    assert!(store.contains("\nmod Closure;\npub use Closure::*;"));
    assert!(store.contains("\n#[cfg(test)]\nmod Tests;"));

    let ordered = [
        "pub struct IngestRequest",
        "pub fn recover_hangar_staging",
        "pub fn ingest_tree",
        "fn copy_nofollow_tree",
        "fn stable_meta_identity",
    ];
    let positions: Vec<usize> = ordered
        .iter()
        .map(|needle| ingest.find(needle).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    let closure_ordered = [
        "pub fn closure_graph",
        "pub fn referrers_of",
        "pub fn migrate_closure_graph",
        "fn load_graph",
        "fn validate_graph",
    ];
    let positions: Vec<usize> = closure_ordered
        .iter()
        .map(|needle| closure.find(needle).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
