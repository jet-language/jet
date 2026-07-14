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
        let e = record(
            &roots,
            "fastfetch",
            "2.1.0",
            "nixpkgs:fastfetch",
            "/nix/store/x",
            "/nix/store/x/bin",
            "",
            &super::super::super::Envelope::Envelope::default(),
        )
        .unwrap();
        // Name-and-version first, fingerprint last (D-PM1).
        assert!(e.id.starts_with("fastfetch-2.1.0-"));
        let listed = list(&roots);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], e);
    }

    #[test]
    fn clean_keeps_fresh_entries() {
        let (roots, _g) = temp_roots();
        record(
            &roots,
            "a",
            "1.0",
            "nixpkgs:a",
            "/nix/store/a",
            "/nix/store/a/bin",
            "",
            &super::super::super::Envelope::Envelope::default(),
        )
        .unwrap();
        record(
            &roots,
            "b",
            "1.0",
            "nixpkgs:b",
            "/nix/store/b",
            "/nix/store/b/bin",
            "",
            &super::super::super::Envelope::Envelope::default(),
        )
        .unwrap();
        let report = clean(&roots).unwrap();
        assert_eq!(report.removed_objects, 0);
        assert_eq!(list(&roots).len(), 2);
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
            referrers_of(&roots, &first.entry.envelope.output_hash),
            vec![third.entry.envelope.output_hash.clone()]
        );
        assert_eq!(recover_hangar_staging(&roots).unwrap(), 0);
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
        let installed = Path::new(&ingested.entry.out).join(".named/dev");
        assert_eq!(
            fs::read_to_string(installed.join("payload")).unwrap(),
            "development"
        );
        let actual = super::super::super::Envelope::try_output_hash_of(&installed.to_string_lossy())
            .unwrap();
        assert_eq!(ingested.entry.named_outputs.get("dev"), Some(&actual));
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
        set_user_xattr(&file, "user.jet.test", b"keep");
        let mut outputs = BTreeMap::new();
        outputs.insert("out".to_string(), src);
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
        assert_eq!(ingested.entry.platform_artifact_kind, "macos-app");
        verify_hangar_object(&roots, &ingested.entry).unwrap();
        let sealed = Path::new(&ingested.entry.out).join("payload");
        let names = super::super::super::Envelope::list_xattr_names(&sealed).unwrap();
        assert!(
            names.iter().any(|n| n == "user.jet.test"),
            "semantic xattr must be preserved on sealed object: {names:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cas_pool_hardlink_preserves_verify_and_rejects_outside_peers() {
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
    let tests = read("src/Store/Tests.rs");
    let tests_production = tests
        .split("#[test]\nfn store_stays_split_along_existing_phases")
        .next()
        .unwrap();

    for (relative, source) in [
        ("src/Store.rs", store.as_str()),
        ("src/Store/Ingest.rs", ingest.as_str()),
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
    assert!(store.contains("\n#[cfg(test)]\nmod Tests;"));

    let ordered = [
        "pub struct IngestRequest",
        "pub fn recover_hangar_staging",
        "pub fn ingest_tree",
        "pub fn referrers_of",
        "fn copy_nofollow_tree",
        "fn stable_meta_identity",
    ];
    let positions: Vec<usize> = ordered
        .iter()
        .map(|needle| ingest.find(needle).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
