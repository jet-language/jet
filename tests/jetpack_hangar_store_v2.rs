//! E4-JP1 Hangar Store v2 — atomic ingest, path law (E1299), verify (E1315).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

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
    assert!(jetpack::Store::closure_graph(&roots)
        .unwrap()
        .records
        .is_empty());
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
        fs::set_permissions(
            src.path.join("bin/hello"),
            fs::Permissions::from_mode(0o755),
        )
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
        .args(["hangar", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(list.status.success());
    let listed = String::from_utf8_lossy(&list.stderr);
    assert!(listed.contains("hello"), "{listed}");

    let list_json = jetpack()
        .args(["hangar", "list", "--json", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(list_json.status.success(), "stderr: {:?}", list_json.stderr);
    assert!(
        list_json.stderr.is_empty(),
        "JSON list leaked stderr: {:?}",
        list_json.stderr
    );
    let list_report = jetpack::JSON::parse(String::from_utf8_lossy(&list_json.stdout).trim())
        .expect("list JSON report");
    assert_eq!(json_string(&list_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&list_report, "action"), "list");
    assert!(String::from_utf8_lossy(&list_json.stdout).contains("\"packages\":["));

    // Verify via digest from first ingest line.
    let digest = stderr
        .split_whitespace()
        .find(|tok| tok.starts_with("sha256-"))
        .expect("digest in ingest status");
    let verify = jet()
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
    let verify_stderr = String::from_utf8_lossy(&verify.stderr);
    assert!(verify_stderr.contains("verified"), "{verify_stderr}");
    assert!(
        !String::from_utf8_lossy(&verify.stdout).contains("hangar is empty"),
        "top-level `jet hangar verify` bypassed Jetpack: {}",
        String::from_utf8_lossy(&verify.stdout)
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

#[cfg(unix)]
#[test]
fn distinct_hangar_roots_share_one_cas_object_and_file_bytes() {
    use std::collections::BTreeMap;
    use std::os::unix::fs::MetadataExt;

    fn physical_files(paths: &[&Path]) -> (usize, u64) {
        fn walk(path: &Path, files: &mut BTreeMap<(u64, u64), u64>) {
            let metadata = fs::symlink_metadata(path).unwrap();
            if metadata.file_type().is_symlink() {
                return;
            }
            if metadata.is_file() {
                files.insert(
                    (metadata.dev(), metadata.ino()),
                    metadata.blocks().saturating_mul(512),
                );
                return;
            }
            for entry in fs::read_dir(path).unwrap() {
                walk(&entry.unwrap().path(), files);
            }
        }

        let mut files = BTreeMap::new();
        for path in paths {
            walk(path, &mut files);
        }
        (files.len(), files.values().sum())
    }

    let shared = Scratch::new("hangar-shared-cas");
    let left_root = Scratch::new("hangar-shared-left");
    let right_root = Scratch::new("hangar-shared-right");
    let project = Scratch::new("hangar-shared-project");
    let source = Scratch::new("hangar-shared-source");
    fs::create_dir_all(source.join("bin")).unwrap();
    fs::write(source.join("bin/tool"), "shared package bytes\n").unwrap();
    fs::create_dir_all(source.join("share")).unwrap();
    fs::write(source.join("share/data"), "second shared object\n").unwrap();

    let ingest = |root: &Scratch| {
        jetpack()
            .args([
                "hangar",
                "ingest",
                source.path.to_str().unwrap(),
                "--name",
                "shared-tool",
                "--version",
                "1.0.0",
                "--ref",
                "shared-tool@fixture#1.0.0",
                "--no-color",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_SHARED_CAS", &shared.path)
            .output()
            .unwrap()
    };

    for root in [&left_root, &right_root] {
        let output = ingest(root);
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let left = jetpack::Store::Roots::at(left_root.path.clone());
    let right = jetpack::Store::Roots::at(right_root.path.clone());
    let left_entry = jetpack::Store::find_by_reference(&left, "shared-tool@fixture#1.0.0").unwrap();
    let right_entry =
        jetpack::Store::find_by_reference(&right, "shared-tool@fixture#1.0.0").unwrap();
    assert_eq!(
        left_entry.envelope.output_hash,
        right_entry.envelope.output_hash
    );
    assert_ne!(left_entry.out, right_entry.out);

    let shared_objects = fs::read_dir(&shared.path)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    assert_eq!(shared_objects.len(), 1, "shared CAS object count");

    let expected = physical_files(&[shared_objects[0].as_path()]);
    let actual = physical_files(&[
        Path::new(&left_entry.out),
        Path::new(&right_entry.out),
        shared_objects[0].as_path(),
    ]);
    assert_eq!(actual, expected, "shared CAS physical file count and bytes");
}

#[test]
fn hangar_sign_and_verify_production_path_rejects_tamper() {
    let root = Scratch::new("hangar-sign-verify-root");
    let project = Scratch::new("hangar-sign-verify-project");
    let source = Scratch::new("hangar-sign-verify-source");
    fs::write(source.join("payload"), "signed bytes\n").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "signed",
            "--ref",
            "signed@fixture",
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

    let sign = jet()
        .args([
            "hangar",
            "sign",
            "signed@fixture",
            "--yes",
            "--json",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        sign.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&sign.stderr)
    );
    let sign_stdout = String::from_utf8_lossy(&sign.stdout);
    assert!(
        sign_stdout.contains("\"schema\":\"jet.report/v1\""),
        "{sign_stdout}"
    );
    assert!(sign_stdout.contains("\"action\":\"sign\""), "{sign_stdout}");
    assert!(sign_stdout.contains("\"signed\":true"), "{sign_stdout}");
    let sign_report = jetpack::JSON::parse(sign_stdout.trim()).expect("sign JSON report");
    assert_eq!(json_string(&sign_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&sign_report, "moment"), "tool");
    assert_eq!(json_string(&sign_report, "status"), "ok");
    assert_eq!(json_string(&sign_report, "action"), "sign");
    assert!(
        sign.stderr.is_empty(),
        "JSON sign leaked stderr: {:?}",
        sign.stderr
    );

    let dump_json = jet()
        .args(["hangar", "dump", "signed@fixture", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(dump_json.status.code(), Some(2));
    assert!(
        dump_json.stderr.is_empty(),
        "JSON dump leaked stderr: {:?}",
        dump_json.stderr
    );
    let dump_report = jetpack::JSON::parse(String::from_utf8_lossy(&dump_json.stdout).trim())
        .expect("dump JSON report");
    assert_eq!(json_string(&dump_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&dump_report, "moment"), "compile");
    assert_eq!(json_string(&dump_report, "severity"), "error");
    assert_eq!(json_string(&dump_report, "code"), "E1340");

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "signed@fixture").unwrap();
    let sidecar = roots.hangar_dir().join(&entry.id).join(".hangar");
    assert!(
        sidecar.is_file(),
        "sign must publish the detached archive sidecar"
    );

    let verify = jet()
        .args(["hangar", "verify", "signed@fixture", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let verify_stdout = String::from_utf8_lossy(&verify.stdout);
    assert!(
        verify_stdout.contains("\"schema\":\"jet.report/v1\""),
        "{verify_stdout}"
    );
    assert!(
        verify_stdout.contains("\"action\":\"verify\""),
        "{verify_stdout}"
    );
    assert!(verify_stdout.contains("\"signed\":true"), "{verify_stdout}");

    let unsigned_archive = root.path.join("unsigned.hangar");
    let signed_archive_path = root.path.join("signed.hangar");
    fs::write(
        &unsigned_archive,
        jetpack::Store::export_unsigned_archive(&roots, &entry.id, false).unwrap(),
    )
    .unwrap();
    let sign_archive = jet()
        .args([
            "hangar",
            "sign",
            unsigned_archive.to_str().unwrap(),
            "--to",
            signed_archive_path.to_str().unwrap(),
            "--yes",
            "--json",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        sign_archive.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&sign_archive.stderr)
    );
    assert!(signed_archive_path.is_file());
    let verify_archive = jet()
        .args([
            "hangar",
            "verify",
            signed_archive_path.to_str().unwrap(),
            "--json",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        verify_archive.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verify_archive.stderr)
    );

    let mut corrupt_unsigned = fs::read(&unsigned_archive).unwrap();
    let payload = b"signed bytes\n";
    let offset = corrupt_unsigned
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("archive carries the fixture payload");
    corrupt_unsigned[offset] ^= 1;
    fs::write(&unsigned_archive, corrupt_unsigned).unwrap();
    let corrupt_signed_path = root.path.join("corrupt-signed.hangar");
    let reject_corrupt_sign = jet()
        .args([
            "hangar",
            "sign",
            unsigned_archive.to_str().unwrap(),
            "--to",
            corrupt_signed_path.to_str().unwrap(),
            "--yes",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(reject_corrupt_sign.status.code(), Some(2));
    assert!(!corrupt_signed_path.exists());

    let all_verify = jet()
        .args(["hangar", "verify", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        all_verify.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&all_verify.stderr)
    );
    assert!(
        String::from_utf8_lossy(&all_verify.stdout).contains("\"objects\":1"),
        "{}",
        String::from_utf8_lossy(&all_verify.stdout)
    );

    let signed_archive = fs::read(&sidecar).unwrap();
    let mut corrupted_archive = signed_archive.clone();
    *corrupted_archive.last_mut().unwrap() ^= 1;
    fs::write(&sidecar, corrupted_archive).unwrap();
    let rejected_signature = jet()
        .args(["hangar", "verify", "signed@fixture", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(
        rejected_signature.status.code(),
        Some(2),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&rejected_signature.stderr),
        String::from_utf8_lossy(&rejected_signature.stdout)
    );
    assert!(
        String::from_utf8_lossy(&rejected_signature.stderr).contains("Hangar verify failed"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected_signature.stderr)
    );
    let rejected_signature_json = jet()
        .args(["hangar", "verify", "signed@fixture", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(rejected_signature_json.status.code(), Some(2));
    let rejected_signature_json_stdout = String::from_utf8_lossy(&rejected_signature_json.stdout);
    assert!(
        rejected_signature_json_stdout.contains("\"schema\":\"jet.report/v1\""),
        "{rejected_signature_json_stdout}"
    );
    assert!(
        rejected_signature_json_stdout.contains("\"code\":\"E1340\""),
        "{rejected_signature_json_stdout}"
    );
    assert!(rejected_signature_json.stderr.is_empty());
    let rejected_report =
        jetpack::JSON::parse(rejected_signature_json_stdout.trim()).expect("verify JSON report");
    assert_eq!(json_string(&rejected_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&rejected_report, "moment"), "compile");
    assert_eq!(json_string(&rejected_report, "severity"), "error");
    fs::write(&sidecar, signed_archive).unwrap();

    make_tree_writable(Path::new(&entry.out));
    let payload = Path::new(&entry.out).join("payload");
    fs::write(&payload, "tampered bytes\n").unwrap();
    let rejected_contents = jet()
        .args(["hangar", "verify", "signed@fixture", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(rejected_contents.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rejected_contents.stderr).contains("E1315"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected_contents.stderr)
    );
    assert_eq!(fs::read_to_string(payload).unwrap(), "tampered bytes\n");
}

#[test]
fn hangar_export_plan_does_not_create_archive_or_signer() {
    let root = Scratch::new("hangar-export-plan-root");
    let project = Scratch::new("hangar-export-plan-project");
    let source = Scratch::new("hangar-export-plan-source");
    fs::write(source.join("payload"), "plan only\n").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "planned",
            "--ref",
            "planned@fixture",
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

    let archive = root.path.join("planned.hangar");
    let planned = jet()
        .args([
            "hangar",
            "export",
            "planned@fixture",
            "--to",
            archive.to_str().unwrap(),
            "--json",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        planned.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&planned.stderr)
    );
    assert!(
        planned.stderr.is_empty(),
        "JSON plan leaked stderr: {:?}",
        planned.stderr
    );
    let planned_stdout = String::from_utf8_lossy(&planned.stdout);
    let planned_report = jetpack::JSON::parse(planned_stdout.trim()).expect("export plan JSON");
    assert_eq!(json_string(&planned_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&planned_report, "moment"), "tool");
    assert_eq!(json_string(&planned_report, "status"), "plan");
    assert_eq!(json_string(&planned_report, "action"), "export");
    assert!(matches!(
        planned_report.get("applied").unwrap(),
        jetpack::JSON::JSONValue::Bool(false)
    ));
    assert!(!archive.exists());
    assert!(!root.path.join("trust/hangar.key").exists());
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
        destination_entry.id, expected_destination_id,
        "source id: {}; source out: {}; destination out: {}",
        source_entry.id, source_entry.out, destination_entry.out
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
    assert_eq!(
        rejected.status.code(),
        Some(2),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&rejected.stdout),
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("Hangar import failed"));
    let after = jetpack::Store::find_by_reference(&destination_roots, "portable@fixture").unwrap();
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
fn hangar_dump_restore_streams_one_archive_and_preserves_live_data_on_corruption() {
    let source_root = Scratch::new("hangar-dump-source-root");
    let destination_root = Scratch::new("hangar-dump-destination-root");
    let project = Scratch::new("hangar-dump-project");
    let source = Scratch::new("hangar-dump-source");
    fs::write(source.join("payload"), "streamed bytes\n").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "streamed",
            "--ref",
            "streamed@fixture",
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

    let dumped = jet()
        .args(["hangar", "dump", "streamed@fixture", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &source_root.path)
        .output()
        .unwrap();
    assert!(
        dumped.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&dumped.stderr)
    );
    assert!(
        dumped.stdout.starts_with(b"jet-hangar-archive-v1\0"),
        "dump must emit the canonical archive bytes"
    );
    let key = source_root.path.join("trust/hangar.key");
    assert!(key.is_file(), "dump must create the source Hangar signer");

    let restore = |bytes: &[u8]| {
        let mut child = jet()
            .args([
                "hangar",
                "restore",
                "--key",
                key.to_str().unwrap(),
                "--yes",
                "--no-color",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &destination_root.path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(bytes).unwrap();
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    };

    let restored = restore(&dumped.stdout);
    assert!(
        restored.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let roots = jetpack::Store::Roots {
        root: destination_root.path.clone(),
        dev_mode: false,
    };
    let entry = jetpack::Store::find_by_reference(&roots, "streamed@fixture").unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&entry.out).join("payload")).unwrap(),
        "streamed bytes\n"
    );

    let repeated = restore(&dumped.stdout);
    assert!(
        repeated.status.success(),
        "repeated restore stderr: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(jetpack::Store::list_checked(&roots).unwrap().len(), 1);

    let mut corrupt = dumped.stdout.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    let rejected = restore(&corrupt);
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(Path::new(&entry.out).join("payload")).unwrap(),
        "streamed bytes\n"
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
    assert!(
        first_stdout.contains("\"schema\":\"jet.report/v1\""),
        "{first_stdout}"
    );
    assert!(
        first_stdout.contains("\"action\":\"copy\""),
        "{first_stdout}"
    );

    let destination_roots = jetpack::Store::Roots {
        root: destination_root.path.clone(),
        dev_mode: false,
    };
    let destination_entry =
        jetpack::Store::find_by_reference(&destination_roots, "copyable@fixture")
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
    assert_eq!(
        jetpack::Store::list_checked(&destination_roots)
            .unwrap()
            .len(),
        1
    );

    make_tree_writable(Path::new(&destination_entry.out));
    let destination_payload = Path::new(&destination_entry.out).join("payload");
    fs::write(&destination_payload, "keep corrupt bytes\n").unwrap();
    let rejected = copy();
    assert_eq!(rejected.status.code(), Some(2));
    let rejected_stderr = String::from_utf8_lossy(&rejected.stderr);
    let rejected_stdout = String::from_utf8_lossy(&rejected.stdout);
    assert!(
        rejected_stdout.contains("conflicting digest"),
        "stderr: {rejected_stderr}\nstdout: {rejected_stdout}"
    );
    assert_eq!(
        fs::read_to_string(destination_payload).unwrap(),
        "keep corrupt bytes\n"
    );
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
    let corrupt_backup = quarantine.join(format!("repair-{}-corrupt", entry.envelope.output_hash));
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
    assert!(fs::read_dir(&quarantine).unwrap().flatten().any(|item| item
        .file_name()
        .to_string_lossy()
        .starts_with("rejected-repair-")));
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
        Path::new(&entry.out)
            .file_name()
            .and_then(|name| name.to_str()),
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
    assert!(stale_stderr.contains("Error [E1320]"), "{stale_stderr}");
    assert!(stale_stderr.contains("No requested root mutation was applied."));
    let diagnostic = stale_stderr
        .find("Error [E1320]")
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
        assert_eq!(
            fs::read_to_string(outside.join("must-survive")).unwrap(),
            "live data"
        );
        fs::remove_file(stage_root).unwrap();

        let lease_root = roots.root.join("leases");
        fs::remove_dir_all(&lease_root).unwrap();
        std::os::unix::fs::symlink(&outside.path, &lease_root).unwrap();
        let error = jetpack::Store::recover_hangar(&roots).unwrap_err();
        assert!(error.to_string().contains("lease directory"), "{error}");
        assert_eq!(
            fs::read_to_string(outside.join("must-survive")).unwrap(),
            "live data"
        );
        fs::remove_file(lease_root).unwrap();
    }
}

#[test]
fn hangar_verify_reports_corrupt_store_instead_of_empty_success() {
    let root = Scratch::new("hangar-v2-verify-corrupt-root");
    let project = Scratch::new("hangar-v2-verify-corrupt-project");
    let source = Scratch::new("hangar-v2-verify-corrupt-source");
    fs::write(source.join("payload"), "live bytes").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "verify-corrupt",
            "--ref",
            "verify-corrupt@fixture",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(ingest.status.success(), "stderr: {:?}", ingest.stderr);

    let roots = jetpack::Store::Roots {
        root: root.path.clone(),
        dev_mode: false,
    };
    let journal = roots.hangar_dir().join("closure-db/journal");
    let journal_entry = fs::read_dir(&journal)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("txn"))
        .expect("committed closure transaction");
    let journal_bytes = fs::read(&journal_entry).unwrap();
    fs::write(&journal_entry, b"corrupt closure journal").unwrap();

    let verify = jet()
        .args([
            "hangar",
            "verify",
            "verify-corrupt@fixture",
            "--json",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    fs::write(&journal_entry, journal_bytes).unwrap();
    assert_eq!(
        verify.status.code(),
        Some(2),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );
    assert!(
        verify.stderr.is_empty(),
        "JSON verify leaked stderr: {:?}",
        verify.stderr
    );
    let report = jetpack::JSON::parse(String::from_utf8_lossy(&verify.stdout).trim())
        .expect("corrupt store report");
    assert_eq!(json_string(&report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&report, "code"), "E1340");
    assert_eq!(json_string(&report, "what"), "could not read the Hangar");
    assert!(!String::from_utf8_lossy(&verify.stdout).contains("no hangar object"));
    assert_eq!(
        fs::read_to_string(source.join("payload")).unwrap(),
        "live bytes"
    );
}

#[test]
fn hangar_du_rejects_corrupt_output_projection_without_following_escape() {
    let root = Scratch::new("hangar-v2-du-root");
    let project = Scratch::new("hangar-v2-du-project");
    let source = Scratch::new("hangar-v2-du-source");
    fs::write(source.join("payload"), "owned bytes").unwrap();

    let ingest = jetpack()
        .args([
            "hangar",
            "ingest",
            source.path.to_str().unwrap(),
            "--name",
            "du-proof",
            "--ref",
            "du-proof@fixture",
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
    let entry = jetpack::Store::find_by_reference(&roots, "du-proof@fixture").unwrap();
    let entry_dir = roots.hangar_dir().join(&entry.id);
    make_tree_writable(&entry_dir);
    let outside = Scratch::new("hangar-v2-du-outside");
    fs::write(outside.join("must-survive"), "outside bytes").unwrap();
    let meta = entry_dir.join("meta.json");
    let original = fs::read_to_string(&meta).unwrap();
    let escaped = original.replace(&entry.out, &outside.path.to_string_lossy());
    assert_ne!(escaped, original);
    fs::write(&meta, escaped).unwrap();
    let journal = roots.hangar_dir().join("closure-db/journal");
    let journal_backup = roots.hangar_dir().join("closure-db/journal.corrupt-backup");
    fs::rename(&journal, &journal_backup).unwrap();

    let du = jetpack()
        .args(["hangar", "du", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    fs::rename(&journal_backup, &journal).unwrap();
    assert_eq!(
        du.status.code(),
        Some(2),
        "du stdout: {}; stderr: {}",
        String::from_utf8_lossy(&du.stdout),
        String::from_utf8_lossy(&du.stderr)
    );
    assert!(
        du.stderr.is_empty(),
        "JSON disk-usage failure leaked stderr: {:?}",
        du.stderr
    );
    let report = jetpack::JSON::parse(String::from_utf8_lossy(&du.stdout).trim())
        .expect("du corruption report");
    assert_eq!(json_string(&report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&report, "moment"), "compile");
    assert_eq!(json_string(&report, "code"), "E1340");
    assert!(entry.out.starts_with(root.path.to_str().unwrap()));
    assert!(Path::new(&entry.out).is_dir());
    assert_eq!(
        fs::read_to_string(outside.path.join("must-survive")).unwrap(),
        "outside bytes"
    );

    let repair = jetpack()
        .args(["hangar", "recover", "--json", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        repair.status.success(),
        "repair stderr: {}",
        String::from_utf8_lossy(&repair.stderr)
    );
    assert!(repair.stderr.is_empty(), "JSON repair leaked stderr");
    let repair_report = jetpack::JSON::parse(String::from_utf8_lossy(&repair.stdout).trim())
        .expect("repair report");
    assert_eq!(json_string(&repair_report, "schema"), "jet.report/v1");
    assert_eq!(json_string(&repair_report, "action"), "recover");
    assert_eq!(fs::read_to_string(&meta).unwrap(), original);
    assert_eq!(
        fs::read_to_string(outside.path.join("must-survive")).unwrap(),
        "outside bytes"
    );
}

#[test]
fn explain_json_uses_the_report_schema_for_registered_queries() {
    let project = Scratch::new("hangar-v2-explain-project");
    for command in ["jet", "jetpack"] {
        let mut process = if command == "jet" { jet() } else { jetpack() };
        let output = process
            .args(["explain", "@", "--json", "--no-color"])
            .current_dir(&project.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{command} explain stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{command} JSON explain leaked stderr"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("schema_version"),
            "legacy explain schema: {stdout}"
        );
        let report = jetpack::JSON::parse(stdout.trim()).expect("explain report");
        assert_eq!(json_string(&report, "schema"), "jet.report/v1");
        assert_eq!(json_string(&report, "moment"), "tool");
        assert_eq!(json_string(&report, "action"), "explain");
        assert_eq!(json_string(&report, "status"), "ok");
    }
}

#[test]
fn hangar_quota_evicts_oldest_unreferenced_before_publishing_past_ceiling() {
    fn logical_bytes(path: &Path) -> u64 {
        let metadata = fs::symlink_metadata(path).unwrap();
        if metadata.is_dir() {
            fs::read_dir(path)
                .unwrap()
                .map(|entry| logical_bytes(&entry.unwrap().path()))
                .sum()
        } else {
            metadata.len()
        }
    }

    fn quota_fixture(
        roots: &jetpack::Store::Roots,
        name: &str,
        fill: u8,
        bytes: usize,
    ) -> jetpack::Store::StoreEntry {
        let source = roots.root.join(format!("quota-source-{name}"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("payload"), vec![fill; bytes]).unwrap();
        let entry = jetpack::Store::ingest_tree(
            roots,
            &jetpack::Store::IngestRequest {
                name: format!("quota-{name}"),
                version: "1".into(),
                reference: format!("quota@fixture#{name}"),
                cache_identity: jetpack::Store::CacheIdentity::default(),
                references: Vec::new(),
                outputs: std::collections::BTreeMap::from([("out".into(), source.clone())]),
                signature: String::new(),
                provenance: "quota-test".into(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap()
        .entry;
        fs::remove_dir_all(source).unwrap();
        entry
    }

    let root = Scratch::new("hangar-quota-root");
    let roots = jetpack::Store::Roots::at(root.path.clone());
    let oldest = quota_fixture(&roots, "oldest", b'a', 4 * 1024 * 1024);
    let newer = quota_fixture(&roots, "newer", b'b', 4 * 1024 * 1024);
    jetpack::Store::test_backdate_last_used_at(&roots, &oldest.id, 1).unwrap();
    jetpack::Store::test_backdate_last_used_at(&roots, &newer.id, 2).unwrap();

    let before = logical_bytes(&roots.hangar_dir());
    let limit = before - 2 * 1024 * 1024;
    fs::create_dir_all(root.path.join("config")).unwrap();
    fs::write(
        root.path.join("config/hangar-max-bytes"),
        format!("{limit}\n"),
    )
    .unwrap();
    assert!(before > limit);

    let admitted = quota_fixture(&roots, "admitted", b'c', 64 * 1024);
    let after = logical_bytes(&roots.hangar_dir());
    assert!(
        after <= limit,
        "quota admitted {after} logical bytes above ceiling {limit}"
    );
    assert!(!root.path.join("hangar").join(&oldest.id).exists());
    assert!(root.path.join("hangar").join(&newer.id).exists());
    assert!(root.path.join("hangar").join(&admitted.id).exists());
}
