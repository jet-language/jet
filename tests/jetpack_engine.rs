//! `jetpack` package engine tests (Tower card #367 slice 6 split).
//!
//! Core package/env mechanics driven through the compiled `jetpack`/`jet`
//! binaries against offline provider fixtures: doctor, build/hangar list/hangar clean/run,
//! env add/remove, channel update/outdated, typed sources (copy/prebuilt/
//! core/bad-adapter), no-nix reporting, bridge-flake, and monorepo/build-cache
//! behavior. Split out of the former `tests/jetpack.rs`; see
//! `tests/jetpack_dispatch.rs` / `tests/jetpack_jetos.rs` /
//! `tests/jetpack_studio.rs` for the other slices and
//! `tests/support/jetpack_fixtures.rs` for shared helpers.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use jet_foundation::BuildEffect;

fn make_writable(path: &str) {
    fn walk(path: &Path) {
        let metadata = fs::symlink_metadata(path).unwrap();
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(&entry.unwrap().path());
            }
        }
        let mut permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            permissions.set_mode(permissions.mode() | 0o200);
        }
        fs::set_permissions(path, permissions).unwrap();
    }
    walk(Path::new(path));
}

fn make_directories_writable(path: &Path) {
    let metadata = fs::symlink_metadata(path).unwrap();
    if !metadata.is_dir() {
        return;
    }
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
    for entry in fs::read_dir(path).unwrap() {
        make_directories_writable(&entry.unwrap().path());
    }
}

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
#[path = "support/nix_index_cache_server.rs"]
mod nix_index_cache_server;
#[path = "support/no_nix_namespace.rs"]
mod no_nix_namespace;
use jetpack_fixtures::*;

#[test]
fn hangar_path_reports_the_native_user_data_location() {
    let data = Scratch::new("hangar-path-data");
    let output = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_DATA_HOME", &data.path)
        .env_remove("XDG_STATE_HOME")
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .expect("home directory environment variable");
    let expected = if cfg!(target_os = "macos") {
        Path::new(&home).join("Library/Application Support/Jet/Hangar")
    } else if cfg!(windows) {
        Path::new(&home).join("AppData/Local/Jet/Hangar")
    } else {
        data.path.join("jet/hangar")
    };
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        expected.display().to_string()
    );
}

#[test]
fn checked_hangar_listing_rejects_oversized_metadata() {
    let scratch = Scratch::new("hangar-scale-budget");
    let roots = jetpack::Store::Roots::at(scratch.path.clone());
    let object = roots.hangar_dir().join("oversized");
    fs::create_dir_all(&object).unwrap();
    fs::write(object.join("meta.json"), vec![b'x'; (1 << 20) + 1]).unwrap();

    let error = jetpack::Store::list_checked(&roots).unwrap_err();
    assert!(
        error.to_string().contains("exceeds 1048576 bytes"),
        "{error}"
    );
}

#[test]
fn hangar_migration_round_trips_without_consuming_legacy_state() {
    let legacy = Scratch::new("hangar-migration-legacy");
    let data = Scratch::new("hangar-migration-data");
    let old_hangar = legacy.join("jet/hangar");
    fs::create_dir_all(&old_hangar).unwrap();
    fs::write(old_hangar.join("migration-marker"), "legacy bytes").unwrap();

    let output = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_STATE_HOME", &legacy.path)
        .env("XDG_DATA_HOME", &data.path)
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let new_hangar = data.join("jet/hangar");
    assert_eq!(
        fs::read_to_string(new_hangar.join("migration-marker")).unwrap(),
        "legacy bytes"
    );
    assert_eq!(
        fs::read_to_string(old_hangar.join("migration-marker")).unwrap(),
        "legacy bytes"
    );
    assert!(!data.join("jet/.hangar-migration.partial").exists());

    fs::remove_dir_all(&new_hangar).unwrap();
    let rollback = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_STATE_HOME", &legacy.path)
        .env("XDG_DATA_HOME", &data.path)
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    assert_eq!(
        fs::read_to_string(new_hangar.join("migration-marker")).unwrap(),
        "legacy bytes"
    );
    assert_eq!(
        fs::read_to_string(old_hangar.join("migration-marker")).unwrap(),
        "legacy bytes"
    );
    assert!(!data.join("jet/.hangar-migration.partial").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn hangar_migration_rejects_path_escape_and_leaves_repair_state_visible() {
    use std::os::unix::fs::symlink;

    let legacy = Scratch::new("hangar-migration-escape-legacy");
    let data = Scratch::new("hangar-migration-escape-data");
    let old_hangar = legacy.join("jet/hangar");
    fs::create_dir_all(&old_hangar).unwrap();
    let outside = legacy.join("escape-target");
    fs::write(&outside, "must stay outside").unwrap();
    symlink("../../escape-target", old_hangar.join("escape")).unwrap();

    let output = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_STATE_HOME", &legacy.path)
        .env("XDG_DATA_HOME", &data.path)
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E2604"), "{stderr}");
    assert!(stderr.contains("escapes its migration root"), "{stderr}");
    assert!(!data.join("jet/hangar").exists());
    assert!(data.join("jet/.hangar-migration.partial").is_dir());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "must stay outside");

    let retry = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_STATE_HOME", &legacy.path)
        .env("XDG_DATA_HOME", &data.path)
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert_eq!(retry.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&retry.stderr).contains("incomplete Hangar migration"),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "must stay outside");
}

#[cfg(target_os = "linux")]
#[test]
fn hangar_migration_rejects_missing_path_escape() {
    use std::os::unix::fs::symlink;

    let legacy = Scratch::new("hangar-migration-missing-escape-legacy");
    let data = Scratch::new("hangar-migration-missing-escape-data");
    let old_hangar = legacy.join("jet/hangar");
    fs::create_dir_all(&old_hangar).unwrap();
    symlink("../../missing-target", old_hangar.join("escape")).unwrap();

    let output = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_STATE_HOME", &legacy.path)
        .env("XDG_DATA_HOME", &data.path)
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E2604"), "{stderr}");
    assert!(stderr.contains("escapes its migration root"), "{stderr}");
    assert!(!data.join("jet/hangar").exists());
    assert!(data.join("jet/.hangar-migration.partial").is_dir());
}

#[cfg(unix)]
#[test]
fn hangar_migration_rejects_destination_symlink_without_following_it() {
    use std::os::unix::fs::symlink;

    let data = Scratch::new("hangar-migration-destination-link-data");
    let state = Scratch::new("hangar-migration-destination-link-state");
    let outside = Scratch::new("hangar-migration-destination-link-outside");
    let destination_parent = data.path.join("jet");
    fs::create_dir_all(&destination_parent).unwrap();
    fs::write(outside.join("marker"), "must stay outside").unwrap();
    symlink(&outside.path, destination_parent.join("hangar")).unwrap();

    let output = jet()
        .args(["hangar", "path", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("XDG_DATA_HOME", &data.path)
        .env("XDG_STATE_HOME", &state.path)
        .env_remove("LOCALAPPDATA")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E2604"), "{stderr}");
    assert!(
        stderr.contains("Inspect the reported Hangar path"),
        "{stderr}"
    );
    assert!(stderr.contains("is a symlink"), "{stderr}");
    assert_eq!(
        fs::read_to_string(outside.join("marker")).unwrap(),
        "must stay outside"
    );
    assert_eq!(
        fs::read_link(destination_parent.join("hangar")).unwrap(),
        outside.path
    );
}

#[test]
fn binary_cache_local_publish_verify_and_reject_corruption() {
    let root = Scratch::new("cache-root");
    let source = Scratch::new("cache-source");
    let blocked = Scratch::new("cache-blocked-mirror");
    let mirror = Scratch::new("cache-mirror");
    fs::remove_dir_all(&blocked.path).unwrap();
    fs::write(&blocked.path, "the first mirror is unavailable").unwrap();
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    fs::write(source.join("payload"), "cache bytes\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("payload", source.join("alias")).unwrap();
    let entry = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "cache-demo".into(),
            version: "1".into(),
            reference: "./cache-demo".into(),
            cache_identity: jetpack::Store::CacheIdentity {
                source_fingerprint: "sha256:cache-source".into(),
                recipe_fingerprint: "sha256:cache-recipe".into(),
                policy_fingerprint: "sha256:cache-policy".into(),
                platform: jetpack::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), source.path.clone())]),
            signature: String::new(),
            provenance: "cache test".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    jetpack::Store::bind_cache(
        &roots,
        "public",
        vec![
            blocked.path.display().to_string(),
            mirror.path.display().to_string(),
        ],
        None,
        None,
        true,
    )
    .unwrap();
    let published = jetpack::Store::publish_cache_entry(&roots, &entry.id, "public").unwrap();
    assert_eq!(published.output_hash, entry.envelope.output_hash);
    assert_eq!(published.mirror, mirror.path.display().to_string());
    let verified = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").unwrap();
    assert_eq!(verified.output_hash, entry.envelope.output_hash);
    assert_eq!(verified.builder, published.builder);
    assert_eq!(verified.provenance, published.provenance);
    assert!(published
        .witness
        .as_deref()
        .is_some_and(|witness| !witness.is_empty()));
    assert_eq!(verified.witness, published.witness);
    assert_eq!(published.receipt_version, Some(1));
    assert!(published.receipt_expires_unix.is_some());
    assert_eq!(verified.receipt_version, Some(1));
    assert!(verified.receipt_expires_unix.is_some());
    assert!(!verified.signed_fingerprint.is_empty());
    let report_json = jetpack::Store::cache_report_json("verify", &verified);
    assert!(report_json.contains("\"operation\":\"verify\""));
    assert!(report_json.contains("\"signed_fingerprint\":"));
    assert!(report_json.contains("\"builder\":"));
    assert!(report_json.contains("\"provenance\":"));
    assert!(report_json.contains("\"witness\":"));
    assert!(report_json.contains("\"receipt_version\":1"));
    assert!(report_json.contains("\"receipt_expires_unix\":"));

    let explanation =
        jetpack::Store::explain_package(&roots, &entry.id, jetpack::Store::ExplainLens::Rebuild)
            .unwrap()
            .expect("published cache entry should be explainable");
    let explanation_json = explanation.to_json();
    assert!(explanation_json.contains("\"cache_admissions\":"));
    assert!(explanation_json.contains("\"decision\":\"accepted\""));
    assert!(explanation_json.contains("\"receipt_version\":1"));
    assert!(explanation_json.contains("\"receipt_expires_unix\":"));
    assert!(explanation_json.contains(&published.builder));
    assert!(explanation.text().contains("cache-trust public accepted"));
    assert!(explanation_json.contains("\"decision\":\"rebuild-required\""));
    assert!(explanation_json.contains("producer record lacks the shared provider-facts carrier"));

    let restored = Scratch::new("cache-restored");
    jetpack::Store::substitute_cache_entry(&roots, &entry.id, "public", &restored.join("out"))
        .unwrap();
    assert_eq!(
        fs::read_to_string(restored.join("out/payload")).unwrap(),
        "cache bytes\n"
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(restored.join("out/alias")).unwrap(),
        Path::new("payload")
    );

    // A corrupt first mirror is advisory failure. Lookup must continue to the
    // next ordered mirror and never install its bytes.
    let nar = mirror
        .join("nar")
        .join(format!("{}.nar", entry.envelope.output_hash));
    let info = mirror.join(&format!(
        "{}-{}.narinfo",
        entry.envelope.output_hash, entry.id
    ));
    let nar_bytes = fs::read(&nar).unwrap();
    let info_bytes = fs::read(&info).unwrap();
    fs::remove_file(&blocked.path).unwrap();
    fs::create_dir_all(blocked.join("nar")).unwrap();
    fs::write(
        blocked
            .join("nar")
            .join(format!("{}.nar", entry.envelope.output_hash)),
        b"corrupted first mirror",
    )
    .unwrap();
    fs::write(
        blocked.join(&format!(
            "{}-{}.narinfo",
            entry.envelope.output_hash, entry.id
        )),
        &info_bytes,
    )
    .unwrap();
    let fallback = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").unwrap();
    assert_eq!(fallback.mirror, mirror.path.display().to_string());
    fs::remove_file(&nar).unwrap();
    fs::remove_file(&info).unwrap();
    fs::write(
        nar.with_extension("partial"),
        &nar_bytes[..nar_bytes.len().max(2) / 2],
    )
    .unwrap();
    fs::write(
        info.with_extension("partial"),
        &info_bytes[..info_bytes.len().max(2) / 2],
    )
    .unwrap();
    jetpack::Store::publish_cache_entry(&roots, &entry.id, "public").unwrap();
    assert_eq!(fs::read(&nar).unwrap(), nar_bytes);
    assert_eq!(fs::read(&info).unwrap(), info_bytes);
    assert!(!nar.with_extension("partial").exists());
    assert!(!info.with_extension("partial").exists());

    let cache_key_bytes = fs::read(root.join("trust/cache-public.key")).unwrap();
    let cache_key = jetpack::TrustRoot::TrustKey::from_secret(cache_key_bytes.clone()).unwrap();
    let clear_negative_hint = || {
        let _ = fs::remove_dir_all(mirror.join(".jet-negative"));
    };

    let unsigned = String::from_utf8(info_bytes.clone())
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("Sig: "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&info, unsigned).unwrap();
    assert!(jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").is_err());
    let unsigned_destination = Scratch::new("cache-unsigned-rejected");
    assert!(jetpack::Store::substitute_cache_entry(
        &roots,
        &entry.id,
        "public",
        &unsigned_destination.join("out")
    )
    .is_err());
    assert!(!unsigned_destination.join("out").exists());
    clear_negative_hint();
    fs::write(&info, &info_bytes).unwrap();

    let mut mismatched =
        jetpack::Store::NarInfo::parse(std::str::from_utf8(&info_bytes).unwrap()).unwrap();
    mismatched.deriver = Some(format!("/jet/derivations/{}", "f".repeat(64)));
    fs::write(
        &info,
        mismatched.signed(&cache_key).unwrap().to_text().unwrap(),
    )
    .unwrap();
    assert!(jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").is_err());
    let mismatched_destination = Scratch::new("cache-mismatched-rejected");
    assert!(jetpack::Store::substitute_cache_entry(
        &roots,
        &entry.id,
        "public",
        &mismatched_destination.join("out")
    )
    .is_err());
    assert!(!mismatched_destination.join("out").exists());
    clear_negative_hint();

    let mut replayed =
        jetpack::Store::NarInfo::parse(std::str::from_utf8(&info_bytes).unwrap()).unwrap();
    replayed.store_path = format!("/jet/hangar/{}-replayed", entry.envelope.output_hash);
    fs::write(
        &info,
        replayed.signed(&cache_key).unwrap().to_text().unwrap(),
    )
    .unwrap();
    assert!(jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").is_err());
    clear_negative_hint();
    fs::write(&info, &info_bytes).unwrap();

    let malicious_nar = b"this is not a NAR";
    let mut malicious =
        jetpack::Store::NarInfo::parse(std::str::from_utf8(&info_bytes).unwrap()).unwrap();
    malicious.file_size = malicious_nar.len() as u64;
    malicious.nar_size = malicious_nar.len() as u64;
    malicious.nar_hash = jetpack::Store::nar_digest(malicious_nar);
    fs::write(&nar, malicious_nar).unwrap();
    fs::write(
        &info,
        malicious.signed(&cache_key).unwrap().to_text().unwrap(),
    )
    .unwrap();
    assert!(jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").is_err());
    let malicious_destination = Scratch::new("cache-malicious-rejected");
    assert!(jetpack::Store::substitute_cache_entry(
        &roots,
        &entry.id,
        "public",
        &malicious_destination.join("out")
    )
    .is_err());
    assert!(!malicious_destination.join("out").exists());
    clear_negative_hint();
    fs::write(&nar, &nar_bytes).unwrap();
    fs::write(&info, &info_bytes).unwrap();

    fs::write(&nar, b"corrupted cache bytes").unwrap();
    assert!(jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").is_err());
    let rejected = Scratch::new("cache-rejected");
    assert!(jetpack::Store::substitute_cache_entry(
        &roots,
        &entry.id,
        "public",
        &rejected.join("out")
    )
    .is_err());
    assert!(!rejected.join("out").exists());

    // A changed cache signer is a compromise signal. The host pin rejects it
    // before any mirror bytes can become usable.
    fs::write(root.join("trust/cache-public.key"), vec![0x7f; 32]).unwrap();
    let changed_key = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(
        changed_key.contains("cache key for `public` changed"),
        "{changed_key}"
    );
    fs::write(root.join("trust/cache-public.key"), cache_key_bytes).unwrap();

    // Revocation also blocks an already-published object and tells the owner
    // to rebuild; relocation or relabeling is not a recovery path.
    jetpack::TrustRoot::revoke_cache_builder(&root.path, &published.builder).unwrap();
    let revoked = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(revoked.contains("revoked"), "{revoked}");
    assert!(revoked.contains("rebuild"), "{revoked}");
    let revoked_destination = Scratch::new("cache-revoked-rejected");
    assert!(jetpack::Store::substitute_cache_entry(
        &roots,
        &entry.id,
        "public",
        &revoked_destination.join("out")
    )
    .is_err());
    assert!(!revoked_destination.join("out").exists());
    let revoked_explanation =
        jetpack::Store::explain_package(&roots, &entry.id, jetpack::Store::ExplainLens::Rebuild)
            .unwrap()
            .unwrap()
            .to_json();
    assert!(revoked_explanation.contains("\"decision\":\"denied\""));
    assert!(revoked_explanation.contains("cache builder is revoked"));
}

#[test]
fn cache_cli_json_uses_shared_report_schema() {
    let root = Scratch::new("cache-cli-json-root");
    let mirror = Scratch::new("cache-cli-json-mirror");
    let bind = jetpack()
        .args([
            "hangar",
            "cache",
            "bind",
            "public",
            mirror.path.to_str().unwrap(),
            "--yes",
            "--json",
            "--no-color",
        ])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(bind.status.success(), "stderr: {:?}", bind.stderr);
    assert!(
        bind.stderr.is_empty(),
        "JSON bind leaked stderr: {:?}",
        bind.stderr
    );
    let bind_report = jetpack::JSON::parse(String::from_utf8_lossy(&bind.stdout).trim())
        .expect("cache bind JSON report");
    assert_eq!(json_string(&bind_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&bind_report, "moment"), "tool");
    assert_eq!(json_string(&bind_report, "action"), "cache-bind");
    assert_eq!(json_string(&bind_report, "role"), "public");

    let list = jetpack()
        .args(["hangar", "cache", "list", "--json", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(list.status.success(), "stderr: {:?}", list.stderr);
    assert!(
        list.stderr.is_empty(),
        "JSON list leaked stderr: {:?}",
        list.stderr
    );
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    let list_report = jetpack::JSON::parse(list_stdout.trim()).expect("cache list JSON report");
    assert_eq!(json_string(&list_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&list_report, "action"), "cache-list");
    assert!(list_stdout.contains("\"bindings\":["));

    let repeated = jetpack()
        .args(["hangar", "cache", "list", "--json", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(repeated.stdout, list.stdout);

    let missing = jetpack()
        .args([
            "hangar",
            "cache",
            "verify",
            "missing",
            "--json",
            "--no-color",
        ])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        missing.stderr.is_empty(),
        "JSON failure leaked stderr: {:?}",
        missing.stderr
    );
    let missing_report = jetpack::JSON::parse(String::from_utf8_lossy(&missing.stdout).trim())
        .expect("cache failure JSON report");
    assert_eq!(json_string(&missing_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&missing_report, "code"), "E1340");

    let remove = jetpack()
        .args([
            "hangar",
            "cache",
            "remove",
            "public",
            "--yes",
            "--json",
            "--no-color",
        ])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(remove.status.success(), "stderr: {:?}", remove.stderr);
    let remove_report = jetpack::JSON::parse(String::from_utf8_lossy(&remove.stdout).trim())
        .expect("cache remove JSON report");
    assert_eq!(json_string(&remove_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&remove_report, "action"), "cache-remove");
}

#[test]
fn retired_store_verbs_teach_hangar_routes() {
    let root = Scratch::new("retired-store-verbs");
    for (retired, route) in [
        ("cache", "hangar cache"),
        ("shared-store", "hangar shared"),
        ("vendor", "hangar vendor"),
        ("clean", "hangar clean"),
        ("list", "hangar list"),
    ] {
        let output = jetpack()
            .args([retired, "--no-color"])
            .env("JETPACK_ROOT", &root.path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{retired}: {stderr}");
        assert!(stderr.contains("Error [E1354]"), "{retired}: {stderr}");
        assert!(
            stderr.contains(&format!("jetpack {route}")),
            "{retired}: {stderr}"
        );
    }
}

#[test]
fn hangar_shared_status_stays_read_only() {
    let root = Scratch::new("hangar-shared-status");
    let output = jetpack()
        .args(["hangar", "shared", "status", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(String::from_utf8_lossy(&output.stderr).contains("broker is not installed"));
}

#[test]
fn binary_cache_trust_receipt_rejects_rollback_freeze_and_mix_and_match() {
    let root = Scratch::new("cache-trust-root");
    let source = Scratch::new("cache-trust-source");
    let dependency_source = Scratch::new("cache-trust-dependency-source");
    let mirror = Scratch::new("cache-trust-mirror");
    fs::write(source.join("payload"), "trusted bytes\n").unwrap();
    fs::write(dependency_source.join("payload"), "dependency bytes\n").unwrap();
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let dependency = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "cache-trust-dependency".into(),
            version: "1".into(),
            reference: "./cache-trust-dependency".into(),
            cache_identity: jetpack::Store::CacheIdentity {
                source_fingerprint: "sha256:cache-trust-dependency-source".into(),
                recipe_fingerprint: "sha256:cache-trust-dependency-recipe".into(),
                policy_fingerprint: "sha256:cache-trust-dependency-policy".into(),
                platform: jetpack::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([(
                "out".into(),
                dependency_source.path.clone(),
            )]),
            signature: String::new(),
            provenance: "cache trust dependency".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    let identity = jetpack::Store::CacheIdentity {
        source_fingerprint: "sha256:cache-trust-source".into(),
        recipe_fingerprint: "sha256:cache-trust-recipe".into(),
        policy_fingerprint: "sha256:cache-trust-policy".into(),
        platform: jetpack::Envelope::host_platform(),
    };
    let entry = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "cache-trust-demo".into(),
            version: "1".into(),
            reference: "./cache-trust-demo".into(),
            cache_identity: identity.clone(),
            references: vec![dependency.envelope.output_hash.clone()],
            outputs: std::collections::BTreeMap::from([("out".into(), source.path.clone())]),
            signature: String::new(),
            provenance: "cache trust test".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    let no_closure_root = Scratch::new("cache-trust-no-closure-root");
    let no_closure_roots = jetpack::Store::Roots {
        root: no_closure_root.path.clone(),
        dev_mode: false,
    };
    let no_closure = jetpack::Store::ingest_tree(
        &no_closure_roots,
        &jetpack::Store::IngestRequest {
            name: "cache-trust-demo".into(),
            version: "1".into(),
            reference: "./cache-trust-demo".into(),
            cache_identity: identity,
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), source.path.clone())]),
            signature: String::new(),
            provenance: "cache trust test".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    let with_closure_action = jetpack::Store::ProducerRecord::decode(&entry.producer_record)
        .unwrap()
        .facts
        .get("cache.action")
        .cloned()
        .unwrap();
    let without_closure_action =
        jetpack::Store::ProducerRecord::decode(&no_closure.producer_record)
            .unwrap()
            .facts
            .get("cache.action")
            .cloned()
            .unwrap();
    assert_ne!(
        with_closure_action, without_closure_action,
        "the production Store action identity must bind the realized closure"
    );
    jetpack::Store::bind_cache(
        &roots,
        "public",
        vec![mirror.path.display().to_string()],
        None,
        None,
        true,
    )
    .unwrap();
    jetpack::Store::publish_cache_entry(&roots, &entry.id, "public").unwrap();
    jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").unwrap();

    let nar = mirror
        .join("nar")
        .join(format!("{}.nar", entry.envelope.output_hash));
    let info = mirror.join(&format!(
        "{}-{}.narinfo",
        entry.envelope.output_hash, entry.id
    ));
    let nar_bytes = fs::read(&nar).unwrap();
    let info_bytes = fs::read(&info).unwrap();

    let key = jetpack::TrustRoot::TrustKey::from_secret(
        fs::read(root.join("trust/cache-public.key")).unwrap(),
    )
    .unwrap();
    let store_name = format!("{}-{}", entry.envelope.output_hash, entry.id);
    let receipt_path = mirror
        .path
        .join("trust")
        .join(format!("{store_name}.receipt"));
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let field = |name: &str| {
        String::from_utf8(receipt_bytes.clone())
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
            .unwrap()
            .to_string()
    };
    let provenance = jetpack::TrustRoot::CacheProvenance {
        reference: field("reference"),
        source: field("source"),
        builder: field("builder"),
        action: field("action"),
        output: field("output"),
        platform: field("platform"),
        sandbox: field("sandbox"),
        policy: field("policy"),
    };
    let receipt_text = |receipt: &jetpack::TrustRoot::CacheReceipt| {
        format!(
            "jet-cache-receipt-v1\nrole={}\nwitness={}\nversion={}\nissued={}\nexpires={}\nreference={}\nsource={}\nbuilder={}\naction={}\noutput={}\nplatform={}\nsandbox={}\npolicy={}\nsignature_key={}\nsignature_algorithm={}\nsignature={}\n",
            receipt.role,
            receipt.witness,
            receipt.version,
            receipt.issued_unix,
            receipt.expires_unix,
            receipt.provenance.reference,
            receipt.provenance.source,
            receipt.provenance.builder,
            receipt.provenance.action,
            receipt.provenance.output,
            receipt.provenance.platform,
            receipt.provenance.sandbox,
            receipt.provenance.policy,
            receipt.signature.key_id,
            receipt.signature.algorithm,
            receipt.signature.sig_hex,
        )
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut wrong_closure = provenance.clone();
    wrong_closure.action = without_closure_action;
    let wrong_closure = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        wrong_closure,
        2,
        now.saturating_sub(1),
        now.saturating_add(600),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&wrong_closure)).unwrap();
    let closure_error = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(closure_error.contains("mix-and-match"), "{closure_error}");
    fs::write(&receipt_path, &receipt_bytes).unwrap();
    let tampered = String::from_utf8(receipt_bytes.clone())
        .unwrap()
        .replace(&format!("signature={}", field("signature")), "signature=00");
    fs::write(&receipt_path, tampered).unwrap();
    let compromised = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(
        compromised.contains("signature") && compromised.contains("discard"),
        "{compromised}"
    );
    fs::write(&receipt_path, &receipt_bytes).unwrap();

    fs::remove_file(&receipt_path).unwrap();
    let missing = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(missing.contains("no signed trust receipt"), "{missing}");
    fs::write(&receipt_path, &receipt_bytes).unwrap();

    let v2 = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        provenance.clone(),
        2,
        now.saturating_sub(1),
        now.saturating_add(600),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&v2)).unwrap();
    jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").unwrap();

    // A signature from an unknown producer is not enough. The receipt's
    // witness must also be named by this host's role policy.
    let untrusted_witness = if field("witness") == "stranger" {
        "other-stranger"
    } else {
        "stranger"
    };
    let untrusted = jetpack::TrustRoot::CacheReceipt::issue_with_witness(
        "public",
        untrusted_witness,
        provenance.clone(),
        3,
        now.saturating_sub(1),
        now.saturating_add(600),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&untrusted)).unwrap();
    let witness_error = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(witness_error.contains(untrusted_witness), "{witness_error}");
    assert!(
        witness_error.contains("trust policy 'cache-witnesses/public.allow'"),
        "{witness_error}"
    );
    fs::write(&receipt_path, receipt_text(&v2)).unwrap();
    jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").unwrap();

    // A validly signed newer receipt must not be admitted before its payload
    // proves the requested output identity. Otherwise a bad mirror can
    // advance the host pin and freeze the still-valid receipt behind it.
    let v3 = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        provenance.clone(),
        3,
        now.saturating_sub(1),
        now.saturating_add(600),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&v3)).unwrap();
    let wrong_source = Scratch::new("cache-trust-wrong-payload");
    fs::write(wrong_source.join("payload"), "wrong trusted bytes\n").unwrap();
    let (wrong_nar, wrong_stats) = jetpack::Store::write_nar(&wrong_source.path).unwrap();
    let mut wrong_info =
        jetpack::Store::NarInfo::parse(std::str::from_utf8(&info_bytes).unwrap()).unwrap();
    wrong_info.file_size = wrong_stats.bytes;
    wrong_info.nar_size = wrong_stats.bytes;
    wrong_info.nar_hash = wrong_stats.digest;
    fs::write(&nar, wrong_nar).unwrap();
    fs::write(&info, wrong_info.signed(&key).unwrap().to_text().unwrap()).unwrap();
    let wrong_payload = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(wrong_payload.contains("output identity"), "{wrong_payload}");

    // Restore the valid artifact with the already-admitted v2 receipt. This
    // succeeds only if the failed payload did not consume a newer pin.
    fs::write(&nar, &nar_bytes).unwrap();
    fs::write(&info, &info_bytes).unwrap();
    fs::write(&receipt_path, receipt_text(&v2)).unwrap();
    jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public").unwrap();

    let same_version = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        provenance.clone(),
        2,
        now.saturating_sub(2),
        now.saturating_add(601),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&same_version)).unwrap();
    let same_version_error = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(
        same_version_error.contains("same version"),
        "{same_version_error}"
    );

    // A valid old receipt is replayed after version 2 was admitted.
    let v1 = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        provenance.clone(),
        1,
        now.saturating_sub(1),
        now.saturating_add(600),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&v1)).unwrap();
    let rollback = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(rollback.contains("rollback"), "{rollback}");

    // Exact expiry is a hard freeze. The signed receipt is validly formed,
    // but no package bytes become usable after its deadline.
    let frozen = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        provenance.clone(),
        3,
        now.saturating_sub(20),
        now.saturating_sub(1),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&frozen)).unwrap();
    let frozen_error = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(frozen_error.contains("expired"), "{frozen_error}");
    let frozen_destination = Scratch::new("cache-trust-frozen-destination");
    assert!(jetpack::Store::substitute_cache_entry(
        &roots,
        &entry.id,
        "public",
        &frozen_destination.join("out"),
    )
    .is_err());
    assert!(!frozen_destination.join("out").exists());

    // A signed receipt for another output cannot be combined with this
    // narinfo/NAR pair.
    let mut mixed = provenance;
    mixed.output = "sha256:another-output".into();
    let mixed = jetpack::TrustRoot::CacheReceipt::issue(
        "public",
        mixed,
        4,
        now.saturating_sub(1),
        now.saturating_add(600),
        &key,
    )
    .unwrap();
    fs::write(&receipt_path, receipt_text(&mixed)).unwrap();
    let mix_error = jetpack::Store::verify_cache_transfer(&roots, &entry.id, "public")
        .unwrap_err()
        .to_string();
    assert!(mix_error.contains("mix-and-match"), "{mix_error}");
}

