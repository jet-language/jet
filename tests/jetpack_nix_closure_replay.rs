//! Focused witnesses for locked Nix closure identity and CAS archive policy.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::Scratch;

#[path = "support/nix_index_cache_server.rs"]
mod nix_index_cache_server;

#[cfg(target_os = "linux")]
#[test]
fn local_index_lock_preserves_admission_and_rejects_trust_tampering() {
    use jetpack::Store::{self, ProducerRecord, Roots};
    use nix_index_cache_server::NixIndexCacheServer;

    let project = Scratch::new("local-index-lock-project");
    let root = Scratch::new("local-index-lock-hangar");
    let replay = Scratch::new("local-index-replay-hangar");
    let catalog = Scratch::new("local-index-catalog");
    let replacement = Scratch::new("local-index-replacement");
    let home = Scratch::new("local-index-lock-home");
    let nested_project = Scratch::new("local-index-nested-project");
    let source = nested_project.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), b"isolated adapter output\n").unwrap();
    let plan = jet_env_model::ModuleEval::AdapterPlan {
        name: "local-tool-consumer".into(),
        source: source.to_string_lossy().into_owned(),
        deps: vec![jet_env_model::Merge::Pkg { source: "jetpack".into(), name: "ripgrep".into() }],
        recipe: jet_env_model::ModuleEval::AdapterRecipe::Build(jet_pkg_model::Recipe::BuildRecipe {
            steps: vec![jet_pkg_model::Recipe::BuildStep::Install {
                src: "payload".into(),
                dest: "payload".into(),
            }],
        }),
    };
    let temporary = home.join("tmp");
    fs::create_dir_all(&temporary).unwrap();
    let server = NixIndexCacheServer::start_ripgrep(&project.path);
    server.install(&root.path);
    fs::remove_file(root.join("trust/nix-index-v1.ed25519.pub")).unwrap();
    let index_path = catalog.join(
        server.signed_index.target_url
            .strip_prefix(&server.index_endpoint).unwrap().trim_start_matches('/'),
    );
    fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    fs::write(&index_path, &server.signed_index.index_bytes).unwrap();
    let index_authority = index_path.file_name().unwrap().to_str().unwrap()
        .strip_suffix(".json.zst").unwrap().to_string();
    fs::create_dir_all(project.join(".jet")).unwrap();
    // Declare the adapter before Nix admission binds the complete lock digest.
    // The nested project receives this same declaration, not an after-the-fact
    // lock change that would refresh the original Nix producer's authority.
    let expected_adapter = Scratch::new("local-index-expected-adapter");
    // Scratch's own package.jet is fixture authority, not an installed output.
    let expected_tree = expected_adapter.join("output");
    fs::create_dir(&expected_tree).unwrap();
    fs::write(expected_tree.join("payload"), b"isolated adapter output\n").unwrap();
    // Recipe publishes an owner-only private output directory.
    fs::set_permissions(&expected_tree, fs::Permissions::from_mode(0o700)).unwrap();
    Store::seal_local_output(&expected_tree).unwrap();
    let source_hash = jetpack::Envelope::try_output_hash_of(&plan.source).unwrap();
    let adapter_reference = format!("adapt:{}:{}", plan.name, plan.source);
    let expected_output = jetpack::Envelope::try_output_hash_of(
        &expected_tree.to_string_lossy(),
    ).unwrap();
    let mut initial_lock = jetpack::Lock::parse(&format!(
        "version = 1\n\n[[source_channel]]\nname = \"jetpack\"\nchannel = \"nixpkgs-unstable\"\nexact = \"github:NixOS/nixpkgs#{}\"\n\n[root]\ndependencies = []\n",
        nix_index_cache_server::REVISION,
    )).unwrap();
    initial_lock.packages.push(jetpack::Lock::LockedPackage {
        name: plan.name.clone(),
        version: String::new(),
        source: jetpack::Lock::LockSource::Path(adapter_reference),
        nix_closure: None,
        locked: None,
        fingerprint: source_hash.clone(),
        content_hash: Some(source_hash),
        dependencies: vec!["ripgrep".into()],
        layer: None,
        inferred_layer: None,
        effects: Vec::new(),
        effect_grants: Vec::new(),
        required_effects: Vec::new(),
        granted_effects: Vec::new(),
        denied_effects: Vec::new(),
        effect_authority: None,
        envelope: Some(jetpack::Lock::LockEnvelope {
            output_hash: expected_output,
            platform: jetpack::Envelope::host_platform(),
            ..Default::default()
        }),
        receipt: None,
        provenance: None,
    });
    fs::write(project.join(".jet/lock"), jetpack::Lock::write(&initial_lock)).unwrap();
    let prepare = |hangar: &Path, selected_catalog: &Path, offline: bool| {
        let mut command = Command::new(common::jetpack_bin());
        command.args(["env", "--env", "dev", "--prep", "--trust", "--no-color", "--yes", "--local-nix-catalog"])
            .arg(selected_catalog)
            .current_dir(&project.path)
            .env_clear()
            .env("PATH", "")
            .env("HOME", &home.path)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("TMPDIR", &temporary)
            .env("TMP", &temporary)
            .env("TEMP", &temporary)
            .env("JETPACK_SHARED_CAS", hangar.join("hangar/cas"))
            .env("JETPACK_ROOT", hangar)
            .env("JETPACK_NIX_FALLBACK_POLICY", "deny");
        if offline {
            command.arg("--offline");
        }
        command.output().unwrap()
    };
    let assert_success = |output: std::process::Output| {
        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    };
    // Invocation-only trust is confined to this loopback fixture. No live
    // provider approval, real installation, or persistent grant is authorized.
    let env_command = |local: bool| {
        let mut command = Command::new(common::jetpack_bin());
        command.args(["env", "--env", "dev", "--offline", "--trust", "--no-color"])
            .current_dir(&project.path)
            .env_clear()
            .env("PATH", "")
            .env("HOME", &home.path)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("TMPDIR", &temporary)
            .env("TMP", &temporary)
            .env("TEMP", &temporary)
            .env("JETPACK_SHARED_CAS", root.join("hangar/cas"))
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_NIX_FALLBACK_POLICY", "deny");
        if local {
            command.arg("--local-nix-catalog").arg(&replacement.path);
        }
        command
    };
    let prepare_env = |local: bool| env_command(local).arg("--prep").output().unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev {\n    sources: { jetpack: NixOS/nixpkgs/nixpkgs-unstable@github }\n    packages: [jetpack.ripgrep]\n}\n",
    ).unwrap();
    assert_success(prepare(&root.path, &catalog.path, false));
    let (closure, envelope) =
        jetpack::Lock::nix_realization_strict(&project.path, "ripgrep@jetpack")
            .unwrap().unwrap();
    assert_eq!(closure.index_authority, index_authority);
    assert_eq!(envelope.catalog_tier, "local-unofficial");
    assert_eq!(envelope.catalog_trust, "unverified");
    let original_lock = fs::read(project.join(".jet/lock")).unwrap();
    let entries = Store::list_checked(&Roots::at(root.path.clone())).unwrap();
    let original = entries.iter().find(|entry| entry.reference == "ripgrep@jetpack").unwrap().clone();
    let producer = ProducerRecord::decode(&original.producer_record).unwrap();
    assert_eq!(producer.facts["nix.index.target.sha256"], index_authority);
    assert_eq!(producer.facts["nix.index.manifest.sha256"], "");
    assert_eq!(producer.facts["nix.project-cas-bundle"], closure.project_cas_bundle);
    let original_graph = Store::closure_graph(&Roots::at(root.path.clone())).unwrap();
    let original_closure = original_graph.closure(&closure.output);

    server.reset_counts();
    server.stop_network();
    let bundle = project.join(".jet/nix-cas").join(&closure.project_cas_bundle);
    let saved_bundle = project.join("saved-bundle");
    fs::rename(&bundle, &saved_bundle).unwrap();
    assert_success(prepare(&root.path, &replacement.path, true));
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    let env_receipt = project.join(".jet/receipts/env-entry");
    fs::remove_file(&env_receipt).unwrap();
    let unapproved = prepare_env(false);
    assert_eq!(unapproved.status.code(), Some(1));
    assert!(!env_receipt.exists(), "unapproved local admission created an environment receipt");
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    assert_success(prepare_env(true));
    fs::remove_file(&env_receipt).unwrap();
    assert_success(prepare_env(true));
    // An intact warm environment receipt is not an invocation permission grant.
    let allowed = env_command(true).args(["--", "/bin/sh", "-c", "printf allowed"])
        .output().unwrap();
    assert!(allowed.status.success(), "{}", String::from_utf8_lossy(&allowed.stderr));
    assert_eq!(allowed.stdout, b"allowed");
    let intact_receipt = fs::read(&env_receipt).unwrap();
    let unapproved = env_command(false).args(["--", "/bin/sh", "-c", "printf forbidden"])
        .output().unwrap();
    assert_eq!(unapproved.status.code(), Some(1));
    assert!(unapproved.stdout.is_empty(), "unapproved command ran");
    assert!(String::from_utf8_lossy(&unapproved.stderr).contains("--local-nix-catalog"));
    assert_eq!(fs::read(&env_receipt).unwrap(), intact_receipt);
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    let exported = env_command(true).args(["export", "bash"]).output().unwrap();
    assert!(exported.status.success(), "{}", String::from_utf8_lossy(&exported.stderr));
    assert!(String::from_utf8_lossy(&exported.stdout).contains("export PATH="));
    let intact_receipt = fs::read(&env_receipt).unwrap();
    let unapproved = env_command(false).args(["export", "bash"]).output().unwrap();
    // Export's quiet refusal must preserve the caller's existing activation.
    assert!(unapproved.status.success());
    assert!(unapproved.stdout.is_empty(), "unapproved export emitted activation");
    assert_eq!(fs::read(&env_receipt).unwrap(), intact_receipt);
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    let (env_closure, env_envelope) =
        jetpack::Lock::nix_realization_strict(&project.path, "ripgrep@jetpack")
            .unwrap().unwrap();
    assert_eq!(env_closure, closure);
    assert_eq!(env_envelope, envelope);
    let original_lock = fs::read(project.join(".jet/lock")).unwrap();
    let warm_entries = Store::list_checked(&Roots::at(root.path.clone())).unwrap();
    let warm = warm_entries.iter().find(|entry| entry.reference == "ripgrep@jetpack").unwrap();
    let warm_producer = ProducerRecord::decode(&warm.producer_record).unwrap();
    assert_eq!(warm_producer.facts["nix.output.out"], server.root_store_path);
    assert_eq!(warm.producer_record, original.producer_record);
    assert_eq!(warm.named_outputs, original.named_outputs);
    assert_eq!(
        Store::closure_graph(&Roots::at(root.path.clone())).unwrap().closure(&closure.output),
        original_closure,
    );
    // JetOS and other non-CLI consumers cross this same Store boundary.
    let roots = Roots::at(root.path.clone());
    assert_eq!(roots.shared_cas_dir(), root.join("hangar/cas"));
    let store_dir = roots.hangar_dir();
    let spec = jetpack::RefSpec::classify("ripgrep@jetpack").unwrap();
    let table = jetpack::RefSpec::SourceTable::empty();
    let mut ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: true,
        project_dir: Some(&project.path),
        nix_index: None,
        nix_roots: Some(&roots),
        allow_local_nix_catalog: false,
    };
    let denied = Store::realize_verified(
        &roots, &ctx, Store::RealizeRequest::Package { spec: &spec, table: &table },
    );
    match denied {
        Err(Store::RealizeError::LockedNix(error)) => {
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
            assert!(error.to_string().contains("--local-nix-catalog"));
        }
        Err(error) => panic!("unexpected replay error: {error:?}"),
        Ok(_) => panic!("Store reused a local admission without invocation permission"),
    }
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    ctx.allow_local_nix_catalog = true;
    let allowed = Store::realize_verified(
        &roots, &ctx, Store::RealizeRequest::Package { spec: &spec, table: &table },
    ).unwrap();
    assert_eq!(allowed.metadata().producer_record, original.producer_record);
    assert_eq!(allowed.metadata().envelope, original.envelope);
    assert_eq!(allowed.metadata().named_outputs, original.named_outputs);
    drop(allowed);
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    // An adapter's private build roots must inherit denial, and an allowed
    // dependency must retain its original verified native-Hangar admission.
    fs::create_dir_all(nested_project.join(".jet")).unwrap();
    fs::write(nested_project.join(".jet/lock"), &original_lock).unwrap();
    let mut nested_ctx = jetpack::Provider::Ctx {
        project_dir: Some(&nested_project.path),
        allow_local_nix_catalog: false,
        ..ctx
    };
    let expectation = jetpack::Provider::adapter_cache_expectation(&plan, &table, &nested_ctx).unwrap();
    let denied = Store::realize_verified(
        &roots, &nested_ctx,
        Store::RealizeRequest::Adapter { plan: &plan, table: &table, expectation: &expectation },
    );
    match denied {
        Err(Store::RealizeError::LockedNix(error)) => {
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        }
        Err(error) => panic!("unexpected nested replay error: {error:?}"),
        Ok(_) => panic!("adapter reused a local dependency without invocation permission"),
    }
    assert!(!Store::list_checked(&roots).unwrap().iter().any(|entry| entry.name == plan.name));
    assert_eq!(fs::read(nested_project.join(".jet/lock")).unwrap(), original_lock);
    nested_ctx.allow_local_nix_catalog = true;
    let adapted = Store::realize_verified(
        &roots, &nested_ctx,
        Store::RealizeRequest::Adapter { plan: &plan, table: &table, expectation: &expectation },
    ).unwrap();
    let adapted_entry = adapted.metadata();
    assert!(adapted_entry.version.is_empty(), "unversioned adapter acquired an invented version");
    let nested_lock = jetpack::Lock::load(&nested_project.path).unwrap();
    let locked_adapter = nested_lock.packages.iter().find(|package| package.name == plan.name).unwrap();
    assert_eq!(locked_adapter.receipt.as_deref(), Some(adapted_entry.receipt.as_str()));
    assert_eq!(fs::read(Path::new(&adapted_entry.out).join("payload")).unwrap(), b"isolated adapter output\n");
    assert!(Store::verify_cache_entry(
        &roots, adapted_entry, &adapted_entry.reference, &expectation,
    ).trusted());
    let adapted_producer = ProducerRecord::decode(&adapted_entry.producer_record).unwrap();
    assert_eq!(adapted_producer.facts["build.dependencies"], "ripgrep@jetpack");
    assert_eq!(adapted_producer.facts["build.sandbox"], "non-executing");
    assert_eq!(adapted_producer.facts["build.sandbox_policy"], "no child launched");
    assert!(adapted_entry.references.is_empty(), "build-only tool became a dangling runtime reference");
    let warm_adapter = Store::realize_verified(
        &roots, &nested_ctx,
        Store::RealizeRequest::Adapter { plan: &plan, table: &table, expectation: &expectation },
    ).unwrap();
    assert_eq!(warm_adapter.source_state(), jetpack::Provider::SourceState::Cached);
    assert_eq!(warm_adapter.metadata().producer_record, adapted_entry.producer_record);
    assert_eq!(fs::read(warm_adapter.original_output().join("payload")).unwrap(), b"isolated adapter output\n");
    drop(warm_adapter);
    drop(adapted);
    let entries = Store::list_checked(&roots).unwrap();
    let after_nested = entries.iter().find(|entry| entry.reference == original.reference).unwrap();
    assert_eq!(after_nested.producer_record, original.producer_record);
    assert_eq!(after_nested.envelope, original.envelope);
    assert_eq!(after_nested.named_outputs, original.named_outputs);
    assert_eq!(Store::closure_graph(&roots).unwrap().closure(&closure.output), original_closure);
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), original_lock);
    fs::rename(&saved_bundle, &bundle).unwrap();
    assert_success(prepare(&replay.path, &replacement.path, true));
    let (replayed_closure, replayed_envelope) =
        jetpack::Lock::nix_realization_strict(&project.path, "ripgrep@jetpack")
            .unwrap().unwrap();
    assert_eq!(replayed_closure, closure);
    assert_eq!(replayed_envelope, envelope);
    let entries = Store::list_checked(&Roots::at(replay.path.clone())).unwrap();
    let replayed = entries.iter().find(|entry| entry.reference == "ripgrep@jetpack").unwrap();
    assert_eq!(replayed.envelope.output_hash, closure.output);
    assert_eq!(replayed.references, closure.references);
    let producer = ProducerRecord::decode(&replayed.producer_record).unwrap();
    assert_eq!(producer.facts["nix.index.target.sha256"], index_authority);
    assert_eq!(producer.facts["nix.index.tier"], "local-unofficial");
    assert_eq!(producer.facts["nix.index.trust"], "unverified");
    assert!(!producer.facts.contains_key("nix.index.manifest.sha256"));
    assert_eq!(server.object_request_count(&server.root_store_path), 0);
    for path in &server.transitive_store_paths {
        assert_eq!(server.object_request_count(path), 0);
    }

    let baseline = jetpack::Lock::parse(std::str::from_utf8(&original_lock).unwrap()).unwrap();
    let mut promoted = baseline.clone();
    let promoted_envelope = promoted.packages.iter_mut()
        .find(|package| package.nix_closure.is_some()).unwrap().envelope.as_mut().unwrap();
    promoted_envelope.catalog_tier = "official-signed".into();
    promoted_envelope.catalog_trust = "verified".into();
    fs::write(project.join(".jet/lock"), jetpack::Lock::write(&promoted)).unwrap();
    assert!(!prepare(&root.path, &replacement.path, true).status.success());
    assert!(!prepare_env(true).status.success());
    let empty = Scratch::new("local-index-forged-official-cold");
    assert!(!prepare(&empty.path, &replacement.path, true).status.success());
    assert!(Store::list_checked(&Roots::at(empty.path.clone())).unwrap().is_empty());

    for change_authority in [true, false] {
        let mut tampered = baseline.clone();
        let record = tampered.packages.iter_mut()
            .find_map(|package| package.nix_closure.as_mut()).unwrap();
        if change_authority {
            record.index_authority = "d".repeat(64);
        } else {
            record.project_cas_bundle = format!("sha256-{}", "d".repeat(64));
        }
        fs::write(project.join(".jet/lock"), jetpack::Lock::write(&tampered)).unwrap();
        assert!(!prepare(&root.path, &replacement.path, true).status.success());
    }
    let after_rejections = Store::list_checked(&Roots::at(root.path.clone())).unwrap();
    let unchanged = after_rejections.iter()
        .find(|entry| entry.reference == "ripgrep@jetpack").unwrap();
    assert_eq!(unchanged.producer_record, warm.producer_record);
    assert_eq!(unchanged.receipt, warm.receipt);
    fs::write(project.join(".jet/lock"), &original_lock).unwrap();

    let runtime = after_rejections.iter().find(|entry| {
        ProducerRecord::decode(&entry.producer_record).ok().is_some_and(|record| {
            record.facts.get("nix.store-path").map(String::as_str)
                == Some(nix_index_cache_server::RUNTIME_PATH)
        })
    }).unwrap();
    let marker = Path::new(&runtime.out).join("share/marker");
    use std::os::unix::fs::PermissionsExt as _;
    let marker_parent = marker.parent().unwrap();
    let parent_permissions = fs::metadata(marker_parent).unwrap().permissions();
    fs::set_permissions(marker_parent, fs::Permissions::from_mode(
        parent_permissions.mode() | 0o200,
    )).unwrap();
    let replacement_marker = marker_parent.join("replacement-marker");
    use std::io::Write as _;
    fs::OpenOptions::new().write(true).create_new(true)
        .open(&replacement_marker).unwrap()
        .write_all(b"corrupted runtime").unwrap();
    fs::rename(&replacement_marker, &marker).unwrap();
    fs::set_permissions(marker_parent, parent_permissions).unwrap();
    assert!(!prepare(&root.path, &replacement.path, true).status.success());
    assert!(!prepare_env(true).status.success());
    assert_eq!(fs::read(&marker).unwrap(), b"corrupted runtime");
}

