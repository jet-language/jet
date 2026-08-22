//! E4-JP1 Hangar Store v2 — atomic ingest, path law (E1299), verify (E1315).

use std::fs;
use std::path::Path;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

#[test]
fn hangar_ingest_path_law_reserved_is_e1299() {
    let root = Scratch::new("hangar-v2-path-root");
    let proj = Scratch::new("hangar-v2-path-proj");
    let src = Scratch::new("hangar-v2-path-src");
    fs::write(src.path.join("CON"), "nope").unwrap();
    let output = jetpack()
        .args([
            "hangar",
            "ingest",
            src.path.to_str().unwrap(),
            "--name",
            "bad",
            "--no-color",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostic = stderr
        .find("Error [E1299]:")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("hangar_path_law_reserved", diagnostic);
}

#[test]
fn hangar_ingest_missing_output_is_atomic_and_retryable() {
    let root = Scratch::new("hangar-v2-missing-root");
    let proj = Scratch::new("hangar-v2-missing-proj");
    let source = root.path.join("late-output");
    let ingest = || {
        jetpack()
            .args([
                "hangar",
                "ingest",
                source.to_str().unwrap(),
                "--name",
                "late",
                "--no-color",
            ])
            .current_dir(&proj.path)
            .env("JETPACK_ROOT", &root.path)
            .output()
            .unwrap()
    };

    let failed = ingest();
    assert_eq!(failed.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("Error [E1315]"), "{stderr}");
    assert!(stderr.contains("does not exist"), "{stderr}");
    assert!(
        !stderr.contains("ingested"),
        "failure emitted a success receipt: {stderr}"
    );

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    assert!(jetpack::Store::list_checked(&roots).unwrap().is_empty());
    assert!(jetpack::Store::closure_graph(&roots).unwrap().records.is_empty());
    assert!(!root.path.join("leases").exists());

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), "now-real").unwrap();
    let retried = ingest();
    assert!(
        retried.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&retried.stderr)
    );
    assert!(String::from_utf8_lossy(&retried.stderr).contains("ingested"));
    assert_eq!(jetpack::Store::list_checked(&roots).unwrap().len(), 1);
}

