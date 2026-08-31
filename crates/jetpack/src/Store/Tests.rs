use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    mod late;
    mod nix_projection;

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
            "fastfetch@nixpkgs",
            "nix",
        );
        let mut e = record_verified(
            &roots,
            "fastfetch",
            "2.1.0",
            "fastfetch@nixpkgs",
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
    fn abi_universe_isolation_keeps_native_closures_hangar_only() {
        fn fixture(
            roots: &Roots,
            name: &str,
            provider: &str,
            references: Vec<String>,
            with_rlib: bool,
        ) -> crate::Provider::Realized {
            let out = roots.hangar_dir().join(format!("abi-{name}"));
            fs::create_dir_all(out.join("lib")).unwrap();
            fs::write(out.join("lib").join(format!("lib{name}.so")), name).unwrap();
            let rlib = if with_rlib {
                let path = out.join(format!("lib{name}.rlib"));
                fs::write(&path, format!("{name}-rlib")).unwrap();
                path.to_string_lossy().into_owned()
            } else {
                String::new()
            };
            let reference = if provider == "nix" {
                format!("{name}@nixpkgs")
            } else {
                format!("{name}@core")
            };
            let output = out.to_string_lossy().into_owned();
            let mut facts = if provider == "nix" {
                BTreeMap::from([("nix.output.out".into(), output.clone())])
            } else {
                BTreeMap::new()
            };
            if provider == "nix" {
                facts.extend(crate::Provider::nix_build_facts_record());
            }
            seal_node(&out).unwrap();
            let encoded = canonical_producer(
                provider,
                &format!("abi:{name}"),
                &format!("source:{name}"),
                &test_identity(),
                facts,
            )
            .unwrap();
            crate::Provider::Realized {
                name: name.to_string(),
                version: "1".to_string(),
                reference,
                out: output.clone(),
                bin: String::new(),
                rlib,
                envelope: super::super::super::Envelope::Envelope::for_output(
                    &output,
                    &format!("{name}@{provider}"),
                    provider,
                ),
                cache_identity: test_identity(),
                source_state: if provider == "nix" {
                    crate::Provider::SourceState::Substituted
                } else {
                    crate::Provider::SourceState::Built
                },
                named_outputs: BTreeMap::from([("out".to_string(), output)]),
                references,
                producer: ProducerRecord::decode(&encoded).unwrap(),
            }
        }

        let (roots, _guard) = temp_roots();

        // Stage-1 compat bytes are Hangar-owned, but remain a separate
        // provider universe.
        let compat =
            record_realized_mode(&roots, &fixture(&roots, "compat", "nix", Vec::new(), false))
                .unwrap();
        let compat_digest = compat.envelope.output_hash.clone();

        // A native runtime closure may contain a Hangar-native dependency and
        // a native library artifact, but no compat-root object.
        let native_dependency = ingest_fixture(
            &roots,
            "abi-native-dependency",
            &[("out", "native dependency")],
            Vec::new(),
        )
        .entry;
        let native = record_realized_mode(
            &roots,
            &fixture(
                &roots,
                "native",
                "core",
                vec![native_dependency.envelope.output_hash.clone()],
                true,
            ),
        )
        .unwrap();
        assert!(
            Path::new(&native.rlib).starts_with(roots.hangar_dir().join("objects")),
            "native library artifact escaped the Hangar object pool: {}",
            native.rlib
        );

        let graph = closure_graph(&roots).unwrap();
        let native_closure = graph.closure(&native.envelope.output_hash);
        assert!(
            native_closure.iter().all(|digest| graph
                .objects
                .get(digest)
                .is_some_and(|object| !object.external)),
            "native closure contains a non-Hangar object: {native_closure:?}"
        );
        assert!(
            graph.transitive_references(&compat_digest).is_empty(),
            "compat closure borrowed native objects"
        );

        // Both directions are guarded: removing either the pre-registration
        // check or the graph check makes one of these registrations succeed.
        let native_leak = fixture(
            &roots,
            "native-leak",
            "core",
            vec![compat_digest.clone()],
            true,
        );
        let error = record_realized_mode(&roots, &native_leak).unwrap_err();
        assert!(error.to_string().contains("ABI universe"), "{error}");
        assert!(!roots
            .hangar_dir()
            .join("objects")
            .join(&native_leak.envelope.output_hash)
            .exists());

        let compat_leak = fixture(
            &roots,
            "compat-leak",
            "nix",
            vec![native.envelope.output_hash.clone()],
            false,
        );
        let error = record_realized_mode(&roots, &compat_leak).unwrap_err();
        assert!(error.to_string().contains("ABI universe"), "{error}");
    }

    #[test]
    fn clean_keeps_fresh_entries() {
        let (roots, _g) = temp_roots();
        for name in ["a", "b"] {
            let reference = format!("{name}@nixpkgs");
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
    fn clean_quarantines_malformed_objects_and_continues() {
        let (roots, _g) = temp_roots();
        let mut stale = ingest_fixture(&roots, "gc-stale", &[("out", "stale")], Vec::new()).entry;
        let fresh = ingest_fixture(&roots, "gc-fresh", &[("out", "fresh")], Vec::new()).entry;
        stale.last_used_at = 1;
        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked(&roots, &stale)
        })
        .unwrap();

        let malformed = roots.hangar_dir().join("malformed-object");
        fs::create_dir_all(&malformed).unwrap();
        fs::write(malformed.join("payload"), "bad metadata").unwrap();
        fs::write(malformed.join("meta.json"), "not json").unwrap();
        let metadata_less = roots.hangar_dir().join("metadata-less-object");
        fs::create_dir_all(&metadata_less).unwrap();
        fs::write(metadata_less.join("payload"), "no metadata").unwrap();

        assert_eq!(clean_plan(&roots).unwrap().quarantined_objects, 2);
        let report = clean(&roots).unwrap();
        assert_eq!(report.quarantined_objects, 2);
        assert!(!roots.hangar_dir().join(&stale.id).exists());
        assert!(roots.hangar_dir().join(&fresh.id).is_dir());
        let names = fs::read_dir(roots.hangar_dir().join("quarantine"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name.contains("malformed-object")));
        assert!(names
            .iter()
            .any(|name| name.contains("metadata-less-object")));
    }

    #[test]
    fn lock_receipt_root_keeps_entry_reachable_without_other_root_facts() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("receipt-root-output");
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/receipt-root"), "receipt-root").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "path:receipt-root",
            "path",
        );
        let entry = record_verified(
            &roots,
            "receipt-root",
            "1",
            "path:receipt-root",
            &out.to_string_lossy(),
            &out.join("bin/receipt-root").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(valid_receipt_digest(&entry.receipt));

        let project = roots.root.join("project");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        fs::create_dir_all(&managed).unwrap();
        fs::write(
            managed.join("lock"),
            format!(
                "version = 1\n\n[[package]]\nname = \"other-name\"\nversion = \"other-version\"\nsource = {{ path = \"{}\" }}\nfingerprint = \"\"\ndependencies = []\nreceipt = \"{}\"\n",
                entry.reference, entry.receipt
            ),
        )
        .unwrap();

        let live = lock_roots_from(&project).unwrap();
        let mut meta = parse_meta(&entry.meta_json()).unwrap();
        meta.envelope.output_hash.clear();
        meta.version = "not-the-lock-version".to_string();
        assert!(!is_live(&entry.id, &meta, &LiveRoots::default()));
        assert!(is_live(&entry.id, &meta, &live));
    }

    #[test]
    fn lock_receipt_root_keeps_connected_closure_reachable() {
        let (roots, _g) = temp_roots();
        let base = ingest_fixture(&roots, "receipt-base", &[("out", "base")], Vec::new());
        let middle = ingest_fixture(
            &roots,
            "receipt-middle",
            &[("out", "middle")],
            vec![base.entry.envelope.output_hash.clone()],
        );
        let consumer = ingest_fixture(
            &roots,
            "receipt-consumer",
            &[("out", "consumer")],
            vec![middle.entry.envelope.output_hash.clone()],
        );

        let project = roots.root.join("receipt-closure-project");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        fs::create_dir_all(&managed).unwrap();
        fs::write(
            managed.join("lock"),
            format!(
                "version = 1\n\n[[package]]\nname = \"receipt-consumer\"\nversion = \"1\"\nsource = {{ path = \"{}\" }}\nfingerprint = \"\"\ndependencies = []\nreceipt = \"{}\"\n",
                consumer.entry.reference, consumer.entry.receipt
            ),
        )
        .unwrap();

        let live = live_roots_from(&roots, &project).unwrap();
        for entry in [&base.entry, &middle.entry, &consumer.entry] {
            let meta = parse_meta(&entry.meta_json()).unwrap();
            assert!(
                is_live(&entry.id, &meta, &live),
                "receipt root did not retain connected entry {}",
                entry.id
            );
        }
        assert!(live.receipts.contains(&consumer.entry.receipt));
    }

    #[test]
    fn lock_receipt_identity_mismatch_fails_closed_before_cleanup() {
        let (roots, _g) = temp_roots();
        let entry = ingest_fixture(&roots, "receipt-mismatch", &[("out", "bytes")], Vec::new());
        let project = roots.root.join("receipt-mismatch-project");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        fs::create_dir_all(&managed).unwrap();
        fs::write(
            managed.join("lock"),
            format!(
                "version = 1\n\n[[package]]\nname = \"wrong-package\"\nversion = \"1\"\nsource = {{ path = \"{}\" }}\nfingerprint = \"\"\ndependencies = []\nreceipt = \"{}\"\n",
                entry.entry.reference, entry.entry.receipt
            ),
        )
        .unwrap();

        let error = live_roots_from(&roots, &project).unwrap_err();
        assert!(error
            .to_string()
            .contains("disagrees with Hangar closure record"));
        assert!(roots.hangar_dir().join(&entry.entry.id).exists());
        assert!(roots
            .hangar_dir()
            .join(Closure::RECEIPTS_DIR)
            .join(&entry.entry.receipt)
            .is_file());
    }

    #[test]
    fn lock_receipt_version_mismatch_fails_closed_before_projection() {
        let (roots, _g) = temp_roots();
        let entry = ingest_fixture(
            &roots,
            "receipt-version-mismatch",
            &[("out", "bytes")],
            Vec::new(),
        );
        let project = roots.root.join("receipt-version-mismatch-project");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        fs::create_dir_all(&managed).unwrap();
        fs::write(
            managed.join("lock"),
            format!(
                "version = 1\n\n[[package]]\nname = \"{}\"\nversion = \"wrong-version\"\nsource = {{ path = \"{}\" }}\nfingerprint = \"\"\ndependencies = []\n",
                entry.entry.name, entry.entry.reference
            ),
        )
        .unwrap();

        let error = super::record_receipt_projection(
            &project,
            &entry.entry.name,
            &entry.entry.version,
            &entry.entry.reference,
            &entry.entry.envelope.output_hash,
            &entry.entry.receipt,
        )
        .unwrap_err();
        assert!(error.to_string().contains("no matching package"));
        let lock = crate::Lock::load(&project).unwrap();
        assert!(lock.packages[0].receipt.is_none());
    }

    #[test]
    fn clean_sweeps_unreachable_receipt_objects() {
        let (roots, _g) = temp_roots();
        let entry = ingest_fixture(&roots, "orphan-receipt", &[("out", "bytes")], Vec::new());
        let receipt = roots
            .hangar_dir()
            .join(Closure::RECEIPTS_DIR)
            .join(&entry.entry.receipt);
        let object = roots.hangar_dir().join(&entry.entry.id);
        assert!(Closure::remove_closure_record(&roots, &entry.entry.id).unwrap());
        make_tree_writable_for_removal(&object).unwrap();
        fs::remove_dir_all(&object).unwrap();
        assert!(receipt.is_file());

        let plan = clean_plan(&roots).unwrap();
        assert_eq!(plan.removed_receipts, 1);
        let report = clean(&roots).unwrap();
        assert_eq!(report.removed_receipts, 1);
        assert!(!receipt.exists());
    }

    #[test]
    fn malformed_project_lock_fails_closed_before_hangar_cleanup() {
        let (roots, _g) = temp_roots();
        let project = roots.root.join("project");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        fs::create_dir_all(&managed).unwrap();
        fs::write(
            managed.join("lock"),
            "version = 1\n\n[build.stamp]\ndirty = maybe\n",
        )
        .unwrap();

        let error = lock_roots_from(&project).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("could not parse project lock"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_project_lock_is_rejected_before_hangar_cleanup() {
        use std::os::unix::fs::symlink;

        let (roots, _g) = temp_roots();
        let project = roots.root.join("project");
        let managed = project.join(crate::Syntax::SOURCE_ROOT_DIR);
        let outside = roots.root.join("outside-lock");
        fs::create_dir_all(&managed).unwrap();
        fs::write(&outside, "version = 1\n").unwrap();
        symlink(&outside, managed.join("lock")).unwrap();

        let error = nearest_lock_path(&project).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("is a symlink"));
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
        assert_eq!(
            root.identity.id.as_str(),
            format!(
                "external-consumer:jetos-generation:{}",
                SHA256::sha256_hex(b"host\0generation")
            )
        );
        assert_eq!(root.identity.producer.as_str(), "jetos-generation");
        assert_eq!(root.identity.incarnation.get(), 1);
        assert_eq!(root.identity.witness.as_str(), witness);
        assert_eq!(root.targets, BTreeSet::from([first_hash.clone()]));
        assert_eq!(root.protected_targets, BTreeSet::from([first_hash]));
        assert_eq!(root.phase, Lifecycle::RootPhase::Committed);
    }

    #[test]
    fn manual_external_root_is_typed_atomic_and_cas_bound() {
        let (roots, _g) = temp_roots();
        let first = ingest_fixture(&roots, "manual-first", &[("out", "first")], Vec::new());
        let second = ingest_fixture(&roots, "manual-second", &[("out", "second")], Vec::new());
        let principal = "manual-root-test";
        let now = super::now_secs();
        let first_reference = first.entry.reference.clone();
        let second_reference = second.entry.reference.clone();

        let created = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &first_reference,
            Some(now + 3600),
            None,
            now,
        )
        .unwrap();
        assert_eq!(created.etag, "1.1");
        assert_eq!(created.closure_size, 1);
        assert!(!created.prepared);

        let repeated = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &first_reference,
            Some(now + 3600),
            None,
            now + 1,
        )
        .unwrap();
        assert_eq!(repeated.etag, created.etag);

        let missing_etag = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &second_reference,
            Some(now + 7200),
            None,
            now + 2,
        )
        .unwrap_err();
        assert!(matches!(
            missing_etag,
            ExternalRootError::Conflict {
                expected: None,
                current: Some(ref current),
                ..
            } if current == "1.1"
        ));

        let stale = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &second_reference,
            Some(now + 7200),
            Some("1.0"),
            now + 2,
        )
        .unwrap_err();
        assert!(matches!(
            stale,
            ExternalRootError::Conflict {
                expected: Some(ref expected),
                current: Some(ref current),
                ..
            } if expected == "1.0" && current == "1.1"
        ));

        let updated = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &second_reference,
            Some(now + 7200),
            Some("1.1"),
            now + 2,
        )
        .unwrap();
        assert_eq!(updated.etag, "2.2");
        let committed_retry = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &second_reference,
            Some(now + 7200),
            Some("1.1"),
            now + 3,
        )
        .unwrap();
        assert_eq!(committed_retry, updated);
        let listed = list_external_roots(&roots, principal).unwrap();
        assert_eq!(listed, vec![updated.clone()]);

        let snapshot = Lifecycle::snapshot(&roots).unwrap();
        let root = snapshot
            .roots
            .values()
            .find(|root| root.identity.kind == Lifecycle::RootKind::Manual)
            .unwrap();
        assert_eq!(root.metadata.label.as_deref(), Some("backup-sdk"));
        assert_eq!(
            root.metadata.reference.as_deref(),
            Some(second_reference.as_str())
        );
        assert_eq!(root.revision, 2);
        assert_eq!(root.identity.incarnation.get(), 2);

        let stale_remove =
            unregister_external_root_at(&roots, principal, "backup-sdk", "1.1", now + 3)
                .unwrap_err();
        assert!(matches!(stale_remove, ExternalRootError::Conflict { .. }));
        unregister_external_root_at(&roots, principal, "backup-sdk", "2.2", now + 3).unwrap();
        unregister_external_root_at(&roots, principal, "backup-sdk", "2.2", now + 4).unwrap();
        assert!(list_external_roots(&roots, principal).unwrap().is_empty());

        let recreated = register_external_root_at(
            &roots,
            principal,
            "backup-sdk",
            &first_reference,
            None,
            None,
            now + 5,
        )
        .unwrap();
        assert_eq!(recreated.etag, "3.3");
    }

    #[test]
    fn manual_external_root_rejects_path_escape_and_unknown_reference() {
        let (roots, _g) = temp_roots();
        assert!(matches!(
            register_external_root_at(&roots, "principal", "../escape", "missing", None, None, 1),
            Err(ExternalRootError::Store(error)) if error.kind() == std::io::ErrorKind::InvalidInput
        ));
        assert!(matches!(
            register_external_root_at(&roots, "principal", "valid", "missing", None, None, 1),
            Err(ExternalRootError::ReferenceNotFound(reference)) if reference == "missing"
        ));
    }

    #[test]
    fn ids_differ_by_ref() {
        let a = entry_id("x", "1.0", "x@nixpkgs", "/o");
        let b = entry_id("x", "1.0", "o/x@github", "/o");
        assert_ne!(a, b);
    }

    #[test]
    fn id_omits_empty_version() {
        // Unknown version falls back to `<name>-<fp>`, no dangling segment.
        let id = entry_id("x", "", "x@nixpkgs", "/o");
        assert!(id.starts_with("x-"));
        assert!(!id.starts_with("x--"));
    }

    #[test]
    fn verified_cache_rejects_deleted_and_tampered_outputs() {
        let (roots, _g) = temp_roots();
        let ingested = ingest_fixture(&roots, "verified-cache", &[("out", "trusted")], Vec::new());
        let entry = ingested.entry;
        let out = PathBuf::from(&entry.out);
        let reference = entry.reference.as_str();
        let expectation = test_expectation(&out);
        let proof = verify_cache_entry(&roots, &entry, reference, &expectation);
        assert!(!proof.signature_verified);
        assert!(proof.unsigned_local_allowed);
        assert!(verified(&roots, reference, &expectation));

        // Sealed verification manifests track per-file (inode, size, mtime)
        // tuples, so an in-place payload rewrite is drift even when the root
        // stat identity is unchanged: the use path re-hashes and rejects.
        let root_stamp = super::super::Ingest::object_stamp(&fs::symlink_metadata(&out).unwrap());
        let payload = out.join("payload");
        let mut permissions = fs::metadata(&payload).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&payload, permissions).unwrap();
        fs::write(&payload, "tampered").unwrap();
        assert_eq!(
            root_stamp,
            super::super::Ingest::object_stamp(&fs::symlink_metadata(&out).unwrap())
        );
        let use_proof = verify_cache_entry(&roots, &entry, reference, &expectation);
        assert!(!use_proof.output_digest, "{use_proof:?}");
        assert!(!use_proof.trusted(), "{use_proof:?}");
        assert!(verify_hangar_object(&roots, &entry).is_err());

        // Deletion removes the stamped object identity and must miss even
        // though the process-local memo still contains the former digest.
        let mut permissions = fs::metadata(&out).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&out, permissions).unwrap();
        fs::remove_dir_all(&out).unwrap();
        assert!(!verified(&roots, reference, &expectation));
    }

    #[test]
    fn cache_candidate_preflight_does_not_read_closure_graph() {
        let (roots, _g) = temp_roots();
        let ingested = ingest_fixture(
            &roots,
            "candidate-preflight",
            &[("out", "trusted")],
            Vec::new(),
        );
        let expectation = test_expectation(Path::new(&ingested.entry.out));
        let closure_db = roots.hangar_dir().join("closure-db");
        fs::remove_dir_all(&closure_db).unwrap();

        assert!(cache_candidate_matches(
            &roots,
            &ingested.entry.reference,
            &expectation
        ));
        assert!(
            !closure_db.exists(),
            "candidate preflight must not create or migrate the closure graph"
        );
    }

    #[test]
    fn verified_cache_rejects_wrong_platform_and_incomplete_proof() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("owned-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let mut envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "demo@mine",
            "core-source",
        );
        envelope.platform = "not-this-host".to_string();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "demo@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let mut expectation = test_expectation(&out);
        assert!(!verified(&roots, "demo@mine", &expectation));

        envelope.platform = super::super::super::Envelope::host_platform();
        envelope.provenance.clear();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "demo@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(!verified(&roots, "demo@mine", &expectation));

        envelope.provenance = "demo@mine via core-source".to_string();
        envelope.signature = "unverified-signature-text".to_string();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "demo@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        assert!(!verified(&roots, "demo@mine", &expectation));

        envelope.signature.clear();
        record_verified(
            &roots,
            "demo",
            "1.0",
            "demo@mine",
            &out.to_string_lossy(),
            &out.join("missing-bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        expectation.owned_output = Some(out.clone());
        assert!(!verified(&roots, "demo@mine", &expectation));
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
            "escape@mine",
            "core-source",
        );
        let mut entry = record_verified(
            &roots,
            "escape",
            "1",
            "escape@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        entry.bin = out.join("../other").to_string_lossy().into_owned();
        let proof = verify_cache_entry(&roots, &entry, "escape@mine", &test_expectation(&out));
        assert!(!proof.closure);
        assert!(!proof.trusted());
    }

    #[test]
    fn nix_compat_output_fails_closed_without_native_store_authority() {
        let (roots, _g) = temp_roots();
        let error = pin_nix_gc_root(&roots.root.join("entry"), "/nix/store/demo").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("needs a verified native store authority"),
            "error: {error}"
        );
    }

    #[test]
    fn nix_external_output_projects_into_hangar_without_mutating_source() {
        let (roots, _g) = temp_roots();
        let source = roots.root.join("external-nix-output");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/tool"), "projected").unwrap();
        seal_local_output(&source).unwrap();
        let digest = crate::Envelope::try_output_hash_of(&source.to_string_lossy()).unwrap();

        let projected = project_external_output_unlocked(&roots, &source, &digest).unwrap();
        assert!(Path::new(&projected).starts_with(roots.hangar_dir().join("objects")));
        assert!(
            source.is_dir(),
            "projection must not mutate the source output"
        );
        assert_eq!(
            crate::Envelope::try_output_hash_of_in_hangar(&projected, &roots.hangar_dir(), false,)
                .unwrap(),
            digest
        );
    }

    #[test]
    fn nix_multi_output_lease_projects_binary_from_non_primary_output() {
        let (roots, _g) = temp_roots();
        let snapshot = roots.root.join("lease-snapshot");
        fs::create_dir_all(snapshot.join("bin")).unwrap();
        fs::write(snapshot.join("bin/tool"), "out").unwrap();
        seal_local_output(&snapshot).unwrap();
        let out_digest = crate::Envelope::try_output_hash_of(&snapshot.to_string_lossy()).unwrap();

        let dev = roots.root.join("external-dev-output");
        fs::create_dir_all(dev.join("bin")).unwrap();
        fs::write(dev.join("bin/tool"), "dev").unwrap();
        seal_local_output(&dev).unwrap();
        let dev_digest = crate::Envelope::try_output_hash_of(&dev.to_string_lossy()).unwrap();
        let dev_object = project_external_output_unlocked(&roots, &dev, &dev_digest).unwrap();

        let drv = "/nix/store/projection.drv";
        let mut producer_facts = crate::Provider::nix_build_facts_record();
        producer_facts.extend(BTreeMap::from([
            ("nix.drv_path".into(), drv.into()),
            ("nix.output.out".into(), "/nix/store/projection-out".into()),
            ("nix.output.dev".into(), "/nix/store/projection-dev".into()),
        ]));
        let producer = ProducerRecord::new(
            "nix",
            drv,
            crate::SHA256::sha256_hex(drv.as_bytes()),
            crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::from([
                ("nix.drv_path".into(), drv.into()),
                ("nix.output.out".into(), "/nix/store/projection-out".into()),
                ("nix.output.dev".into(), "/nix/store/projection-dev".into()),
            ]))
            .unwrap(),
            "nix-derivation:projection",
            "policy=test\nplatform=test",
            producer_facts,
        )
        .unwrap();
        let entry = StoreEntry {
            id: "projection".into(),
            name: "projection".into(),
            version: "1".into(),
            reference: "projection@nixpkgs".into(),
            out: snapshot.to_string_lossy().into_owned(),
            bin: snapshot.join("bin").to_string_lossy().into_owned(),
            rlib: String::new(),
            envelope: crate::Envelope::Envelope::for_output(
                &snapshot.to_string_lossy(),
                "projection@nixpkgs",
                "nix",
            ),
            cache_identity: CacheIdentity::default(),
            references: Vec::new(),
            named_outputs: BTreeMap::from([("out".into(), out_digest), ("dev".into(), dev_digest)]),
            platform_artifact_kind: String::new(),
            producer_record: producer.encode(),
            receipt: String::new(),
            realized_at: 0,
            last_used_at: 0,
        };
        let projection = nix_store_projection_for_entry(&roots, &entry, &snapshot).unwrap();
        let projection = projection
            .into_iter()
            .map(|projection| (projection.logical, projection.source))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(projection.get("/nix/store/projection-out"), Some(&snapshot));
        assert_eq!(
            projection.get("/nix/store/projection-dev"),
            Some(&PathBuf::from(&dev_object))
        );
        assert!(
            projection
                .values()
                .all(|source| !source.starts_with("/nix/store")),
            "Nix runtime projection must never source bytes from the host store"
        );

        let lease = snapshot_lease(&roots, &entry).unwrap();
        assert_eq!(
            fs::read_to_string(
                lease
                    .stable_path(&Path::new(&entry.bin).join("tool").to_string_lossy())
                    .unwrap()
            )
            .unwrap(),
            "out"
        );
        assert!(lease.projected_executable("tool").is_some());
        assert!(lease
            .executable_for_command(&Path::new(&dev_object).join("unrecorded").to_string_lossy())
            .is_err());
        lease.validate().unwrap();

        #[cfg(target_os = "linux")]
        assert!(lease
            .nix_store_projection()
            .iter()
            .all(|(_, source)| source.starts_with("/proc/self/fd/")));
    }

    #[test]
    fn hostile_out_pointer_never_quarantines_another_object() {
        let (roots, _g) = temp_roots();
        let survivor = roots.hangar_dir().join("survivor-output");
        let expected = roots.hangar_dir().join("expected-output");
        fs::create_dir_all(&survivor).unwrap();
        fs::create_dir_all(&expected).unwrap();
        fs::write(survivor.join("keep"), "survivor").unwrap();
        seal_node(&survivor).unwrap();
        fs::write(expected.join("bad"), "candidate").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &survivor.to_string_lossy(),
            "hostile@mine",
            "core-source",
        );
        let entry = record_verified(
            &roots,
            "hostile",
            "1.0",
            "hostile@mine",
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
        assert_eq!(
            fs::read_to_string(survivor.join("keep")).unwrap(),
            "survivor"
        );
        assert_eq!(
            fs::read_to_string(expected.join("bad")).unwrap(),
            "candidate"
        );
    }

    #[test]
    fn proven_digest_mismatch_still_quarantines_object() {
        let (roots, _g) = temp_roots();
        let ingested = ingest_fixture(&roots, "sealed", &[("out", "tampered")], Vec::new());
        let entry = ingested.entry;
        let out = PathBuf::from(&entry.out);
        let payload = out.join("payload");
        let mut permissions = fs::metadata(&payload).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&payload, permissions).unwrap();
        fs::write(&payload, "corrupt").unwrap();

        quarantine_invalid_entry(&roots, &entry, &test_expectation(&out)).unwrap();

        assert!(!out.exists());
        assert!(find_by_reference(&roots, "path:sealed").is_none());
        assert!(fs::read_dir(roots.hangar_dir().join("quarantine"))
            .unwrap()
            .flatten()
            .any(|item| item
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("output-{}-", entry.envelope.output_hash))));
        ingest_fixture(&roots, "sealed", &[("out", "tampered")], Vec::new());
    }

    #[test]
    fn identity_only_quarantine_preserves_shared_cas_object() {
        let (roots, _g) = temp_roots();
        let stale = ingest_fixture(&roots, "shared-stale", &[("out", "same")], Vec::new());
        let survivor = ingest_fixture(&roots, "shared-survivor", &[("out", "same")], Vec::new());
        assert_eq!(stale.entry.out, survivor.entry.out);

        let mut mismatch = test_expectation(Path::new(&stale.entry.out));
        mismatch.identity.source_fingerprint = "wrong-source".into();
        quarantine_invalid_entry(&roots, &stale.entry, &mismatch).unwrap();

        assert!(Path::new(&survivor.entry.out).exists());
        assert!(find_by_reference(&roots, &stale.entry.reference).is_none());
        find_verified_by_reference(
            &roots,
            &survivor.entry.reference,
            &test_expectation(Path::new(&survivor.entry.out)),
        )
        .unwrap()
        .unwrap()
        .lease
        .validate()
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_shared_cas_object_survives_quarantine() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let stale = ingest_fixture(&roots, "unreadable-stale", &[("out", "same")], Vec::new());
        let survivor = ingest_fixture(
            &roots,
            "unreadable-survivor",
            &[("out", "same")],
            Vec::new(),
        );
        assert_eq!(stale.entry.out, survivor.entry.out);

        let object = PathBuf::from(&stale.entry.out);
        let payload = object.join("payload");
        let original_permissions = fs::metadata(&payload).unwrap().permissions();
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o000)).unwrap();
        // The sealed stat manifest trusts unchanged (inode, size, mtime)
        // tuples without reading bytes; the full audit path still fails on
        // the unreadable payload.
        assert!(full_verified_output_hash(&object, &roots.hangar_dir(), false,).is_err());
        let mut mismatch = test_expectation(&object);
        mismatch.identity.source_fingerprint = "wrong-source".into();

        quarantine_invalid_entry(&roots, &stale.entry, &mismatch).unwrap();

        assert!(object.exists());
        assert!(find_by_reference(&roots, &stale.entry.reference).is_none());
        fs::set_permissions(&payload, original_permissions).unwrap();
        let graph = closure_graph(&roots).unwrap();
        assert!(!graph.records.contains_key(&stale.entry.id));
        assert!(graph.deleted_records.contains(&stale.entry.id));
        find_verified_by_reference(
            &roots,
            &survivor.entry.reference,
            &test_expectation(&object),
        )
        .unwrap()
        .unwrap()
        .lease
        .validate()
        .unwrap();
    }

    #[test]
    fn quarantine_skips_concurrently_refreshed_record() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("refreshed-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "refreshed@mine",
            "core-source",
        );
        let stale = record_verified(
            &roots,
            "refreshed",
            "1.0",
            "refreshed@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let mut fresh_identity = test_identity();
        fresh_identity.source_fingerprint = "source-v2".into();
        let fresh = record_verified(
            &roots,
            "refreshed",
            "1.0",
            "refreshed@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &fresh_identity,
        )
        .unwrap();
        assert_eq!(stale.id, fresh.id);
        assert_ne!(entry_action_key(&stale), entry_action_key(&fresh));

        quarantine_invalid_entry(&roots, &stale, &test_expectation(&out)).unwrap();

        let current = find_by_reference(&roots, "refreshed@mine").unwrap();
        assert_eq!(current.cache_identity, fresh_identity);
        let expectation = CacheExpectation {
            identity: fresh_identity,
            owned_output: Some(out),
            allow_unsigned_local: true,
        };
        assert!(verified(&roots, "refreshed@mine", &expectation));
    }

    #[test]
    fn quarantine_skips_same_action_record_repaired_before_lock() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("repaired-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let mut stale_envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "repaired@mine",
            "core-source",
        );
        stale_envelope.provenance.clear();
        let stale = record_verified(
            &roots,
            "repaired",
            "1.0",
            "repaired@mine",
            &out.to_string_lossy(),
            "",
            "",
            &stale_envelope,
            &test_identity(),
        )
        .unwrap();
        let mut repaired_envelope = stale_envelope;
        repaired_envelope.provenance = "repaired provenance".into();
        let repaired = record_verified(
            &roots,
            "repaired",
            "1.0",
            "repaired@mine",
            &out.to_string_lossy(),
            "",
            "",
            &repaired_envelope,
            &test_identity(),
        )
        .unwrap();
        assert_eq!(stale.envelope.output_hash, repaired.envelope.output_hash);
        assert_eq!(entry_action_key(&stale), entry_action_key(&repaired));

        quarantine_invalid_entry(&roots, &stale, &test_expectation(&out)).unwrap();

        let current = find_by_reference(&roots, "repaired@mine").unwrap();
        assert_eq!(current.envelope.provenance, "repaired provenance");
        assert!(verified(&roots, "repaired@mine", &test_expectation(&out)));
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_failure_restores_permissions_without_tombstone() {
        use std::os::unix::fs::PermissionsExt as _;

        let (roots, _g) = temp_roots();
        let ingested = ingest_fixture(&roots, "restore", &[("out", "same")], Vec::new());
        let hangar = roots.hangar_dir();
        fs::write(hangar.join("quarantine"), "blocks directory creation").unwrap();
        fs::set_permissions(&hangar, fs::Permissions::from_mode(0o555)).unwrap();
        let mut mismatch = test_expectation(Path::new(&ingested.entry.out));
        mismatch.identity.source_fingerprint = "wrong-source".into();

        let error = quarantine_invalid_entry(&roots, &ingested.entry, &mismatch).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            fs::metadata(&hangar).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert!(hangar.join(&ingested.entry.id).exists());

        fs::set_permissions(&hangar, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(find_by_reference(&roots, &ingested.entry.reference).is_some());
    }

    #[test]
    fn cache_lease_is_private_snapshot_without_long_object_lock() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("leased-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "leased@mine",
            "core-source",
        );
        record_verified(
            &roots,
            "leased",
            "1.0",
            "leased@mine",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let hit = find_verified_by_reference(&roots, "leased@mine", &test_expectation(&out))
            .unwrap()
            .unwrap();
        let lease_root = hit.lease.snapshot_root.parent().unwrap().to_path_buf();
        fs::write(out.join("payload"), "mutated outside cooperative lock").unwrap();
        hit.lease.validate().unwrap();
        let stable = hit
            .lease
            .stable_path(&out.join("payload").to_string_lossy())
            .unwrap();
        assert_eq!(fs::read_to_string(stable).unwrap(), "trusted");
        drop(hit);
        assert!(!lease_root.exists(), "idle lease was not reclaimed on drop");
    }

    #[cfg(unix)]
    #[test]
    fn lease_recovery_waits_for_descendant_then_reclaims_idle_container() {
        let (roots, _g) = temp_roots();
        let ingested = ingest_fixture(&roots, "lease-tree", &[("out", "trusted")], Vec::new());
        let hit = find_verified_by_reference(
            &roots,
            &ingested.entry.reference,
            &test_expectation(Path::new(&ingested.entry.out)),
        )
        .unwrap()
        .unwrap();
        let lease_root = hit.lease.snapshot_root.parent().unwrap().to_path_buf();
        hit.lease.mark_process_handoff();

        let mut child = Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 2 >/dev/null 2>&1 &"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        drop(hit);

        assert!(lease_root.exists());
        assert_eq!(recover_hangar(&roots).unwrap(), 0);
        assert!(lease_root.exists());

        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(recover_hangar(&roots).unwrap(), 1);
        assert!(!lease_root.exists());
    }

    #[test]
    fn realization_type_distinguishes_fresh_cached_and_missing_outputs() {
        let (roots, _g) = temp_roots();
        let out = roots.root.join("typed-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "trusted").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "typed@mine",
            "core-source",
        );
        let entry = record_verified(
            &roots,
            "typed",
            "1",
            "typed@mine",
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

        let hit = find_verified_by_reference(&roots, "typed@mine", &test_expectation(&out))
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
            reference: "missing@mine".to_string(),
            out: missing_out.to_string_lossy().into_owned(),
            bin: String::new(),
            rlib: String::new(),
            envelope: super::super::super::Envelope::Envelope::default(),
            cache_identity: test_identity(),
            references: Vec::new(),
            named_outputs: BTreeMap::new(),
            platform_artifact_kind: String::new(),
            producer_record: String::new(),
            receipt: String::new(),
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
        assert!(missing
            .lease
            .stable_path(&missing_out.to_string_lossy())
            .is_err());
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
        fs::set_permissions(out.join("bin/tool-real"), fs::Permissions::from_mode(0o555)).unwrap();
        symlink("tool-real", &tool).unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "fd-view@mine",
            "core-source",
        );
        record_verified(
            &roots,
            "fd-view",
            "1",
            "fd-view@mine",
            &out.to_string_lossy(),
            &out.join("bin").to_string_lossy(),
            "",
            &envelope,
            &test_identity(),
        )
        .unwrap();
        let hit = find_verified_by_reference(&roots, "fd-view@mine", &test_expectation(&out))
            .unwrap()
            .unwrap();
        let projected_bin = hit.lease.projected_bin_dir().unwrap();
        assert!(projected_bin.starts_with("/proc/self/fd/"));
        assert!(!projected_bin.starts_with(&out));
        let stable_tool = hit.lease.stable_path(&tool.to_string_lossy()).unwrap();
        let explicit_tool = hit.lease.executable_for(&tool.to_string_lossy()).unwrap();
        assert_eq!(
            Command::new(&explicit_tool).output().unwrap().stdout,
            b"trusted"
        );
        assert!(hit
            .lease
            .executable_for(&out.join("bin/other-tool").to_string_lossy())
            .is_none());
        assert!(hit
            .lease
            .executable_for_command(&out.join("bin/other-tool").to_string_lossy())
            .is_err());

        let moved = roots.root.join("fd-view-original");
        let attacker = roots.root.join("fd-view-attacker");
        fs::rename(&out, &moved).unwrap();
        fs::create_dir_all(attacker.join("bin")).unwrap();
        fs::write(attacker.join("bin/tool"), "#!/bin/sh\nprintf attacker").unwrap();
        fs::set_permissions(attacker.join("bin/tool"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&attacker, &out).unwrap();

        hit.lease.validate().unwrap();
        let confined_tool = hit.lease.executable_for(&tool.to_string_lossy()).unwrap();
        assert_eq!(
            Command::new(&confined_tool).output().unwrap().stdout,
            b"trusted"
        );
        assert!(hit
            .lease
            .executable_for(&attacker.join("bin/tool").to_string_lossy())
            .is_none());
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
        assert_eq!(
            fs::read_to_string(out.join("bin/tool")).unwrap(),
            "#!/bin/sh\nprintf attacker"
        );
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
            receipt: String::new(),
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
                key == "public-key" && message.contains("source=source-v1") && signature == "abcd"
            }
        ));
    }

    #[test]
    fn configured_ed25519_signature_verifies_the_cache_receipt() {
        use ed25519_dalek::{Signer as _, SigningKey};

        let (roots, _g) = temp_roots();
        let out = roots.root.join("signed-output-real");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "signed").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "cache:demo",
            "remote-cache",
        );
        let mut entry = StoreEntry {
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
            receipt: String::new(),
            realized_at: 0,
            last_used_at: 0,
        };
        let expectation = CacheExpectation {
            identity: test_identity(),
            owned_output: None,
            allow_unsigned_local: false,
        };
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        fs::create_dir_all(roots.root.join("trust")).unwrap();
        fs::write(
            roots.root.join("trust/cache.ed25519.pub"),
            hex(&signing_key.verifying_key().to_bytes()),
        )
        .unwrap();
        entry.envelope.signature = format!(
            "ed25519:{}",
            hex(&signing_key
                .sign(cache_signature_message(&entry, &expectation).as_bytes())
                .to_bytes())
        );
        assert!(verify_configured_signature(&roots, &entry, &expectation));

        let replacement = if entry.envelope.signature.as_bytes()[9] == b'0' {
            "1"
        } else {
            "0"
        };
        entry.envelope.signature.replace_range(9..10, replacement);
        assert!(!verify_configured_signature(&roots, &entry, &expectation));
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
            receipt: String::new(),
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
    fn ingest_missing_output_is_typed_atomic_and_retryable() {
        let (roots, _g) = temp_roots();
        let src = roots.root.join("late-ingest-output");
        let request = |src: PathBuf| IngestRequest {
            name: "late-ingest".into(),
            version: "1".into(),
            reference: "path:late-ingest".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: BTreeMap::from([("out".into(), src)]),
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        };

        let error = ingest_tree(&roots, &request(src.clone())).unwrap_err();
        assert!(matches!(&error, IngestError::IO(_)), "{error:?}");
        assert_eq!(error.code(), "E1315");
        assert!(list_checked(&roots).unwrap().is_empty());
        assert!(closure_graph(&roots).unwrap().records.is_empty());
        assert!(find_by_reference(&roots, "path:late-ingest").is_none());
        assert!(
            find_verified_by_reference(&roots, "path:late-ingest", &test_expectation(&src))
                .unwrap()
                .is_none()
        );
        assert!(!roots.root.join("leases").exists());

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("payload"), "now-real").unwrap();
        let ingested = ingest_tree(&roots, &request(src)).unwrap();
        assert_eq!(
            find_by_reference(&roots, "path:late-ingest"),
            Some(ingested.entry.clone())
        );
        let expectation = test_expectation(Path::new(&ingested.entry.out));
        let hit = find_verified_by_reference(&roots, "path:late-ingest", &expectation)
            .unwrap()
            .expect("retry must publish one verified cache hit");
        assert_eq!(hit.entry, ingested.entry);
    }

    #[test]
    fn ingest_child_replaced_after_copy_is_typed_atomic_and_retryable() {
        let (roots, _g) = temp_roots();
        let src = roots.root.join("racing-tree");
        let replaced = src.join("replaced-after-copy");
        let replacement = roots.root.join("replacement");
        fs::create_dir_all(&src).unwrap();
        fs::write(&replaced, "first").unwrap();
        fs::write(&replacement, "later").unwrap();
        fs::write(src.join("stable"), "stable").unwrap();
        let request = |src: PathBuf| IngestRequest {
            name: "child-race".into(),
            version: "1".into(),
            reference: "path:child-race".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: BTreeMap::from([("out".into(), src)]),
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        };
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook_fired = fired.clone();
        let hook_replaced = replaced.clone();
        let hook_replacement = replacement.clone();

        let attempted = super::Ingest::with_after_child_copy_hook(
            move |copied| {
                if copied == hook_replaced
                    && !hook_fired.swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    #[cfg(not(unix))]
                    fs::remove_file(&hook_replaced).unwrap();
                    fs::rename(&hook_replacement, &hook_replaced).unwrap();
                }
            },
            || ingest_tree(&roots, &request(src.clone())),
        );
        assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
        let error = attempted.expect_err("source mutation must not return a success receipt");
        assert!(matches!(&error, IngestError::Mutated(_)), "{error:?}");
        assert_eq!(error.code(), "E1315");
        assert!(list_checked(&roots).unwrap().is_empty());
        assert!(closure_graph(&roots).unwrap().records.is_empty());
        assert!(find_by_reference(&roots, "path:child-race").is_none());
        assert!(
            find_verified_by_reference(&roots, "path:child-race", &test_expectation(&src))
                .unwrap()
                .is_none()
        );
        assert!(!roots.root.join("leases").exists());

        fs::write(&replaced, "first").unwrap();
        let ingested = ingest_tree(&roots, &request(src)).unwrap();
        assert_eq!(list_checked(&roots).unwrap(), vec![ingested.entry.clone()]);
        let expectation = test_expectation(Path::new(&ingested.entry.out));
        let hit = find_verified_by_reference(&roots, "path:child-race", &expectation)
            .unwrap()
            .expect("retry must publish one verified cache hit");
        assert_eq!(hit.entry, ingested.entry);
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
        assert!(
            err.what().contains("store path")
                || err.why().contains("reserved")
                || err.why().contains("CON")
                || format!("{err:?}").contains("reserved"),
            "{err:?}"
        );
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

    /// Minimal scoped tempdir for tests (std-only; auto-removes on drop).
    #[cfg(test)]
    mod tempdir {
        use std::ffi::OsString;
        use std::path::{Path, PathBuf};
        use std::sync::{Mutex, MutexGuard, OnceLock};

        static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

        pub struct Guard {
            pub path: PathBuf,
            parent: PathBuf,
            environment: Option<EnvironmentGuard>,
        }

        struct EnvironmentGuard {
            previous: Vec<(&'static str, Option<OsString>)>,
            _lock: MutexGuard<'static, ()>,
        }

        impl EnvironmentGuard {
            fn new(root: &Path, parent: &Path) -> Self {
                let lock = ENVIRONMENT_LOCK
                    .get_or_init(|| Mutex::new(()))
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let desired = [
                    ("HOME", Some(parent.join("home").into_os_string())),
                    ("USERPROFILE", Some(parent.join("home").into_os_string())),
                    ("LOCALAPPDATA", Some(parent.join("local").into_os_string())),
                    ("XDG_DATA_HOME", Some(parent.join("data").into_os_string())),
                    (
                        "XDG_STATE_HOME",
                        Some(parent.join("state").into_os_string()),
                    ),
                    (
                        "XDG_CACHE_HOME",
                        Some(parent.join("cache").into_os_string()),
                    ),
                    (
                        "XDG_CONFIG_HOME",
                        Some(parent.join("config").into_os_string()),
                    ),
                    ("JETPACK_ROOT", Some(root.as_os_str().to_os_string())),
                    (
                        "JETPACK_SHARED_CAS",
                        Some(parent.join("shared-cas").into_os_string()),
                    ),
                ];
                let previous = desired
                    .iter()
                    .map(|(name, _)| (*name, std::env::var_os(name)))
                    .collect();
                for (name, value) in desired {
                    if let Some(value) = value {
                        std::env::set_var(name, value);
                    } else {
                        std::env::remove_var(name);
                    }
                }
                Self {
                    previous,
                    _lock: lock,
                }
            }
        }

        impl Drop for EnvironmentGuard {
            fn drop(&mut self) {
                for (name, value) in self.previous.drain(..).rev() {
                    if let Some(value) = value {
                        std::env::set_var(name, value);
                    } else {
                        std::env::remove_var(name);
                    }
                }
            }
        }

        impl Guard {
            pub fn new(tag: &str) -> Guard {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let parent = std::env::temp_dir().join(format!(
                    "{tag}-{}-{nanos}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                let path = parent.join("root");
                std::fs::create_dir_all(&path).unwrap();
                let environment = EnvironmentGuard::new(&path, &parent);
                Guard {
                    path,
                    parent,
                    environment: Some(environment),
                }
            }
        }

        impl Drop for Guard {
            fn drop(&mut self) {
                drop(self.environment.take());
                let _ = std::fs::remove_dir_all(&self.parent);
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
        let receipt = read("src/Store/Receipt.rs");
        let journal = read("src/Store/Journal.rs");
        let tests = read("src/Store/Tests.rs");
        // Exclude this test's own body: the marker must match the indented
        // source, and the forbidden literals below are built at runtime so the
        // scan cannot trigger on itself.
        let tests_production = tests
            .split("fn store_stays_split_along_existing_phases")
            .next()
            .unwrap();
        let include_marker = format!("include{}(", '!');
        let path_marker = format!("#{}path", '[');

        for (relative, source) in [
            ("src/Store.rs", store.as_str()),
            ("src/Store/Ingest.rs", ingest.as_str()),
            ("src/Store/Closure.rs", closure.as_str()),
            ("src/Store/Receipt.rs", receipt.as_str()),
            ("src/Store/Journal.rs", journal.as_str()),
            ("src/Store/Tests.rs", tests_production),
        ] {
            assert!(
                source.lines().count() < MAX_MODULE_LINES,
                "{relative} must stay below the card #510 module boundary"
            );
            assert!(!source.contains(&include_marker));
            assert!(!source.contains(&path_marker));
        }
        assert!(store.contains("\nmod Ingest;\npub use Ingest::*;"));
        assert!(store.contains("\nmod Closure;\npub use Closure::*;"));
        assert!(store.contains("\nmod Receipt;\n"));
        assert!(store.contains("\nmod Journal;\n"));
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
        ];
        let positions: Vec<usize> = closure_ordered
            .iter()
            .map(|needle| closure.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let journal_ordered = ["fn load_graph", "fn validate_graph"];
        let positions: Vec<usize> = journal_ordered
            .iter()
            .map(|needle| journal.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