fn hex_digest(prefix: &str, fill: char) -> String {
    format!("{prefix}{}", fill.to_string().repeat(64))
}

fn cas_digest(fill: char) -> String {
    hex_digest("sha256-", fill)
}

fn valid_nix_closure(output: &str) -> jetpack::Lock::NixClosureRecord {
    jetpack::Lock::NixClosureRecord {
        channel: "nixpkgs-unstable".into(),
        revision: "a".repeat(40),
        system: jetpack::Envelope::host_platform(),
        index_authority: "b".repeat(64),
        derivation: "c".repeat(64),
        output: output.into(),
        nar_hash: format!("sha256:{}", "d".repeat(64)),
        size: 1,
        compression: "none".into(),
        references: Vec::new(),
        upstream_proof: cas_digest('e'),
        cache_key: "f".repeat(64),
        project_cas_bundle: cas_digest('0'),
    }
}

fn lock_envelope(output: &str, platform: &str) -> jetpack::Lock::LockEnvelope {
    jetpack::Lock::LockEnvelope {
        output_hash: output.into(),
        platform: platform.into(),
        provenance: "integration-test".into(),
        catalog_tier: "local-unofficial".into(),
        catalog_trust: "unverified".into(),
        ..Default::default()
    }
}

#[test]
fn malformed_present_lock_is_a_trust_error_not_a_cache_miss() {
    let project = Scratch::new("nix-lock-malformed");
    let lock_path = project.join(".jet/lock");
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    fs::write(&lock_path, "version = [\n").unwrap();

    let error = jetpack::Lock::load_strict(&project.path).unwrap_err();
    assert!(!error.is_empty(), "malformed present lock must be rejected");
    let error =
        jetpack::Lock::nix_realization_strict(&project.path, "pkg@jetpack").unwrap_err();
    assert!(!error.is_empty(), "Nix lookup must not downgrade malformed lock state");
}