#[test]
fn hangar_ingest_verify_and_dedupe_roundtrip() {
    let root = Scratch::new("hangar-v2-ingest-root");
    let proj = Scratch::new("hangar-v2-ingest-proj");
    let src = Scratch::new("hangar-v2-ingest-src");
    fs::create_dir_all(src.path.join("bin")).unwrap();
    fs::write(src.path.join("bin/hello"), "#!/bin/sh\necho hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(src.path.join("bin/hello"), fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            src.path.to_str().unwrap(),
            "--name",
            "hello",
            "--version",
            "1.0.0",
            "--no-color",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        ingest.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let stderr = String::from_utf8_lossy(&ingest.stderr);
    assert!(stderr.contains("ingested"), "{stderr}");
    assert!(stderr.contains("sha256-"), "{stderr}");

    // Second ingest of identical bytes dedupes.
    let src2 = Scratch::new("hangar-v2-ingest-src2");
    fs::create_dir_all(src2.path.join("bin")).unwrap();
    fs::write(src2.path.join("bin/hello"), "#!/bin/sh\necho hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            src2.path.join("bin/hello"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let ingest2 = jetpack()
        .args([
            "hangar",
            "ingest",
            src2.path.to_str().unwrap(),
            "--name",
            "hello2",
            "--version",
            "1.0.0",
            "--no-color",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(ingest2.status.success());
    let stderr2 = String::from_utf8_lossy(&ingest2.stderr);
    assert!(stderr2.contains("deduplicated"), "{stderr2}");

    let list = jetpack()
        .args(["list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(list.status.success());
    let listed = String::from_utf8_lossy(&list.stderr);
    assert!(listed.contains("hello"), "{listed}");

    // Verify via digest from first ingest line.
    let digest = stderr
        .split_whitespace()
        .find(|tok| tok.starts_with("sha256-"))
        .expect("digest in ingest status");
    let verify = jetpack()
        .args(["hangar", "verify", digest, "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verify.stderr)
    );

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    // Finish the parent-side read before spawning the recovery process. The
    // child owns recovery locking; no parent store value or guard spans it.
    let meta = {
        let entry = jetpack::Store::list(&roots)
            .into_iter()
            .find(|entry| entry.name == "hello")
            .unwrap();
        roots.hangar_dir().join(entry.id).join("meta.json")
    };
    fs::remove_file(&meta).unwrap();

    let recover = jetpack()
        .args(["hangar", "recover", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        recover.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recover.stderr)
    );
    assert!(
        meta.is_file(),
        "CLI recovery must replay committed package metadata"
    );
    let _ = Path::new("."); // keep Path import used on all cfgs
}

#[test]
fn hangar_export_import_rekeys_and_rejects_corruption_without_mutation() {
    let source_root = Scratch::new("hangar-archive-source-root");
    let destination_root = Scratch::new("hangar-archive-destination-root");
    let project = Scratch::new("hangar-archive-project");
    let source = Scratch::new("hangar-archive-source");
    fs::write(source.join("payload"), "portable bytes\n").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "portable",
            "--ref",
            "portable@fixture",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &source_root.path)
        .output()
        .unwrap();
    assert!(
        ingest.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingest.stderr)
    );

    let source_roots = jetpack::Store::Roots {
        root: source_root.path.clone(),
        dev_mode: false,
    };
    let source_entry =
        jetpack::Store::find_by_reference(&source_roots, "portable@fixture").unwrap();
    let archive = source_root.path.join("portable.hangar");
    let export = jet()
        .args([
            "hangar",
            "export",
            "portable@fixture",
            "--to",
            archive.to_str().unwrap(),
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &source_root.path)
        .output()
        .unwrap();
    let export_stderr = String::from_utf8_lossy(&export.stderr);
    assert!(export.status.success(), "stderr: {export_stderr}");
    assert!(export_stderr.contains("export: ") && export_stderr.contains(" object(s)"));
    assert!(archive.is_file());

    let key = source_root.path.join("trust/hangar.key");
    assert!(key.is_file());
    let import = || {
        jet()
            .args([
                "hangar",
                "import",
                archive.to_str().unwrap(),
                "--key",
                key.to_str().unwrap(),
                "--yes",
                "--no-color",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &destination_root.path)
            .output()
            .unwrap()
    };
    let imported = import();
    assert!(
        imported.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let destination_roots = jetpack::Store::Roots {
        root: destination_root.path.clone(),
        dev_mode: false,
    };
    let destination_entry =
        jetpack::Store::find_by_reference(&destination_roots, "portable@fixture").unwrap();
    assert_eq!(
        destination_entry.envelope.output_hash,
        source_entry.envelope.output_hash
    );
    let expected_destination_id = jetpack::Store::entry_id(
        &destination_entry.name,
        &destination_entry.version,
        &destination_entry.reference,
        &destination_entry.out,
    );
    assert_eq!(
        destination_entry.id,
        expected_destination_id,
        "source id: {}; source out: {}; destination out: {}",
        source_entry.id,
        source_entry.out,
        destination_entry.out
    );
    assert_ne!(destination_entry.id, source_entry.id);
    assert_eq!(
        destination_entry.id,
        jetpack::Store::entry_id(
            &destination_entry.name,
            &destination_entry.version,
            &destination_entry.reference,
            &destination_entry.out,
        )
    );
    jetpack::Store::verify_hangar_object(&destination_roots, &destination_entry).unwrap();
    let graph = jetpack::Store::closure_graph(&destination_roots).unwrap();
    assert_eq!(
        graph.records.get(&destination_entry.id).unwrap().primary,
        destination_entry.envelope.output_hash
    );
    assert!(!destination_entry.receipt.is_empty());
    assert!(destination_roots
        .hangar_dir()
        .join("receipts")
        .join(&destination_entry.receipt)
        .is_file());

    let repeated = import();
    assert!(
        repeated.status.success(),
        "repeated import stderr: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(
        jetpack::Store::list_checked(&destination_roots)
            .unwrap()
            .len(),
        1
    );

    let corrupted = source_root.path.join("portable-corrupt.hangar");
    let mut bytes = fs::read(&archive).unwrap();
    let last = bytes.last_mut().unwrap();
    *last ^= 1;
    fs::write(&corrupted, bytes).unwrap();
    let rejected = jet()
        .args([
            "hangar",
            "import",
            corrupted.to_str().unwrap(),
            "--key",
            key.to_str().unwrap(),
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &destination_root.path)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("Hangar import failed"));
    let after =
        jetpack::Store::find_by_reference(&destination_roots, "portable@fixture").unwrap();
    assert_eq!(after.id, destination_entry.id);
    assert_eq!(
        after.envelope.output_hash,
        destination_entry.envelope.output_hash
    );
    assert_eq!(
        fs::read_to_string(Path::new(&after.out).join("payload")).unwrap(),
        "portable bytes\n"
    );
}

#[test]
fn hangar_copy_roundtrip_is_idempotent_and_conflict_safe() {
    let source_root = Scratch::new("hangar-copy-source-root");
    let destination_root = Scratch::new("hangar-copy-destination-root");
    let project = Scratch::new("hangar-copy-project");
    let source = Scratch::new("hangar-copy-source");
    fs::write(source.join("payload"), "copy me\n").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "copyable",
            "--ref",
            "copyable@fixture",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &source_root.path)
        .output()
        .unwrap();
    assert!(
        ingest.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let source_roots = jetpack::Store::Roots {
        root: source_root.path.clone(),
        dev_mode: false,
    };
    let source_entry = jetpack::Store::find_by_reference(&source_roots, "copyable@fixture")
        .expect("ingest must publish the source package record");
    let source_payload = Path::new(&source_entry.out).join("payload");

    let copy = || {
        jet()
            .args([
                "hangar",
                "copy",
                "copyable@fixture",
                "--to",
                destination_root.path.to_str().unwrap(),
                "--yes",
                "--json",
                "--no-color",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &source_root.path)
            .output()
            .unwrap()
    };

    let first = copy();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_stdout = String::from_utf8_lossy(&first.stdout).into_owned();
    assert!(first_stdout.contains("\"action\":\"copy\""), "{first_stdout}");

    let destination_roots = jetpack::Store::Roots {
        root: destination_root.path.clone(),
        dev_mode: false,
    };
    let destination_entry = jetpack::Store::find_by_reference(
        &destination_roots,
        "copyable@fixture",
    )
    .expect("copy must publish the destination package record");
    jetpack::Store::verify_hangar_object(&destination_roots, &destination_entry).unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&destination_entry.out).join("payload")).unwrap(),
        "copy me\n"
    );

    let repeated = copy();
    assert!(
        repeated.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(first_stdout, String::from_utf8_lossy(&repeated.stdout));
    assert_eq!(jetpack::Store::list_checked(&destination_roots).unwrap().len(), 1);

    make_tree_writable(Path::new(&destination_entry.out));
    let destination_payload = Path::new(&destination_entry.out).join("payload");
    fs::write(&destination_payload, "keep corrupt bytes\n").unwrap();
    let rejected = copy();
    assert_eq!(rejected.status.code(), Some(2));
    let rejected_stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(rejected_stderr.contains("conflicting digest"), "{rejected_stderr}");
    assert_eq!(fs::read_to_string(destination_payload).unwrap(), "keep corrupt bytes\n");
    assert_eq!(fs::read_to_string(source_payload).unwrap(), "copy me\n");
}

#[test]
fn hangar_repair_uses_jet_dispatch_and_restores_or_preserves_the_object() {
    let root = Scratch::new("hangar-repair-root");
    let project = Scratch::new("hangar-repair-project");
    let source = Scratch::new("hangar-repair-source");
    fs::write(source.join("payload"), "trusted bytes\n").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "repairable",
            "--ref",
            "repairable@fixture",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        ingest.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingest.stderr)
    );

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "repairable@fixture").unwrap();
    let archive = root.join("repairable.hangar");
    let export = jetpack()
        .args([
            "hangar",
            "export",
            "repairable@fixture",
            "--to",
            archive.to_str().unwrap(),
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let unsigned_archive = root.join("unsigned.hangar");
    fs::write(
        &unsigned_archive,
        jetpack::Store::export_unsigned_archive(&roots, &entry.id, false).unwrap(),
    )
    .unwrap();

    let payload = Path::new(&entry.out).join("payload");
    make_tree_writable(Path::new(&entry.out));
    fs::write(&payload, "tampered bytes\n").unwrap();
    let rejected = jet()
        .args([
            "hangar",
            "repair",
            "repairable@fixture",
            "--from",
            unsigned_archive.to_str().unwrap(),
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("unsigned Hangar archives"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(fs::read_to_string(&payload).unwrap(), "tampered bytes\n");

    let repair = |archive: &Path| {
        jet()
            .args([
                "hangar",
                "repair",
                "repairable@fixture",
                "--from",
                archive.to_str().unwrap(),
                "--yes",
                "--no-color",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .output()
            .unwrap()
    };
    let repaired = repair(&archive);
    assert!(
        repaired.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert_eq!(fs::read_to_string(&payload).unwrap(), "trusted bytes\n");
    jetpack::Store::verify_hangar_object(
        &roots,
        &jetpack::Store::find_by_reference(&roots, "repairable@fixture").unwrap(),
    )
    .unwrap();
    let repeated = repair(&archive);
    assert!(
        repeated.status.success(),
        "repeated repair should be idempotent: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(fs::read_to_string(&payload).unwrap(), "trusted bytes\n");

    let quarantine = root.path.join("hangar").join("quarantine");
    fs::create_dir_all(&quarantine).unwrap();

    // A crash after quarantine can leave the already-corrupt object under a
    // repair-* name. Recovery preserves that evidence and leaves the signed
    // archive as the next repair source.
    make_tree_writable(Path::new(&entry.out));
    fs::write(&payload, "crashed corrupt bytes\n").unwrap();
    let corrupt_backup =
        quarantine.join(format!("repair-{}-corrupt", entry.envelope.output_hash));
    fs::rename(&entry.out, &corrupt_backup).unwrap();
    seal_tree(&corrupt_backup);
    let recovered_corrupt = jet()
        .args(["hangar", "recover", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        recovered_corrupt.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recovered_corrupt.stderr)
    );
    assert!(!Path::new(&entry.out).exists());
    assert!(
        fs::read_dir(&quarantine)
            .unwrap()
            .flatten()
            .any(|item| item.file_name().to_string_lossy().starts_with("rejected-repair-"))
    );
    let repaired_after_crash = repair(&archive);
    assert!(
        repaired_after_crash.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repaired_after_crash.stderr)
    );
    assert_eq!(fs::read_to_string(&payload).unwrap(), "trusted bytes\n");

    make_tree_writable(Path::new(&entry.out));
    let crash_backup = quarantine.join(format!("repair-{}-crash", entry.envelope.output_hash));
    fs::rename(&entry.out, &crash_backup).unwrap();
    seal_tree(&crash_backup);
    let recovered = jet()
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
    assert_eq!(fs::read_to_string(&payload).unwrap(), "trusted bytes\n");
    jetpack::Store::verify_hangar_object(
        &roots,
        &jetpack::Store::find_by_reference(&roots, "repairable@fixture").unwrap(),
    )
    .unwrap();

    make_tree_writable(Path::new(&entry.out));
    fs::remove_dir_all(&entry.out).unwrap();
    let repaired_missing = repair(&archive);
    assert!(
        repaired_missing.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repaired_missing.stderr)
    );
    assert_eq!(fs::read_to_string(&payload).unwrap(), "trusted bytes\n");
    jetpack::Store::verify_hangar_object(
        &roots,
        &jetpack::Store::find_by_reference(&roots, "repairable@fixture").unwrap(),
    )
    .unwrap();
}

#[cfg(unix)]
fn seal_tree(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.is_dir() {
        for child in fs::read_dir(path).unwrap() {
            seal_tree(&child.unwrap().path());
        }
    }
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(not(unix))]
fn seal_tree(path: &Path) {
    let metadata = fs::metadata(path).unwrap();
    if metadata.is_dir() {
        for child in fs::read_dir(path).unwrap() {
            seal_tree(&child.unwrap().path());
        }
    }
    let mut permissions = metadata.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn real_core_provider_registers_canonical_object_and_action_relation() {
    let (_base, project, root) = core_hello_project("hangar-v2-core-closure");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let roots = jetpack::Store::Roots {
        root: root.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "hello@mine").unwrap();
    assert_eq!(
        Path::new(&entry.out).parent(),
        Some(root.join("hangar/objects").as_path())
    );
    assert_eq!(
        Path::new(&entry.out).file_name().and_then(|name| name.to_str()),
        Some(entry.envelope.output_hash.as_str())
    );
    let graph = jetpack::Store::closure_graph(&roots).unwrap();
    let record = graph.records.get(&entry.id).unwrap();
    assert_eq!(record.primary, entry.envelope.output_hash);
    assert_eq!(
        graph.action_outputs(&record.action_key).get("out"),
        Some(&record.primary)
    );
    jetpack::Store::verify_hangar_object(&roots, &entry).unwrap();
}

#[test]
fn manual_external_root_cli_is_atomic_and_reports_stale_etag() {
    let root = Scratch::new("hangar-v2-manual-root");
    let project = Scratch::new("hangar-v2-manual-project");
    let source = Scratch::new("hangar-v2-manual-source");
    fs::write(source.join("payload"), "manual-root-bytes").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "manual",
            "--ref",
            "manual@fixture",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        ingest.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ingest.stderr)
    );

    let register = jetpack()
        .args([
            "hangar",
            "register-external-root",
            "backup-sdk",
            "manual@fixture",
            "--expires-in",
            "1w",
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_PRINCIPAL", "manual-root-cli")
        .output()
        .unwrap();
    assert!(
        register.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&register.stderr)
    );
    assert!(String::from_utf8_lossy(&register.stderr).contains("etag 1.1"));

    let listed = jetpack()
        .args(["hangar", "list-external-roots", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_PRINCIPAL", "manual-root-cli")
        .output()
        .unwrap();
    assert!(listed.status.success());
    let listed_stderr = String::from_utf8_lossy(&listed.stderr);
    assert!(listed_stderr.contains("backup-sdk"));
    assert!(listed_stderr.contains("etag 1.1"));

    let stale = jetpack()
        .args([
            "hangar",
            "register-external-root",
            "backup-sdk",
            "manual@fixture",
            "--if-etag",
            "1.0",
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_PRINCIPAL", "manual-root-cli")
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(2));
    let stale_stderr = String::from_utf8_lossy(&stale.stderr);
    assert!(stale_stderr.contains("error[E1320]"), "{stale_stderr}");
    assert!(stale_stderr.contains("No requested root mutation was applied."));
    let diagnostic = stale_stderr
        .find("\n  error[E1320]")
        .map(|index| &stale_stderr[index..])
        .unwrap_or(&stale_stderr);
    assert_jetos_stderr_snapshot_trimmed("external_root_stale_etag", diagnostic);

    let unregister = jetpack()
        .args([
            "hangar",
            "unregister-external-root",
            "backup-sdk",
            "--etag",
            "1.1",
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_PRINCIPAL", "manual-root-cli")
        .output()
        .unwrap();
    assert!(
        unregister.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&unregister.stderr)
    );
    assert!(String::from_utf8_lossy(&unregister.stderr).contains("Removed external root"));
}

#[test]
fn hangar_recovery_reclaims_crashed_stages_and_dead_leases_without_following_escape() {
    let root = Scratch::new("hangar-v2-recovery-root");
    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let archive_stage = roots.hangar_dir().join(".archive-stage/abandoned");
    fs::create_dir_all(&archive_stage).unwrap();
    fs::write(archive_stage.join("payload"), "partial archive").unwrap();
    let dead_lease = roots.root.join("leases/4294967295-0-abandoned");
    fs::create_dir_all(&dead_lease).unwrap();
    fs::write(dead_lease.join("payload"), "lease snapshot").unwrap();

    assert_eq!(jetpack::Store::recover_hangar(&roots).unwrap(), 2);
    assert!(!roots.hangar_dir().join(".archive-stage/abandoned").exists());
    assert!(!dead_lease.exists());

    #[cfg(unix)]
    {
        let outside = Scratch::new("hangar-v2-recovery-outside");
        fs::write(outside.join("must-survive"), "live data").unwrap();
        let stage_root = roots.hangar_dir().join(".archive-stage");
        fs::remove_dir_all(&stage_root).unwrap();
        std::os::unix::fs::symlink(&outside.path, &stage_root).unwrap();
        let error = jetpack::Store::recover_hangar(&roots).unwrap_err();
        assert!(error.to_string().contains("archive staging"), "{error}");
        assert_eq!(fs::read_to_string(outside.join("must-survive")).unwrap(), "live data");
        fs::remove_file(stage_root).unwrap();

        let lease_root = roots.root.join("leases");
        fs::remove_dir_all(&lease_root).unwrap();
        std::os::unix::fs::symlink(&outside.path, &lease_root).unwrap();
        let error = jetpack::Store::recover_hangar(&roots).unwrap_err();
        assert!(error.to_string().contains("lease directory"), "{error}");
        assert_eq!(fs::read_to_string(outside.join("must-survive")).unwrap(), "live data");
        fs::remove_file(lease_root).unwrap();
    }
}
