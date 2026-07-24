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
        .find("\n  error[E1299]")
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
    assert!(stderr.contains("error[E1315]"), "{stderr}");
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