#[test]
fn nix_lock_rejects_platform_and_envelope_mismatch() {
    let project = Scratch::new("nix-lock-platform");
    let output = cas_digest('1');
    let closure = valid_nix_closure(&output);
    let platform = jetpack::Envelope::host_platform();
    let mismatch = if platform == "aarch64-linux" {
        "x86_64-linux"
    } else {
        "aarch64-linux"
    };

    let error = jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        "pkg@jetpack",
        &output,
        closure.clone(),
        lock_envelope(&output, &mismatch),
    )
    .unwrap_err();
    assert!(error.contains("system does not match"), "{error}");
    assert!(
        !project.join(".jet/lock").exists(),
        "rejected identity must not publish a project lock"
    );

    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        "pkg@jetpack",
        &output,
        closure,
        lock_envelope(&output, &platform),
    )
    .unwrap();
    let lock_path = project.join(".jet/lock");
    let mut lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    lock.packages[0].envelope.as_mut().unwrap().platform = "aarch64-linux".into();
    fs::write(&lock_path, jetpack::Lock::write(&lock)).unwrap();

    let error =
        jetpack::Lock::nix_realization_strict(&project.path, "pkg@jetpack").unwrap_err();
    assert!(error.contains("envelope identity"), "{error}");
}

#[test]
fn nix_lock_receipt_follows_complete_closure_identity() {
    let project = Scratch::new("nix-lock-identity");
    let output = cas_digest('2');
    let mut closure = valid_nix_closure(&output);
    let envelope = lock_envelope(&output, "x86_64-linux");
    let reference = "pkg@jetpack";

    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        reference,
        &output,
        closure.clone(),
        envelope.clone(),
    )
    .unwrap();
    let lock_path = project.join(".jet/lock");
    let mut lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    let receipt = cas_digest('9');
    lock.packages[0].receipt = Some(receipt.clone());
    fs::write(&lock_path, jetpack::Lock::write(&lock)).unwrap();

    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        reference,
        &output,
        closure.clone(),
        envelope.clone(),
    )
    .unwrap();
    let lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.packages[0].receipt.as_deref(), Some(receipt.as_str()));

    closure.index_authority = "d".repeat(64);
    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        reference,
        &output,
        closure,
        envelope,
    )
    .unwrap();
    let lock = jetpack::Lock::parse(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.packages[0].receipt, None);
}