#[test]
fn reproducibility_registration_writes_first_difference_and_blocks_cache() {
    let root = Scratch::new("reproducibility-root");
    let first_source = Scratch::new("reproducibility-first");
    let second_source = Scratch::new("reproducibility-second");
    let mirror = Scratch::new("reproducibility-mirror");
    fs::write(first_source.join("payload"), "first bytes\n").unwrap();
    fs::write(second_source.join("payload"), "second bytes\n").unwrap();
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let identity = jetpack::Store::CacheIdentity {
        source_fingerprint: "sha256:repro-source".into(),
        recipe_fingerprint: "sha256:repro-recipe".into(),
        policy_fingerprint: "sha256:repro-policy".into(),
        platform: jetpack::Envelope::host_platform(),
    };
    let first = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "reproducible-action".into(),
            version: "1".into(),
            reference: "reproducibility:action".into(),
            cache_identity: identity.clone(),
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), first_source.path.clone())]),
            signature: String::new(),
            provenance: "builder:first".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    let error = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "reproducible-action".into(),
            version: "1".into(),
            reference: "reproducibility:action".into(),
            cache_identity: identity,
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), second_source.path.clone())]),
            signature: String::new(),
            provenance: "builder:second".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap_err();
    let error = format!("{error:?}");
    assert!(error.contains("unreproducible"), "{error}");
    assert!(error.contains("conflicting bytes"), "{error}");

    let reports = root.path.join("private/unreproducible");
    let report_path = fs::read_dir(&reports)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .expect("structured reproducibility report");
    let report = fs::read_to_string(report_path).unwrap();
    assert!(
        jetpack::JSON::parse(&report).is_ok(),
        "report must be valid JSON: {report}"
    );
    assert!(report.contains("\"schema\":\"jet-reproducibility-report-v1\""));
    assert!(report.contains("\"producer_action\""));
    assert!(report.contains("\"first_difference\""));
    assert!(report.contains("\"path\":\"payload\""), "{report}");
    assert!(report.contains(&first.envelope.output_hash));
    assert!(!jetpack::Store::list_checked(&roots)
        .unwrap()
        .iter()
        .any(|entry| { entry.envelope.output_hash != first.envelope.output_hash }));

    jetpack::Store::bind_cache(
        &roots,
        "trusted",
        vec![mirror.path.display().to_string()],
        None,
        None,
        true,
    )
    .unwrap();
    assert!(jetpack::Store::publish_cache_entry(&roots, &first.id, "trusted").is_err());
}

#[test]
fn binary_cache_nar_codec_rejects_noncanonical_and_malicious_input() {
    fn string(out: &mut Vec<u8>, value: &[u8]) {
        out.extend_from_slice(&(value.len() as u64).to_le_bytes());
        out.extend_from_slice(value);
        out.resize(out.len() + (8 - value.len() % 8) % 8, 0);
    }

    let mut nar = Vec::new();
    string(&mut nar, b"nix-archive-1");
    string(&mut nar, b"(");
    string(&mut nar, b"type");
    string(&mut nar, b"regular");
    string(&mut nar, b"contents");
    string(&mut nar, b"payload");
    string(&mut nar, b"executable");
    string(&mut nar, b"");
    string(&mut nar, b")");

    assert!(jetpack::Store::validate_nar(&nar).is_err());
    let destination = Scratch::new("nar-malformed-destination");
    let output = destination.join("out");
    assert!(jetpack::Store::read_nar(&nar, &output).is_err());
    assert!(!output.exists());

    fn regular_entry(out: &mut Vec<u8>, name: &[u8]) {
        string(out, b"entry");
        string(out, b"(");
        string(out, b"name");
        string(out, name);
        string(out, b"node");
        string(out, b"(");
        string(out, b"type");
        string(out, b"regular");
        string(out, b"contents");
        string(out, b"");
        string(out, b")");
        string(out, b")");
    }

    let mut unordered_nar = Vec::new();
    string(&mut unordered_nar, b"nix-archive-1");
    string(&mut unordered_nar, b"(");
    string(&mut unordered_nar, b"type");
    string(&mut unordered_nar, b"directory");
    regular_entry(&mut unordered_nar, b"z");
    regular_entry(&mut unordered_nar, b"a");
    string(&mut unordered_nar, b")");
    assert!(jetpack::Store::validate_nar(&unordered_nar).is_err());

    let mut escape_nar = Vec::new();
    for value in [
        b"nix-archive-1".as_slice(),
        b"(".as_slice(),
        b"type".as_slice(),
        b"directory".as_slice(),
        b"entry".as_slice(),
        b"(".as_slice(),
        b"name".as_slice(),
        b"escape".as_slice(),
        b"node".as_slice(),
        b"(".as_slice(),
        b"type".as_slice(),
        b"symlink".as_slice(),
        b"target".as_slice(),
        b"../outside".as_slice(),
        b")".as_slice(),
        b")".as_slice(),
        b")".as_slice(),
    ] {
        string(&mut escape_nar, value);
    }
    assert!(jetpack::Store::validate_nar(&escape_nar).is_err());
    let escape_output = destination.join("escape-out");
    assert!(jetpack::Store::read_nar(&escape_nar, &escape_output).is_err());
    assert!(!escape_output.exists());

    #[cfg(unix)]
    {
        let source = Scratch::new("nar-escape-source");
        fs::write(source.join("outside"), "outside").unwrap();
        std::os::unix::fs::symlink("../outside", source.join("escape")).unwrap();
        assert!(jetpack::Store::write_nar(&source.path).is_err());
    }
}

#[test]
fn binary_cache_narinfo_reads_standard_uncompressed_fields() {
    let hash = jetpack::Store::nar_digest(b"payload");
    let reference = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-tool";
    let text = format!(
        "StorePath: /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-tool\n\
URL: nar/tool.nar\n\
Compression: none\n\
FileHash: {hash}\n\
FileSize: 7\n\
NarHash: {hash}\n\
NarSize: 7\n\
References: {reference}\n\
Deriver: unknown-deriver\n"
    );
    let info = jetpack::Store::NarInfo::parse(&text).unwrap();
    let canonical = info.to_text().unwrap();
    assert!(canonical.contains("FileHash: "));
    assert!(canonical.contains(&format!("References: {reference}\n")));
    assert!(canonical.contains("Deriver: unknown-deriver\n"));
    assert_eq!(jetpack::Store::NarInfo::parse(&canonical).unwrap(), info);
}

#[test]
fn package_generation_plan_uses_the_production_source_backed_resolver() {
    let project = Scratch::new("profile-plan");
    fs::write(
        project.join("env.jet"),
        r#"
module profile.base {
    packages: [default.ripgrep]
}
module profile.dev {
    extends: ["base"]
    packages: [default.fd]
    collisions: { "bin/editor": "fd@default" }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "plan", "dev", "--json", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"raw\":\"ripgrep@default\""), "{stdout}");
    assert!(stdout.contains("\"provider\":\"nix\""), "{stdout}");
    assert!(stdout.contains("\"fingerprint\":\"sha256-"), "{stdout}");
    assert!(stdout.contains("\"provider_facts\":{"), "{stdout}");
    assert!(stdout.contains("\"profile\":\"dev\""), "{stdout}");
    assert!(stdout.contains("\"bin/editor\":\"fd@default\""), "{stdout}");
}