#[test]
fn locked_nix_missing_bundle_reports_registered_diagnostic_before_store_mutation() {
    let project = Scratch::new("nix-lock-missing-bundle-project");
    let root = Scratch::new("nix-lock-missing-bundle-root");
    fs::write(
        project.join("env.jet"),
        "module dev {\n    sources: { default: NixOS/nixpkgs/nixpkgs-unstable@github }\n    env.dev: Env{ packages: [default.pkg] }\n}\n",
    )
    .unwrap();

    let output = cas_digest('6');
    let closure = valid_nix_closure(&output);
    let envelope = lock_envelope(&output, &jetpack::Envelope::host_platform());
    jetpack::Lock::record_nix_realization(
        &project.path,
        "pkg",
        "1.0.0",
        "pkg@default",
        &output,
        closure,
        envelope,
    )
    .unwrap();

    let home = Scratch::new("nix-lock-missing-bundle-home");
    let command = Command::new(common::jetpack_bin())
        .args(["env", "--offline", "--no-color", "--trust", "--", "true"])
        .current_dir(&project.path)
        .env_clear()
        .env("PATH", "/usr/bin")
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&command.stderr);
    assert!(!command.status.success(), "missing lock bytes must fail");
    assert!(stderr.contains("E1350"), "stderr: {stderr}");
    assert!(
        stderr.contains("project Nix CAS bundle") || stderr.contains("locked Nix"),
        "stderr must name the lock-bound replay: {stderr}"
    );
    assert!(
        !root.join("hangar/objects").exists(),
        "missing lock bytes must fail before Hangar mutation"
    );
}

#[cfg(unix)]
fn copy_tree_nofollow(source: &Path, destination: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = fs::symlink_metadata(source).unwrap();
    if metadata.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(source).unwrap(), destination).unwrap();
    } else if metadata.is_dir() {
        fs::create_dir(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_tree_nofollow(&entry.path(), &destination.join(entry.file_name()));
        }
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )
        .unwrap();
    } else {
        fs::copy(source, destination).unwrap();
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )
        .unwrap();
    }
}

#[cfg(unix)]
fn relative_link_entry(
    root: &Scratch,
    source: &Scratch,
    target: &str,
) -> jetpack::Store::StoreEntry {
    fs::write(source.path.join(target), b"link target payload\n").unwrap();
    std::os::unix::fs::symlink(target, source.join("link")).unwrap();

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let request = jetpack::Store::IngestRequest {
        name: "link-fixture".into(),
        version: "1.0.0".into(),
        reference: "link-fixture@fixture".into(),
        cache_identity: jetpack::Store::CacheIdentity {
            source_fingerprint: "source-fingerprint".into(),
            recipe_fingerprint: "recipe-fingerprint".into(),
            policy_fingerprint: "policy-fingerprint".into(),
            platform: "x86_64-linux".into(),
        },
        references: Vec::new(),
        outputs: BTreeMap::from([(String::from("out"), source.path.clone())]),
        signature: String::new(),
        provenance: "integration-test".into(),
        platform_artifact_kind: String::new(),
    };
    jetpack::Store::ingest_tree(&roots, &request).unwrap().entry
}