#[test]
fn package_generation_lifecycle_preserves_source_facts_and_history() {
    let project = Scratch::new("profile-generation-lifecycle");
    let root = Scratch::new("profile-generation-root");
    let fixtures = Scratch::new("profile-generation-fixtures");
    let staging = Scratch::new("profile-generation-staging");
    write_runnable_fixture(&fixtures.path, &root.path, &staging.path);
    fs::write(
        project.join("env.jet"),
        "module profile.dev { packages: [default.greet] }\nmodule env.full { packages: [] }\n",
    )
    .unwrap();

    let run = |args: &[&str]| {
        jetpack()
            .args(args)
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", &fixtures.path)
            .output()
            .unwrap()
    };
    let built = run(["profile", "build", "dev", "--no-color", "--offline"].as_slice());
    assert!(
        built.status.success(),
        "build stderr: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let generation_dir = project.join(".jet/profiles/dev/generations/1");
    let metadata = fs::read_to_string(generation_dir.join("meta.json")).unwrap();
    assert!(metadata.contains("jet-package-generation-v1"));
    assert!(metadata.contains("\"raw\":\"greet@default\""));
    assert!(metadata.contains("\"target\":\""));
    assert!(metadata.contains("\"provider\":\"nix\""));
    assert!(metadata.contains("\"provider_facts\":{"));
    assert!(metadata.contains("\"root_hash\":\"sha256-"));
    assert!(metadata.contains("\"output_hash\":\"sha256-"));
    assert!(!project.join(".jet/profiles/dev/current").exists());

    let explained = jet()
        .args(["explain", "greet", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        explained.status.success(),
        "explain stderr: {}",
        String::from_utf8_lossy(&explained.stderr)
    );
    let explained_stdout = String::from_utf8_lossy(&explained.stdout);
    assert!(explained_stdout.contains("\"schema\":\"jet.report/v1\""));
    assert!(explained_stdout.contains("\"moment\":\"tool\""));
    assert!(explained_stdout.contains("\"profile_facts\":["));
    assert!(explained_stdout.contains("\"profile\":\"dev\""));
    assert!(explained_stdout.contains("\"generation\":1"));
    assert!(explained_stdout.contains("\"output_hash\":\"sha256-"));
    assert!(explained_stdout.contains("\"output_digest\":\"matches\""));
    for lens in [
        "why-depends",
        "what-depends",
        "closure",
        "why-live",
        "rebuild",
    ] {
        let explained = jet()
            .args(["explain", lens, "greet", "--json", "--no-color"])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", &fixtures.path)
            .output()
            .unwrap();
        assert!(
            explained.status.success(),
            "{lens} stderr: {}",
            String::from_utf8_lossy(&explained.stderr)
        );
        let stdout = String::from_utf8_lossy(&explained.stdout);
        assert!(stdout.contains(&format!("\"lens\":\"{lens}\"")), "{stdout}");
    }

    let hook_before_switch = jetpack::EnvHook::definition_fingerprint(&project.path, None);
    let switched = run(["profile", "switch", "dev", "--no-color", "--offline"].as_slice());
    assert!(
        switched.status.success(),
        "switch stderr: {}",
        String::from_utf8_lossy(&switched.stderr)
    );
    assert!(
        fs::read_to_string(project.join(".jet/profiles/dev/current"))
            .unwrap()
            .contains("\"generation\":1")
    );
    let hook_after_switch = jetpack::EnvHook::definition_fingerprint(&project.path, None);
    assert_ne!(hook_before_switch, hook_after_switch);

    let entered = run([
        "enter",
        "--no-color",
        "--trust",
        "--offline",
        "--env",
        "full",
        "--",
        "/bin/sh",
        "-c",
        "command -v greet",
    ]
    .as_slice());
    assert!(
        entered.status.success(),
        "dev-shell projection stderr: {}",
        String::from_utf8_lossy(&entered.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&entered.stdout).trim(),
        project
            .join(".jet/profiles/dev/generations/1/root/bin/greet")
            .to_string_lossy()
    );

    let rebuilt = run(["profile", "build", "dev", "--no-color", "--offline"].as_slice());
    assert!(
        rebuilt.status.success(),
        "second build stderr: {}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    let history = run(["profile", "generations", "dev", "--json"].as_slice());
    assert!(history.status.success());
    let history_stdout = String::from_utf8_lossy(&history.stdout);
    assert!(history_stdout.contains("\"generation\":1"));
    assert!(history_stdout.contains("\"generation\":2"));

    let switched_new = run(["profile", "switch", "dev", "--no-color", "--offline"].as_slice());
    assert!(
        switched_new.status.success(),
        "second switch stderr: {}",
        String::from_utf8_lossy(&switched_new.stderr)
    );
    assert!(
        fs::read_to_string(project.join(".jet/profiles/dev/current"))
            .unwrap()
            .contains("\"generation\":2")
    );

    let rolled_back = run(["profile", "rollback", "dev", "--no-color"].as_slice());
    assert!(
        rolled_back.status.success(),
        "rollback stderr: {}",
        String::from_utf8_lossy(&rolled_back.stderr)
    );
    assert!(
        fs::read_to_string(project.join(".jet/profiles/dev/current"))
            .unwrap()
            .contains("\"generation\":1")
    );

    let switched_again = run(["profile", "switch", "dev", "--no-color", "--offline"].as_slice());
    assert!(
        switched_again.status.success(),
        "switch after rollback stderr: {}",
        String::from_utf8_lossy(&switched_again.stderr)
    );
    assert!(
        fs::read_to_string(project.join(".jet/profiles/dev/current"))
            .unwrap()
            .contains("\"generation\":2")
    );

    let generation_one_root = project.join(".jet/profiles/dev/generations/1/root");
    let generation_one_binary = generation_one_root.join("bin/greet");
    let generation_one_original = fs::read(&generation_one_binary).unwrap();
    make_writable(&generation_one_root.to_string_lossy());
    fs::write(&generation_one_binary, "tampered generation\n").unwrap();
    let failed_rollback = run(["profile", "rollback", "dev", "1", "--no-color"].as_slice());
    assert_eq!(failed_rollback.status.code(), Some(2));
    let failed_rollback_stderr = String::from_utf8_lossy(&failed_rollback.stderr);
    assert!(
        failed_rollback_stderr.contains("root hash"),
        "{failed_rollback_stderr}"
    );
    assert!(
        fs::read_to_string(project.join(".jet/profiles/dev/current"))
            .unwrap()
            .contains("\"generation\":2")
    );
    fs::write(&generation_one_binary, generation_one_original).unwrap();

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::list(&roots)
        .into_iter()
        .find(|entry| entry.name == "greet")
        .expect("profile build must publish greet to the Store");
    let store_binary = Path::new(&entry.out).join("bin/greet");
    let store_binary_original = fs::read(&store_binary).unwrap();
    let store_out_permissions = fs::metadata(&entry.out).unwrap().permissions();
    let store_binary_permissions = fs::metadata(&store_binary).unwrap().permissions();
    make_writable(&entry.out);
    fs::write(&store_binary, "tampered\n").unwrap();
    let rebuild = jet()
        .args(["explain", "rebuild", "greet", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        rebuild.status.success(),
        "rebuild explain stderr: {}",
        String::from_utf8_lossy(&rebuild.stderr)
    );
    let rebuild_stdout = String::from_utf8_lossy(&rebuild.stdout);
    assert!(rebuild_stdout.contains("\"decision\":\"rebuild-required\""));
    assert!(rebuild_stdout.contains("\"output_digest\":\"mismatch\""));
    assert!(rebuild_stdout.contains("\"kind\":\"loss\""));
    assert!(rebuild_stdout.contains("stored output digest differs"));

    fs::write(&store_binary, store_binary_original).unwrap();
    fs::set_permissions(&entry.out, store_out_permissions).unwrap();
    fs::set_permissions(&store_binary, store_binary_permissions).unwrap();

    fs::remove_dir_all(root.join("hangar/lifecycle-db")).unwrap();
    let missing_root = run(["profile", "generations", "dev", "--json"].as_slice());
    assert_eq!(missing_root.status.code(), Some(2));
    let missing_root_stderr = String::from_utf8_lossy(&missing_root.stderr);
    assert!(
        missing_root_stderr.contains("profile generation lifecycle root is missing"),
        "{missing_root_stderr}"
    );
}

fn seed_profile_fixture(root: &Path, fixtures: &Path, package: &str, setup: impl FnOnce(&Path)) {
    fs::create_dir_all(fixtures).unwrap();
    let staging = Scratch::new(&format!("profile-generation-{package}-staging"));
    setup(&staging.path);
    jetpack::Store::seal_local_output(&staging.path).unwrap();
    let digest = jetpack::Envelope::try_output_hash_of(&staging.path.to_string_lossy()).unwrap();
    let out_dir = root.join("hangar/objects").join(&digest);
    fs::create_dir_all(out_dir.parent().unwrap()).unwrap();
    let mut permissions = fs::metadata(&staging.path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(&staging.path, permissions).unwrap();
    fs::rename(&staging.path, &out_dir).unwrap();
    jetpack::Store::seal_local_output(&out_dir).unwrap();
    fs::write(
        fixtures.join(&format!("nixpkgs-{package}.json")),
        format!(
            "[{{\"drvPath\":\"/nix/store/0fixture00000000000000000000-{package}.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
            out_dir.to_string_lossy()
        ),
    )
    .unwrap();
}

#[test]
fn package_generation_build_applies_exact_path_collision_selection() {
    let project = Scratch::new("profile-generation-selected-collision-project");
    let root = Scratch::new("profile-generation-selected-collision-root");
    let fixtures = Scratch::new("profile-generation-selected-collision-fixtures");

    for (package, contents) in [("left", "left editor\n"), ("right", "right editor\n")] {
        seed_profile_fixture(&root.path, &fixtures.path, package, |staging| {
            let bin = staging.join("bin");
            fs::create_dir_all(&bin).unwrap();
            let editor = bin.join("editor");
            fs::write(&editor, contents).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&editor, fs::Permissions::from_mode(0o755)).unwrap();
            }
        });
    }
    fs::write(
        project.join("env.jet"),
        "module profile.dev { packages: [default.left, default.right] collisions: {\"bin/editor\": \"right@default\"} }\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "build", "dev", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generation = project.join(".jet/profiles/dev/generations/1");
    assert_eq!(
        fs::read_to_string(generation.join("root/bin/editor")).unwrap(),
        "right editor\n"
    );
    let metadata = fs::read_to_string(generation.join("meta.json")).unwrap();
    assert!(metadata.contains("\"selected\":\"right@default\""));
    assert!(metadata.contains("left@default"));
    assert!(metadata.contains("right@default"));
    assert!(metadata.matches("sha256-").count() >= 3);
}

#[test]
fn package_generation_build_rejects_unresolved_exact_path_collision() {
    let project = Scratch::new("profile-generation-collision-project");
    let root = Scratch::new("profile-generation-collision-root");
    let fixtures = Scratch::new("profile-generation-collision-fixtures");

    for (package, contents) in [("left", "left editor\n"), ("right", "right editor\n")] {
        seed_profile_fixture(&root.path, &fixtures.path, package, |staging| {
            let bin = staging.join("bin");
            fs::create_dir_all(&bin).unwrap();
            let editor = bin.join("editor");
            fs::write(&editor, contents).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&editor, fs::Permissions::from_mode(0o755)).unwrap();
            }
        });
    }

    fs::write(
        project.join("env.jet"),
        "module profile.dev { packages: [default.left, default.right] }\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "build", "dev", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "{stderr}");
    assert!(stderr.contains("unresolved package collision"), "{stderr}");
    assert!(stderr.contains("left@default"), "{stderr}");
    assert!(stderr.contains("right@default"), "{stderr}");
    assert!(stderr.contains("sha256-"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn package_generation_build_rejects_symlink_target_collision_even_when_selected() {
    use std::os::unix::fs::symlink;

    let project = Scratch::new("profile-generation-symlink-collision-project");
    let root = Scratch::new("profile-generation-symlink-collision-root");
    let fixtures = Scratch::new("profile-generation-symlink-collision-fixtures");

    for (package, target) in [("left", "left-target"), ("right", "right-target")] {
        seed_profile_fixture(&root.path, &fixtures.path, package, |staging| {
            let bin = staging.join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join(target), format!("{target}\n")).unwrap();
            symlink(target, bin.join("editor")).unwrap();
        });
    }
    fs::write(
        project.join("env.jet"),
        "module profile.dev { packages: [default.left, default.right] collisions: {\"bin/editor\": \"left@default\"} }\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "build", "dev", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "{stderr}");
    assert!(stderr.contains("symlink-target mismatch"), "{stderr}");
    assert!(stderr.contains("left@default"), "{stderr}");
    assert!(stderr.contains("right@default"), "{stderr}");
}

#[test]
fn package_generation_plan_reports_inheritance_cycles() {
    let project = Scratch::new("profile-plan-cycle");
    fs::write(
        project.join("env.jet"),
        "module profile.a { extends: [\"b\"] }\nmodule profile.b { extends: [\"a\"] }\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "plan", "a", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1332"), "{stderr}");
    assert!(stderr.contains("inheritance cycle"), "{stderr}");
}

#[test]
fn package_generation_plan_rejects_ambiguous_provider_facts() {
    let project = Scratch::new("profile-plan-ambiguous-provider");
    fs::write(
        project.join("env.jet"),
        r#"
module sources {
    sources: { remote: acme/tools@github }
}
module profile.dev {
    packages: [remote.hello]
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "plan", "dev", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "{stderr}");
    assert!(stderr.contains("ambiguous provider fact"), "{stderr}");
    assert!(stderr.contains("unresolved inference"), "{stderr}");
}

#[test]
fn package_generation_plan_rejects_lossy_external_provider_facts() {
    let project = Scratch::new("profile-plan-lossy-provider");
    fs::write(
        project.join("env.jet"),
        "module profile.dev { packages: [cran.tool] }\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["profile", "plan", "dev", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "{stderr}");
    assert!(
        stderr.contains("lossy or conflicting provider fact"),
        "{stderr}"
    );
    assert!(
        stderr.contains("exact version, revision, or digest"),
        "{stderr}"
    );
}

#[test]
fn package_policy_authorizes_spdx_and_source_mapping_through_publish_seam() {
    let raw = r#"
name: "consumer"
version: "0.1.0"
license: "MIT"
policy: {
    licenses: .Allow(["MIT", "Apache-2.0"]),
    sources: { "Acme.*": ["internal"] },
}
"#;
    let manifest = jet::Manifest::parse(Path::new("package.jet"), raw).expect("policy manifest");
    assert_eq!(
        manifest.policy.licenses,
        Some(vec!["MIT".to_string(), "Apache-2.0".to_string()])
    );
    assert_eq!(
        manifest.policy.source_maps,
        vec![("Acme.*".to_string(), vec!["internal".to_string()])]
    );

    let receipt = jet::Publish::authorize_package_candidate(
        &manifest.policy,
        "Acme.Widget",
        "1.2.3",
        Some("MIT OR Apache-2.0"),
        "internal",
    )
    .expect("mapped SPDX candidate should be allowed");
    assert!(receipt.summary().contains("source-rule=Acme.*"));
    assert!(receipt.summary().contains("fingerprint=sha256-"));
}

#[test]
fn package_policy_parses_exact_expiring_source_exception() {
    let raw = r#"
name: "consumer"
version: "0.1.0"
policy: {
    exceptions: [PolicyException.{
        id: "JSA-2026-0001",
        scope: "Acme.Widget#1.2.3",
        reason: "urgent security fix",
        expires: 9999999999,
    }],
}
"#;
    let manifest = jet::Manifest::parse(Path::new("package.jet"), raw)
        .expect("source policy exception should parse");
    assert_eq!(manifest.policy.exceptions.len(), 1);
    let exception = &manifest.policy.exceptions[0];
    assert_eq!(exception.id, "JSA-2026-0001");
    assert_eq!(exception.scope, "Acme.Widget#1.2.3");
    assert!(exception.matches("Acme.Widget", "1.2.3"));
    assert!(exception.summary().contains("reason=urgent security fix"));

    let invalid = jet::Manifest::parse(
        Path::new("package.jet"),
        "name: \"consumer\"\nversion: \"0.1.0\"\npolicy: { exceptions: [PolicyException.{ id: \"JSA-1\", scope: \"Acme.Widget#^1.2\", reason: \"why\", expires: 10 }] }\n",
    )
    .expect_err("exception scope ranges must fail closed");
    assert_eq!(invalid.code, "E1206");
    assert!(invalid.why.contains("PolicyException.scope"));
}

#[test]
fn package_policy_drops_expired_source_exception_from_receipt() {
    let policy = jet::Package::PackagePolicy {
        exceptions: vec![jet::Package::PackagePolicyException {
            id: "JSA-EXPIRED".to_string(),
            scope: "Acme.Widget#1.2.3".to_string(),
            reason: "old waiver".to_string(),
            expires_at: 0,
        }],
        ..Default::default()
    };
    let receipt = jet::Publish::authorize_package_candidate(
        &policy,
        "Acme.Widget",
        "1.2.3",
        Some("MIT"),
        "public",
    )
    .expect("expired exceptions do not invalidate an otherwise valid candidate");
    assert!(receipt.exception.is_none());
    assert!(!receipt.summary().contains("exception="));
}

#[test]
fn package_policy_rejects_unmapped_or_non_spdx_candidates_before_ingest() {
    let policy = jet::Package::PackagePolicy {
        licenses: Some(vec!["MIT".to_string()]),
        source_maps: vec![("Acme.*".to_string(), vec!["internal".to_string()])],
        ..Default::default()
    };
    let source_error = jet::Publish::authorize_package_candidate(
        &policy,
        "Acme.Widget",
        "1.2.3",
        Some("MIT"),
        "public",
    )
    .expect_err("dependency-confusion source must be denied");
    assert!(source_error.detail.contains("not allowed"));

    let license_error =
        jet::Publish::validate_published_license("Acme.Widget", "1.2.3", Some("not a license"))
            .expect_err("malformed SPDX must be denied");
    assert!(license_error.detail.contains("invalid SPDX"));

    let diagnostic = jet::Publish::package_policy_diagnostic(&license_error);
    assert_eq!(diagnostic.code, "E2607");
    assert!(diagnostic.what.contains("invalid SPDX"));
    assert!(diagnostic.why.contains("security-sensitive"));
    assert!(diagnostic.fix.contains("valid SPDX expression"));

    let malformed = jet::Manifest::parse(
        Path::new("package.jet"),
        "name: \"consumer\"\nversion: \"0.1.0\"\npolicy: { licenses: .Allow([\"MIT\", \"MIT\"]) }\n",
    )
    .expect_err("duplicate license policy entries must be rejected");
    assert_eq!(malformed.code, "E1206");
    assert!(malformed.what.contains("package.jet"));
    assert!(malformed
        .why
        .contains("package metadata policy is malformed"));
    assert!(malformed.fix.contains("syntax-decisions.md"));
}

#[test]
fn real_cargo_project_import_preserves_lock_facts_and_migration_findings() {
    let project = Scratch::new("provider-import-real-cargo");
    fs::write(
        project.join("Cargo.toml"),
        r#"[package]
name = "real-app"
version = "0.4.0"
license = "MIT"
build = "build.rs"

[dependencies]
serde = "1"

[target.'cfg(unix)'.dependencies]
cc = "1"

[dev-dependencies]
insta = "1"
"#,
    )
    .unwrap();
    fs::write(
        project.join("Cargo.lock"),
        r#"version = 4

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://example.invalid"
checksum = "serde-checksum"

[[package]]
name = "cc"
version = "1.0.99"
source = "registry+https://example.invalid"
checksum = "cc-checksum"

[[package]]
name = "insta"
version = "1.39.0"
source = "registry+https://example.invalid"
checksum = "insta-checksum"
"#,
    )
    .unwrap();

    let plan = jetpack::MigrationImport::import_cargo(
        &fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        &fs::read_to_string(project.join("Cargo.lock")).unwrap(),
    );
    assert!(plan
        .todos
        .iter()
        .any(|todo| { todo.source_path == "Cargo.toml" && todo.message.contains("build script") }));
    for (name, version, provider_ref) in [
        ("serde", "1.0.200", "serde@cargo#version=1.0.200"),
        ("cc", "1.0.99", "cc@cargo#version=1.0.99&platform=cfg(unix)"),
    ] {
        assert!(
            plan.emit_pkg_jet()
                .contains(&format!("{name}: {provider_ref}")),
            "generated package omitted {provider_ref}: {}",
            plan.emit_pkg_jet()
        );
        let facts = plan
            .provider_facts
            .get(provider_ref)
            .expect("real project provider facts");
        facts.validate().expect("lossless Cargo dependency facts");
        assert!(facts.native_document.contains("Cargo.toml:"));
        assert!(facts.native_document.contains("Cargo.lock:"));
        assert_eq!(facts.resolved_source, format!("cargo:{name}@{version}"));
        assert_eq!(
            facts.provenance.get("package.version").map(String::as_str),
            Some("Cargo.lock.version")
        );
        assert!(facts
            .explain_lines()
            .iter()
            .any(|line| line == "native Cargo.toml+Cargo.lock: retained"));
        let round_trip = jetpack::ProviderFacts::from_json(&facts.to_json())
            .expect("Cargo provider facts export");
        assert_eq!(round_trip, facts.clone());
        if name == "cc" {
            assert_eq!(facts.selector.platform, "cfg(unix)");
        }
    }
    let insta = plan
        .provider_facts
        .get("insta@cargo#version=1.39.0")
        .expect("real project dev-dependency facts");
    insta
        .validate()
        .expect("lossless Cargo dev-dependency facts");
    assert!(!plan.emit_pkg_jet().contains("insta:"));
    assert!(plan
        .todos
        .iter()
        .any(|todo| { todo.source_path == "Cargo.toml" && todo.message.contains("dev") }));
    assert!(plan.deps.iter().any(|dep| dep.name == "cc"));
    assert!(plan.deps.iter().any(|dep| dep.name == "insta" && dep.dev));
}

#[test]
fn cargo_import_keeps_unlocked_target_identity_without_duplicate_selector_facts() {
    let plan = jetpack::MigrationImport::import_cargo(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[target.'cfg(unix)'.dependencies]\ncc = \"1\"\n",
        "",
    );
    assert_eq!(plan.deps[0].provider_ref, "cc@cargo#platform=cfg(unix)");
    let facts = plan
        .provider_facts
        .get("cc@cargo#platform=cfg(unix)")
        .expect("unlocked target provider facts");
    assert!(facts.losses.iter().all(|loss| {
        !loss
            .reason
            .contains("duplicate selector fact `platform=cfg(unix)`")
    }));
    assert!(plan
        .todos
        .iter()
        .any(|todo| { todo.source_path == "Cargo.lock" && todo.message.contains("unresolved") }));
    assert!(!plan.emit_pkg_jet().contains("cc:"));
}

#[test]
fn jet_registry_dependency_roles_features_and_constraints_round_trip() {
    use jetpack::ProviderFacts;
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let native = r#"{"name":"rolekit","version":"1.0.0","content_hash":"sha256-rolekit","dependencies":{"runtime":"^1.0"},"build_dependencies":{"cc":{"require":"^1.0","strict":true}},"dev_dependencies":{"insta":"^1.0"},"optional_dependencies":{"trace":"^1.0"},"peer_dependencies":{"host":"^2.0"},"features":{"default":["trace"]},"constraints":{"runtime":{"prefer":"1.0.1","reject":["1.0.2"],"strict":true}}}"#;
    let report = normalize_provider_document(ProviderFamily::JetRegistry, native);
    report
        .validate()
        .expect("registry dependency metadata must be lossless");
    for key in [
        "provider.registry.dependency-role.build",
        "provider.registry.dependency.build.cc",
        "provider.registry.features",
        "provider.registry.constraints",
    ] {
        assert!(
            report.facts.typed.contains_key(key),
            "registry fact carrier omitted {key}"
        );
    }

    let lock = report
        .lock_record(
            "engine-test",
            "rolekit@jet-registry#version=1.0.0",
            "x86_64-linux",
        )
        .expect("registry provider lock");
    let facts = ProviderFacts::from_json(
        lock.future_fields
            .get("provider-facts")
            .expect("provider facts in semantic lock"),
    )
    .expect("provider facts round trip");
    assert_eq!(facts.native_document, native);
    assert!(facts
        .facts
        .contains_key("provider.registry.dependency-role.build"));
    assert!(facts.facts.contains_key("provider.registry.constraints"));
}

#[test]
fn provider_conformance_nuget_conan_vcpkg_uses_shared_production_carrier() {
    use jetpack::ProviderFacts;
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let documents = [
        (
            ProviderFamily::NuGet,
            "<package><metadata><id>widget</id><version>1.2.3</version><licenseExpression>MIT</licenseExpression><repository type=\"git\" url=\"https://example.invalid/widget\" /></metadata></package>",
            "widget@nuget#version=1.2.3",
        ),
        (
            ProviderFamily::Conan,
            "name = \"widget\"\nversion = \"1.2.3\"\nlicense = \"MIT\"\ndef requirements(self):\n    self.requires(\"zlib/1.3.1\")\n",
            "widget@conan#version=1.2.3",
        ),
        (
            ProviderFamily::Vcpkg,
            r#"{"name":"widget","version-string":"1.2.3","license":"MIT","dependencies":[{"name":"zlib","version>=":"1.3.0","features":["core"]}],"features":{"tools":["fmt"]}}"#,
            "widget@vcpkg#version=1.2.3",
        ),
    ];

    for (family, native, reference) in documents {
        let report = normalize_provider_document(family, native);
        report.validate().expect("lossless provider report");
        let lock = report
            .lock_record("engine-test", reference, "x86_64-linux")
            .expect("provider lock");
        let facts = ProviderFacts::from_json(
            lock.future_fields
                .get("provider-facts")
                .expect("shared provider facts in lock"),
        )
        .expect("shared provider facts JSON");
        assert_eq!(facts.native_document, native);
        assert_eq!(facts.qualified_reference(), reference);
        assert!(report
            .shared_facts()
            .explain_lines()
            .iter()
            .any(|line| line.contains("provider")));
    }

    let malformed = normalize_provider_document(
        ProviderFamily::Vcpkg,
        r#"{"name":"widget","version":"1.0.0","version-string":"1.1.0","dependencies":[{"features":["core"]}]}"#,
    );
    assert!(malformed.validate().is_err());
    assert!(malformed
        .losses
        .iter()
        .any(|loss| loss.contains("no non-empty `name`")));
    assert!(malformed
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("conflicting version fields")));
}

#[test]
fn provider_conformance_homebrew_github_binary_uses_shared_production_carrier() {
    use jetpack::ProviderFacts;
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let digest = "sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let revision = "0123456789abcdef0123456789abcdef01234567";
    let homebrew = r##"{"name":"jq","version":"1.7.1","versions":{"stable":"1.7.1"},"tap":"homebrew/core","dependencies":["oniguruma"],"source":{"url":"https://example.invalid/jq.tar.gz","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"},"bottle":{"stable":{"files":{"x86_64_linux":{"url":"https://example.invalid/jq.bottle.tar.gz","sha256":"abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"}}}},"relocatable":true,"test":"system \"#{bin}/jq\""}"##;
    let github = format!(
        r#"{{"name":"tool","tag_name":"v1.2.3","target_commitish":"{revision}","repository":{{"full_name":"acme/tool"}},"assets":[{{"name":"tool-linux","platform":"x86_64-linux","digest":"{digest}","browser_download_url":"https://example.invalid/tool"}}],"signature":{{"key":"pk","value":"sig"}},"advisories":["CVE-0000-0000"]}}"#
    );
    let binary = format!(
        r#"{{"name":"tool","hash":"{digest}","platforms":["x86_64-linux"],"url":"https://example.invalid/tool","signature":{{"key":"pk","value":"sig"}},"provenance":{{"builder":"ci"}},"sbom":{{"format":"spdx"}},"variants":{{"debug":{{"features":["trace"]}}}}}}"#
    );

    for (family, native, reference) in [
        (
            ProviderFamily::Homebrew,
            homebrew,
            "jq#version=1.7.1@homebrew",
        ),
        (
            ProviderFamily::Github,
            github.as_str(),
            "tool@github#version=v1.2.3",
        ),
        (
            ProviderFamily::Binary,
            binary.as_str(),
            "tool#digest=sha256-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef@binary",
        ),
    ] {
        let report = normalize_provider_document(family, native);
        report
            .validate()
            .unwrap_or_else(|error| panic!("lossless provider report: {error}"));
        let shared = report.shared_facts();
        assert_eq!(shared.native_document, native);
        assert!(shared
            .explain_lines()
            .iter()
            .any(|line| line.contains("native") && line.contains("retained")));

        let exported = ProviderFacts::from_json(&report.export_json())
            .expect("provider export uses the shared carrier");
        assert_eq!(exported, shared);

        let lock = report
            .lock_record("engine-test", reference, "x86_64-linux")
            .expect("provider lock uses the shared carrier");
        let locked = ProviderFacts::from_json(
            lock.future_fields
                .get("provider-facts")
                .expect("provider facts in lock"),
        )
        .expect("locked provider facts JSON");
        assert_eq!(locked, shared);
        assert_eq!(
            lock.future_fields.get("provider-facts-digest"),
            Some(&shared.digest())
        );
    }

    let homebrew_report = normalize_provider_document(ProviderFamily::Homebrew, homebrew);
    assert!(homebrew_report
        .facts
        .typed
        .contains_key("provider.homebrew.bottle.x86_64_linux.sha256"));
    let github_report = normalize_provider_document(ProviderFamily::Github, &github);
    assert!(github_report
        .facts
        .typed
        .contains_key("provider.github.revision"));
    assert!(github_report
        .shared_facts()
        .facts
        .contains_key("provider.github.asset.tool-linux.browser_download_url"));
    let binary_report = normalize_provider_document(ProviderFamily::Binary, &binary);
    assert!(binary_report
        .shared_facts()
        .facts
        .contains_key("provider.binary.provenance"));
}

#[test]
fn provider_conformance_homebrew_github_binary_reports_loss_and_conflict_without_defaults() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let homebrew = normalize_provider_document(
        ProviderFamily::Homebrew,
        r#"{"name":"jq","version":"1.7.1","versions":{"stable":"1.7.2"}}"#,
    );
    assert!(homebrew
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("conflicting version")));
    assert!(homebrew.validate().is_err());
    assert!(homebrew.shared_facts().conflicts.iter().any(|conflict| {
        conflict.right.contains("conflicting version") && conflict.left != conflict.right
    }));

    let github = normalize_provider_document(
        ProviderFamily::Github,
        r#"{"name":"tool","tag_name":"v1.2.3","assets":[{"name":"tool-linux","platform":"x86_64-linux","browser_download_url":"https://example.invalid/tool"}]}"#,
    );
    assert!(github
        .losses
        .iter()
        .any(|loss| loss.contains("no content digest")));
    assert!(github.validate().is_err());
    assert!(github
        .shared_facts()
        .losses
        .iter()
        .any(|loss| loss.reason.contains("no content digest")));

    let binary = normalize_provider_document(
        ProviderFamily::Binary,
        r#"{"name":"tool","hash":"sha256-aa"}"#,
    );
    assert!(binary
        .losses
        .iter()
        .any(|loss| loss.contains("not an exact digest")));
    assert!(binary
        .losses
        .iter()
        .any(|loss| loss.contains("no target platform")));
    assert!(binary.validate().is_err());
    assert!(binary
        .shared_facts()
        .losses
        .iter()
        .any(|loss| loss.reason.contains("exact version, revision, or digest")));
}

#[test]
fn real_project_import_reports_lossy_npm_requests_without_generated_refs() {
    let project = Scratch::new("provider-import-real-npm");
    fs::write(
        project.join("package.json"),
        r#"{
  "name": "web-app",
  "version": "1.2.3",
  "dependencies": {"vite": "^5.4.0"},
  "devDependencies": {"typescript": "~5.5.0"},
  "optionalDependencies": {"fsevents": "2.3.3"},
  "peerDependencies": {"react": "^18.0.0"},
  "bundledDependencies": ["local-tool"],
  "scripts": {"build": "vite build"}
}
"#,
    )
    .unwrap();

    let document = fs::read_to_string(project.join("package.json")).unwrap();
    let plan = jetpack::MigrationImport::import_npm(&document);
    let generated = plan.emit_pkg_jet();
    for name in ["vite", "typescript", "fsevents", "react"] {
        assert!(
            !generated.contains(&format!("{name}:")),
            "lossy ref generated: {generated}"
        );
    }
    for (name, field) in [
        ("vite", "dependencies"),
        ("typescript", "devDependencies"),
        ("react", "peerDependencies"),
    ] {
        let facts = plan
            .provider_facts
            .get(&format!("{name}@npm"))
            .expect("provider facts for unresolved npm dependency");
        assert!(facts.native_document.contains("web-app"));
        assert!(facts
            .losses
            .iter()
            .any(|loss| loss.source == format!("package.json.{field}")));
        assert!(plan.todos.iter().any(|todo| {
            todo.source_path == "package.json" && todo.message.contains("unresolved")
        }));
    }
    let optional = plan
        .provider_facts
        .get("fsevents#version=2.3.3@npm")
        .expect("provider facts for exact optional npm dependency");
    optional
        .validate()
        .expect("lossless npm optional dependency facts");
    assert_eq!(
        optional
            .provenance
            .get("package.dependency_kind")
            .map(String::as_str),
        Some("package.json.optionalDependencies")
    );
    assert!(optional
        .explain_lines()
        .iter()
        .any(|line| line == "native package.json: retained"));
    let optional_round_trip =
        jetpack::ProviderFacts::from_json(&optional.to_json()).expect("npm facts export");
    assert_eq!(optional_round_trip, optional.clone());
    assert!(plan
        .todos
        .iter()
        .any(|todo| { todo.source_path == "package.json" && todo.message.contains("optional") }));
    assert!(plan
        .todos
        .iter()
        .any(|todo| todo.source_path == "package.json" && todo.message.contains("bundled")));
    assert!(plan
        .todos
        .iter()
        .any(|todo| todo.message.contains("legacy build action")));
}

#[test]
fn cargo_import_reports_ambiguous_lock_identity_without_generation() {
    let plan = jetpack::MigrationImport::import_cargo(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        "[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\nsource = \"registry+one\"\n[[package]]\nname = \"serde\"\nversion = \"1.0.201\"\nsource = \"registry+two\"\n",
    );
    assert!(!plan.emit_pkg_jet().contains("serde:"));
    let facts = plan
        .provider_facts
        .get("serde@cargo")
        .expect("ambiguous Cargo provider facts");
    assert!(facts.conflicts.iter().any(|conflict| {
        conflict.key == "provider.selector.version" && conflict.source == "Cargo.lock"
    }));
    assert!(plan
        .todos
        .iter()
        .any(|todo| { todo.source_path == "Cargo.lock" && todo.message.contains("conflicts") }));
}

#[test]
fn doctor_checks_real_state_and_is_read_only() {
    let project = Scratch::new("doctor-project");
    let root = Scratch::new("doctor-root");
    let keys = Scratch::new("doctor-keys");
    let registry = project.join("registry.git");
    let registry_init = Command::new("git")
        .args(["init", "--bare", registry.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        registry_init.status.success(),
        "registry init: {}",
        String::from_utf8_lossy(&registry_init.stderr)
    );
    let keygen = jet()
        .args(["registry", "keygen"])
        .current_dir(&project.path)
        .env("JET_KEYS_DIR", &keys.path)
        .output()
        .unwrap();
    assert!(
        keygen.status.success(),
        "keygen: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );
    let registry_url = format!("file://{}", registry.display());
    let credentialed_registry_url = "http://user:super-secret@example.invalid/index";
    let helper = jetpack::FFI::cached_crypto_helper_path();
    let helper_before = fs::metadata(&helper).unwrap();
    // #2075: that directory is the Cargo target dir SHARED by every bridge key,
    // so "this bridge's files" is a key filter — another suite building another
    // bridge concurrently must not read as `doctor` touching this one.
    let signing_bridge_files = |dir: &std::path::Path, key: &str| {
        let mut names = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains(key))
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let helper_dir = helper.parent().unwrap().to_path_buf();
    let helper_key = common::ffi_bridge_key(&helper);
    let helper_parent_before = signing_bridge_files(&helper_dir, &helper_key);

    let healthy = jetpack()
        .args(["doctor", "--json", "--online"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env("JET_REGISTRY_URL", &registry_url)
        .output()
        .unwrap();
    assert!(
        healthy.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    let healthy_json = jetpack::JSON::parse(&String::from_utf8_lossy(&healthy.stdout)).unwrap();
    assert_eq!(json_string(&healthy_json, "status"), "healthy");
    assert_eq!(
        fs::metadata(&helper).unwrap().len(),
        helper_before.len(),
        "doctor changed signing helper"
    );
    let helper_parent_after = signing_bridge_files(&helper_dir, &helper_key);
    assert_eq!(
        helper_parent_after, helper_parent_before,
        "doctor changed signing helper cache"
    );

    fs::remove_file(keys.join("jet.ed25519")).unwrap();
    let degraded = jetpack()
        .args(["doctor", "--online"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env("JET_REGISTRY_URL", credentialed_registry_url)
        .output()
        .unwrap();
    assert_eq!(degraded.status.code(), Some(2));
    let degraded_text = String::from_utf8(degraded.stderr).unwrap();
    assert!(degraded_text.contains("[fail] registry"), "{degraded_text}");
    assert!(
        degraded_text.contains("embedded registry credentials"),
        "{degraded_text}"
    );
    assert!(degraded_text.contains("[warn] signing"), "{degraded_text}");
    assert!(
        degraded_text.ends_with("result: broken\n"),
        "{degraded_text}"
    );
    assert!(
        !degraded_text.contains("super-secret"),
        "credential leaked: {degraded_text}"
    );
    let query_degraded = jetpack()
        .args(["doctor", "--online"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env(
            "JET_REGISTRY_URL",
            "file:///registry.git?access_token=super-secret",
        )
        .output()
        .unwrap();
    assert_eq!(query_degraded.status.code(), Some(2));
    let query_degraded_text = String::from_utf8(query_degraded.stderr).unwrap();
    assert!(
        query_degraded_text.contains("embedded registry credentials"),
        "{query_degraded_text}"
    );
    assert!(
        !query_degraded_text.contains("super-secret"),
        "credential leaked: {query_degraded_text}"
    );
    let keygen = jet()
        .args(["registry", "keygen", "--force"])
        .current_dir(&project.path)
        .env("JET_KEYS_DIR", &keys.path)
        .output()
        .unwrap();
    assert!(
        keygen.status.success(),
        "keygen: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );
    let public_path = keys.join("jet.ed25519.pub");
    let matching_public = fs::read_to_string(&public_path).unwrap();
    let mut mismatched_public = matching_public.clone().into_bytes();
    mismatched_public[0] = if mismatched_public[0] == b'0' {
        b'1'
    } else {
        b'0'
    };
    fs::write(&public_path, &mismatched_public).unwrap();
    let mismatch = jetpack()
        .args(["doctor", "--online"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env("JET_REGISTRY_URL", credentialed_registry_url)
        .output()
        .unwrap();
    let mismatch_text = String::from_utf8(mismatch.stderr).unwrap();
    assert_eq!(mismatch.status.code(), Some(2), "{mismatch_text}");
    assert!(
        mismatch_text.contains("does not match its public key"),
        "{mismatch_text}"
    );
    assert!(
        !mismatch_text.contains("super-secret"),
        "credential leaked: {mismatch_text}"
    );
    fs::write(&public_path, matching_public).unwrap();

    let output = root.join("owned-output");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("payload"), "trusted bytes").unwrap();
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "demo".into(),
            version: "1".into(),
            reference: "./demo".into(),
            cache_identity: jetpack::Store::CacheIdentity {
                source_fingerprint: "sha256-test-source".into(),
                recipe_fingerprint: "sha256-test-recipe".into(),
                policy_fingerprint: "sha256-test-policy".into(),
                platform: jetpack::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), output.clone())]),
            signature: String::new(),
            provenance: "./demo via test".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    let meta = root.join(&format!("hangar/{}/meta.json", entry.id));
    let old_meta = fs::read_to_string(&meta).unwrap();
    let stale_meta = old_meta.replace(
        &format!("\"last_used_at\": \"{}\"", entry.last_used_at),
        "\"last_used_at\": \"0\"",
    );
    fs::write(&meta, &stale_meta).unwrap();
    make_writable(&entry.out);
    fs::write(
        std::path::Path::new(&entry.out).join("payload"),
        "corrupt bytes",
    )
    .unwrap();
    fs::create_dir_all(root.join(".locks")).unwrap();
    let stale_lock = root.join(".locks/abandoned.lock");
    fs::write(&stale_lock, "pid=4294967294\n").unwrap();
    fs::remove_file(keys.join("jet.ed25519")).unwrap();
    let before_meta = fs::read(&meta).unwrap();
    let before_lock = fs::read(&stale_lock).unwrap();
    let before_public = fs::read(keys.join("jet.ed25519.pub")).unwrap();
    let before_public_permissions = fs::metadata(keys.join("jet.ed25519.pub"))
        .unwrap()
        .permissions();
    let before_output_permissions = fs::metadata(output.join("payload")).unwrap().permissions();

    let broken = jetpack()
        .args(["doctor", "--json", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JET_KEYS_DIR", &keys.path)
        .env(
            "JET_REGISTRY_URL",
            format!("file://{}", project.join("missing").display()),
        )
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(2));
    let text = String::from_utf8(broken.stdout).unwrap();
    assert!(text.contains("failed its content digest"), "{text}");
    assert!(text.contains("local index missing"), "{text}");
    assert!(!text.contains("stale lock"), "{text}");
    assert!(text.contains("kernel advisory locks readable"), "{text}");
    assert!(text.contains("unused for more than 30 days"), "{text}");
    assert!(text.contains("signing key for `jet` is missing"), "{text}");
    assert_eq!(
        fs::read(&meta).unwrap(),
        before_meta,
        "doctor changed metadata"
    );
    assert_eq!(
        fs::read(&stale_lock).unwrap(),
        before_lock,
        "doctor changed lock state"
    );
    assert_eq!(
        fs::read(keys.join("jet.ed25519.pub")).unwrap(),
        before_public,
        "doctor changed public key"
    );
    assert_eq!(
        fs::metadata(keys.join("jet.ed25519.pub"))
            .unwrap()
            .permissions(),
        before_public_permissions,
        "doctor changed key permissions"
    );
    assert_eq!(
        fs::metadata(output.join("payload")).unwrap().permissions(),
        before_output_permissions,
        "doctor changed output permissions"
    );
}

#[test]
fn override_draft_writes_reviewed_workspace_policy_and_explains_it() {
    let project = Scratch::new("override-draft");
    fs::create_dir_all(project.join("patches")).unwrap();
    fs::write(project.join("patches/foo.patch"), "patch body\n").unwrap();

    let out = jetpack()
        .args([
            "override",
            "draft",
            "foo@nixpkgs",
            "--overlay",
            "plasma_beta",
            "--provider",
            "nixpkgs",
            "--channel",
            "plasma-beta",
            "--patch",
            "patches/foo.patch",
            "--allow-unfree",
            "--no-color",
        ])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let workspace = fs::read_to_string(project.join("workspace.jet")).unwrap();
    assert!(workspace.contains("overlay plasma_beta"), "{workspace}");
    assert!(
        workspace.contains("Provider.nixpkgs(channel: \"plasma-beta\")"),
        "{workspace}"
    );
    assert!(
        workspace.contains("\"foo\": .{")
            && workspace.contains("patches: [patch(\"patches/foo.patch\")]")
            && workspace.contains("allowUnfree: true"),
        "{workspace}"
    );

    let explain = jetpack()
        .args(["explain", "package-overlay:plasma_beta:foo", "--no-color"])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(
        explain.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let stdout = String::from_utf8_lossy(&explain.stdout);
    assert!(
        stdout.contains("package-overlay:plasma_beta:foo")
            && stdout.contains("provider: nixpkgs")
            && stdout.contains("policy: workspace.overlay.resolved:foo"),
        "explain: {stdout}"
    );

    let explain_json = jetpack()
        .args([
            "explain",
            "package-overlay:plasma_beta:foo",
            "--json",
            "--no-color",
        ])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(
        explain_json.status.success(),
        "JSON explain stderr: {}",
        String::from_utf8_lossy(&explain_json.stderr)
    );
    assert!(explain_json.stderr.is_empty());
    let explain_json_stdout = String::from_utf8_lossy(&explain_json.stdout);
    assert!(!explain_json_stdout.contains("schema_version"));
    let report = jetpack::JSON::parse(explain_json_stdout.trim()).expect("overlay explain JSON");
    assert_eq!(json_string(&report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&report, "moment"), "tool");
    assert_eq!(json_string(&report, "action"), "explain");
    assert_eq!(json_string(&report, "status"), "ok");
    assert_eq!(json_string(&report, "lens"), "overlay");
}

#[test]
fn build_resolves_fixture_ref() {
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "fastfetch@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fastfetch"), "stderr: {stderr}");
    assert!(stderr.contains("substituted"), "stderr: {stderr}");
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
    assert!(
        stderr.contains("/hangar/objects/sha256-"),
        "stderr: {stderr}"
    );

    let explained = jet()
        .args(["explain", "fastfetch", "--json", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert!(
        explained.status.success(),
        "explain stderr: {}",
        String::from_utf8_lossy(&explained.stderr)
    );
    let stdout = String::from_utf8_lossy(&explained.stdout);
    assert!(stdout.contains("\"schema\":\"jet.report/v1\""));
    assert!(stdout.contains("\"moment\":\"tool\""));
    let explain_report = jetpack::JSON::parse(stdout.trim()).expect("package explain JSON");
    assert_eq!(json_string(&explain_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&explain_report, "moment"), "tool");
    assert_eq!(json_string(&explain_report, "action"), "explain");
    assert_eq!(json_string(&explain_report, "status"), "ok");
    assert!(stdout.contains("\"provider_facts\":"));
    assert!(stdout.contains("\"direct_dependencies\":"));
    assert!(stdout.contains("\"roots\":"));
    assert!(stdout.contains("\"decision\":\"realized\""), "{stdout}");
    assert!(stdout.contains("\"output_digest\":\"matches\""), "{stdout}");
    for lens in [
        "why-depends",
        "what-depends",
        "closure",
        "why-live",
        "rebuild",
    ] {
        let explained = jet()
            .args(["explain", lens, "fastfetch", "--json", "--no-color"])
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", example_fixtures(&root.path))
            .output()
            .unwrap();
        assert!(
            explained.status.success(),
            "{lens} stderr: {}",
            String::from_utf8_lossy(&explained.stderr)
        );
        let stdout = String::from_utf8_lossy(&explained.stdout);
        assert!(stdout.contains(&format!("\"lens\":\"{lens}\"")), "{stdout}");
    }

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::list(&roots)
        .into_iter()
        .find(|entry| entry.name == "fastfetch")
        .expect("fixture build must publish fastfetch to the Store");
    make_writable(&entry.out);
    fs::write(Path::new(&entry.out).join("payload"), "tampered\n").unwrap();
    let rebuild = jet()
        .args(["explain", "rebuild", "fastfetch", "--json", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert!(
        rebuild.status.success(),
        "rebuild explain stderr: {}",
        String::from_utf8_lossy(&rebuild.stderr)
    );
    let rebuild_stdout = String::from_utf8_lossy(&rebuild.stdout);
    let rebuild_report = jetpack::JSON::parse(rebuild_stdout.trim()).expect("rebuild JSON");
    assert_eq!(json_string(&rebuild_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&rebuild_report, "action"), "explain");
    assert!(rebuild_stdout.contains("\"severity\":\"warning\""));
    assert!(rebuild_stdout.contains("\"what\":"));
    assert!(rebuild_stdout.contains("\"decision\":\"rebuild-required\""));
    assert!(rebuild_stdout.contains("\"output_digest\":\"mismatch\""));
    assert!(rebuild_stdout.contains("\"kind\":\"loss\""));
    assert!(rebuild_stdout.contains("stored output digest differs"));
}

#[test]
fn package_explain_matches_failed_provider_reference_target() {
    let root = Scratch::new("package-explain-failed-provider-ref");
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let mut attempt = jetpack::BuildDebug::Attempt::new(
        "left-pad",
        "left-pad#version=2.0.17@npm",
        "npm",
        "sha256-recipe",
        "sha256-source",
    );
    attempt.push_step(jetpack::BuildDebug::StepLog {
        index: 1,
        total: 1,
        name: "fetch".to_string(),
        command: "fetch left-pad".to_string(),
        cwd: root.path.display().to_string(),
        status: "failed".to_string(),
        stdout: String::new(),
        stderr: "network unavailable".to_string(),
    });
    attempt.persist(&roots.hangar_dir()).unwrap();

    let explanation = jetpack::Store::explain_package(
        &roots,
        "left-pad#version=2.0.17@npm",
        jetpack::Store::ExplainLens::Rebuild,
    )
    .unwrap()
    .expect("failed provider reference should explain its build attempt");
    assert!(explanation.entry.is_none());
    assert_eq!(explanation.rebuild.decision, "rebuild-required");
    assert_eq!(explanation.rebuild.attempt.unwrap().package, "left-pad");
}

#[test]
fn no_nix_projects_external_output_and_build_facts() {
    let root = Scratch::new("nix-projection-root");
    let fixtures = Scratch::new("nix-projection-fixtures");
    let staging = Scratch::new("nix-projection-output");
    let bin = staging.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("greet");
    fs::write(&executable, "#!/bin/sh\necho projected\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Nix outputs are sealed before their bytes are hashed. Keep this source
    // outside Hangar so the production projection, not a fixture object, is
    // what the build exercises.
    jetpack::Store::seal_local_output(&staging.path).unwrap();
    fs::write(
        fixtures.join("nixpkgs-greet.json"),
        format!(
            "[{{\"drvPath\":\"/nix/store/0fixture00000000000000000000-greet.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
            staging.path.to_string_lossy()
        ),
    )
    .unwrap();

    let built = jetpack()
        .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let roots = jetpack::Store::Roots::at(root.path.clone());
    let entry = jetpack::Store::list_checked(&roots)
        .unwrap()
        .into_iter()
        .find(|entry| entry.reference == "greet@nixpkgs")
        .expect("raw Nix output must publish through the Store");
    assert!(
        Path::new(&entry.out).starts_with(roots.hangar_dir().join("objects")),
        "Nix output escaped Hangar projection: {}",
        entry.out
    );
    assert!(
        staging.path.exists(),
        "projection must not mutate the source store"
    );
    let producer = jetpack::Store::ProducerRecord::decode(&entry.producer_record).unwrap();
    assert_eq!(producer.provider, "nix");
    assert_eq!(
        producer
            .facts
            .get("nix.projection.mode")
            .map(String::as_str),
        Some("canonical-hangar")
    );
    assert_eq!(
        producer.facts.get("nix.build.root").map(String::as_str),
        Some("/build")
    );
    assert_eq!(
        producer.facts.get("nix.build.home").map(String::as_str),
        Some("/homeless-shelter")
    );
    let provider_facts = jetpack::ProviderFacts::from_json(
        producer
            .facts
            .get("provider-facts")
            .expect("Nix production path must publish the shared provider carrier"),
    )
    .expect("Nix producer provider facts must decode");
    provider_facts
        .validate()
        .expect("Nix producer provider facts must remain lossless");
    assert_eq!(provider_facts.native_format, "json");
    assert!(provider_facts.native_document.contains("drvPath"));
    assert!(provider_facts.facts.contains_key("nix.native.document"));

    let entered = jetpack()
        .args(["env", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures.path)
        .args([
            "-p",
            "greet",
            "--",
            "/bin/sh",
            "-c",
            "printf '%s|%s|%s|%s' \"$HOME\" \"$NIX_BUILD_TOP\" \"$TMPDIR\" \"$LC_ALL\"",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        entered.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&entered.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&entered.stdout).trim(),
        "/homeless-shelter|/build|/build|C"
    );
}

#[test]
fn connected_receipt_reaches_lock_and_fails_closed_on_corruption() {
    let project = Scratch::new("connected-receipt-project");
    let root = Scratch::new("connected-receipt-root");
    let fixtures = Scratch::new("connected-receipt-fixtures");
    let staging = Scratch::new("connected-receipt-staging");
    write_runnable_fixture(&fixtures.path, &root.path, &staging.path);
    fs::write(
        project.join("env.jet"),
        "module dev { env.dev: Env{ packages: [nixpkgs.greet] } }\n",
    )
    .unwrap();

    let build = || {
        jetpack()
            .args(["build", "--no-color", "--offline"])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", &fixtures.path)
            .output()
            .unwrap()
    };
    let first = build();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::list_checked(&roots)
        .unwrap()
        .into_iter()
        .find(|entry| entry.name == "greet")
        .expect("production build must publish greet");
    assert!(entry.receipt.starts_with("sha256-"));
    let receipt_path = root.join("hangar/receipts").join(&entry.receipt);
    let receipt = fs::read(&receipt_path).unwrap();
    let receipt_text = String::from_utf8(receipt.clone()).unwrap();
    assert!(receipt_text.starts_with("jet-development-receipt-v1\n"));
    assert!(receipt_text.contains("act\t\t7061636b6167652d7265616c697a6174696f6e\t"));
    assert!(receipt_text.contains("closure\t\t7368613235362d"));
    for field in [
        "input\t",
        "action\t",
        "output\t",
        "activation-proof\t\t\n",
        "parent-generation\t\t\n",
    ] {
        assert!(
            receipt_text.contains(field),
            "missing {field:?}: {receipt_text}"
        );
    }
    assert!(receipt_text.contains("witness\t\t"));
    assert!(receipt_text.contains("outcome\t\t706173736564\t"));
    let lock = fs::read_to_string(project.join(".jet/lock")).unwrap();
    assert!(
        lock.contains(&format!("receipt = \"{}\"", entry.receipt)),
        "lock does not project receipt {}: {lock}",
        entry.receipt
    );
    let output = Path::new(&entry.out).join("bin/greet");
    let output_before_failure = fs::read(&output).unwrap();

    fs::write(&receipt_path, b"corrupt receipt").unwrap();
    let failed = build();
    assert!(!failed.status.success());
    let failure = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        failure.contains("receipt"),
        "corruption was not explained: {failure}"
    );
    assert_eq!(fs::read(&output).unwrap(), output_before_failure);

    fs::remove_file(&receipt_path).unwrap();
    let recovered = jetpack()
        .args(["hangar", "recover", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
    let rebuilt = build();
    assert!(
        rebuilt.status.success(),
        "recovered receipt was not reusable: stdout={} stderr={}",
        String::from_utf8_lossy(&rebuilt.stdout),
        String::from_utf8_lossy(&rebuilt.stderr)
    );

    let orphan_bytes = b"orphan receipt";
    let orphan_digest = format!("sha256-{}", jetpack::SHA256::sha256_hex(orphan_bytes));
    let orphan_path = root.join("hangar/receipts").join(&orphan_digest);
    fs::write(&orphan_path, orphan_bytes).unwrap();
    let cleaned = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        cleaned.status.success(),
        "receipt cleanup failed: {}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    assert!(
        !orphan_path.exists(),
        "unreachable receipt was not collected"
    );
    assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
}

#[test]
fn list_shows_realized_package() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fixtures");
    let staging = Scratch::new("greet-out");
    write_runnable_fixture(&fixtures.path, &root.path, &staging.path);
    let built = jetpack()
        .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let out = jetpack()
        .args(["hangar", "list", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("greet"), "stderr: {stderr}");
}

#[test]
fn disappeared_output_build_fails_without_store_state_and_retries() {
    let root = Scratch::new("missing-output-root");
    let fixtures = Scratch::new("missing-output-fixtures");
    let missing = root.join("not-realized");
    fs::create_dir_all(&missing).unwrap();
    fs::write(missing.join("payload"), "present at fixture creation").unwrap();
    fs::write(
        fixtures.join("nixpkgs-greet.json"),
        format!(
            "[{{\"drvPath\":\"/nix/store/0fixture-greet.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
            missing.to_string_lossy()
        ),
    )
    .unwrap();
    fs::remove_dir_all(&missing).unwrap();
    let build = || {
        jetpack()
            .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", &fixtures.path)
            .output()
            .unwrap()
    };

    let failed = build();
    assert!(!failed.status.success());
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("Error [E1315]:"), "stderr: {stderr}");
    assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    assert_no_hangar_entry(&root.path, "greet-");
    let listed = jetpack()
        .args(["hangar", "list", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&listed.stderr).contains("greet"));

    let staging = Scratch::new("retry-greet-output");
    write_runnable_fixture(&fixtures.path, &root.path, &staging.path);
    let retried = build();
    assert!(
        retried.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&retried.stderr)
    );
}

#[cfg(unix)]
#[test]
fn unreadable_and_wrong_kind_outputs_leave_no_store_entry() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::net::UnixListener;

    let root = Scratch::new("invalid-output-root");
    let fixtures = Scratch::new("invalid-output-fixtures");
    let fixture = fixtures.join("nixpkgs-greet.json");
    let write_fixture = |out: &Path| {
        fs::write(
            &fixture,
            format!(
                "[{{\"drvPath\":\"/nix/store/0fixture-greet.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
                out.to_string_lossy()
            ),
        )
        .unwrap();
    };
    let build = || {
        jetpack()
            .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_FIXTURES", &fixtures.path)
            .output()
            .unwrap()
    };

    let unreadable = root.join("unreadable");
    fs::create_dir_all(&unreadable).unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&unreadable).is_err() {
        write_fixture(&unreadable);
        let failed = build();
        let stderr = String::from_utf8_lossy(&failed.stderr);
        assert!(!failed.status.success(), "stderr: {stderr}");
        assert!(stderr.contains("Error [E1315]:"), "stderr: {stderr}");
        assert!(stderr.contains("Permission denied"), "stderr: {stderr}");
        assert_no_hangar_entry(&root.path, "greet-");
    }
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o700)).unwrap();

    // Unix socket addresses are capped at SUN_LEN; the test's scratch root is
    // intentionally verbose, so keep this path short and independent of the
    // Jetpack root.
    let socket = std::env::temp_dir().join(format!("jpk-sock-{}", std::process::id()));
    let listener = UnixListener::bind(&socket).unwrap();
    write_fixture(&socket);
    let failed = build();
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(!failed.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("Error [E1315]:"), "stderr: {stderr}");
    assert!(
        stderr.contains("unsupported special file"),
        "stderr: {stderr}"
    );
    assert_no_hangar_entry(&root.path, "greet-");
    drop(listener);
    let _ = fs::remove_file(socket);
}

#[test]
fn clean_removes_only_stale_unreferenced_hangar_objects() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-1", "old", "1.0", Some(1)).0;
    let fresh = write_hangar_meta(&root.path, "fresh-1", "fresh", "1.0", Some(now_secs())).0;
    fs::write(stale.join("payload"), "old bytes").unwrap();
    fs::write(fresh.join("payload"), "fresh bytes").unwrap();

    let out = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stale.exists(), "stale object should be collected");
    assert!(fresh.exists(), "fresh object should be kept");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("removed 2 stale object"),
        "stderr: {stderr}"
    );
}

#[test]
fn clean_without_yes_prints_plan_and_does_not_apply_in_non_tty() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-plan", "oldplan", "1.0", Some(1)).0;
    fs::write(stale.join("payload"), "old bytes").unwrap();

    let out = jetpack()
        .args(["hangar", "clean", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stale.exists(), "plan-only clean must not delete objects");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Plan hangar clean"), "stderr: {stderr}");
    assert!(stderr.contains("- stale-objects"), "stderr: {stderr}");
    assert!(stderr.contains("-y or --yes"), "stderr: {stderr}");
}

#[test]
fn clean_keeps_lock_reachable_and_legacy_unknown_hangar_objects() {
    let root = Scratch::new("root");
    let project = Scratch::new("proj");
    let (live, live_hash) = write_hangar_meta(&root.path, "live-1", "live", "1.0", Some(1));
    let legacy = write_hangar_meta(&root.path, "legacy-1", "legacy", "1.0", None).0;
    write_lock_with_live_output(&project.path, "live", "1.0", &live_hash);

    let out = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(live.exists(), "lock-reachable object should be kept");
    assert!(
        legacy.exists(),
        "legacy object without timestamps should be kept"
    );
}

#[test]
fn clean_sweeps_orphan_build_scratch_but_keeps_active_scratch() {
    let root = Scratch::new("root");
    let scratch = root.path.join("hangar/build-scratch");
    let orphan = scratch.join("orphan");
    let active = scratch.join("active");
    fs::create_dir_all(&orphan).unwrap();
    fs::create_dir_all(&active).unwrap();
    fs::write(orphan.join("tmp"), "dead").unwrap();
    fs::write(active.join(".active"), "").unwrap();
    fs::write(active.join("tmp"), "live").unwrap();

    let out = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!orphan.exists(), "orphan scratch should be swept");
    assert!(active.exists(), "active scratch marker protects scratch");
}

#[test]
fn clean_sweeps_preserved_failed_build_scratch() {
    let root = Scratch::new("root");
    let scratch = root.path.join("hangar/failed-scratch");
    let failed = scratch.join("weirdctl-1");
    fs::create_dir_all(&failed).unwrap();
    fs::write(failed.join("build.log"), "failed build").unwrap();

    let out = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!failed.exists(), "failed-build scratch should be swept");
}

#[test]
fn clean_optimizes_duplicate_files_inside_hangar_only() {
    let root = Scratch::new("root");
    let first = write_hangar_meta(&root.path, "dup-a", "dupa", "1.0", Some(now_secs())).0;
    let second = write_hangar_meta(&root.path, "dup-b", "dupb", "1.0", Some(now_secs())).0;
    fs::write(first.join("blob"), "same payload").unwrap();
    fs::write(second.join("blob"), "same payload").unwrap();

    let out = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("optimized duplicate Jet-owned files: saved"),
        "stderr: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(first.join("blob")).unwrap(),
        "same payload"
    );
    assert_eq!(
        fs::read_to_string(second.join("blob")).unwrap(),
        "same payload"
    );
}

#[test]
fn clean_reclaims_unreachable_canonical_hangar_objects() {
    let root = Scratch::new("canonical-gc-root");
    let source = Scratch::new("canonical-gc-source");
    fs::write(source.join("payload"), "orphaned bytes\n").unwrap();
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::ingest_tree(
        &roots,
        &jetpack::Store::IngestRequest {
            name: "canonical-gc".into(),
            version: "1".into(),
            reference: "path:canonical-gc".into(),
            cache_identity: jetpack::Store::CacheIdentity {
                source_fingerprint: "sha256:canonical-gc-source".into(),
                recipe_fingerprint: "sha256:canonical-gc-recipe".into(),
                policy_fingerprint: "sha256:canonical-gc-policy".into(),
                platform: jetpack::Envelope::host_platform(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".into(), source.path.clone())]),
            signature: String::new(),
            provenance: "canonical GC test".into(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap()
    .entry;
    let output = Path::new(&entry.out).to_path_buf();
    let receipt = roots.hangar_dir().join("receipts").join(&entry.receipt);
    assert!(output.is_dir());
    assert!(receipt.is_file());
    assert!(jetpack::Store::remove_closure_record(&roots, &entry.id).unwrap());

    let plan = jetpack::Store::clean_plan(&roots).unwrap();
    assert!(
        plan.removed_objects >= 1,
        "canonical object missing from GC plan"
    );
    assert_eq!(plan.removed_receipts, 1);
    let report = jetpack::Store::clean(&roots).unwrap();
    assert!(report.removed_objects >= 1);
    assert_eq!(report.removed_receipts, 1);
    assert!(!output.exists(), "unreachable canonical object survived GC");
    assert!(!receipt.exists(), "unreachable receipt survived GC");
}

#[test]
fn build_runs_opportunistic_clean_after_success() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-auto", "oldauto", "1.0", Some(1)).0;

    let out = jetpack()
        .args(["build", "fastfetch@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .env("JETPACK_AUTO_CLEAN_ALWAYS", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stale.exists(),
        "successful build should run opportunistic clean"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("auto-cleaned hangar"), "stderr: {stderr}");
}

#[test]
fn run_dash_dash_executes_in_env_and_returns_status() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &root.path, &out_dir.path);

    let output = jetpack()
        .args([
            "run",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--",
            "greet",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello from jetpack");
}

#[test]
fn run_explicit_package_without_command_runs_package_visibly() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &root.path, &out_dir.path);

    let output = jetpack()
        .args(["run", "greet@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jetpack"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("running greet@nixpkgs -> greet"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("(no args)"), "stderr: {stderr}");
}

#[test]
fn run_dash_dash_propagates_failure_status() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &root.path, &out_dir.path);

    let output = jetpack()
        .args([
            "run",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--",
            "false",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn parent_env_unchanged_after_run() {
    // The composed PATH only reaches the child. Ask the child to echo PATH and
    // confirm our bin dirs lead; the test process's own PATH is unaffected
    // because we never mutate it.
    //
    // Realization leases are mandatory (card #418): the consumer never sees
    // the raw fixture `out_dir` directly, only a sealed, hardlinked snapshot
    // copy under the hangar's `leases/` dir. The sealed, FD-pinned
    // exec-wrapper dir (`/proc/self/fd/N` on Linux, immutable and race-safe
    // against parent rename/symlink swaps) leads PATH ahead of that snapshot
    // bin dir.
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &root.path, &out_dir.path);
    let before = std::env::var("PATH").unwrap_or_default();

    let output = jetpack()
        .args([
            "run",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--",
            "sh",
            "-c",
            "printf %s \"$PATH\"",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    let child_path = String::from_utf8_lossy(&output.stdout);
    let mut entries = child_path.split(':');
    let wrapper = entries.next().unwrap_or_default();
    assert!(
        wrapper.starts_with("/proc/self/fd/"),
        "expected the sealed FD-pinned exec-wrapper dir first, got: {child_path}"
    );
    let bin = entries.next().unwrap_or_default();
    assert!(
        bin.starts_with(&root.path.to_string_lossy().into_owned()) && bin.ends_with("/bin"),
        "expected the leased snapshot bin dir (under JETPACK_ROOT) second, got: {child_path}"
    );
    assert_eq!(std::env::var("PATH").unwrap_or_default(), before);
}

#[test]
fn bad_ref_is_friendly_and_exits_2() {
    let out = jetpack()
        .args(["run", "fastfetch", "--no-color"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing a source"), "stderr: {stderr}");
    assert!(stderr.contains("package@source#version"), "stderr: {stderr}");
}

#[test]
fn provider_first_ref_is_coded_and_snapshot_pinned() {
    let root = Scratch::new("ref-provider-first");
    let out = jetpack()
        .args(["run", "github@owner/repo", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_jetos_stderr_snapshot_trimmed(
        "ref_provider_first",
        &String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn retired_nixpkgs_source_is_coded_and_snapshot_pinned() {
    let root = Scratch::new("ref-nixpkgs-source-retired");
    let out = jetpack()
        .args(["run", "ripgrep@nixpkgs", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_jetos_stderr_snapshot_trimmed(
        "ref_nixpkgs_source_retired",
        &String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn declared_source_package_named_like_provider_resolves_as_package() {
    let src = r#"
module env.dev {
    sources: { default: NixOS/nixpkgs/nixos-unstable@github }
    packages: [default.[cargo]]
}
"#;
    let plan = jet_env_model::ModuleEval::evaluate_env(src, Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap();
    assert_eq!(plan.package_refs, ["cargo@default"]);

    let spec = jetpack::RefSpec::classify_in(&plan.package_refs[0], &plan.table).unwrap();
    assert_eq!(spec.package, "cargo");
    assert_eq!(
        spec.source,
        jetpack::RefSpec::Source::Named("default".into())
    );
}

#[test]
fn retired_path_provider_is_coded_and_snapshot_pinned() {
    let root = Scratch::new("ref-path-provider");
    let out = jetpack()
        .args(["run", "../vendor/tool@path", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_jetos_stderr_snapshot_trimmed(
        "ref_path_provider_retired",
        &String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn unknown_source_is_friendly() {
    let out = jetpack()
        .args(["build", "wget@brew", "--no-color"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a known source"), "stderr: {stderr}");
}

#[test]
fn add_then_remove_edits_env_file() {
    let (_base, proj, root) = core_hello_project("add-remove");
    let env_path = proj.join("env.jet");
    fs::write(
        &env_path,
        fs::read_to_string(&env_path)
            .unwrap()
            .replace("\"hello@mine\"", ""),
    )
    .unwrap();
    let add = jetpack()
        .args(["add", "hello@mine", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(
        String::from_utf8_lossy(&add.stderr).contains("✓ hello     0.1.0"),
        "add must print its verified resolved version: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(env.contains("hello"), "env.jet: {env}");
    assert!(env.contains("pkg.packages"), "env.jet: {env}");

    let remove = jetpack()
        .args(["remove", "hello@mine", "--no-color", "--yes"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(remove.status.success());
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        !env.contains("\"hello@mine\""),
        "env.jet still has hello: {env}"
    );
}

#[test]
fn remove_without_yes_prints_plan_and_keeps_env_file_in_non_tty() {
    let (_base, proj, root) = core_hello_project("remove-plan");
    let env_path = proj.join("env.jet");
    fs::write(
        &env_path,
        fs::read_to_string(&env_path)
            .unwrap()
            .replace("\"hello@mine\"", ""),
    )
    .unwrap();
    let add = jetpack()
        .args(["add", "hello@mine", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(add.status.success());

    let remove = jetpack()
        .args(["remove", "hello@mine", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(remove.status.success());
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(env.contains("\"hello@mine\""), "env.jet was changed: {env}");
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(stderr.contains("Plan env edit"), "stderr: {stderr}");
    assert!(stderr.contains("- hello"), "stderr: {stderr}");
    assert!(stderr.contains("Download 0 B"), "stderr: {stderr}");
    assert!(stderr.contains("-y or --yes"), "stderr: {stderr}");
}

#[test]
fn remove_with_short_yes_applies_identically_to_long_yes() {
    // D-FE-CLI1: `-y` and `--yes` bypass the mutation gate identically.
    let (_base, proj, root) = core_hello_project("remove-short-yes");
    let env_path = proj.join("env.jet");
    fs::write(
        &env_path,
        fs::read_to_string(&env_path)
            .unwrap()
            .replace("\"hello@mine\"", ""),
    )
    .unwrap();
    let add = jetpack()
        .args(["add", "hello@mine", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let remove = jetpack()
        .args(["remove", "hello@mine", "--no-color", "-y"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        remove.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&remove.stderr)
    );
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        !env.contains("\"hello@mine\""),
        "short -y must apply the remove plan: {env}"
    );
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(stderr.contains("Plan env edit"), "stderr: {stderr}");
    assert!(stderr.contains("- hello"), "stderr: {stderr}");
    assert!(
        stderr.contains("applying plan (--yes)") || stderr.contains("removed"),
        "short -y must take the yes-bypass path: {stderr}"
    );
}

#[test]
fn run_with_project_env_file_resolves_declared_packages() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fixtures");
    let fastfetch_out = Scratch::new("fastfetch-out");
    write_fastfetch_fixture(&fixtures.path, &root.path, &fastfetch_out.path);
    // Declare one package, then run with no ref → it resolves from env.jet.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() [JSON] {\n    return [\n        pkg.source(\"nixpkgs\");\n        pkg.packages([\"fastfetch\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["run", "--no-color", "--offline", "--", "true"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fastfetch"), "stderr: {stderr}");
}

#[test]
fn nested_package_commands_use_the_nearest_package_root() {
    let (base, project, root) = core_hello_project("package-root");
    let package = project.join("package");
    let nested = package.join("src");
    fs::create_dir_all(&nested).unwrap();
    fs::rename(project.join("env.jet"), package.join("env.jet")).unwrap();
    fs::write(package.join("package.jet"), "name: \"demo\"\n").unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--offline", "--", "true"])
        .current_dir(&nested)
        .env("JETPACK_ROOT", &root)
        .env("JETPACK_FIXTURES", example_fixtures(&root))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "nearest package root was not used: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("hello"),
        "package-root env facts were not loaded: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(base);
}

#[test]
fn typed_env_copy_adapter_realizes_local_source() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let vendor = proj.join("vendor/tool");
    fs::create_dir_all(vendor.join("share")).unwrap();
    fs::write(vendor.join("share/readme.txt"), "adapted\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: "./vendor/tool",
                recipe: Recipe.copy()
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entries = jetpack::Store::list_checked(&roots).unwrap_or_else(|error| {
        panic!(
            "portfolio Hangar listing failed under {}: {error}",
            roots.hangar_dir().display()
        )
    });
    assert!(
        entries.iter().any(|entry| {
            fs::read_to_string(Path::new(&entry.out).join("share/readme.txt")).unwrap_or_default()
                == "adapted\n"
        }),
        "adapter output missing copied file: {entries:?}"
    );
    let cached = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        cached.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cached.stderr).contains("1 cached"),
        "stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
}

#[test]
fn typed_env_build_recipe_realizes_local_source() {
    let proj = Scratch::new("build-recipe-project");
    let root = Scratch::new("build-recipe-root");
    let home = Scratch::new("build-recipe-home");
    let vendor = proj.join("vendor/tool");
    fs::create_dir_all(&vendor).unwrap();
    fs::write(vendor.join("payload.txt"), "built recipe\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: "./vendor/tool",
                recipe: Recipe.build(steps: [
                    .install_tree(src: ".", dest: "share"),
                ])
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--trust"])
        .current_dir(&proj.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entries = jetpack::Store::list(&roots);
    assert!(
        entries.iter().any(|entry| {
            fs::read_to_string(Path::new(&entry.out).join("share/payload.txt")).unwrap_or_default()
                == "built recipe\n"
        }),
        "build recipe output missing copied file: {entries:?}"
    );
}

#[test]
fn typed_env_build_recipe_uses_declared_tool_dependencies() {
    let proj = Scratch::new("build-recipe-dependency-project");
    let root = Scratch::new("build-recipe-dependency-root");
    let fixtures = Scratch::new("build-recipe-dependency-fixtures");
    let staging = Scratch::new("build-recipe-dependency-staging");
    write_runnable_fixture(&fixtures.path, &root.path, &staging.path);
    fs::create_dir_all(proj.join("vendor/tool")).unwrap();
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: "./vendor/tool",
                deps: [default.greet],
                recipe: Recipe.build(steps: [
                    .exec(tool: "greet", args: []),
                ])
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline", "--trust"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    assert!(
        jetpack::Store::list(&roots)
            .iter()
            .any(|entry| entry.reference == "adapt:tool:./vendor/tool"),
        "adapter with a declared executable dependency was not realized: {:?}",
        jetpack::Store::list(&roots)
    );
}

#[test]
fn typed_env_build_hook_approval_binds_declared_dependency_refs() {
    let proj = Scratch::new("build-hook-dependency-identity-project");
    let root = Scratch::new("build-hook-dependency-identity-root");
    let fixtures = Scratch::new("build-hook-dependency-identity-fixtures");
    let home = Scratch::new("build-hook-dependency-identity-home");
    let staging = Scratch::new("build-hook-dependency-identity-staging");
    write_runnable_fixture(&fixtures.path, &root.path, &staging.path);
    // Both refs resolve to the same fixture executable. Only the declared
    // package ref changes, so a cache/trust key that ignores adapter deps would
    // incorrectly accept the second build.
    fs::copy(
        fixtures.join("nixpkgs-greet.json"),
        fixtures.join("nixpkgs-hello.json"),
    )
    .unwrap();
    fs::create_dir_all(proj.join("vendor/tool")).unwrap();
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: "./vendor/tool",
                deps: [default.greet],
                recipe: Recipe.build(steps: [
                    .exec(tool: "greet", args: []),
                ])
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let first = jetpack()
        .args(["build", "--no-color", "--offline", "--trust"])
        .current_dir(&proj.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "initial build failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::list(&roots)
        .into_iter()
        .find(|entry| entry.reference == "adapt:tool:./vendor/tool")
        .expect("initial adapter should be recorded");
    let old_identity = jetpack::Store::ProducerRecord::decode(&entry.producer_record)
        .expect("adapter producer record should decode")
        .facts
        .get("build.identity")
        .cloned()
        .expect("adapter producer should record its build identity");

    let selector = format!("build:{old_identity}");
    let grant = jetpack()
        .args(["trust", "grant", &selector, "--scope", "user"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "build identity trust setup failed: {}",
        String::from_utf8_lossy(&grant.stderr)
    );

    let changed = fs::read_to_string(proj.join("env.jet"))
        .unwrap()
        .replace("default.greet", "default.hello");
    fs::write(proj.join("env.jet"), changed).unwrap();
    let second = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert_eq!(second.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("E1255"),
        "changed dependency was accepted: {stderr}"
    );
    assert!(
        stderr.contains("build hook"),
        "wrong trust failure: {stderr}"
    );
}

#[test]
fn typed_env_prebuilt_adapter_runs_from_path() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let vendor = proj.join("vendor/weirdctl");
    fs::create_dir_all(&vendor).unwrap();
    let bin = vendor.join("weirdctl");
    fs::write(&bin, "#!/bin/sh\necho weird ok\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env{
        packages: [
            Pkg.adapt(
                name: "weirdctl",
                source: "./vendor/weirdctl",
                recipe: Recipe.prebuilt(bin: "weirdctl", as: "weirdctl")
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["run", "--no-color", "--", "weirdctl"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "weird ok");
}

#[test]
fn no_nix_unindexed_nixpkgs_package_reports_e1349() {
    let project = Scratch::new("unindexed-nixpkgs-project");
    let root = Scratch::new("root");
    let server = nix_index_cache_server::NixIndexCacheServer::start_unindexed(&project.path);
    server.install(&root.path);
    fs::create_dir_all(project.join(".jet")).unwrap();
    fs::write(
        project.join(".jet/lock"),
        format!(
            "version = 1\n\n[[source_channel]]\nname = \"nixpkgs\"\nchannel = \"nixpkgs-unstable\"\nexact = \"github:NixOS/nixpkgs#{}\"\n\n[root]\ndependencies = []\n",
            nix_index_cache_server::REVISION
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "postgres@nixpkgs", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1349"), "stderr: {stderr}");
    assert!(stderr.contains("postgres"), "stderr: {stderr}");
    assert!(stderr.contains("not covered"), "stderr: {stderr}");
    assert!(
        !stderr.contains("E1272"),
        "unindexed signed record must not use the retired bridge diagnostic: {stderr}"
    );
    assert!(!stderr.contains("E1256"), "stderr: {stderr}");
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
}

#[test]
fn indexed_nixpkgs_closure_reuses_offline_and_repairs_one_object() {
    const TEST_NAME: &str = "indexed_nixpkgs_closure_reuses_offline_and_repairs_one_object";
    const PHASE_ENV: &str = "JETPACK_INDEXED_NIX_PHASE";
    const PROJECT_ENV: &str = "JETPACK_INDEXED_NIX_PROJECT";
    const ROOT_ENV: &str = "JETPACK_INDEXED_NIX_ROOT";

    let (project_dir, root_dir, _owned_project, _owned_root, server, child) = match (
        std::env::var_os(PROJECT_ENV).map(PathBuf::from),
        std::env::var_os(ROOT_ENV).map(PathBuf::from),
    ) {
        (Some(project_dir), Some(root_dir)) => (project_dir, root_dir, None, None, None, true),
        (None, None) => {
            let project = Scratch::new("indexed-nixpkgs-project");
            let root = Scratch::new("root");
            let project_dir = project.path.clone();
            let root_dir = root.path.clone();
            let server = nix_index_cache_server::NixIndexCacheServer::start_ripgrep(&project_dir);
            server.install(&root_dir);
            (
                project_dir,
                root_dir,
                Some(project),
                Some(root),
                Some(server),
                false,
            )
        }
        _ => panic!("indexed Nix project/root environment must be configured together"),
    };
    if !child {
        fs::write(
            project_dir.join("env.jet"),
            format!(
                // The manifest names the channel the signed index is keyed by,
                // so it has to agree with the fixture server's own channel.
                "module dev {{\n    sources: {{ default: NixOS/nixpkgs/{}@github }}\n    env.dev: Env{{ packages: [default.ripgrep] }}\n}}\n",
                nix_index_cache_server::CHANNEL,
            ),
        )
        .unwrap();
        fs::create_dir_all(project_dir.join(".jet")).unwrap();
        fs::write(
            project_dir.join(".jet/lock"),
            format!(
                "version = 1\n\n[[source_channel]]\nname = \"default\"\nchannel = \"{}\"\nexact = \"github:NixOS/nixpkgs#{}\"\n\n[root]\ndependencies = []\n",
                nix_index_cache_server::CHANNEL,
                nix_index_cache_server::REVISION,
            ),
        )
        .unwrap();
    }

    let run = |offline: bool| {
        let mut command = jetpack();
        command
            .args(["env", "--no-color", "--trust"])
            .current_dir(&project_dir)
            .env("JETPACK_ROOT", &root_dir)
            .env("PATH", "/usr/bin")
            .env("JETPACK_DEBUG_NIX_CACHE", "1")
            .env_remove("JETPACK_FIXTURES");
        if offline {
            command.arg("--offline");
        }
        command.args(["--", "rg", "--version"]);
        command.output().unwrap()
    };

    if child {
        let phase = std::env::var(PHASE_ENV).expect("indexed Nix child phase");
        let network = match phase.as_str() {
            "online-initial" | "online-repair" => no_nix_namespace::NetworkMode::Enabled,
            "offline-reuse" | "offline-missing" => no_nix_namespace::NetworkMode::Disabled,
            other => panic!("unknown indexed Nix test phase `{other}`"),
        };
        no_nix_namespace::run_in_no_nix_namespace(TEST_NAME, network, || {
            let output = run(phase.starts_with("offline-"));
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            match phase.as_str() {
                "online-initial" | "offline-reuse" | "online-repair" => {
                    assert!(
                        output.status.success(),
                        "stdout: {stdout}\nstderr: {stderr}"
                    );
                    assert!(stdout.contains("ripgrep 15.2.0"), "stdout: {stdout}");
                }
                "offline-missing" => {
                    assert_eq!(
                        output.status.code(),
                        Some(2),
                        "stdout: {stdout}\nstderr: {stderr}"
                    );
                    assert!(stderr.contains("E1350"), "stderr: {stderr}");
                    assert!(
                        stderr.contains(nix_index_cache_server::RUNTIME_PATH),
                        "offline error must name missing reference: {stderr}"
                    );
                }
                _ => unreachable!(),
            }
        });
        return;
    }

    std::env::set_var(PROJECT_ENV, &project_dir);
    std::env::set_var(ROOT_ENV, &root_dir);
    for (phase, network) in [
        ("online-initial", no_nix_namespace::NetworkMode::Enabled),
        ("offline-reuse", no_nix_namespace::NetworkMode::Disabled),
    ] {
        std::env::set_var(PHASE_ENV, phase);
        no_nix_namespace::run_in_no_nix_namespace(TEST_NAME, network, || {});
    }
    std::env::remove_var(PHASE_ENV);

    let roots = jetpack::Store::Roots::at(root_dir.clone());
    let entries = jetpack::Store::list_checked(&roots).unwrap();
    let producer = |entry: &jetpack::Store::StoreEntry| {
        jetpack::Store::ProducerRecord::decode(&entry.producer_record).unwrap()
    };
    let package = entries
        .iter()
        .find(|entry| producer(entry).facts.contains_key("nix.index.proof.v1"))
        .expect("indexed Nix package entry");
    let package_producer = producer(package);
    assert!(!package_producer.facts["nix.index.proof.v1"].is_empty());
    assert!(!package_producer.facts["nix.cache.output.out.proof.sha256"].is_empty());
    assert!(!package_producer.facts["nix.cache.closure.receipt.sha256"].is_empty());

    let object_digest = |store_path: &str| {
        entries
            .iter()
            .find_map(|entry| {
                let producer = producer(entry);
                (producer.facts.get("nix.store-path").map(String::as_str) == Some(store_path))
                    .then(|| entry.envelope.output_hash.clone())
            })
            .unwrap_or_else(|| panic!("missing admitted object {store_path}"))
    };
    let root_digest = object_digest(nix_index_cache_server::ROOT_PATH);
    let library_digest = object_digest(nix_index_cache_server::LIB_PATH);
    let runtime_digest = object_digest(nix_index_cache_server::RUNTIME_PATH);
    let graph = jetpack::Store::closure_graph(&roots).unwrap();
    assert_eq!(package.envelope.output_hash, root_digest);
    assert_eq!(package.references, vec![library_digest.clone()]);
    assert_eq!(
        graph.direct_references(&root_digest),
        vec![library_digest.clone()]
    );
    assert_eq!(
        graph.direct_references(&library_digest),
        vec![runtime_digest.clone()]
    );
    assert!(graph
        .transitive_references(&root_digest)
        .contains(&runtime_digest));

    let missing = roots.hangar_dir().join("objects").join(&runtime_digest);
    make_tree_writable(&missing);
    fs::remove_dir_all(&missing).unwrap();

    std::env::set_var(PHASE_ENV, "offline-missing");
    no_nix_namespace::run_in_no_nix_namespace(
        TEST_NAME,
        no_nix_namespace::NetworkMode::Disabled,
        || {},
    );
    std::env::remove_var(PHASE_ENV);

    std::env::set_var(PHASE_ENV, "online-repair");
    no_nix_namespace::run_in_no_nix_namespace(
        TEST_NAME,
        no_nix_namespace::NetworkMode::Enabled,
        || {},
    );
    std::env::remove_var(PHASE_ENV);

    std::env::remove_var(PROJECT_ENV);
    std::env::remove_var(ROOT_ENV);
    let server = server.as_ref().expect("parent owns indexed Nix server");
    assert_eq!(
        server.object_request_count(nix_index_cache_server::ROOT_PATH),
        2,
        "repair must reuse root object"
    );
    assert_eq!(
        server.object_request_count(nix_index_cache_server::LIB_PATH),
        2,
        "repair must reuse library object"
    );
    assert_eq!(
        server.object_request_count(nix_index_cache_server::RUNTIME_PATH),
        4,
        "repair must fetch only missing runtime object"
    );
}

#[test]
fn package_and_environment_paths_have_no_installed_nix_shellout() {
    let sources = [
        ("Bridge", include_str!("../crates/jetpack/src/Bridge.rs")),
        (
            "Provider",
            include_str!("../crates/jetpack/src/Provider.rs"),
        ),
        (
            "NixIndex",
            include_str!("../crates/jetpack/src/NixIndex.rs"),
        ),
        (
            "Store/NixCache",
            include_str!("../crates/jetpack/src/Store/NixCache.rs"),
        ),
        (
            "CLI/realize",
            include_str!("../crates/jetpack/src/CLI/realize.rs"),
        ),
        (
            "CLI/run_enter_dev",
            include_str!("../crates/jetpack/src/CLI/run_enter_dev.rs"),
        ),
        (
            "CLI/profile",
            include_str!("../crates/jetpack/src/CLI/profile.rs"),
        ),
        (
            "CLI/tool",
            include_str!("../crates/jetpack/src/CLI/tool.rs"),
        ),
        (
            "CLI/trust_env_build",
            include_str!("../crates/jetpack/src/CLI/trust_env_build.rs"),
        ),
        ("Store", include_str!("../crates/jetpack/src/Store.rs")),
    ];
    for (path, source) in sources {
        for forbidden in [
            "Command::new(\"nix\")",
            "Command::new(\"nix-store\")",
            "Command::new(\"curl\")",
            "Command::new(\"wget\")",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} must not shell out to installed Nix via {forbidden}"
            );
        }
    }
}

#[test]
fn no_nix_ad_hoc_package_reports_e1272() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let output = jetpack()
        .args([
            "enter",
            "-p",
            "postgres",
            "--no-color",
            "--trust",
            "--",
            "true",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("postgres@nixpkgs"), "stderr: {stderr}");
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
}

#[test]
fn no_nix_mixed_env_realizes_core_then_reports_nix_hole() {
    let (base, proj, root) = core_hello_project("no-nix-mixed");
    fs::write(
        proj.join("env.jet"),
        fs::read_to_string(proj.join("env.jet")).unwrap().replace(
            "pkg.packages([\"hello@mine\"])",
            "pkg.packages([\"hello@mine\", \"postgres@nixpkgs\"])",
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // T4 ledger row: `✓ hello  <version>  built [duration]` (columns padded).
    assert!(
        stderr
            .lines()
            .any(|l| l.contains("hello") && l.split_whitespace().any(|word| word == "built")),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("postgres@nixpkgs"), "stderr: {stderr}");
    let metas = fs::read_dir(root.join("hangar"))
        .unwrap()
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path().join("meta.json")).ok())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(metas.contains("\"name\": \"hello\""), "metas: {metas}");
}

#[test]
fn no_nix_json_lists_realized_refs_and_holes() {
    let (base, proj, root) = core_hello_project("no-nix-json");
    fs::write(
        proj.join("env.jet"),
        fs::read_to_string(proj.join("env.jet")).unwrap().replace(
            "pkg.packages([\"hello@mine\"])",
            "pkg.packages([\"hello@mine\", \"postgres@nixpkgs\"])",
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--json"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"E1272\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"realized\":[\"hello@mine\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"holes\":[\"postgres@nixpkgs\"]"),
        "stdout: {stdout}"
    );
}

#[test]
fn typed_env_bad_adapter_is_e1270() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env{
        packages: [
            Pkg.adapt(
                name: "broken",
                source: "./vendor/broken",
                recipe: Recipe.build()
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1270"), "stderr: {stderr}");
}

#[test]
fn channel_update_writes_exact_lock_and_build_uses_it_offline() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: acme/tools@github#latest }
    env.dev: Env{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.2.0",
    );
    fs::write(
        fixtures.join("default-greet.json"),
        r#"[{"drvPath":"/nix/store/0000000000000000000000000000000b-greet-1.2.0.drv","outputs":{"out":"/nix/store/0000000000000000000000000000000a-greet-1.2.0"}}]"#,
    )
    .unwrap();

    let update = jetpack()
        .args(["update", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        update.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert!(
        String::from_utf8_lossy(&update.stderr).contains("Download 240 MB"),
        "stderr: {}",
        String::from_utf8_lossy(&update.stderr)
    );
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("[[source_channel]]"), "lock: {lock}");
    assert!(lock.contains("channel = \"latest\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#v1.2.0\""),
        "lock: {lock}"
    );

    let build = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env(
            "JETPACK_FIXTURES",
            realized_fixtures(&fixtures.path, &root.path),
        )
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn channel_build_without_lock_is_e1271() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: acme/tools@github#latest }
    env.dev: Env{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1271"), "stderr: {stderr}");
    assert!(
        stderr.contains("jetpack update default"),
        "stderr: {stderr}"
    );
}

#[test]
fn channel_update_accepts_main_and_semver_mask() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: {
        trunk: acme/tools@github#main,
        stable: acme/tools@github#v0.x,
    }
    env.dev: Env{ packages: [trunk.greet, stable.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(&fixtures.path).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        "github:acme/tools main github:acme/tools#abc123\n\
         github:acme/tools v0.x github:acme/tools#v0.9.4\n",
    )
    .unwrap();

    let out = jetpack()
        .args(["update", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("name = \"trunk\""), "lock: {lock}");
    assert!(lock.contains("channel = \"main\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#abc123\""),
        "lock: {lock}"
    );
    assert!(lock.contains("name = \"stable\""), "lock: {lock}");
    assert!(lock.contains("channel = \"v0.x\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#v0.9.4\""),
        "lock: {lock}"
    );
}

#[test]
fn outdated_reports_newer_channel_without_mutating_lock() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: acme/tools@github#latest }
    env.dev: Env{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(proj.join(".jet")).unwrap();
    fs::write(
        proj.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"default\"\nchannel = \"latest\"\nexact = \"github:acme/tools#v1.2.0\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.3.0",
    );

    let out = jetpack()
        .args(["outdated", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("v1.2.0"), "stderr: {stderr}");
    assert!(stderr.contains("v1.3.0"), "stderr: {stderr}");
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("exact = \"github:acme/tools#v1.2.0\""),
        "lock mutated: {lock}"
    );
}

#[test]
fn automatic_channel_refresh_writes_lock_and_manifest_without_update() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { automatic: omp@nixpkgs#auto }
    env.dev: Env{ packages: [] }
}
"#,
    )
    .unwrap();
    write_channel_fixture(&fixtures.path, "nixpkgs:omp", "latest", "nixpkgs:omp#1.2.3");

    let build = jetpack()
        .args(["build", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("name = \"automatic\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"nixpkgs:omp#1.2.3\""),
        "lock: {lock}"
    );
    let manifest = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        manifest.contains("omp@nixpkgs#auto"),
        "manifest: {manifest}"
    );
}

#[test]
fn automatic_channel_refresh_moves_again_after_manifest_writeback() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { automatic: omp@nixpkgs#auto }
    env.dev: Env{ packages: [] }
}
"#,
    )
    .unwrap();
    for exact in ["nixpkgs:omp#1.2.3", "nixpkgs:omp#1.2.4"] {
        write_channel_fixture(&fixtures.path, "nixpkgs:omp", "latest", exact);
        let build = jetpack()
            .args(["build", "--no-color", "--yes", "--fixtures"])
            .arg(&fixtures.path)
            .current_dir(&proj.path)
            .env("JETPACK_ROOT", &root.path)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&build.stderr)
        );
    }

    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("exact = \"nixpkgs:omp#1.2.4\""),
        "lock: {lock}"
    );
    let manifest = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        manifest.contains("omp@nixpkgs#auto"),
        "manifest: {manifest}"
    );
}

#[test]
fn update_of_pinned_source_is_e1352() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { stable: rustc@jetpack }
    env.dev: Env{ packages: [] }
}
"#,
    )
    .unwrap();
    let out = jetpack()
        .args(["update", "stable", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1352"), "stderr: {stderr}");
    assert!(stderr.contains("#auto"), "stderr: {stderr}");
}

#[test]
fn outdated_labels_pinned_manual_and_automatic_sources() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: {
        pinned: rustc@jetpack,
        manual: jq@jetpack#latest,
        automatic: omp@jetpack#auto,
    }
    env.dev: Env{ packages: [] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(proj.join(".jet")).unwrap();
    fs::write(
        proj.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"manual\"\nchannel = \"latest\"\nexact = \"nixpkgs:jq#1.0\"\n\n[[source_channel]]\nname = \"automatic\"\nchannel = \"latest\"\nexact = \"nixpkgs:omp#1.0\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_channel_fixture(&fixtures.path, "nixpkgs:jq", "latest", "nixpkgs:jq#1.0");
    write_channel_fixture(&fixtures.path, "nixpkgs:omp", "latest", "nixpkgs:omp#1.0");

    let out = jetpack()
        .args(["outdated", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("pinned"), "stderr: {stderr}");
    assert!(stderr.contains("manual"), "stderr: {stderr}");
    assert!(stderr.contains("automatic"), "stderr: {stderr}");
}

#[test]
fn add_adapt_prints_snippet_without_editing_env() {
    let proj = Scratch::new("proj");
    let output = jetpack()
        .args(["add", "./vendor/weirdctl", "--adapt", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Pkg.adapt("), "stdout: {stdout}");
    assert!(
        stdout.contains("source: \"./vendor/weirdctl\""),
        "stdout: {stdout}"
    );
    assert!(!proj.join("env.jet").exists());
}

#[test]
fn named_source_env_resolves_with_pin() {
    // An env that declares a named source `stable` and references it inline as
    // `ripgrep@stable` resolves via the nix provider against the pin. The
    // fixture is keyed by the source name (`stable-ripgrep.json`).
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() [JSON] {\n    return [\n        pkg.source(\"stable\", \"NixOS/nixpkgs/nixos-24.05@github\");\n        pkg.packages([\"ripgrep@stable\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ripgrep"), "stderr: {stderr}");
}

#[test]
fn unknown_named_source_in_env_is_friendly() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    // References `neovim@beta` but only declares `stable`.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() [JSON] {\n    return [\n        pkg.source(\"stable\", \"NixOS/nixpkgs/nixos-24.05@github\");\n        pkg.packages([\"neovim@beta\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a known source"), "stderr: {stderr}");
    assert!(
        stderr.contains("stable"),
        "should list declared names: {stderr}"
    );
}

#[test]
fn jetpack_env_runs_command_in_project_env() {
    // Gap #6 / U §8 (Scale-2): `jetpack env` is the project-env command — it
    // never takes an explicit ref, it always composes the env declared by the
    // project `env.jet`. The `-- cmd` form runs a one-off command in the
    // realized env, which is how we prove `env` put the package on PATH.
    let (base, proj, root) = core_hello_project("env");
    let output = jetpack()
        // U19: `env` trust-gates a project that declares packages; `--trust`
        // is the one-shot bypass so this test can assert on PATH composition
        // without exercising the interactive prompt.
        .args(["env", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
    let roots = jetpack::Store::Roots::at(root);
    let entry = jetpack::Store::find_by_reference(&roots, "hello@mine")
        .expect("core realization should register a Store entry");
    let shared = jetpack::ProviderFacts::from_json(
        jetpack::Store::ProducerRecord::decode(&entry.producer_record)
            .expect("Store producer record")
            .facts
            .get("provider-facts")
            .expect("Store entry should carry shared provider facts"),
    )
    .expect("shared provider facts JSON");
    shared
        .validate()
        .expect("Store provider facts are lossless");
    assert_eq!(shared.reference, entry.reference);
    assert!(!shared.native_document.is_empty());
}

#[test]
fn jetpack_env_propagates_child_exit_status() {
    let (base, proj, root) = core_hello_project("env-status");
    let output = jetpack()
        .args(["env", "--no-color", "--trust", "--", "sh", "-c", "exit 17"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(17), "stderr: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn jetpack_env_prep_materializes_without_entering() {
    let (base, proj, root) = core_hello_project("env-prep");
    let prep = jetpack()
        .args(["env", "--prep", "--no-color", "--trust"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        prep.status.success(),
        "env --prep failed: {}",
        String::from_utf8_lossy(&prep.stderr)
    );
    let enter = jetpack()
        .args(["env", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        enter.status.success(),
        "entry after prep failed: {}",
        String::from_utf8_lossy(&enter.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&enter.stdout).trim(), "hello from jet-pkgs");
}

#[test]
fn provider_conformance_pypi_swiftpm_maven_round_trips_the_production_carrier() {
    use jetpack::ProviderFacts;
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let revision = "0123456789abcdef0123456789abcdef01234567";
    let pypi = r#"{"info":{"name":"sample-pkg","version":"2.4.1","license":"MIT","requires_python":">=3.10","requires_dist":["httpx>=0.27"],"classifiers":["Programming Language :: Python :: 3"]},"urls":[{"filename":"sample_pkg-2.4.1.tar.gz","digests":{"sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"},"yanked":false}],"vulnerabilities":[{"id":"PYSEC-0000"}]}"#;
    let swiftpm = format!(
        r#"{{"version":1,"pins":[{{"package":"swift-log","repositoryURL":"https://github.com/apple/swift-log.git","state":{{"revision":"{revision}","version":"1.5.4"}}}}]}}"#
    );
    let maven = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>sample</artifactId>
  <version>1.2.3</version>
  <packaging>jar</packaging>
  <licenses><license><name>Apache-2.0</name><url>https://www.apache.org/licenses/LICENSE-2.0</url></license></licenses>
  <dependencies><dependency><groupId>org.example</groupId><artifactId>dep</artifactId><version>3.0.0</version><scope>test</scope></dependency></dependencies>
  <build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version><goals><goal>compile</goal></goals></plugin></plugins></build>
  <profiles><profile><id>linux</id><activation><os><name>Linux</name></os></activation></profile></profiles>
  <scm><url>https://example.invalid/sample</url></scm>
</project>"#;

    for (family, native) in [
        (ProviderFamily::PyPI, pypi),
        (ProviderFamily::SwiftPM, swiftpm.as_str()),
        (ProviderFamily::Maven, maven),
    ] {
        let report = normalize_provider_document(family, native);
        report
            .validate()
            .unwrap_or_else(|error| panic!("lossless provider report: {error}"));
        let shared = report.shared_facts();
        assert_eq!(shared.native_document, native);
        assert!(shared
            .explain_lines()
            .iter()
            .any(|line| line.contains("native") && line.contains("retained")));

        let exported = ProviderFacts::from_json(&report.export_json())
            .expect("provider export uses the shared carrier");
        assert_eq!(exported, shared);

        let lock = report
            .lock_record("app", &shared.reference, "x86_64-linux")
            .expect("provider lock uses the shared carrier");
        let locked = ProviderFacts::from_json(
            lock.future_fields
                .get("provider-facts")
                .expect("provider facts in lock"),
        )
        .expect("locked provider facts JSON");
        assert_eq!(locked, shared);
        let digest = shared.digest();
        assert_eq!(
            lock.future_fields.get("provider-facts-digest"),
            Some(&digest)
        );
    }
}

#[test]
fn provider_conformance_reports_loss_and_conflict_without_defaults() {
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let missing_version = normalize_provider_document(
        ProviderFamily::PyPI,
        r#"{"info":{"name":"sample-pkg"},"urls":[]}"#,
    );
    assert!(missing_version
        .losses
        .iter()
        .any(|loss| loss.contains("exact version")));
    assert!(missing_version.validate().is_err());
    assert!(missing_version
        .shared_facts()
        .losses
        .iter()
        .any(|loss| loss.reason.contains("exact version")));

    let branch_only = normalize_provider_document(
        ProviderFamily::SwiftPM,
        r#"{"version":1,"pins":[{"package":"swift-log","state":{"branch":"main"}}]}"#,
    );
    assert!(branch_only
        .losses
        .iter()
        .any(|loss| loss.contains("exact revision")));
    assert!(branch_only.validate().is_err());

    let conflicting_pom = r#"<project><groupId>com.example</groupId><artifactId>sample</artifactId><version>1.0.0</version><version>2.0.0</version></project>"#;
    let conflict = normalize_provider_document(ProviderFamily::Maven, conflicting_pom);
    assert!(conflict
        .conflicts
        .iter()
        .any(|finding| finding.contains("conflicting version")));
    assert!(conflict.validate().is_err());

    let unsupported_xml = normalize_provider_document(
        ProviderFamily::Maven,
        r#"<m:project xmlns:m="urn:example"><m:artifactId>sample</m:artifactId></m:project>"#,
    );
    assert!(unsupported_xml
        .losses
        .iter()
        .any(|loss| loss.contains("namespaced XML")));
    assert!(unsupported_xml.validate().is_err());
}

#[test]
fn enter_dash_p_adds_adhoc_package_with_no_manifest_at_all() {
    // U16: `jet env -p <pkg>... -- cmd` needs no env.jet/package.jet at all — the
    // ad-hoc package becomes an ordinary nixpkgs RefSpec, folded into an
    // otherwise-empty plan, trust-gated and realized exactly like a
    // manifest-declared ref.
    let root = Scratch::new("dashp-root");
    let proj = Scratch::new("dashp-proj");
    let fixtures = Scratch::new("dashp-fx");
    let out = Scratch::new("dashp-out");
    write_runnable_fixture(&fixtures.path, &root.path, &out.path);
    let output = jetpack()
        .args(["env", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures.path)
        .args(["-p", "greet", "--", "greet"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jetpack"
    );
}

#[test]
fn enter_dash_p_merges_with_project_declared_packages() {
    // The project's own declared package (`hello`, a `core` ref) and the
    // ad-hoc `-p greet` (nixpkgs) both land on PATH in the same shell.
    let (base, proj, root) = core_hello_project("dashp-merge");
    let fixtures = base.join("fixtures");
    let out = base.join("greet-out");
    write_runnable_fixture(&fixtures, &root, &out);
    let output = jetpack()
        .args(["env", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures)
        .args(["-p", "greet", "--", "sh", "-c", "hello && greet"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello from jet-pkgs"), "stdout: {stdout}");
    assert!(stdout.contains("hello from jetpack"), "stdout: {stdout}");
}

#[test]
fn enter_without_env_jet_or_packages_is_still_nothing_to_do() {
    // The pre-U16 refusal is unchanged when there is truly nothing: no
    // env.jet and no `-p`.
    let root = Scratch::new("nothing-root");
    let proj = Scratch::new("nothing-proj");
    let output = jetpack()
        .args(["env", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nothing to do"), "stderr: {stderr}");
}

#[test]
fn enter_flake_detection_ordering_project_env_wins_without_flag() {
    // U16's ordering rule: a project that declares `env.*` (here the
    // Phase-1 directive surface) is never silently swapped for a foreign
    // flake.nix, even when one is present — only `--flake` forces it. Proven
    // here by an offline realize of the *declared* `hello` package
    // succeeding with no `nix` on PATH and no flake.nix ever being touched
    // (a bad flake.nix would fail loudly if `nix develop` ran against it).
    let (base, proj, root) = core_hello_project("flake-ordering");
    fs::write(proj.join("flake.nix"), "this is not valid nix").unwrap();
    let output = jetpack()
        .args(["env", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn enter_flake_flag_requires_trust_before_native_projection() {
    // `--flake` forces the foreign-flake projection even though the project
    // declares `env.*`; the trust boundary runs before native evaluation.
    let (base, proj, root) = core_hello_project("flake-forced");
    fs::write(proj.join("flake.nix"), "{ }").unwrap();
    let output = jetpack()
        .args(["env", "--no-color", "--flake"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1255"), "stderr: {stderr}");
    assert!(stderr.contains("--trust"), "stderr: {stderr}");
    assert!(!stderr.contains("E1256"), "stderr: {stderr}");
}

#[test]
fn enter_flake_native_projection_runs_without_nix_on_path() {
    // U16 product proof: `enter --flake --trust` uses the bounded native
    // evaluator with an empty PATH, so no installed Nix executable can be
    // discovered by the production path.
    let project = Scratch::new("flake-native-enter");
    fs::write(
        project.join("flake.nix"),
        "{ devShells.x86_64-linux.default = { }; }",
    )
    .unwrap();
    let output = jetpack()
        .args([
            "enter",
            "--flake",
            "--trust",
            "--no-color",
            "--",
            "/bin/sh",
            "-c",
            "printf native-flake",
        ])
        .current_dir(&project.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "native-flake"
    );
}

#[test]
fn bridge_flake_projects_imported_flake_parts_module_and_preserves_last_lock_on_failure() {
    let project = Scratch::new("flake-parts-bridge");
    fs::create_dir_all(project.join("parts")).unwrap();
    fs::write(
        project.join("flake.nix"),
        r#"
let marker = "flake-parts mkFlake"; in {
  imports = [ ./parts/dev.nix ];
  systems = [ "x86_64-linux" ];
  perSystem = true;
  outputs = import ./parts/dev.nix;
}
"#,
    )
    .unwrap();
    fs::write(
        project.join("parts/dev.nix"),
        "{ devShells.x86_64-linux.default = { packages = [ pkgs.fd ]; }; }\n",
    )
    .unwrap();

    let first = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&project.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        String::from_utf8_lossy(&first.stdout).contains("packages: [fd]"),
        "stdout: {}",
        String::from_utf8_lossy(&first.stdout)
    );
    let lock_path = project.join(".jet/lock");
    let before = fs::read(&lock_path).expect("successful bridge must commit a semantic lock");
    let lock_text = String::from_utf8(before.clone()).unwrap();
    assert!(
        lock_text.contains("flake-composition:flake-parts"),
        "{lock_text}"
    );
    assert!(lock_text.contains("./parts/dev.nix"), "{lock_text}");
    let previous = jetpack::SemanticLock::parse(&lock_text);
    let previous_graph =
        jetpack::SemanticLock::FlakeGraph::from_semantic_lock("flake.nix", &previous)
            .expect("the committed imported-module projection must remain usable");
    assert!(previous_graph
        .named_dev_shells()
        .iter()
        .any(|output| output.provenance.contains("./parts/dev.nix")));

    fs::write(
        project.join("parts/dev.nix"),
        "{ devShells.x86_64-linux.default = { packages = pkgs.lib.optionals true [ pkgs.fd ]; }; }\n",
    )
    .unwrap();
    let failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&project.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
    assert_eq!(fs::read(&lock_path).unwrap(), before);
}

#[test]
fn enter_flake_dynamic_projection_reports_e1256_without_nix() {
    let project = Scratch::new("flake-dynamic-enter");
    fs::write(
        project.join("flake.nix"),
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = pkgs.lib.optionals true [ pkgs.fd ]; }; }",
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "--flake", "--trust", "--no-color"])
        .current_dir(&project.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
}

#[test]
fn enter_flake_with_no_foreign_flake_present_is_friendly() {
    let root = Scratch::new("flake-none-root");
    let proj = Scratch::new("flake-none-proj");
    let output = jetpack()
        .args(["env", "--no-color", "--flake"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no foreign flake"), "stderr: {stderr}");
}

#[test]
fn retired_profile_flag_teaches_preset() {
    let project = Scratch::new("retired-profile-flag");
    fs::write(
        project.join("env.jet"),
        "module env.dev {\n    packages: [nixpkgs.ripgrep]\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "info", "--profile", "work", "--no-color"])
        .current_dir(&project.path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1300"), "stderr: {stderr}");
    assert!(stderr.contains("--preset"), "stderr: {stderr}");
    assert!(
        !output.status.success(),
        "the retired spelling must not select anything"
    );
}

#[test]
fn retired_environment_flag_teaches_env() {
    let project = Scratch::new("retired-environment-flag");
    fs::write(
        project.join("env.jet"),
        "module env.dev {\n    packages: [nixpkgs.ripgrep]\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "info", "--env-profile", "full", "--no-color"])
        .current_dir(&project.path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1342"), "stderr: {stderr}");
    assert!(stderr.contains("`--env <name>`"), "stderr: {stderr}");
    assert!(
        !output.status.success(),
        "the retired spelling must not select an environment module"
    );
}

#[test]
fn env_info_json_discloses_selected_preset_and_language_projection() {
    let project = Scratch::new("env-info-composition");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    presets: {
        host: { hostname: "epoch5-host", variables: [String:String]{ "MODE": "dev" } }
        user: { user: "epoch5-user" }
    }
    services: {
        redis: { enable: true, ports: [6379], after: ["database"] }
    }
    languages: [
        "rust": Lang{ enable: true, channel: .Stable },
        "zig": Lang{ enable: true }
    ]
    packages: [nixpkgs.ripgrep]
}
module env.full {
    packages: [nixpkgs.fd]
}
"#,
    )
    .unwrap();
    fs::write(project.join("run.jet"), "#Job\nfn lint() {}\n").unwrap();
    let output = jetpack()
        .args(["env", "info", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("HOSTNAME", "epoch5-host")
        .env("USER", "epoch5-user")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"preset\":\"host+user\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"selected_presets\":[\"host\",\"user\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"active_environment\":\"dev\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"active_environment_provenance\":[\"env.dev\"]"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"language_catalog\""), "stdout: {stdout}");
    assert!(stdout.contains("\"fingerprint\":\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"language_projections\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"host\":\"native\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"platform\":\"x86_64-linux\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"license\":\"Apache-2.0 OR MIT\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"missing_tools\":[]"), "stdout: {stdout}");
    assert!(stdout.contains("\"included\""), "stdout: {stdout}");
    assert!(stdout.contains("\"omitted\""), "stdout: {stdout}");
    assert!(stdout.contains("\"name\":\"Zig\""), "stdout: {stdout}");
    assert!(stdout.contains("\"zig@nixpkgs\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"services\":[{\"name\":\"redis\""),
        "stdout: {stdout}"
    );
    // D-JOB-NAME2=B (card #1448): the merged `tasks` key is gone. `lint` is the
    // project's own `#Job fn`, so it reports under `jobs`; this env declares no
    // `checks:` hook records, so the environment's key is present but empty.
    // Both keys always appear, so a consumer reads the one it means.
    assert!(
        stdout.contains("\"checks\":[],\"jobs\":[\"lint\"]"),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("\"tasks\":[\"lint\"]"), "stdout: {stdout}");
    assert!(
        stdout.contains("\"variables\":[{\"name\":\"MODE\""),
        "stdout: {stdout}"
    );

    let full = jetpack()
        .args(["env", "info", "--json", "--no-color", "--env", "full"])
        .current_dir(&project.path)
        .env("HOSTNAME", "epoch5-host")
        .env("USER", "epoch5-user")
        .output()
        .unwrap();
    assert!(
        full.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&full.stderr)
    );
    let full_stdout = String::from_utf8_lossy(&full.stdout);
    assert!(
        full_stdout.contains("\"active_environment\":\"full\""),
        "stdout: {full_stdout}"
    );
    assert!(
        full_stdout.contains("\"active_environment_provenance\":[\"env.full\"]"),
        "stdout: {full_stdout}"
    );
    assert!(
        full_stdout.contains("\"fd@nixpkgs\""),
        "stdout: {full_stdout}"
    );
    assert!(
        !full_stdout.contains("\"ripgrep@nixpkgs\""),
        "stdout: {full_stdout}"
    );

    let missing = jetpack()
        .args(["env", "info", "--no-color", "--env", "missing"])
        .current_dir(&project.path)
        .env("HOSTNAME", "epoch5-host")
        .env("USER", "epoch5-user")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_stderr.contains("E1337"), "stderr: {missing_stderr}");
    assert!(
        missing_stderr.contains("environment module `missing` is not declared"),
        "stderr: {missing_stderr}"
    );
}

#[test]
fn env_info_json_discloses_reads_and_typed_service_facts_without_starting_processes() {
    let project = Scratch::new("env-info-facts");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    prompt: $HOME
    services: {
        fixture: {
            enable: false,
            ports: [8080],
            run: ["fixture", "--port", "8080"],
            ready: "fixture --ready",
            data_dir: "state/fixture",
            watch: ["src"],
            after: ["database"],
            before_start: ["lint"],
            sockets: ["run/fixture.sock"]
        }
    }
    files: ["config/generated.txt": File{ content: "generated\n", mode: .Copy }]
}
"#,
    )
    .unwrap();
    fs::write(project.join("run.jet"), "#Job\nfn lint() {}\n").unwrap();

    let output = jetpack()
        .args(["env", "info", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("HOME", "/test/home")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        jetpack::JSON::parse(&stdout).is_ok(),
        "info JSON must parse: {stdout}"
    );
    assert!(
        stdout.contains("\"variables\":[{\"name\":\"HOME\",\"sources\":[\"environment\"]}"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"name\":\"fixture\",\"enabled\":false"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"run\":[\"fixture\",\"--port\",\"8080\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"ready\":\"fixture --ready\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"data_dir\":\"state/fixture\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"watch\":[\"src\"]"), "stdout: {stdout}");
    assert!(
        stdout.contains("\"after\":[\"database\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"before_start\":[\"lint\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"sockets\":[\"run/fixture.sock\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"files\":[\"config/generated.txt\"]"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"jobs\":[\"lint\"]"), "stdout: {stdout}");
    assert!(
        !project.join(".jet/services").exists(),
        "info must not start services"
    );
}

#[test]
fn env_sync_applies_typed_files_and_refuses_unmanaged_destinations() {
    let project = Scratch::new("env-sync-files");
    let root = Scratch::new("env-sync-files-root");
    let home = Scratch::new("env-sync-files-home");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    files: [
        "generated/config.txt": File{ content: "generated\n", mode: .Copy },
        "seed/config.txt": File{ content: "seeded\n", mode: .Seed }
    ]
}
"#,
    )
    .unwrap();
    fs::create_dir_all(project.join("seed")).unwrap();
    fs::write(project.join("seed/config.txt"), "keep me\n").unwrap();

    let output = jetpack()
        .args([
            "enter",
            "sync",
            "--trust",
            "--yes",
            "--offline",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("generated/config.txt")).unwrap(),
        "generated\n"
    );
    assert_eq!(
        fs::read_to_string(project.join("seed/config.txt")).unwrap(),
        "keep me\n"
    );
    assert!(project.join(".jet/files/state").is_file());

    let blocked = Scratch::new("env-sync-files-blocked");
    let blocked_root = Scratch::new("env-sync-files-blocked-root");
    let blocked_home = Scratch::new("env-sync-files-blocked-home");
    fs::write(
        blocked.join("env.jet"),
        fs::read(project.join("env.jet")).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(blocked.join("generated")).unwrap();
    fs::write(blocked.join("generated/config.txt"), "user-owned\n").unwrap();
    let refused = jetpack()
        .args([
            "enter",
            "sync",
            "--trust",
            "--yes",
            "--offline",
            "--no-color",
        ])
        .current_dir(&blocked.path)
        .env("JETPACK_ROOT", &blocked_root.path)
        .env("HOME", &blocked_home.path)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("unmanaged destination"),
        "stderr: {}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_eq!(
        fs::read_to_string(blocked.join("generated/config.txt")).unwrap(),
        "user-owned\n"
    );
    assert!(!blocked.join(".jet/files/state").exists());
}

#[test]
fn env_info_json_discloses_typed_integration_projection() {
    let project = Scratch::new("env-info-integrations");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    imports: [
        env.platform.android(api: 35, build_tools: "35.0.0", ndk: "27.1"),
        env.platform.apple(targets: [.IOS]),
        env.security.certificates([dev_certificate]),
        env.cloud.credentials([aws_production]),
        env.security.vault([database_password]),
        env.network.hosts(["api.local": "127.0.0.1"]),
        env.agent.codex(mcp: [repo_server]),
        env.editor.vscode()
    ]
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "info", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JET_TARGET", "x86_64-linux-darwin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"integrations\":["), "stdout: {stdout}");
    assert!(stdout.contains("\"kind\":\"android\""), "stdout: {stdout}");
    assert!(stdout.contains("\"kind\":\"apple\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"kind\":\"certificates\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"kind\":\"cloud-credentials\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"kind\":\"vault\""), "stdout: {stdout}");
    assert!(stdout.contains("\"kind\":\"hosts\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"kind\":\"codex-agent\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"kind\":\"editor\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"option_keys\":[\"api\",\"build_tools\",\"license\",\"ndk\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"option_keys\":[\"license\",\"targets\"]"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"host_checks\":["), "stdout: {stdout}");
    assert!(stdout.contains("\"grants\":["), "stdout: {stdout}");
    assert!(
        stdout.contains("\"secrets\":[\"dev_certificate\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"grants\":[\"certificate.read\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"secrets\":[\"aws_production\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"grants\":[\"credential.read\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"secrets\":[\"database_password\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"grants\":[\"vault.read\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"grants\":[\"mcp.read\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"packages\":[\"vscode@nixpkgs\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"option_keys\":[\"host.api.local\"]"),
        "stdout: {stdout}"
    );
}

#[test]
fn enter_requires_a_persisted_cloud_integration_grant() {
    let project = Scratch::new("cloud-integration-grant");
    let root = Scratch::new("cloud-integration-grant-root");
    let home = Scratch::new("cloud-integration-grant-home");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    imports: [env.cloud.credentials([aws_production])]
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "--trust", "--offline", "--no-color", "--", "true"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "stderr: {stderr}");
    assert!(
        stderr.contains("cloud-credentials:credential.read"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("aws_production"),
        "secret name leaked: {stderr}"
    );
}

#[test]
fn enter_requires_a_persisted_vault_integration_grant() {
    let project = Scratch::new("vault-integration-grant");
    let root = Scratch::new("vault-integration-grant-root");
    let home = Scratch::new("vault-integration-grant-home");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    imports: [env.security.vault([database_password])]
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "--trust", "--offline", "--no-color", "--", "true"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "stderr: {stderr}");
    assert!(stderr.contains("vault:vault.read"), "stderr: {stderr}");
    assert!(
        !stderr.contains("database_password"),
        "secret name leaked: {stderr}"
    );
}

#[test]
fn env_info_rejects_unredactable_integration_secret() {
    let project = Scratch::new("env-info-integration-secret");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    imports: [env.cloud.credentials("super_secret_value")]
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "info", "--no-color"])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "stderr: {stderr}");
    assert!(!stderr.contains("super_secret_value"), "stderr: {stderr}");
}

#[test]
fn env_info_rejects_unsupported_apple_integration_target() {
    let project = Scratch::new("env-info-apple-target");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    imports: [env.platform.apple(targets: [.IOS])]
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["env", "info", "--no-color"])
        .current_dir(&project.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1335"), "stderr: {stderr}");
    assert!(stderr.contains("apple integration"), "stderr: {stderr}");
}

#[test]
fn bridge_flake_uses_native_evaluator_without_nix() {
    let dir = Scratch::new("bridge-nonix");
    fs::write(
        dir.join("flake.nix"),
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.fd ]; }; }",
    )
    .unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "") // no Nix or other evaluator executable on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("packages: [fd]"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let lock_text = fs::read_to_string(dir.join(".jet/lock"))
        .expect("native bridge must commit the pinned evaluator ledger");
    let lock = jetpack::SemanticLock::parse(&lock_text);
    let inventory = lock
        .records
        .iter()
        .filter(|record| {
            record
                .identity
                .key
                .starts_with("flake-evaluator-inventory:")
        })
        .collect::<Vec<_>>();
    assert_eq!(inventory.len(), 17, "{lock_text}");
    assert_eq!(
        inventory
            .iter()
            .filter(|record| record.identity.exact.contains("class=evaluable"))
            .count(),
        10
    );
    assert_eq!(
        inventory
            .iter()
            .filter(|record| record.identity.exact.contains("class=buildable"))
            .count(),
        5
    );
    let skipped = inventory
        .iter()
        .filter(|record| record.identity.exact.starts_with("status=skipped;"))
        .collect::<Vec<_>>();
    assert_eq!(skipped.len(), 2, "{lock_text}");
    assert!(skipped.iter().any(|record| {
        record.identity.key.ends_with(":dynamic-derivations")
            && record.identity.exact
                == "status=skipped;class=skipped;reason=dynamic staging has a separate compatibility boundary"
    }));
    assert!(skipped.iter().any(|record| {
        record.identity.key.ends_with(":ifd")
            && record.identity.exact
                == "status=skipped;class=skipped;reason=import-from-derivation requires a separate authority grant"
    }));
    for (surface, class, reason) in [
        (
            "fixed-output-fetchers",
            "buildable",
            "explicit fetch authority returns verified canonical store paths",
        ),
        (
            "cross-system-packages",
            "buildable",
            "explicit target authority projects pinned systems",
        ),
        (
            "external-flakes",
            "evaluable",
            "explicit provider authority evaluates bounded external sources",
        ),
    ] {
        let key = format!("flake-evaluator-inventory:{surface}");
        let exact = format!("status=covered;class={class};reason={reason}");
        assert!(
            inventory
                .iter()
                .any(|record| record.identity.key == key && record.identity.exact == exact),
            "missing inventory record {key}: {lock_text}"
        );
    }
    assert!(lock.records.iter().any(|record| {
        record.identity.key == "flake-evaluator"
            && record.identity.exact
                == "native-nix:2.34.8:b5aa0fbd538984f6e3d201be0005b4463d8b09f8:x86_64-linux"
    }));

    let before_failure = lock_text;
    fs::write(dir.join("flake.nix"), " ".repeat((1 << 20) + 1)).unwrap();
    let failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
    assert!(!stderr.contains("nix eval"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(dir.join(".jet/lock")).unwrap(),
        before_failure
    );
}

#[test]
fn bridge_flake_breadth_and_json_budget_use_native_evaluator_without_nix() {
    let dir = Scratch::new("bridge-native-breadth");
    fs::write(
        dir.join("flake.nix"),
        r#"{ devShells.x86_64-linux.default = pkgs.mkShell {
  packages = builtins.attrValues (builtins.fromJSON "{\"b\":\"ripgrep\",\"a\":\"fd\"}");
}; }"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("packages: [fd, ripgrep]"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let lock_before_budget_failure = fs::read_to_string(dir.join(".jet/lock"))
        .expect("native breadth bridge must commit a lock before the failure case");

    let nested_json = format!("{}true{}", "[".repeat(257), "]".repeat(257));
    let over_budget = format!(
        "{{ devShells.x86_64-linux.default = pkgs.mkShell {{ packages = builtins.fromJSON \"{nested_json}\"; }}; }}"
    );
    fs::write(dir.join("flake.nix"), over_budget).unwrap();
    let failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
    assert!(
        stderr.contains("JSON value is too deeply nested"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("nix eval"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(dir.join(".jet/lock")).unwrap(),
        lock_before_budget_failure
    );
}

#[test]
fn bridge_flake_projects_fetchers_cross_packages_and_external_flakes_without_nix() {
    let dir = Scratch::new("bridge-native-nix-breadth");
    fs::create_dir_all(dir.join("dep")).unwrap();
    fs::write(
        dir.join("dep/flake.nix"),
        "{ packages.x86_64-linux.default = pkgs.fd; }",
    )
    .unwrap();
    let source_bytes = b"fixed-output\n";
    fs::write(dir.join("source.txt"), source_bytes).unwrap();
    let source_hash = jetpack::SHA256::sha256_hex(source_bytes);
    let source = format!(
        r#"{{
  devShells.x86_64-linux.default = pkgs.mkShell {{
    packages = [
      (builtins.getFlake "path:./dep").packages.x86_64-linux.default
      pkgs.pkgsCross.aarch64-multiplatform.foo
    ];
  }};
  packages.x86_64-linux.fetched = builtins.derivationStrict {{
    name = "fetched";
    system = "x86_64-linux";
    builder = "/bin/sh";
    args = [ "-c" "echo $src > $out" ];
    src = builtins.fetchurl {{
      url = "file:./source.txt";
      sha256 = "{source_hash}";
      name = "source.txt";
    }};
  }};
}}
"#
    );
    fs::write(dir.join("flake.nix"), source).unwrap();

    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("packages: [fd, fetched]"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cross-package:aarch64-linux/foo"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("nix eval"), "stderr: {stderr}");

    let lock_path = dir.join(".jet/lock");
    let lock_before_failure = fs::read_to_string(&lock_path).expect("breadth bridge lock");
    assert!(
        lock_before_failure.contains("flake-devshell:devShells:x86_64-linux:default")
            && lock_before_failure
                .contains("packages=fd;unsupported=cross-package:aarch64-linux/foo"),
        "lock: {lock_before_failure}"
    );
    assert!(
        lock_before_failure.contains("flake-derivation:packages:x86_64-linux:fetched"),
        "lock: {lock_before_failure}"
    );

    fs::write(dir.join("source.txt"), b"tampered\n").unwrap();
    let fetch_failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(fetch_failed.status.code(), Some(1));
    let fetch_failure_stderr = String::from_utf8_lossy(&fetch_failed.stderr);
    assert!(
        fetch_failure_stderr.contains("E1256"),
        "stderr: {fetch_failure_stderr}"
    );
    assert!(
        fetch_failure_stderr.contains("verified fetch hash mismatch"),
        "stderr: {fetch_failure_stderr}"
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), lock_before_failure);

    fs::write(
        dir.join("flake.nix"),
        r#"{ devShells.x86_64-linux.default = pkgs.mkShell {
  packages = [ (builtins.getFlake "github:NixOS/nixpkgs").packages.x86_64-linux.default ];
}; }"#,
    )
    .unwrap();
    let failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let failure_stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(failure_stderr.contains("E1256"), "stderr: {failure_stderr}");
    assert!(
        failure_stderr.contains("provider authority"),
        "stderr: {failure_stderr}"
    );
    assert!(
        !failure_stderr.contains("nix eval"),
        "stderr: {failure_stderr}"
    );
    assert_eq!(fs::read_to_string(lock_path).unwrap(), lock_before_failure);
}

#[test]
fn bridge_flake_projects_overlays_named_devshells_and_all_outputs_without_nix() {
    let dir = Scratch::new("bridge-native-evaluator-breadth");
    fs::write(
        dir.join("flake.nix"),
        r#"{
  packages.x86_64-linux.default = builtins.derivation {
    name = "many";
    system = "x86_64-linux";
    builder = "/bin/sh";
    outputs = [ "out" "dev" "doc" ];
  };
  devShells.x86_64-linux.default =
    (import pkgs { overlays = [ (final: prev: { custom = prev.fd; }) ]; }).mkShell {
      packages = [ (import pkgs { overlays = [ (final: prev: { custom = prev.fd; }) ]; }).custom ];
    };
  devShells.x86_64-linux.debug = {
    packages = [ pkgs.ripgrep ];
    inputsFrom = [];
    shellHook = "export DEBUG=1";
  };
}
"#,
    )
    .unwrap();

    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("packages: [fd]"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("L0204"), "stderr: {stderr}");
    assert!(stderr.contains("inputsFrom"), "stderr: {stderr}");
    let lock = fs::read_to_string(dir.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("flake-devshell:devShells:x86_64-linux:debug"),
        "lock: {lock}"
    );
    assert!(
        lock.contains("packages=ripgrep;unsupported=inputsFrom,shellHook"),
        "lock: {lock}"
    );
    assert!(
        lock.contains("flake-derivation:packages:x86_64-linux:default"),
        "lock: {lock}"
    );
    assert!(
        lock.contains("drvPath=/nix/store/iqjpmbf780gw1gqzhcvarkhw6h9y4c98-many.drv;dev=/nix/store/0cmdcz6gdcx6k0wf2ir78dh5isib04w3-many-dev;doc=/nix/store/v5m6bfihjhlp2ggsr9zcgw1j4wz1z1az-many-doc;out=/nix/store/3d8xbsqfn74lgzz2x82y3hh8i9mmx7xy-many"),
        "lock: {lock}"
    );

    let overlay_list = std::iter::repeat("overlay")
        .take(65)
        .collect::<Vec<_>>()
        .join(" ");
    let over_budget = format!(
        "let overlay = final: prev: {{}}; in {{ devShells.x86_64-linux.default = (import pkgs {{ overlays = [ {overlay_list} ]; }}).mkShell {{ packages = []; }}; }}"
    );
    fs::write(dir.join("flake.nix"), over_budget).unwrap();
    let failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let failure_stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(failure_stderr.contains("E1256"), "stderr: {failure_stderr}");
    assert!(
        failure_stderr.contains("overlay list exceeds 64"),
        "stderr: {failure_stderr}"
    );
    assert_eq!(fs::read_to_string(dir.join(".jet/lock")).unwrap(), lock);
}

#[test]
fn bridge_flake_native_commits_losses_and_preserves_lock_on_failure() {
    let dir = Scratch::new("bridge-native-loss-lock");
    fs::write(
        dir.join("flake.nix"),
        r#"{
  devShells.x86_64-linux.default = {
    packages = [ pkgs.fd ];
    shellHook = "export FOO=1";
  };
}
"#,
    )
    .unwrap();
    let first = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        String::from_utf8_lossy(&first.stdout).contains("packages: [fd]"),
        "stdout: {}",
        String::from_utf8_lossy(&first.stdout)
    );
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        stderr.contains("L0204"),
        "native loss was not disclosed: {stderr}"
    );
    assert!(
        stderr.contains("shellHook"),
        "native loss name missing: {stderr}"
    );
    let lock_path = dir.join(".jet/lock");
    let before = fs::read(&lock_path).expect("native bridge must commit its lock");
    let lock_text = String::from_utf8_lossy(&before);
    assert!(
        lock_text.contains("shellHook"),
        "lock lost native loss fact: {lock_text}"
    );

    fs::write(
        dir.join("flake.nix"),
        "{ devShells.x86_64-linux.default = { packages = pkgs.lib.optionals true [ pkgs.fd ]; }; }\n",
    )
    .unwrap();
    let failed = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("E1256"));
    assert_eq!(fs::read(&lock_path).unwrap(), before);
}

#[test]
fn bridge_flake_commits_transitive_locked_registry_facts_without_nix() {
    let dir = Scratch::new("bridge-locked-flake");
    fs::write(
        dir.join("flake.nix"),
        r#"{
  inputs.tools.url = "github:example/tools?rev=0123456789abcdef0123456789abcdef01234567";
  devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.fd ]; };
}
"#,
    )
    .unwrap();
    let flake_lock = r#"{
  "nodes": {
    "root": { "inputs": { "tools": "tools" } },
    "tools": {
      "inputs": { "nixpkgs": "nixpkgs" },
      "locked": { "owner": "example", "repo": "tools", "rev": "0123456789abcdef0123456789abcdef01234567", "type": "github" },
      "original": { "owner": "example", "repo": "tools", "type": "github" }
    },
    "nixpkgs": {
      "locked": { "owner": "NixOS", "repo": "nixpkgs", "rev": "89abcdef0123456789abcdef0123456789abcdef", "type": "github" },
      "original": { "id": "nixpkgs", "type": "indirect" }
    }
  },
  "root": "root",
  "version": 7
}
"#;
    fs::write(dir.join("flake.lock"), flake_lock).unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("packages: [fd]"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let semantic = fs::read_to_string(dir.join(".jet/lock")).unwrap();
    let lock = jetpack::SemanticLock::parse(&semantic);
    let edge = lock
        .records
        .iter()
        .find(|record| record.identity.key == "flake-lock-node:tools")
        .expect("tools lock node record");
    assert_eq!(
        edge.identity.exact,
        r#"{"inputs":[{"name":"nixpkgs","target":"nixpkgs"}],"locked":{"owner":"example","repo":"tools","rev":"0123456789abcdef0123456789abcdef01234567","type":"github"},"name":"tools","original":{"owner":"example","repo":"tools","type":"github"}}"#
    );
    let registry = lock
        .records
        .iter()
        .find(|record| record.identity.key == "flake-registry:nixpkgs")
        .expect("nixpkgs registry record");
    assert_eq!(
        registry.identity.exact,
        r#"{"alias":"nixpkgs","locked":{"owner":"NixOS","repo":"nixpkgs","rev":"89abcdef0123456789abcdef0123456789abcdef","type":"github"},"node":"nixpkgs","original":{"id":"nixpkgs","type":"indirect"}}"#
    );

    let replay = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(
        replay.status.code(),
        Some(0),
        "replay stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(replay.stdout, output.stdout);
    assert_eq!(fs::read_to_string(dir.join(".jet/lock")).unwrap(), semantic);

    let wrong_lock = flake_lock.replace(
        "\"original\": { \"owner\": \"example\", \"repo\": \"tools\", \"type\": \"github\" }",
        "\"original\": { \"owner\": \"wrong\", \"repo\": \"repo\", \"type\": \"github\" }",
    );
    fs::remove_file(dir.join(".jet/lock")).unwrap();
    fs::write(dir.join("flake.lock"), wrong_lock).unwrap();
    let failure = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let failure_stderr = String::from_utf8(failure.stderr).unwrap();
    assert_eq!(failure.status.code(), Some(1), "{failure_stderr}");
    assert!(
        failure_stderr.contains("Error [E1340]: couldn't load the foreign graph"),
        "{failure_stderr}"
    );
    assert!(
        failure_stderr.contains(
            "flake.lock root input `tools` maps to node `tools` with a different source URL or indirect alias"
        ),
        "{failure_stderr}"
    );
}

#[test]
fn bridge_flake_rejects_dynamic_native_evaluator_input() {
    let dir = Scratch::new("bridge-native-unsupported");
    fs::write(
        dir.join("flake.nix"),
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = pkgs.lib.optionals true [ pkgs.fd ]; }; }",
    )
    .unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("literal package list"), "stderr: {stderr}");
}

#[test]
fn bridge_flake_prints_shim_and_warns_on_unmapped_shell_hook() {
    // The bounded translation: buildInputs become a plain env.dev
    // packages list on stdout; a non-empty shellHook (no env.* equivalent)
    // fires L0204 on stderr without blocking the print.
    let dir = Scratch::new("bridge-shim");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let fixtures = Scratch::new("bridge-shim-fx");
    fs::write(
        fixtures.join("flake-devshell.json"),
        r#"{"buildInputs": ["ripgrep", "fd"], "shellHook": "export FOO=1", "fixtureOnly": true}"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("module env.dev {"), "stdout: {stdout}");
    assert!(
        stdout.contains("packages: [fd, ripgrep]"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("L0204"), "stderr: {stderr}");
    assert!(stderr.contains("shellHook"), "stderr: {stderr}");
    assert!(stderr.contains("fixtureOnly"), "stderr: {stderr}");
    let lock = fs::read_to_string(dir.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("flake-unsupported:fixtureOnly"),
        "fixture-only loss was not persisted: {lock}"
    );
}

#[test]
fn bridge_flake_twice_produces_identical_shim_stdout() {
    // Drift-check (U16 plan doc): the bridge is a pure function of the
    // flake's facts, so two runs against the same fixture print
    // byte-identical shims.
    let dir = Scratch::new("bridge-drift");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let fixtures = Scratch::new("bridge-drift-fx");
    fs::write(
        fixtures.join("flake-devshell.json"),
        r#"{"buildInputs": ["nodejs", "ripgrep"], "shellHook": ""}"#,
    )
    .unwrap();
    let run = || {
        jetpack()
            .args(["bridge", "flake", "--no-color", "--fixtures"])
            .arg(&fixtures.path)
            .current_dir(&dir.path)
            .output()
            .unwrap()
    };
    let a = run();
    let b = run();
    assert!(a.status.success());
    assert!(b.status.success());
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn bridge_flake_no_flake_nix_here_is_friendly() {
    let dir = Scratch::new("bridge-noflake");
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no foreign flake"), "stderr: {stderr}");
}

#[test]
fn core_provider_runs_first_party_package_without_nix() {
    // R2/U10: a `core` named source realizes a first-party Jet package with no
    // nix anywhere. Package is discovered by module name — no env.jet index.
    let base = Scratch::new("core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The project declares a `core` named source pointing at the local repo.
    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() [JSON] {{\n    return [\n        pkg.source(\"mine\", \"{}\", \"core\");\n        pkg.packages([\"hello@mine\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn typed_core_source_inferred_from_pack_jet() {
    // U9/D-JPK-REF1: a typed module declares a local source as a bare path.
    // with no provider marker. The kind is *inferred* from `package.jet` in the
    // target → realizes through the first-party `core` provider. U10 Chunk 3:
    // the package is discovered by module name — `module hello` in the source tree
    // — with no `env.jet` index. No nix on PATH proves no nix is involved.
    let base = Scratch::new("typed-core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    // `package.jet` is both the U9 probe marker and the U10 package index.
    fs::write(
        repo.join("package.jet"),
        "name: \"jet-pkgs\"\nversion: \"0.1.0\"\npackages: {\n    hello: executable,\n}\n",
    )
    .unwrap();
    // The `module hello` declaration is the U10 Chunk 3 discovery target — no
    // `env.jet` pkg.package index needed anymore (dual marker retired).
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The typed env declares the source with no `via`/`core` marker — just
    // `target@provider`. `mine.hello` is the Pkg sugar → `hello@mine`.
    fs::write(
        proj.join("env.jet"),
        format!(
            "module dev {{\n    sources: {{ mine: {} }}\n    env.dev: Env{{\n        packages: [mine.hello],\n    }}\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn core_provider_builds_library_package_without_nix() {
    // U10 Chunk 4: a `library` package realizes through the `core` provider
    // (no nix), staging its module source. Unlike an `executable`, it puts no
    // `bin/` on PATH — but `jetpack env --prep` realizes it just the same. The kind
    // comes from the repo's `package.jet` `packages:` index.
    let base = Scratch::new("core-library");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let lib_pkg = repo.join("lib/mathlib");
    fs::create_dir_all(&lib_pkg).unwrap();
    fs::create_dir_all(&proj).unwrap();
    // `package.jet` declares the package as a `library` (the kind index).
    fs::write(
        repo.join("package.jet"),
        "name: \"jet-pkgs\"\nversion: \"0.1.0\"\npackages: {\n    mathlib: library,\n}\n",
    )
    .unwrap();
    // The library's source: a `module mathlib` discovered by name (Chunk 3),
    // with no `bin/` — it is imported for its code, not installed on PATH.
    fs::write(
        lib_pkg.join("mathlib.jet"),
        "module mathlib {\n    pub fn add(a: Int, b: Int) Int { return a + b }\n}\n",
    )
    .unwrap();
    // A typed env references the library package; the source kind is inferred
    // from `package.jet` → core.
    fs::write(
        proj.join("env.jet"),
        format!(
            "module dev {{\n    sources: {{ mine: {} }}\n    env.dev: Env{{\n        packages: [mine.mathlib],\n    }}\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("built 1 package(s)"),
        "expected build success status, got: {stderr}"
    );
}

#[test]
fn core_cargo_build_requires_exact_trust_before_running_build_script() {
    let base = Scratch::new("core-cargo-trust");
    let repo = base.join("jet-pkgs");
    let project = base.join("project");
    let root = base.join("root");
    let home = base.join("home");
    let marker = base.join("host-marker");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        repo.join("package.jet"),
        "name: \"jet-pkgs\"\nversion: \"0.1.0\"\npackages: { escape: library }\n",
    )
    .unwrap();
    fs::write(repo.join("escape.jet"), "module escape { }\n").unwrap();
    fs::write(repo.join("lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"escape\"\nversion = \"0.1.0\"\nbuild = \"build.rs\"\n[lib]\npath = \"lib.rs\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\nversion = 3\n\n[[package]]\nname = \"escape\"\nversion = \"0.1.0\"\n\n",
    )
    .unwrap();
    fs::write(
        repo.join("build.rs"),
        format!(
            "fn main() {{ let _ = std::fs::write({:?}, \"escaped\"); }}\n",
            marker.to_string_lossy()
        ),
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() [JSON] {{\n    return [\n        pkg.source(\"mine\", \"{}\", \"core\");\n        pkg.packages([\"escape@mine\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&project)
        .env("HOME", &home)
        .env("JETPACK_ROOT", &root)
        .env("JETPACK_FAKE_SANDBOX", "unavailable")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("E1255"), "stderr: {stderr}");
    assert!(!marker.exists(), "build.rs ran before exact trust approval");
    assert!(jetpack::Store::list(&jetpack::Store::Roots::at(root)).is_empty());
}

#[test]
fn committed_example_builds_offline_end_to_end() {
    // I5: the committed jetpack project fixture is the executable spec for
    // a real env.jet. `jetpack env --prep` with no ref reads env.jet and realizes
    // everything it declares — nix-backed named sources (`ripgrep@stable`,
    // `neovim@unstable`) resolved from the committed fixtures, plus a
    // first-party `hello@mine` realized through the `core` provider with no
    // nix. The whole thing runs fully offline. The store lives under a scratch
    // JETPACK_ROOT, so nothing is written back into the example dir.
    let project = Scratch::new("example-e2e-project");
    copy_dir_recursive(&example_dir(), &project.path);
    let root = Scratch::new("example-e2e");
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("building completed 0/3 · current: stable -> ripgrep · resolving")
            && stderr.contains("building completed 1/3 · current: unstable -> neovim · resolving")
            && stderr.contains("building completed 2/3 · current: mine -> hello · resolving"),
        "plain non-TTY output must preserve ordered source-to-package edges: {stderr}"
    );
    for pkg in ["ripgrep", "neovim", "hello"] {
        assert!(
            stderr.contains(pkg),
            "expected `{pkg}` in build output: {stderr}"
        );
    }
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn use_many_packages_settles_one_row_per_package_without_output_flood() {
    let project = Scratch::new("use-many-packages");
    copy_dir_recursive(&example_dir(), &project.path);
    let root = Scratch::new("use-many-packages-root");
    let mut args = vec![
        "use".to_string(),
        "--prep".to_string(),
        "--offline".to_string(),
        "--no-color".to_string(),
        "-y".to_string(),
    ];
    args.extend((0..26).map(|_| "ripgrep@stable".to_string()));
    let output = jetpack()
        .args(&args)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let settled = stderr
        .lines()
        .filter(|line| line.trim_start().starts_with('✓'))
        .count();
    assert_eq!(settled, 26, "one settled row per package: {stderr}");
    assert!(
        stderr.lines().count() <= 27,
        "26-package run must fit one aggregate line plus settled rows: {stderr}"
    );
}

#[test]
fn failed_first_dependency_reports_zero_completed_nodes() {
    let (_base, proj, root) = core_hello_project("progress-first-failure");
    let env_path = proj.join("env.jet");
    let env = fs::read_to_string(&env_path)
        .unwrap()
        .replace("[\"hello@mine\"]", "[\"missing@mine\", \"hello@mine\"]");
    fs::write(&env_path, env).unwrap();
    let out = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("building completed 0/2 · current: mine -> missing · resolving"),
        "first failure must not claim completion: {stderr}"
    );
    assert!(!stderr.contains("building completed 1/2 · current: mine -> missing"));
    // Region erased before diagnostic: a verbatim error block follows the
    // dependency-status line (D-FE-CLI1 failure rule / hybrid.html still 8).
    assert!(
        stderr.contains("error:") || stderr.to_lowercase().contains("could not"),
        "failure must print a diagnostic after erasing the live region: {stderr}"
    );
}

#[test]
fn typed_module_example_builds_offline_end_to_end() {
    // I5: the committed jetpack-typed fixture is the executable spec
    // for the typed `module { … }` env surface (U3/U6/U8) including U4 import-tree
    // discovery. `jetpack env --prep` with no ref evaluates env.jet through `modeval`:
    // the `default` source merges to its pinned nixpkgs upstream,
    // `[default.ripgrep, default.fd]` gives two `Pkg` refs, and `imports:
    // find("./modules")` walks `modules/tools.jet` and folds its `default.jq`
    // into the same merge. All three realize from the committed fixtures, fully
    // offline. The store lives under a scratch JETPACK_ROOT, so nothing is
    // written back.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-typed");
    let project = Scratch::new("typed-e2e-project");
    copy_dir_recursive(&source, &project.path);
    let root = Scratch::new("typed-e2e");
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env(
            "JETPACK_FIXTURES",
            realized_fixtures(&project.join("fixtures"), &root.path),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for pkg in ["ripgrep", "fd", "jq"] {
        assert!(
            stderr.contains(pkg),
            "expected `{pkg}` in build output: {stderr}"
        );
    }
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn core_provider_fetches_remote_git_package_from_env() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("note: skipping remote core provider integration test (git not found)");
        return;
    }

    let base = Scratch::new("core-remote");
    let repo = base.join("remote");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from remote jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }

    for args in [
        vec!["init"],
        vec!["config", "user.email", "jetpack@example.invalid"],
        vec!["config", "user.name", "Jet Test"],
        vec!["add", "."],
        vec!["commit", "-m", "init"],
    ] {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fs::write(
        proj.join("env.jet"),
        format!(
        "use jetpack as pkg;\npub fn shell() [JSON] {{\n    return [\n        pkg.source(\"mine\", \"file://{}#HEAD\", \"core\");\n        pkg.packages([\"hello@mine\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from remote jet-pkgs"
    );
    assert!(
        root.join("sources").is_dir(),
        "remote source cache was not created"
    );

    let offline = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        offline.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&offline.stderr)
    );
}

// ── E7 jetos runtime: `jet os <verb> <host>` / `host@root` ─────────

#[test]
fn offline_without_fixtures_errors() {
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "fastfetch@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1276"), "stderr: {stderr}");
    assert!(stderr.contains("fastfetch@nixpkgs"), "stderr: {stderr}");
}

// ── D-ONCE-RETIRE1: jetpack.toml adoption ratchet ─────────────────────────

#[test]
fn jetpack_toml_retirement_fires_e1225_from_cli() {
    // Semantic retirement: presence is enough to reject the second config
    // grammar, regardless of which old table it contains.
    let proj = Scratch::new("retired-toml");
    let root = Scratch::new("retired-toml-root");
    fs::write(proj.join("jetpack.toml"), "[repo]\nname = \"old\"\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures(&root.path))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.ends_with(include_str!("cli/jetpack_toml_retired.txt")),
        "stderr: {stderr}"
    );
}

#[test]
fn jetpack_toml_retirement_fires_from_nested_directory() {
    let project = Scratch::new("retired-toml-nested");
    let nested = project.join("packages/app/src");
    let root = Scratch::new("retired-toml-nested-root");
    fs::create_dir_all(&nested).unwrap();
    fs::write(project.join("jetpack.toml"), "[repo]\nname = \"old\"\n").unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&nested)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.ends_with(include_str!("cli/jetpack_toml_retired.txt")),
        "stderr: {stderr}"
    );
}

#[test]
fn env_jet_sources_resolve_without_toml() {
    // Source aliases are Jet facts in env.jet. The same project resolves a
    // local package without any TOML file present.
    let base = Scratch::new("env-sources");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from env-source\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // env.jet declares `mine` as a path source and references `hello@mine`.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() [JSON] {\n    return [\n        pkg.source(\"mine\", \"PLACEHOLDER\", \"core\");\n        pkg.packages([\"hello@mine\"]);\n    ];\n}\n".replace(
            "PLACEHOLDER",
            &repo.to_string_lossy(),
        ),
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn package_transition_cli_covers_split_fold_init_restore_and_failures() {
    let env_project = Scratch::new("transition-cli-env");
    let env_original =
        "name: \"demo\"\nenvironments: .{ development: Environment{ tools: [\"git\"] } }\n";
    fs::write(env_project.join("package.jet"), env_original).unwrap();

    let checked = jet()
        .args(["split", "env", "--check"])
        .current_dir(&env_project.path)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(String::from_utf8_lossy(&checked.stdout).contains("No files changed."));
    assert!(!env_project.join("package/env.jet").exists());

    let split = jet()
        .args(["split", "env"])
        .current_dir(&env_project.path)
        .output()
        .unwrap();
    assert!(
        split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&split.stderr)
    );
    assert!(env_project.join("package/env.jet").is_file());

    let fold_check = jet()
        .args(["fold", "package/env.jet", "--check"])
        .current_dir(&env_project.path)
        .output()
        .unwrap();
    assert!(
        fold_check.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&fold_check.stderr)
    );
    assert!(String::from_utf8_lossy(&fold_check.stdout).contains("No files changed."));

    let fold = jet()
        .args(["fold", "package/env.jet"])
        .current_dir(&env_project.path)
        .output()
        .unwrap();
    assert!(
        fold.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&fold.stderr)
    );
    assert_eq!(
        fs::read_to_string(env_project.join("package.jet")).unwrap(),
        env_original
    );
    assert!(!env_project.join("package/env.jet").exists());

    let package_project = Scratch::new("transition-cli-package");
    let package_original = "name: \"workspace\"\napp :: Config{ version: \"1\" }\n";
    fs::write(package_project.join("package.jet"), package_original).unwrap();
    let package_split = jet()
        .args(["split", "package", "app", "--check"])
        .current_dir(&package_project.path)
        .output()
        .unwrap();
    assert!(
        package_split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&package_split.stderr)
    );
    let package_json = jet()
        .args(["split", "package", "app", "--check", "--json"])
        .current_dir(&package_project.path)
        .output()
        .unwrap();
    assert!(
        package_json.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&package_json.stderr)
    );
    let package_json =
        jetpack::JSON::parse(&String::from_utf8_lossy(&package_json.stdout)).unwrap();
    assert_eq!(
        json_string(&package_json, "before"),
        json_string(&package_json, "after")
    );
    let package_split = jet()
        .args(["split", "package", "app"])
        .current_dir(&package_project.path)
        .output()
        .unwrap();
    assert!(
        package_split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&package_split.stderr)
    );
    assert!(package_project.join("packages/app/package.jet").is_file());
    let package_fold = jet()
        .args(["fold", "packages/app/package.jet"])
        .current_dir(&package_project.path)
        .output()
        .unwrap();
    assert!(
        package_fold.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&package_fold.stderr)
    );
    assert_eq!(
        fs::read_to_string(package_project.join("package.jet")).unwrap(),
        package_original
    );

    let stale_project = Scratch::new("transition-cli-stale-member");
    fs::write(stale_project.join("package.jet"), package_original).unwrap();
    let stale_split = jet()
        .args(["split", "package", "app"])
        .current_dir(&stale_project.path)
        .output()
        .unwrap();
    assert!(
        stale_split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stale_split.stderr)
    );
    fs::OpenOptions::new()
        .append(true)
        .open(stale_project.join("packages/app/package.jet"))
        .unwrap()
        .write_all(b"// changed after the plan\n")
        .unwrap();
    let stale_fold = jet()
        .args(["fold", "packages/app/package.jet"])
        .current_dir(&stale_project.path)
        .output()
        .unwrap();
    assert_eq!(stale_fold.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&stale_fold.stderr).contains("stale transition"));
    assert!(stale_project.join("packages/app/package.jet").is_file());

    let duplicate_project = Scratch::new("transition-cli-duplicate-member");
    fs::create_dir_all(duplicate_project.join("packages/existing")).unwrap();
    fs::write(
        duplicate_project.join("packages/existing/package.jet"),
        "name: \"app\"\n",
    )
    .unwrap();
    let duplicate_original =
        "name: \"workspace\"\nmembers: [\"packages/existing\"]\napp :: Config{ version: \"1\" }\n";
    fs::write(duplicate_project.join("package.jet"), duplicate_original).unwrap();
    let duplicate = jet()
        .args(["split", "package", "app"])
        .current_dir(&duplicate_project.path)
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&duplicate.stderr)
        .contains("member Package name `app` is declared more than once"));
    assert_eq!(
        fs::read_to_string(duplicate_project.join("package.jet")).unwrap(),
        duplicate_original
    );
    assert!(!duplicate_project.join("packages/app/package.jet").exists());

    let hosts_project = Scratch::new("transition-cli-hosts");
    fs::write(
        hosts_project.join("package.jet"),
        "name: \"demo\"\noutputs: .{ server: System{ name: \"server\" } }\n",
    )
    .unwrap();
    let hosts_split = jet()
        .args(["split", "hosts", "server"])
        .current_dir(&hosts_project.path)
        .output()
        .unwrap();
    assert!(
        hosts_split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&hosts_split.stderr)
    );
    assert!(hosts_project.join("package/fleet.jet").is_file());
    let hosts_fold = jet()
        .args(["fold", "package/fleet.jet"])
        .current_dir(&hosts_project.path)
        .output()
        .unwrap();
    assert!(
        hosts_fold.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&hosts_fold.stderr)
    );

    let unknown_hosts = Scratch::new("transition-cli-unknown-host");
    fs::write(
        unknown_hosts.join("package.jet"),
        "name: \"demo\"\noutputs: .{ server: System{ name: \"server\" } }\n",
    )
    .unwrap();
    let unknown = jet()
        .args(["split", "hosts", "missing"])
        .current_dir(&unknown_hosts.path)
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("outputs.missing"));
    assert!(!unknown_hosts.join("package/fleet.jet").exists());

    let stale_hosts = Scratch::new("transition-cli-stale-host");
    fs::write(
        stale_hosts.join("package.jet"),
        "name: \"demo\"\noutputs: .{ server: System{ name: \"server\" } }\n",
    )
    .unwrap();
    let stale_split = jet()
        .args(["split", "hosts", "server"])
        .current_dir(&stale_hosts.path)
        .output()
        .unwrap();
    assert!(
        stale_split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&stale_split.stderr)
    );
    fs::OpenOptions::new()
        .append(true)
        .open(stale_hosts.join("package/fleet.jet"))
        .unwrap()
        .write_all(b"// changed after the plan\n")
        .unwrap();
    let stale_fold = jet()
        .args(["fold", "package/fleet.jet"])
        .current_dir(&stale_hosts.path)
        .output()
        .unwrap();
    assert_eq!(stale_fold.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&stale_fold.stderr).contains("stale transition"));

    let ambiguous_hosts = Scratch::new("transition-cli-ambiguous-host-journal");
    fs::write(
        ambiguous_hosts.join("package.jet"),
        "name: \"demo\"\noutputs: .{ server: System{ name: \"server\" } }\n",
    )
    .unwrap();
    let ambiguous_split = jet()
        .args(["split", "hosts", "server"])
        .current_dir(&ambiguous_hosts.path)
        .output()
        .unwrap();
    assert!(
        ambiguous_split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ambiguous_split.stderr)
    );
    let journal_dir = ambiguous_hosts.join(".jet/package-transition");
    let journal = fs::read_dir(&journal_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::copy(&journal, journal_dir.join("duplicate-journal")).unwrap();
    let ambiguous_fold = jet()
        .args(["fold", "package/fleet.jet"])
        .current_dir(&ambiguous_hosts.path)
        .output()
        .unwrap();
    assert_eq!(ambiguous_fold.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&ambiguous_fold.stderr).contains("found 2"));

    let invalid_hosts = Scratch::new("transition-cli-invalid-host");
    fs::write(
        invalid_hosts.join("package.jet"),
        "name: \"demo\"\noutputs: .{ server: System{ name: \"server\" } }\n",
    )
    .unwrap();
    let invalid = jet()
        .args(["split", "hosts", "server/name"])
        .current_dir(&invalid_hosts.path)
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    let invalid_stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(invalid_stderr.contains("E1206"), "{invalid_stderr}");
    assert!(!invalid_hosts.join("package/fleet.jet").exists());

    let legacy_project = Scratch::new("transition-cli-legacy");
    let originals = [
        ("pkg.jet", "name: \"demo\"\nversion: \"0.1.0\"\n"),
        ("env.jet", "module env.dev { tools: [\"git@nixpkgs\"] }\n"),
        ("workspace.jet", "module workspace { members: [] }\n"),
        ("config.jet", "Config{ }\n"),
    ];
    for (name, source) in originals {
        fs::write(legacy_project.join(name), source).unwrap();
    }
    let init_check = jet()
        .args(["init", "--check"])
        .current_dir(&legacy_project.path)
        .output()
        .unwrap();
    assert!(
        init_check.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init_check.stderr)
    );
    assert!(String::from_utf8_lossy(&init_check.stdout).contains("No files changed."));
    let init = jet()
        .args(["init"])
        .current_dir(&legacy_project.path)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(legacy_project.join("package.jet").is_file());
    let restore_check = jet()
        .args(["init", "--restore-role-files", "--check"])
        .current_dir(&legacy_project.path)
        .output()
        .unwrap();
    assert!(
        restore_check.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&restore_check.stderr)
    );
    let restore = jet()
        .args(["init", "--restore-role-files"])
        .current_dir(&legacy_project.path)
        .output()
        .unwrap();
    assert!(
        restore.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&restore.stderr)
    );
    for (name, source) in originals {
        assert_eq!(
            fs::read_to_string(legacy_project.join(name)).unwrap(),
            source
        );
    }
    assert!(!legacy_project.join("package.jet").exists());
}

#[test]
fn mono_example_has_two_package_jet_members() {
    // D-WORKSPACE1: the committed monorepo example now uses workspace.jet
    // instead of the retired TOML config plane.
    let mono = mono_example_dir();
    assert!(
        mono.join("workspace.jet").exists(),
        "workspace.jet missing from mono example"
    );
    let greeter_pkg = mono.join("packages/greeter/package.jet");
    let logger_pkg = mono.join("packages/logger/package.jet");
    assert!(
        greeter_pkg.exists(),
        "packages/greeter/package.jet missing: {greeter_pkg:?}"
    );
    assert!(
        logger_pkg.exists(),
        "packages/logger/package.jet missing: {logger_pkg:?}"
    );
    let workspace_src = fs::read_to_string(mono.join("workspace.jet")).unwrap();
    assert!(
        workspace_src.contains("find(\"./packages\")"),
        "workspace.jet should use find-based member discovery"
    );
}

// ── Card #99 T4: build-from-source surface (build states / vendor / audit) ────

#[test]
fn jet_build_reports_source_states() {
    // T4: `jetpack env --prep` reports how each package was satisfied. A first build
    // of a core package is `built`; the content-addressed re-build is `cached`.
    let (_base, proj, root) = core_hello_project("t4-build");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let out1 = String::from_utf8_lossy(&first.stderr);
    assert!(
        out1.contains("built"),
        "first build must report `built`: {out1}"
    );
    assert!(
        out1.contains("1 built"),
        "summary must count the built package: {out1}"
    );
    assert!(
        out1.contains("build sandbox outcome: non-executing (no child launched)"),
        "build must report the recorded non-executing receipt: {out1}"
    );

    let second = run();
    assert!(second.status.success());
    let out2 = String::from_utf8_lossy(&second.stderr);
    assert!(
        out2.contains("cached"),
        "re-build of the same content must report `cached`: {out2}"
    );
    assert!(
        out2.contains("1 cached"),
        "summary must count the cache hit: {out2}"
    );
    assert!(
        out2.contains("build sandbox outcome: verified cache only"),
        "cache-only build must not claim a fresh sandbox execution: {out2}"
    );
}

#[test]
fn epoch4_dogfood_portfolio_rebuilds_offline_after_component_loss() {
    // Card #955: this is the smallest real package-manager portfolio gate. It
    // uses the local Core provider, the Hangar, and the CLI build path. An
    // empty PATH proves the result does not depend on an installed Nix or
    // ambient build tool. Removing both the realized output and its source
    // executable injects a component failure; that run must not look cached.
    let (base, project, root) = core_hello_project("epoch4-dogfood-portfolio");
    let missing_tools = base.join("missing-tools");
    fs::create_dir_all(&missing_tools).unwrap();
    let source_executable = base.join("jet-pkgs/pkgs/hello/bin/hello");
    let build = |offline: bool| {
        let mut command = jetpack();
        command
            .args(["build", "--no-color"])
            .current_dir(&project)
            .env("JETPACK_ROOT", &root)
            .env("PATH", &missing_tools);
        if offline {
            command.arg("--offline");
        }
        command.output().unwrap()
    };

    let first = build(false);
    assert!(
        first.status.success(),
        "first portfolio build failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        first_stderr.contains("built") && first_stderr.contains("1 built"),
        "first portfolio build must publish a real result: {first_stderr}"
    );

    let cached = build(true);
    assert!(
        cached.status.success(),
        "offline portfolio reuse failed: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    let cached_stderr = String::from_utf8_lossy(&cached.stderr);
    assert!(
        cached_stderr.contains("cached") && cached_stderr.contains("1 cached"),
        "offline portfolio reuse must report a verified cache hit: {cached_stderr}"
    );

    let entered = jetpack()
        .args(["env", "--no-color", "--trust", "--offline", "--", "hello"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", &missing_tools)
        .output()
        .unwrap();
    assert!(
        entered.status.success(),
        "offline front-door entry needs no ambient tools: {}",
        String::from_utf8_lossy(&entered.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&entered.stdout).trim(),
        "hello from jet-pkgs"
    );

    let roots = jetpack::Store::Roots::at(root.clone());
    let entry = fs::read_dir(roots.hangar_dir())
        .unwrap()
        .filter_map(Result::ok)
        .find_map(|node| {
            let text = fs::read_to_string(node.path().join("meta.json")).ok()?;
            let entry = jetpack::Store::parse_meta(&text)?;
            (entry.reference == "hello@mine").then_some(entry)
        })
        .expect("portfolio build must register its Hangar entry");
    make_directories_writable(Path::new(&entry.out));
    fs::remove_dir_all(&entry.out).unwrap();
    fs::remove_file(&source_executable).unwrap();

    let failed = build(true);
    assert!(
        !failed.status.success(),
        "missing portfolio component must fail closed"
    );
    let failed_stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        !failed_stderr.contains("built 1 package(s): 0 built, 1 cached"),
        "missing portfolio component was falsely reported as cached: {failed_stderr}"
    );

    fs::write(&source_executable, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&source_executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let rebuilt = build(true);
    assert!(
        rebuilt.status.success(),
        "offline portfolio rebuild failed: {}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    let rebuilt_stderr = String::from_utf8_lossy(&rebuilt.stderr);
    assert!(
        rebuilt_stderr.contains("built") && !rebuilt_stderr.contains("1 cached"),
        "repaired portfolio must rebuild instead of hiding the failure: {rebuilt_stderr}"
    );

    let stale = write_hangar_meta(&root, "epoch4-dogfood-stale", "stale", "1.0", Some(1)).0;
    let clean = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root)
        .env("PATH", &missing_tools)
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "portfolio clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(!stale.exists(), "portfolio clean left stale Hangar state");
}

#[test]
fn jet_build_publishes_then_falls_back_to_source_when_cache_is_unavailable() {
    let (_base, proj, root) = core_hello_project("cache-source-fallback");
    let mirror = Scratch::new("cache-source-fallback-mirror");
    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    jetpack::Store::bind_cache(
        &roots,
        "public",
        vec![mirror.path.display().to_string()],
        None,
        None,
        true,
    )
    .unwrap();

    let run = |fake_sandbox: Option<&str>| {
        let mut command = jetpack();
        command
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin");
        if let Some(value) = fake_sandbox {
            command.env("JETPACK_FAKE_SANDBOX", value);
        }
        command.output().unwrap()
    };
    let first = run(None);
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let entry = jetpack::Store::find_by_reference(&roots, "hello@mine").unwrap();
    let producer = jetpack::Store::ProducerRecord::decode(&entry.producer_record).unwrap();
    assert!(
        producer
            .facts
            .get("cache.reproducibility")
            .is_some_and(|value| value.starts_with("independent-agreeing-v1:")),
        "uncached source builds must carry fresh independent agreement"
    );
    assert!(!root.join("private/unreproducible").exists());
    assert!(
        fs::read_dir(&mirror.path)
            .unwrap()
            .flatten()
            .any(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("narinfo")),
        "source realization must publish a signed cache record"
    );
    make_writable(&entry.out);
    fs::write(Path::new(&entry.out).join("bin/hello"), "tampered\n").unwrap();

    let substituted = run(Some("unavailable"));
    assert!(
        substituted.status.success(),
        "verified cache substitution must repair the local candidate: {}",
        String::from_utf8_lossy(&substituted.stderr)
    );
    let substituted_stderr = String::from_utf8_lossy(&substituted.stderr);
    assert!(
        substituted_stderr.contains("L0205"),
        "unavailable allow-mode action must record the fallback decision: {substituted_stderr}"
    );
    assert!(
        substituted_stderr.contains("substituted") && substituted_stderr.contains("1 substituted"),
        "production build must report the verified substitution: {substituted_stderr}"
    );
    let repaired = jetpack::Store::find_by_reference(&roots, "hello@mine").unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&repaired.out).join("bin/hello")).unwrap(),
        "#!/bin/sh\necho hello from jet-pkgs\n"
    );

    make_writable(&repaired.out);
    fs::write(
        Path::new(&repaired.out).join("bin/hello"),
        "tampered again\n",
    )
    .unwrap();
    fs::remove_dir_all(&mirror.path).unwrap();
    fs::write(&mirror.path, "cache substituter unavailable").unwrap();

    let fallback = run(None);
    assert!(
        fallback.status.success(),
        "source fallback must survive an unavailable substituter: {}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    let stderr = String::from_utf8_lossy(&fallback.stderr);
    assert!(
        stderr.contains("built"),
        "fallback must rebuild from source: {stderr}"
    );
    assert!(
        !stderr.contains("1 cached"),
        "an unavailable substituter must not report a cache hit: {stderr}"
    );
}

#[test]
fn jet_build_rejects_cache_after_manifest_semantics_change() {
    let (base, proj, root) = core_hello_project("truth-manifest-identity");
    let manifest = base.join("jet-pkgs/package.jet");
    fs::write(
        &manifest,
        "name: \"demo\"\nversion: \"1.0.0\"\npackages: { hello: executable }\n",
    )
    .unwrap();
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    fs::write(
        &manifest,
        "name: \"demo\"\nversion: \"2.0.0\"\npackages: { hello: executable }\n",
    )
    .unwrap();
    let rejected = run();
    assert!(!rejected.status.success());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("E2604"), "stderr: {stderr}");
    assert!(
        stderr.contains("recipe identity verification"),
        "stderr: {stderr}"
    );
}

#[test]
fn independent_root_runner_promotes_only_agreed_source_output() {
    let (_base, project, root) = core_hello_project("independent-root-runner");
    let roots = jetpack::Store::Roots::at(root.clone());
    let repo = project.parent().unwrap().join("jet-pkgs");
    let table = jetpack::RefSpec::SourceTable::from_decls([(
        "mine".into(),
        format!("path:{}", repo.display()),
        jetpack::RefSpec::ProviderKind::Core,
    )]);
    let spec = jetpack::RefSpec::classify_in("hello@mine", &table).unwrap();
    let store_dir = roots.hangar_dir();
    let ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: true,
        project_dir: None,
        nix_index: None,
        nix_roots: None,
    };
    let result = jetpack::Store::certify_independent_root_build(
        &roots,
        &ctx,
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
        jetpack::Store::IndependentRootOptions::default(),
    )
    .unwrap();
    assert!(result.attestation.starts_with("independent-agreeing-v1:"));
    assert!(Path::new(&result.entry.out).starts_with(roots.hangar_dir().join("objects")));
    let producer = jetpack::Store::ProducerRecord::decode(&result.entry.producer_record).unwrap();
    assert_eq!(
        producer.facts.get("cache.reproducibility"),
        Some(&result.attestation)
    );
    assert!(!root.join("private/unreproducible").exists());
    assert!(
        !fs::read_dir(roots.hangar_dir().join("reproducibility-staging"))
            .map(|entries| entries.flatten().next().is_some())
            .unwrap_or(false)
    );
}

#[test]
fn independent_root_runner_rejects_divergence_before_registration() {
    let (_base, project, root) = core_hello_project("independent-root-divergence");
    let roots = jetpack::Store::Roots::at(root.clone());
    let repo = project.parent().unwrap().join("jet-pkgs");
    let table = jetpack::RefSpec::SourceTable::from_decls([(
        "mine".into(),
        format!("path:{}", repo.display()),
        jetpack::RefSpec::ProviderKind::Core,
    )]);
    let spec = jetpack::RefSpec::classify_in("hello@mine", &table).unwrap();
    let store_dir = roots.hangar_dir();
    let ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: true,
        project_dir: None,
        nix_index: None,
        nix_roots: None,
    };
    let checks = std::cell::Cell::new(0);
    let source = repo.join("pkgs/hello/bin/hello");
    let changed = || {
        let count = checks.get() + 1;
        checks.set(count);
        if count == 2 {
            fs::write(&source, "#!/bin/sh\necho divergent\n").unwrap();
        }
        false
    };
    let error = jetpack::Store::certify_independent_root_build(
        &roots,
        &ctx,
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
        jetpack::Store::IndependentRootOptions {
            retries: 0,
            cancelled: Some(&changed),
        },
    )
    .unwrap_err();
    let error = format!("{error:?}");
    assert!(error.contains("conflicting independent roots"), "{error}");
    assert!(jetpack::Store::list(&roots).is_empty());
    let reports = root.join("private/unreproducible");
    let report = fs::read_dir(reports)
        .unwrap()
        .flatten()
        .find(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .expect("divergence evidence");
    let report = fs::read_to_string(report.path()).unwrap();
    assert!(report.contains("\"kind\":\"action-identity\""), "{report}");
    assert!(report.contains("\"left_action_key\""), "{report}");
    assert!(report.contains("\"right_action_key\""), "{report}");
}

#[test]
fn independent_root_runner_cancellation_leaves_no_store_result() {
    let (_base, project, root) = core_hello_project("independent-root-cancel");
    let roots = jetpack::Store::Roots::at(root.clone());
    let repo = project.parent().unwrap().join("jet-pkgs");
    let table = jetpack::RefSpec::SourceTable::from_decls([(
        "mine".into(),
        format!("path:{}", repo.display()),
        jetpack::RefSpec::ProviderKind::Core,
    )]);
    let spec = jetpack::RefSpec::classify_in("hello@mine", &table).unwrap();
    let store_dir = roots.hangar_dir();
    let ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: true,
        project_dir: None,
        nix_index: None,
        nix_roots: None,
    };
    let cancelled = || true;
    let error = jetpack::Store::certify_independent_root_build(
        &roots,
        &ctx,
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
        jetpack::Store::IndependentRootOptions {
            retries: 1,
            cancelled: Some(&cancelled),
        },
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("cancelled"), "{error:?}");
    assert!(jetpack::Store::list(&roots).is_empty());
}

#[test]
fn two_process_reverse_package_order_does_not_deadlock() {
    let base = Scratch::new("reverse-order-leases");
    let repo = base.join("repo");
    let root = base.join("root");
    for name in ["a", "b"] {
        let package = repo.join(format!("pkgs/{name}"));
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(
            package.join(format!("{name}.jet")),
            format!("module {name} {{ }}\n"),
        )
        .unwrap();
        let tool = package.join(format!("bin/{name}"));
        fs::write(&tool, format!("#!/bin/sh\necho {name}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(tool, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    fs::write(
        repo.join("package.jet"),
        "name: \"pair\"\nversion: \"1.0.0\"\npackages: { a: executable, b: executable }\n",
    )
    .unwrap();
    let write_project = |name: &str, packages: &[&str]| {
        let project = base.join(name);
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("env.jet"),
            format!(
        "use jetpack as pkg;\npub fn shell() [JSON] {{\n return [pkg.source(\"mine\", \"{}\", \"core\"); pkg.packages([{}]);];\n}}\n",
                repo.display(),
                packages
                    .iter()
                    .map(|package| format!("\"{package}@mine\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .unwrap();
        project
    };
    let ab = write_project("ab", &["a", "b"]);
    let ba = write_project("ba", &["b", "a"]);
    let seeded = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&ab)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        seeded.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let spawn = |project: &Path| {
        jetpack()
            .args([
                "enter",
                "--no-color",
                "--trust",
                "--",
                "/bin/sh",
                "-c",
                "true",
            ])
            .current_dir(project)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .spawn()
            .unwrap()
    };
    let first = spawn(&ab);
    let second = spawn(&ba);
    assert!(first.wait_with_output().unwrap().status.success());
    assert!(second.wait_with_output().unwrap().status.success());
}

#[test]
fn jet_build_never_reports_deleted_output_as_cached() {
    let (_base, proj, root) = core_hello_project("truth-deleted-cache");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "hello@mine").unwrap();
    make_tree_writable(Path::new(&entry.out));
    fs::remove_dir_all(&entry.out).unwrap();

    let rejected = run();
    assert!(!rejected.status.success());
    let rejected_stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        rejected_stderr.contains("E2604"),
        "stderr: {rejected_stderr}"
    );
    let rebuilt = run();
    assert!(rebuilt.status.success());
    let stderr = String::from_utf8_lossy(&rebuilt.stderr);
    assert!(
        stderr.contains("built"),
        "deleted output must rebuild: {stderr}"
    );
    assert!(
        !stderr.contains("1 cached"),
        "deleted output must never count as cache hit: {stderr}"
    );
}

#[test]
fn jet_build_never_reports_tampered_output_as_cached() {
    let (_base, proj, root) = core_hello_project("truth-tampered-cache");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "hello@mine").unwrap();
    make_tree_writable(Path::new(&entry.out));
    fs::write(Path::new(&entry.out).join("bin/hello"), "tampered").unwrap();

    let rejected = run();
    assert!(!rejected.status.success());
    let rejected_stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        rejected_stderr.contains("E2604"),
        "stderr: {rejected_stderr}"
    );
    let rebuilt = run();
    assert!(rebuilt.status.success());
    let stderr = String::from_utf8_lossy(&rebuilt.stderr);
    assert!(
        stderr.contains("built"),
        "tampered output must rebuild: {stderr}"
    );
    assert!(
        !stderr.contains("1 cached"),
        "tampered output must never count as cache hit: {stderr}"
    );
}

#[test]
fn jet_vendor_writes_pinned_sources() {
    // T4 / D-BFS1: `jetpack hangar vendor` copies each source-built package and writes a
    // `<name>.sha256` pin (the A4 output hash) so a later build is reproducible.
    let (_base, proj, root) = core_hello_project("t4-vendor");
    // Realize first so the hangar has a source-built object.
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["hangar", "vendor", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pin = proj.join("vendor/hello.sha256");
    assert!(pin.is_file(), "vendor must write a per-package sha256 pin");
    let hash = fs::read_to_string(&pin).unwrap();
    assert!(
        hash.trim().starts_with("sha256-"),
        "the pin must be a content hash: {hash}"
    );
    assert!(
        proj.join("vendor/hello").is_dir(),
        "vendor must copy the package source tree"
    );
}

#[test]
fn jet_audit_reads_without_exec() {
    // T4 / D-BUILDSCOPE1: `jetpack audit` reads build provenance and executes
    // nothing — no "resolving …" / "built" build activity, just a read-only
    // report of the realized objects' provenance.
    let (_base, proj, root) = core_hello_project("t4-audit");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["audit", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("read-only, no build ran"),
        "audit is read-only: {report}"
    );
    assert!(
        report.contains("provenance"),
        "audit reports provenance: {report}"
    );
    assert!(
        report.contains("source:"),
        "audit reports source identity: {report}"
    );
    assert!(
        report.contains("action:"),
        "audit reports action identity: {report}"
    );
    assert!(
        report.contains("sandbox:"),
        "audit reports sandbox policy: {report}"
    );
    assert!(
        report.contains("closure:"),
        "audit reports closure: {report}"
    );
    // Audit must not run a build: it never prints the realize progress line.
    assert!(
        !report.contains("resolving"),
        "audit must not realize anything: {report}"
    );
}

#[test]
fn jet_inspect_audit_missing_inputs_fails_closed() {
    // Card #431 criterion 6: an audit without its lock or signed advisory
    // database must not report a false clean result.
    let project = Scratch::new("inspect-audit-missing-inputs");
    fs::write(
        project.join("package.jet"),
        "name: \"audit-probe\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();

    let missing_lock = jet()
        .args(["inspect", "audit", "--no-color"])
        .current_dir(&project.path)
        .env_remove("JET_ADVISORY_DB")
        .env_remove("JET_ADVISORY_TRUST")
        .env_remove("JET_ADVISORY_PUBLIC_KEY")
        .output()
        .unwrap();
    assert_eq!(missing_lock.status.code(), Some(1));
    let missing_lock_stderr = String::from_utf8_lossy(&missing_lock.stderr);
    assert!(
        missing_lock_stderr.contains("E2611"),
        "{missing_lock_stderr}"
    );
    assert!(
        missing_lock_stderr.contains("unified lockfile"),
        "{missing_lock_stderr}"
    );

    fs::create_dir_all(project.join(".jet")).unwrap();
    fs::write(
        project.join(".jet/lock"),
        "version = 1\n\n[[package]]\nname = \"audit-probe\"\nversion = \"0.1.0\"\nsource = { root = \".\" }\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    let missing_db = jet()
        .args(["inspect", "audit", "--no-color"])
        .current_dir(&project.path)
        .env_remove("JET_ADVISORY_DB")
        .env_remove("JET_ADVISORY_TRUST")
        .env_remove("JET_ADVISORY_PUBLIC_KEY")
        .output()
        .unwrap();
    assert_eq!(
        missing_db.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&missing_db.stdout),
        String::from_utf8_lossy(&missing_db.stderr)
    );
    let missing_db_stderr = String::from_utf8_lossy(&missing_db.stderr);
    assert!(missing_db_stderr.contains("E2611"), "{missing_db_stderr}");
    assert!(
        missing_db_stderr.contains("signed advisory database"),
        "{missing_db_stderr}"
    );
}

#[test]
fn jet_hangar_du_counts_source_built_objects() {
    // T0 exit: `jetpack hangar du` counts realized objects honestly, marking
    // source-built ones. A first-party core build shows up as a `(built)` object.
    let (_base, proj, root) = core_hello_project("t0-du");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["hangar", "du", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("built"),
        "du must mark source-built objects: {report}"
    );
    assert!(
        report.contains("1 built from source"),
        "du summary must count source-built objects honestly: {report}"
    );
}

#[test]
fn staged_plan_action_production_path_is_deterministic_and_complete() {
    let scratch = Scratch::new("staged-plan-success");
    let source = scratch.join("source");
    let artifacts = scratch.join("artifacts");
    fs::create_dir_all(&source).unwrap();
    let input = b"declared staged input";
    fs::write(source.join("manifest"), input).unwrap();
    let digest = format!("sha256-{}", jetpack::SHA256::sha256_hex(input));
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let action = jetpack::Recipe::StagedPlanAction::new(
        "discover",
        0,
        1,
        vec![jetpack::Recipe::PlanInput::new("manifest", digest.clone())],
        jetpack::Recipe::PlanAuthority {
            tools: vec!["planner".to_string(), "planner-alt".to_string()],
            effects: vec![BuildEffect::Exec, BuildEffect::FS],
            platform: platform.clone(),
        },
    );
    let executable = std::env::current_exe().unwrap();
    let tools = HashMap::from([
        ("planner".to_string(), executable.clone()),
        ("planner-alt".to_string(), executable),
    ]);
    let context = jetpack::Recipe::StagedPlanContext {
        source_dir: &source,
        artifact_root: &artifacts,
    };

    let first = jetpack::Recipe::run_staged_plan_action(&action, &context, &tools, |sandbox| {
        assert_eq!(sandbox.read_input("manifest").unwrap(), input);
        let mut emitted = jetpack::Recipe::PlanFragmentAction::new("compile", "planner");
        emitted.args = vec!["--stable".to_string()];
        emitted.inputs = vec!["manifest".to_string()];
        emitted.outputs = vec!["result.bin".to_string()];
        emitted.env = BTreeMap::from([("STAGED_INPUT".to_string(), "manifest".to_string())]);
        emitted.effects = vec![BuildEffect::Exec, BuildEffect::FS];
        emitted.platform = platform.clone();
        Ok(jetpack::Recipe::BuildPlanFragment {
            actions: vec![emitted],
        })
    })
    .unwrap();
    let second = jetpack::Recipe::run_staged_plan_action(&action, &context, &tools, |sandbox| {
        assert_eq!(sandbox.read_input("manifest").unwrap(), input);
        let mut emitted = jetpack::Recipe::PlanFragmentAction::new("compile", "planner");
        emitted.args = vec!["--stable".to_string()];
        emitted.inputs = vec!["manifest".to_string()];
        emitted.outputs = vec!["result.bin".to_string()];
        emitted.env = BTreeMap::from([("STAGED_INPUT".to_string(), "manifest".to_string())]);
        emitted.effects = vec![BuildEffect::Exec, BuildEffect::FS];
        emitted.platform = platform.clone();
        Ok(jetpack::Recipe::BuildPlanFragment {
            actions: vec![emitted],
        })
    })
    .unwrap();

    assert_eq!(first.action_identity, second.action_identity);
    assert_eq!(first.fragment_digest, second.fragment_digest);
    assert_eq!(first.plan_fingerprint, second.plan_fingerprint);
    assert_eq!(first.artifact_dir, second.artifact_dir);
    assert_eq!(first.lock.inputs[0].digest, digest);
    assert!(first.lock.encode().contains("tool=planner\n"));
    assert!(first.lock.encode().contains("effect=exec\n"));
    assert!(first
        .lock
        .encode()
        .contains(&format!("platform={platform}\n")));
    assert!(first.artifact_dir.join("fragment.plan").is_file());
    assert!(first.artifact_dir.join("lock").is_file());
    assert!(first.artifact_dir.join("plan.fingerprint").is_file());

    let plan = jetpack::Recipe::lower_staged_plan_action(
        &action,
        &jetpack::Recipe::BuildPlanFragment {
            actions: vec![{
                let mut emitted = jetpack::Recipe::PlanFragmentAction::new("compile", "planner");
                emitted.inputs = vec!["manifest".to_string()];
                emitted.outputs = vec!["result.bin".to_string()];
                emitted.effects = vec![BuildEffect::Exec, BuildEffect::FS];
                emitted.platform = platform.clone();
                emitted
            }],
        },
        &tools,
    )
    .unwrap();
    assert_eq!(plan.actions().len(), 2, "planner plus one emitted action");
    assert!(plan
        .actions()
        .iter()
        .any(|item| item.inputs.iter().any(|path| path.as_str() == "manifest")));
    assert!(plan.actions().iter().any(|item| item
        .outputs
        .iter()
        .any(|path| path.as_str() == "result.bin")));
    assert!(plan.actions().iter().all(|item| item
        .labels
        .get("staged.platform")
        .is_some_and(|value| value == &platform)));
    assert!(plan.actions().iter().any(|item| item
        .labels
        .get("authority.tool.0")
        .is_some_and(|value| value == "planner")));

    let mut reordered = action.clone();
    reordered.authority.tools.reverse();
    reordered.authority.effects.reverse();
    let reordered_plan = jetpack::Recipe::lower_staged_plan_action(
        &reordered,
        &jetpack::Recipe::BuildPlanFragment {
            actions: vec![{
                let mut emitted = jetpack::Recipe::PlanFragmentAction::new("compile", "planner");
                emitted.inputs = vec!["manifest".to_string()];
                emitted.outputs = vec!["result.bin".to_string()];
                emitted.effects = vec![BuildEffect::Exec, BuildEffect::FS];
                emitted.platform = platform.clone();
                emitted
            }],
        },
        &tools,
    )
    .unwrap();
    assert_eq!(
        jetpack::Recipe::plan_recipe_fingerprint(&plan).unwrap(),
        jetpack::Recipe::plan_recipe_fingerprint(&reordered_plan).unwrap(),
        "authority declaration order must not change canonical plan identity"
    );
}

#[test]
fn staged_plan_action_rejects_sandbox_and_graph_failures_without_artifact() {
    let scratch = Scratch::new("staged-plan-failures");
    let source = scratch.join("source");
    let artifacts = scratch.join("artifacts");
    fs::create_dir_all(&source).unwrap();
    let input = b"declared staged input";
    fs::write(source.join("manifest"), input).unwrap();
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let tools = HashMap::from([("planner".to_string(), std::env::current_exe().unwrap())]);
    let context = jetpack::Recipe::StagedPlanContext {
        source_dir: &source,
        artifact_root: &artifacts,
    };
    let make_action = |name: &str| {
        jetpack::Recipe::StagedPlanAction::new(
            name,
            0,
            1,
            vec![jetpack::Recipe::PlanInput::new(
                "manifest",
                format!("sha256-{}", jetpack::SHA256::sha256_hex(input)),
            )],
            jetpack::Recipe::PlanAuthority {
                tools: vec!["planner".to_string()],
                effects: vec![BuildEffect::Exec],
                platform: platform.clone(),
            },
        )
    };
    let valid_fragment = |tool: &str, output: &str| {
        let mut emitted = jetpack::Recipe::PlanFragmentAction::new("step", tool);
        emitted.inputs = vec!["manifest".to_string()];
        emitted.outputs = vec![output.to_string()];
        emitted.effects = vec![BuildEffect::Exec];
        emitted.platform = platform.clone();
        jetpack::Recipe::BuildPlanFragment {
            actions: vec![emitted],
        }
    };

    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("undeclared"),
        &context,
        &tools,
        |sandbox| {
            assert!(sandbox.read_store("private/store").is_err());
            assert!(sandbox.resolve_package("unlocked").is_err());
            sandbox.read_input("not-declared")?;
            unreachable!("read_input must reject undeclared access")
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("staged plan action denied"));
    assert!(!artifacts.exists());

    let mut cycle_a = jetpack::Recipe::PlanFragmentAction::new("a", "planner");
    cycle_a.inputs = vec!["manifest".to_string()];
    cycle_a.outputs = vec!["a.out".to_string()];
    cycle_a.dependencies = vec!["b".to_string()];
    cycle_a.effects = vec![BuildEffect::Exec];
    cycle_a.platform = platform.clone();
    let mut cycle_b = jetpack::Recipe::PlanFragmentAction::new("b", "planner");
    cycle_b.inputs = vec!["manifest".to_string()];
    cycle_b.outputs = vec!["b.out".to_string()];
    cycle_b.dependencies = vec!["a".to_string()];
    cycle_b.effects = vec![BuildEffect::Exec];
    cycle_b.platform = platform.clone();
    let error =
        jetpack::Recipe::run_staged_plan_action(&make_action("cycle"), &context, &tools, |_| {
            Ok(jetpack::Recipe::BuildPlanFragment {
                actions: vec![cycle_a, cycle_b],
            })
        })
        .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("cycle"));
    assert!(!artifacts.exists());

    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("unauthorized-tool"),
        &context,
        &tools,
        |_| Ok(valid_fragment("other-tool", "tool.out")),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("undeclared tool `other-tool`"));
    assert!(!artifacts.exists());

    let missing_tools = HashMap::new();
    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("missing-realized-tool"),
        &context,
        &missing_tools,
        |_| Ok(valid_fragment("planner", "missing-tool.out")),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.what.contains("not a realized dependency"));
    assert!(!artifacts.exists());

    let mut unauthorized_effect = valid_fragment("planner", "effect.out");
    unauthorized_effect.actions[0].effects = vec![BuildEffect::FS];
    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("unauthorized-effect"),
        &context,
        &tools,
        |_| Ok(unauthorized_effect),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("undeclared effect `fs`"));
    assert!(!artifacts.exists());

    let mut mismatched_platform = valid_fragment("planner", "platform.out");
    mismatched_platform.actions[0].platform = "other-platform".to_string();
    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("platform-mismatch"),
        &context,
        &tools,
        |_| Ok(mismatched_platform),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("not declared platform"));
    assert!(!artifacts.exists());

    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("invalid-output"),
        &context,
        &tools,
        |_| Ok(valid_fragment("planner", "../escape")),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("output `../escape`"));
    assert!(!artifacts.exists());

    let mut overlapping_outputs = valid_fragment("planner", "overlap.out");
    let mut nested_output = jetpack::Recipe::PlanFragmentAction::new("nested", "planner");
    nested_output.inputs = vec!["manifest".to_string()];
    nested_output.outputs = vec!["overlap.out/child".to_string()];
    nested_output.effects = vec![BuildEffect::Exec];
    nested_output.platform = platform.clone();
    overlapping_outputs.actions.push(nested_output);
    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("overlapping-output"),
        &context,
        &tools,
        |_| Ok(overlapping_outputs),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("overlap"));
    assert!(!artifacts.exists());

    let mut mismatched_input = make_action("input-mismatch");
    mismatched_input.inputs[0].digest = format!("sha256-{}", "0".repeat(64));
    let error =
        jetpack::Recipe::run_staged_plan_action(&mismatched_input, &context, &tools, |_| {
            Ok(valid_fragment("planner", "mismatch.out"))
        })
        .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("digest mismatch"));
    assert!(!artifacts.exists());

    let error =
        jetpack::Recipe::run_staged_plan_action(&make_action("failed"), &context, &tools, |_| {
            Err(jetpack::Recipe::StagedPlanActionError::Failed(
                "boom".to_string(),
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.why.contains("failed before publication"));
    assert!(!artifacts.exists());

    let error = jetpack::Recipe::run_staged_plan_action(
        &make_action("cancelled"),
        &context,
        &tools,
        |_| Err(jetpack::Recipe::StagedPlanActionError::Cancelled),
    )
    .unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(error.what.contains("cancelled"));
    assert!(!artifacts.exists());
}

#[test]
fn jetos_plan_projects_package_system_output_deterministically() {
    let project = Scratch::new("jetos-package-system-output");
    fs::write(
        project.join("package.jet"),
        r#"name: "demo"
outputs: .{
    workstation: System{
        name: "workstation"
        target: linux.x64
        packages: [ripgrep, "fd@nixpkgs", ripgrep]
        services: .{ ssh: .{ enable: true, ports: [22] } }
        options: .{ network: .{ hostName: "workstation" } }
    }
    prod: Fleet{ hosts: .{ edge: "system.workstation" } }
}"#,
    )
    .unwrap();

    let run = || {
        jet()
            .args([
                "os",
                "plan",
                "workstation",
                "--json",
                "--no-color",
                "--offline",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", project.join("jet-root"))
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run();
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    let json = String::from_utf8_lossy(&first.stdout);
    let compact: String = json
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    assert!(compact.contains("\"host\":\"workstation\""), "{json}");
    assert!(compact.contains("\"target\":\"linux.x64\""), "{json}");
    assert!(compact.contains("\"graph_identity\":\""), "{json}");
    assert!(compact.contains("\"ref\":\"ripgrep@nixpkgs\""), "{json}");
    assert!(compact.contains("\"name\":\"ssh\""), "{json}");
    assert!(
        compact.contains("\"fields\":[{\"key\":\"ports\",\"value\":\"[22]\"}]"),
        "{json}"
    );
    assert!(compact.contains("\"key\":\"network.hostName\""), "{json}");
    assert!(
        !project.join("jet-root/systems/generations").exists(),
        "plan must not create a generation"
    );
}

#[test]
fn package_host_split_preserves_system_projection_and_reaches_jetos() {
    let project = Scratch::new("package-host-split-parity");
    fs::write(
        project.join("package.jet"),
        "name: \"demo\"\noutputs: .{ server: System{ name: \"halcyon\", target: linux.x64, packages: [ripgrep] } }\n",
    )
    .unwrap();

    let before_facts = jetpack::Package::PackageFacts::load(&project.path)
        .unwrap()
        .unwrap();
    let before = jet_env_model::ModuleEval::project_package_outputs(&before_facts).unwrap();
    let split = jet()
        .args(["split", "hosts", "server"])
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        split.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&split.stderr)
    );

    let fleet_source = fs::read_to_string(project.join("package/fleet.jet")).unwrap();
    assert!(fleet_source.contains("system.halcyon"), "{fleet_source}");
    let after_facts = jetpack::Package::PackageFacts::load(&project.path)
        .unwrap()
        .unwrap();
    let after = jet_env_model::ModuleEval::project_package_outputs(&after_facts).unwrap();
    assert_eq!(before.systems, after.systems);
    assert_eq!(after.fleets[0].hosts[0].system, "halcyon");

    let plan = jet()
        .args(["os", "plan", "halcyon", "--json", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let plan_json = String::from_utf8_lossy(&plan.stdout);
    assert!(plan_json.contains("\"host\":\"halcyon\""), "{plan_json}");
    assert!(plan_json.contains("\"graph_identity\":\""), "{plan_json}");
}

#[test]
fn jetos_plan_rejects_package_fleet_host_path_collision_before_generation() {
    let project = Scratch::new("jetos-package-fleet-host-collision");
    fs::write(
        project.join("package.jet"),
        r#"name: "demo"
outputs: .{
    workstation: System{ target: linux.x64 }
    laptop: System{ target: linux.arm64 }
    blue: Fleet{ hosts: .{ edge: system.workstation } }
    green: Fleet{ hosts: .{ edge: system.laptop } }
}"#,
    )
    .unwrap();

    let output = jet()
        .args([
            "os",
            "plan",
            "workstation",
            "--json",
            "--no-color",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("outputs.green.hosts.edge"), "{stderr}");
    assert!(stderr.contains("collides with Fleet host"), "{stderr}");
    assert!(
        !project.join("jet-root/systems").exists(),
        "projection failure must not publish generation files"
    );
}

#[test]
fn jetos_plan_rejects_invalid_package_system_without_mutating_store() {
    let project = Scratch::new("jetos-package-system-invalid");
    fs::write(
        project.join("package.jet"),
        r#"name: "demo"
outputs: .{ workstation: System{ target: linux.x64, services: .{ ssh: .{} } } }"#,
    )
    .unwrap();

    let output = jet()
        .args([
            "os",
            "plan",
            "workstation",
            "--json",
            "--no-color",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", project.join("jet-root"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1340"), "{stderr}");
    assert!(
        stderr.contains("outputs.workstation.services.ssh.enable"),
        "{stderr}"
    );
    assert!(
        !project.join("jet-root/systems").exists(),
        "a rejected projection must not publish JetOS state"
    );
}

#[test]
fn remote_scheduler_selects_capabilities_then_fails_over_in_order() {
    use jetpack::Remote::{
        ActionKey, BuildCapability, BuildResourcePool, RemoteAttemptError, RemoteBuildBinding,
        RemoteBuildRequest, RemoteBuilder, RemoteBuilderCapabilities, RemoteScheduler,
    };

    let root = Scratch::new("remote-scheduler");
    let fast_binding = RemoteBuildBinding::new("fast", root.join("fast"), b"fast-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_platform("linux-x86_64")
        .with_execute(true);
    let safe_binding = RemoteBuildBinding::new("safe", root.join("safe"), b"safe-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_platform("linux-x86_64")
        .with_execute(true);
    let capabilities = |priority| {
        RemoteBuilderCapabilities::new("linux-x86_64", "trusted")
            .with_capability(BuildCapability::Exec)
            .with_feature("clang")
            .with_pool(BuildResourcePool::CPU)
            .with_concurrency(2)
            .with_priority(priority)
            .with_execute(true)
    };
    let scheduler = RemoteScheduler::new([
        RemoteBuilder::new(fast_binding, capabilities(20)).unwrap(),
        RemoteBuilder::new(safe_binding, capabilities(10)).unwrap(),
    ])
    .unwrap();
    let request = RemoteBuildRequest::new(ActionKey::new("remote-action"))
        .with_capability(BuildCapability::Exec)
        .with_platform("linux-x86_64")
        .with_trust_domain("trusted")
        .with_feature("clang")
        .with_pool(BuildResourcePool::CPU)
        .with_execute(true)
        .with_local_fallback(true);

    let selected = scheduler.select(&request).unwrap();
    assert_eq!(selected.builder(), "fast");
    let dispatch = scheduler
        .dispatch(&request, |builder| {
            if builder.builder() == "fast" {
                Err(RemoteAttemptError::worker_lost("fast worker disappeared"))
            } else {
                Ok(builder.builder().to_string())
            }
        })
        .unwrap();
    assert_eq!(dispatch.builder, "safe");
    assert_eq!(dispatch.attempted, vec!["fast", "safe"]);
    assert_eq!(dispatch.value, "safe");
}

#[test]
fn remote_ineligible_builder_honors_local_fallback() {
    use jet::Comptime::Build::{
        execute_build_plan_with_front_end_and_remote, ActionSpec, BuildCapability, BuildContext,
        BuildExecutionEvent, BuildResourcePool, FrontEndCompletion, RemoteBuildBinding,
    };

    let project = Scratch::new("remote-ineligible-fallback");
    let mut context = BuildContext::new();
    let action = context
        .action(
            "local-fallback",
            ActionSpec::cached(["sh", "-c", "printf fallback > build/fallback.txt"])
                .with_outputs(["build/fallback.txt"])
                .with_cap(BuildCapability::Exec)
                .with_cap(BuildCapability::FS)
                .with_cap(BuildCapability::Net)
                .with_pool(BuildResourcePool::GPU),
        )
        .unwrap();
    let target = context
        .add_executable(
            "fallback",
            jet::Comptime::Build::TargetSpec::new().with_action(action),
        )
        .unwrap();
    let plan = context.plan_with_default(target).unwrap();
    let grants = [
        BuildCapability::Exec,
        BuildCapability::FS,
        BuildCapability::Net,
    ]
    .into_iter()
    .collect();
    let binding = RemoteBuildBinding::new("gpu-builder", project.join("remote"), b"fallback-key")
        .unwrap()
        .with_trust_domain("trusted")
        .with_execute(true)
        .with_local_fallback(true);

    let execution = execute_build_plan_with_front_end_and_remote(
        &plan,
        &project.path,
        &grants,
        FrontEndCompletion::all_complete(),
        Some(&binding),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(project.join("build/fallback.txt")).unwrap(),
        "fallback"
    );
    assert!(execution.report.events.iter().any(|event| {
        matches!(
            event,
            BuildExecutionEvent::Finished {
                action: finished,
                outcome: jet::Comptime::Build::ActionOutcome::Succeeded { exit_code: 0 },
            } if *finished == action.id()
        )
    }));
}

#[test]
fn remote_execution_identity_binds_the_complete_action_request() {
    use jetpack::Remote::{
        remote_execution_identity, ActionInputSnapshot, ActionKey, BuildPath, ContentDigest,
        RemoteExecutionRequest, RemoteSandboxProof,
    };

    let key = ActionKey::new("remote-action");
    let toolchain = ContentDigest::from_bytes(b"toolchain");
    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: "attempt-1".to_string(),
        argv: vec!["compiler".to_string(), "src/main.jet".to_string()],
        inputs: vec![ActionInputSnapshot {
            path: BuildPath::new("src/main.jet").unwrap(),
            digest: ContentDigest::from_bytes(b"source"),
            byte_len: 6,
        }],
        outputs: vec![BuildPath::new("build/app").unwrap()],
        toolchain_digest: toolchain.clone(),
        sandbox: RemoteSandboxProof::new("sandbox", key.as_str(), toolchain),
    };
    let identity = remote_execution_identity(&request);

    let mut changed_argv = request.clone();
    changed_argv.argv[1] = "src/other.jet".to_string();
    assert_ne!(identity, remote_execution_identity(&changed_argv));

    let mut changed_input = request.clone();
    changed_input.inputs[0].byte_len = 7;
    assert_ne!(identity, remote_execution_identity(&changed_input));

    let mut changed_output = request;
    changed_output.outputs[0] = BuildPath::new("build/other").unwrap();
    assert_ne!(identity, remote_execution_identity(&changed_output));
}

#[test]
fn provider_conformance_real_path_preserves_registry_npm_and_cargo_facts() {
    use jetpack::MigrationImport::{import_cargo, import_npm};
    use jetpack::ProviderFacts;
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let npm_document = r#"{"name":"web","version":"1.0.0","license":"MIT","dependencies":{"vite":"5.4.0"},"scripts":{"build":"vite build"},"repository":{"type":"git","url":"https://example.invalid/web.git"}}"#;
    let npm = import_npm(npm_document);
    let npm_facts = npm
        .provider_facts
        .get("vite#version=5.4.0@npm")
        .expect("npm production importer emits exact dependency facts");
    npm_facts.validate().expect("npm facts are lossless");
    assert!(npm.emit_pkg_jet().contains("vite: vite#version=5.4.0@npm"));
    assert_eq!(npm_facts.native_document, npm_document);
    assert!(npm_facts
        .explain_lines()
        .iter()
        .any(|line| line == "native package.json: retained"));
    assert_eq!(
        ProviderFacts::from_json(&npm_facts.to_json()).unwrap(),
        *npm_facts
    );

    let cargo_manifest = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\nrepository = \"https://example.invalid/app.git\"\n[dependencies]\nserde = \"1\"\n[dev-dependencies]\ninsta = \"1\"\n";
    let cargo_lock = "[[package]]\nname = \"serde\"\nversion = \"1.0.200\"\nsource = \"registry+https://example.invalid\"\nchecksum = \"serde-checksum\"\n";
    let cargo = import_cargo(cargo_manifest, cargo_lock);
    let cargo_facts = cargo
        .provider_facts
        .get("serde#version=1.0.200@cargo")
        .expect("Cargo production importer emits exact dependency facts");
    cargo_facts.validate().expect("Cargo facts are lossless");
    assert!(cargo
        .emit_pkg_jet()
        .contains("serde: serde#version=1.0.200@cargo"));
    assert!(cargo_facts.native_document.contains("Cargo.lock:"));
    assert!(cargo_facts
        .facts
        .contains_key("provider.cargo.lock.serde.checksum"));
    assert!(cargo
        .provider_facts
        .values()
        .any(|facts| facts.facts.contains_key("package.dependency_kind")));
    let registry_document = r#"{"name":"web","version":"1.0.0","content_hash":"sha256-web","license":"MIT","yanked":false,"signature":"sig","owner":"team-web","source":{"kind":"git","url":"https://example.invalid/web.git"},"hooks":{"build":{"digest":"hook-digest"}}}"#;
    let registry = normalize_provider_document(ProviderFamily::JetRegistry, registry_document);
    registry
        .validate()
        .expect("Jet registry facts are lossless");
    let shared = registry.shared_facts();
    assert!(shared.facts.contains_key("provider.registry.signature"));
    assert!(shared.facts.contains_key("provider.registry.yanked"));
    assert_eq!(shared.native_document, registry_document);
    assert!(shared
        .explain_lines()
        .iter()
        .any(|line| line == "native json: retained"));
    let lock = registry
        .lock_record("app", &shared.reference, "x86_64-linux")
        .expect("Jet registry provider lock");
    let locked = ProviderFacts::from_json(
        lock.future_fields
            .get("provider-facts")
            .expect("provider facts in Jet registry lock"),
    )
    .expect("Jet registry lock facts JSON");
    assert_eq!(locked, shared);
}

#[test]
fn provider_conformance_real_path_refuses_mutable_missing_and_conflicting_facts() {
    use jetpack::MigrationImport::{import_cargo, import_npm};
    use jetpack::ProviderGraph::{normalize_provider_document, ProviderFamily};

    let npm = import_npm(r#"{"name":"web","version":"1.0.0","dependencies":{"vite":"^5"}}"#);
    let npm_facts = npm
        .provider_facts
        .get("vite@npm")
        .expect("mutable npm dependency remains in the fact carrier");
    assert!(!npm.emit_pkg_jet().contains("vite: vite@npm"));
    assert!(npm_facts
        .losses
        .iter()
        .any(|loss| loss.reason.contains("not an exact lock identity")));
    assert!(npm
        .todos
        .iter()
        .any(|todo| { todo.source_path == "package.json" && todo.message.contains("unresolved") }));

    let cargo = import_cargo(
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        "",
    );
    let cargo_facts = cargo
        .provider_facts
        .get("serde@cargo")
        .expect("missing Cargo lock remains in the fact carrier");
    assert!(!cargo.emit_pkg_jet().contains("serde: serde@cargo"));
    assert!(cargo_facts
        .losses
        .iter()
        .any(|loss| loss.reason.contains("no exact version")));
    assert!(cargo
        .todos
        .iter()
        .any(|todo| todo.source_path == "Cargo.lock" && todo.message.contains("missing")));

    let conflicting_registry = concat!(
        r#"{"name":"web","version":"1.0.0","content_hash":"sha256-a"}"#,
        "\n",
        r#"{"name":"web","version":"1.0.0","content_hash":"sha256-b"}"#
    );
    let registry = normalize_provider_document(ProviderFamily::JetRegistry, conflicting_registry);
    assert!(!registry.is_lossless());
    assert!(registry
        .conflicts
        .iter()
        .any(|conflict| conflict.contains("conflicting native facts")));
    assert!(registry
        .lock_record("app", "web#version=1.0.0@jet-registry", "any")
        .expect_err("conflicting registry facts must not enter the lock")
        .contains("conflict"));
}