#[cfg(unix)]
fn read_u32(bytes: &[u8], at: &mut usize) -> u32 {
    let end = *at + 4;
    let value = u32::from_le_bytes(bytes[*at..end].try_into().unwrap());
    *at = end;
    value
}

#[cfg(unix)]
fn read_u64(bytes: &[u8], at: &mut usize) -> u64 {
    let end = *at + 8;
    let value = u64::from_le_bytes(bytes[*at..end].try_into().unwrap());
    *at = end;
    value
}

#[cfg(unix)]
fn skip_string(bytes: &[u8], at: &mut usize) {
    let length = read_u32(bytes, at) as usize;
    *at += length;
}

#[cfg(unix)]
fn read_bytes(bytes: &[u8], at: &mut usize) -> Vec<u8> {
    let length = read_u32(bytes, at) as usize;
    let end = *at + length;
    let value = bytes[*at..end].to_vec();
    *at = end;
    value
}

#[cfg(unix)]
#[derive(Clone)]
struct RawArchiveNode {
    kind: u8,
    path: Vec<u8>,
    mode: u32,
    bytes: Vec<u8>,
}

#[cfg(unix)]
#[derive(Clone)]
struct RawArchiveOutput {
    root_mode: u32,
    nodes: Vec<RawArchiveNode>,
}

#[cfg(unix)]
fn read_generic_output(bytes: &[u8], expected: &str) -> RawArchiveOutput {
    let magic = b"jet-hangar-archive-v1\0";
    let mut at = magic.len();
    skip_string(bytes, &mut at);
    let object_count = read_u32(bytes, &mut at) as usize;
    let mut output = None;
    for _ in 0..object_count {
        let _id = read_bytes(bytes, &mut at);
        let digest = read_bytes(bytes, &mut at);
        let meta = read_bytes(bytes, &mut at);
        let root_mode = read_u32(bytes, &mut at);
        let node_count = read_u32(bytes, &mut at) as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let kind = bytes[at];
            at += 1;
            let path = read_bytes(bytes, &mut at);
            let mode = read_u32(bytes, &mut at);
            let size = read_u64(bytes, &mut at) as usize;
            let end = at + size;
            let payload = bytes[at..end].to_vec();
            at = end;
            nodes.push(RawArchiveNode {
                kind,
                path,
                mode,
                bytes: payload,
            });
        }
        if meta.is_empty() && digest == expected.as_bytes() {
            output = Some(RawArchiveOutput { root_mode, nodes });
        }
    }
    output.expect("generic archive must contain the requested output")
}

#[cfg(unix)]
fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(unix)]
fn put_raw_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(unix)]
fn encode_nix_bundle(
    output: &str,
    source: &RawArchiveOutput,
    platform: &str,
    revision: &str,
    cache_key: &str,
) -> Vec<u8> {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&source.root_mode.to_le_bytes());
    for node in &source.nodes {
        canonical.push(node.kind);
        put_bytes(&mut canonical, &node.path);
        canonical.extend_from_slice(&node.mode.to_le_bytes());
        put_raw_u64(&mut canonical, node.bytes.len() as u64);
        canonical.extend_from_slice(&node.bytes);
    }
    let member_hash = format!("sha256-{}", jetpack::SHA256::sha256_hex(&canonical));

    let mut out = Vec::new();
    out.extend_from_slice(b"jet-hangar-archive-v1\0");
    put_bytes(&mut out, output.as_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, &[]);
    out.extend_from_slice(&source.root_mode.to_le_bytes());
    out.extend_from_slice(&(source.nodes.len() as u32).to_le_bytes());
    for node in &source.nodes {
        out.push(node.kind);
        put_bytes(&mut out, &node.path);
        out.extend_from_slice(&node.mode.to_le_bytes());
        put_raw_u64(&mut out, node.bytes.len() as u64);
        out.extend_from_slice(&node.bytes);
    }
    out.push(2);
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, platform.as_bytes());
    put_bytes(&mut out, revision.as_bytes());
    put_bytes(&mut out, cache_key.as_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    put_bytes(&mut out, output.as_bytes());
    put_bytes(&mut out, member_hash.as_bytes());
    put_raw_u64(&mut out, canonical.len() as u64);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out
}

#[cfg(unix)]
fn replace_link_target(bytes: &[u8], old: &[u8], replacement: &[u8]) -> Vec<u8> {
    assert!(!old.is_empty());
    assert_eq!(old.len(), replacement.len());
    let magic = b"jet-hangar-archive-v1\0";
    assert!(bytes.starts_with(magic));
    let mut at = magic.len();
    skip_string(bytes, &mut at);
    let object_count = read_u32(bytes, &mut at) as usize;
    assert_eq!(object_count, 1, "fixture must contain one output object");
    let mut object_digest = None;
    let mut object = None;
    for _ in 0..object_count {
        let _id = read_bytes(bytes, &mut at);
        let digest = read_bytes(bytes, &mut at);
        let meta = read_bytes(bytes, &mut at);
        let root_mode = read_u32(bytes, &mut at);
        let node_count = read_u32(bytes, &mut at) as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let kind = bytes[at];
            at += 1;
            let path = read_bytes(bytes, &mut at);
            let mode = read_u32(bytes, &mut at);
            let size = read_u64(bytes, &mut at) as usize;
            let end = at + size;
            nodes.push(RawArchiveNode {
                kind,
                path,
                mode,
                bytes: bytes[at..end].to_vec(),
            });
            at = end;
        }
        if meta.is_empty() {
            object_digest = Some(digest);
            object = Some(RawArchiveOutput { root_mode, nodes });
        }
    }

    assert_eq!(bytes[at], 2, "fixture must carry a Nix manifest");
    at += 1;
    let manifest_output = read_bytes(bytes, &mut at);
    let platform = read_bytes(bytes, &mut at);
    let revision = read_bytes(bytes, &mut at);
    let cache_key = read_bytes(bytes, &mut at);
    let member_count = read_u32(bytes, &mut at) as usize;
    for _ in 0..member_count {
        skip_string(bytes, &mut at);
        skip_string(bytes, &mut at);
        read_u64(bytes, &mut at);
        let reference_count = read_u32(bytes, &mut at) as usize;
        for _ in 0..reference_count {
            skip_string(bytes, &mut at);
        }
    }
    let object_digest = object_digest.expect("fixture must carry an output object");
    assert_eq!(object_digest, manifest_output);
    let mut object = object.expect("fixture must carry an output object");
    let mut matches = 0;
    for node in &mut object.nodes {
        if node.kind == 2 && node.bytes == old {
            node.bytes = replacement.to_vec();
            matches += 1;
        }
    }
    assert_eq!(matches, 1, "archive must contain one symlink target");
    encode_nix_bundle(
        &String::from_utf8(manifest_output).unwrap(),
        &object,
        &String::from_utf8(platform).unwrap(),
        &String::from_utf8(revision).unwrap(),
        &String::from_utf8(cache_key).unwrap(),
    )
}

#[cfg(unix)]
fn replace_all(bytes: &[u8], old: &[u8], replacement: &[u8]) -> (Vec<u8>, usize) {
    assert!(!old.is_empty());
    assert_eq!(old.len(), replacement.len());
    let mut result = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    let mut count = 0;
    while let Some(relative) = bytes[cursor..]
        .windows(old.len())
        .position(|window| window == old)
    {
        let offset = cursor + relative;
        result.extend_from_slice(&bytes[cursor..offset]);
        result.extend_from_slice(replacement);
        cursor = offset + old.len();
        count += 1;
    }
    result.extend_from_slice(&bytes[cursor..]);
    (result, count)
}

#[cfg(unix)]
fn cas_digest_from_bytes(bytes: &[u8]) -> String {
    format!("sha256-{}", jetpack::SHA256::sha256_hex(bytes))
}

#[cfg(unix)]
#[test]
fn nix_cas_build_and_import_preserve_absolute_links_but_generic_paths_reject_them() {
    let source_root = Scratch::new("nix-cas-source-root");
    let source = Scratch::new("nix-cas-source");
    let absolute_target = "/nix/store/jet-closure-target";
    let relative_target = "x".repeat(absolute_target.len());
    let entry = relative_link_entry(&source_root, &source, &relative_target);
    let roots = jetpack::Store::Roots {
        root: source_root.path.clone(),
        dev_mode: false,
    };
    let generic_archive =
        jetpack::Store::export_unsigned_archive(&roots, &entry.reference, true).unwrap();
    let generic_output = read_generic_output(&generic_archive, &entry.envelope.output_hash);
    let platform = jetpack::Envelope::host_platform();
    let revision = "a".repeat(40);
    let cache_key = "f".repeat(64);
    let bundle = encode_nix_bundle(
        &entry.envelope.output_hash,
        &generic_output,
        &platform,
        &revision,
        &cache_key,
    );
    let absolute_source = Scratch::new("nix-cas-absolute-digest");
    let absolute_tree = absolute_source.join("tree");
    copy_tree_nofollow(&source.path, &absolute_tree);
    fs::remove_file(absolute_tree.join("link")).unwrap();
    std::os::unix::fs::symlink(absolute_target, absolute_tree.join("link")).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(
        &absolute_tree,
        fs::Permissions::from_mode(generic_output.root_mode),
    )
    .unwrap();
    for node in &generic_output.nodes {
        if node.kind != 2 {
            let path = absolute_tree.join(String::from_utf8(node.path.clone()).unwrap());
            fs::set_permissions(path, fs::Permissions::from_mode(node.mode)).unwrap();
        }
    }
    let new_output = jetpack::Envelope::try_output_hash_of_in_hangar(
        &absolute_tree.to_string_lossy(),
        &roots.hangar_dir(),
        false,
    )
    .unwrap();
    assert_ne!(new_output, entry.envelope.output_hash);

    let (mut absolute_bundle, replaced_digests) = replace_all(
        &bundle,
        entry.envelope.output_hash.as_bytes(),
        new_output.as_bytes(),
    );
    assert!(replaced_digests >= 2, "root and output digest must be rewritten");
    absolute_bundle = replace_link_target(
        &absolute_bundle,
        relative_target.as_bytes(),
        absolute_target.as_bytes(),
    );

    let project = Scratch::new("nix-cas-project");
    let bundle_dir = project.join(".jet/nix-cas");
    fs::create_dir_all(&bundle_dir).unwrap();
    let bundle_digest = cas_digest_from_bytes(&absolute_bundle);
    fs::write(bundle_dir.join(&bundle_digest), &absolute_bundle).unwrap();

    let destination = Scratch::new("nix-cas-destination");
    let destination_roots = jetpack::Store::Roots {
        root: destination.path.clone(),
        dev_mode: false,
    };
    let (report, digests) = jetpack::Store::import_nix_cas_bundle(
        &project.path,
        &destination_roots,
        &bundle_digest,
    )
    .unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(digests, vec![new_output.clone()]);
    let imported_link = destination_roots
        .hangar_dir()
        .join("objects")
        .join(&new_output)
        .join("link");
    assert_eq!(
        fs::read_link(imported_link).unwrap(),
        PathBuf::from(absolute_target)
    );

    let mut closure = valid_nix_closure(&new_output);
    closure.project_cas_bundle = bundle_digest.clone();
    let replay_envelope = lock_envelope(&new_output, &platform);
    jetpack::Lock::record_nix_realization(
        &project.path,
        "link-fixture",
        "1.0.0",
        "link-fixture@jetpack",
        &new_output,
        closure,
        replay_envelope,
    )
    .unwrap();
    let spec = jetpack::RefSpec::classify("link-fixture@jetpack").unwrap();
    let table = jetpack::RefSpec::SourceTable::empty();
    let replayed = jetpack::Store::realize_verified(
        &destination_roots,
        &jetpack::Provider::Ctx {
            fixtures: None,
            store_dir: &destination.path,
            offline: true,
            project_dir: Some(&project.path),
            nix_index: None,
            nix_roots: Some(&destination_roots),
            allow_local_nix_catalog: true,
        },
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    )
    .unwrap();
    let nix_entry = replayed.metadata().clone();
    let warm = jetpack::Store::realize_verified(
        &destination_roots,
        &jetpack::Provider::Ctx {
            fixtures: None,
            store_dir: &destination.path,
            offline: true,
            project_dir: Some(&project.path),
            nix_index: None,
            nix_roots: Some(&destination_roots),
            allow_local_nix_catalog: true,
        },
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    )
    .unwrap();
    let mut expected_entry = nix_entry.clone();
    expected_entry.realized_at = 0;
    expected_entry.last_used_at = 0;
    let mut warm_entry = warm.metadata().clone();
    warm_entry.realized_at = 0;
    warm_entry.last_used_at = 0;
    assert_eq!(warm_entry, expected_entry, "warm replay changed lock identity");

    let copied_project = Scratch::new("nix-cas-copied-project");
    let copied_bundle_dir = copied_project.join(".jet/nix-cas");
    fs::create_dir_all(&copied_bundle_dir).unwrap();
    fs::copy(
        project.join(".jet/lock"),
        copied_project.join(".jet/lock"),
    )
    .unwrap();
    fs::copy(
        bundle_dir.join(&bundle_digest),
        copied_bundle_dir.join(&bundle_digest),
    )
    .unwrap();
    let copied_destination = Scratch::new("nix-cas-copied-destination");
    let copied_roots = jetpack::Store::Roots {
        root: copied_destination.path.clone(),
        dev_mode: false,
    };
    let copied_replayed = jetpack::Store::realize_verified(
        &copied_roots,
        &jetpack::Provider::Ctx {
            fixtures: None,
            store_dir: &copied_destination.path,
            offline: true,
            project_dir: Some(&copied_project.path),
            nix_index: None,
            nix_roots: Some(&copied_roots),
            allow_local_nix_catalog: true,
        },
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    )
    .unwrap();
    let mut copied_identity = copied_replayed.metadata().clone();
    copied_identity.out.clear();
    copied_identity.bin.clear();
    copied_identity.rlib.clear();
    let mut expected_identity = expected_entry.clone();
    expected_identity.out.clear();
    expected_identity.bin.clear();
    expected_identity.rlib.clear();
    // Hangar paths are intentionally root-local; every identity field must
    // remain equal when the lock and project CAS bundle move together.
    assert_eq!(
        copied_identity, expected_identity,
        "copied repository changed locked replay identity"
    );


    let (exported, _) =
        jetpack::Store::export_nix_cas_bundle(&destination_roots, &nix_entry.reference).unwrap();
    assert!(
        exported
            .windows(absolute_target.len())
            .any(|window| window == absolute_target.as_bytes()),
        "Nix CAS export must retain the absolute target as link data"
    );
    let generic_export =
        jetpack::Store::export_unsigned_archive(&destination_roots, &nix_entry.reference, false)
            .unwrap_err();
    assert!(
        generic_export
            .to_string()
            .contains("absolute symlinks are not portable Hangar archive nodes"),
        "{generic_export}"
    );

    let generic_absolute_root = Scratch::new("nix-cas-generic-absolute");
    let generic_absolute_roots = jetpack::Store::Roots {
        root: generic_absolute_root.path.clone(),
        dev_mode: false,
    };
    let error = jetpack::Store::import_archive(
        &generic_absolute_roots,
        &absolute_bundle,
        None,
        true,
    )
    .unwrap_err();
    assert!(error.to_string().contains("archive link target is absolute"), "{error}");

    let escape_target = format!("../{}", "x".repeat(relative_target.len() - 3));
    let control_target = {
        let mut target = vec![b'x'; relative_target.len()];
        target[relative_target.len() / 2] = 1;
        target
    };
    let backslash_target = {
        let mut target = vec![b'x'; relative_target.len()];
        target[relative_target.len() / 2] = b'\\';
        target
    };
    for (tag, target, message) in [
        (
            "escape",
            escape_target.into_bytes(),
            "archive link target escapes its output root",
        ),
        (
            "control",
            control_target,
            "archive link target is not portable",
        ),
        (
            "backslash",
            backslash_target,
            "archive link target is not portable",
        ),
    ] {
        let hostile_bundle = replace_link_target(&bundle, relative_target.as_bytes(), &target);
        let hostile_root = Scratch::new(&format!("nix-cas-generic-{tag}"));
        let hostile_roots = jetpack::Store::Roots {
            root: hostile_root.path.clone(),
            dev_mode: false,
        };
        let error = jetpack::Store::import_archive(&hostile_roots, &hostile_bundle, None, true)
            .unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
}
