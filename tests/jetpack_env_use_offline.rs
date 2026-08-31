//! Card #2330: warm env-entry reuse is offline, deterministic, and near-instant.

use std::fs;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

mod common;

use jetpack::SHA256;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jetpack, Scratch};

fn write_native_omp_fixture(fixtures: &Path) -> String {
    fs::create_dir_all(fixtures).unwrap();
    let artifact = fixtures.join("omp-1.0.0");
    fs::write(&artifact, "#!/bin/sh\nprintf '%s\\n' cached\n").unwrap();
    let digest = SHA256::sha256_file_hex(&artifact).unwrap();
    fs::write(
        fixtures.join("jetpackage-omp.json"),
        format!(
            "{{\"tag\":\"v1.0.0\",\"version\":\"1.0.0\",\"sha256\":\"{digest}\",\"artifact\":\"omp-1.0.0\"}}"
        ),
    )
    .unwrap();
    digest
}

#[cfg(target_os = "linux")]
fn run_env(project: &Path, root: &Path, home: &Path, timing: bool) -> std::process::Output {
    let mut command = jetpack();
    command
        .args(["env", "--trust", "--offline", "--no-color", "--", "omp"])
        .current_dir(project)
        .env("JETPACK_ROOT", root)
        .env("HOME", home)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES");
    if timing {
        command.env("JETPACK_TIMING", "1");
    }
    command.output().unwrap()
}

#[cfg(target_os = "linux")]
fn tree_bytes(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    fs::read_dir(path)
        .unwrap()
        .map(|entry| tree_bytes(&entry.unwrap().path()))
        .sum()
}

#[cfg(target_os = "linux")]
fn restore_mtime(path: &Path, metadata: &fs::Metadata) {
    use std::os::unix::fs::MetadataExt as _;
    let timestamp = format!("@{}.{}", metadata.mtime(), metadata.mtime_nsec());
    let status = Command::new("touch")
        .args(["-d", timestamp.as_str()])
        .arg(path)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "could not restore {} mtime",
        path.display()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn fully_materialized_env_and_use_enter_without_network_or_prompt() {
    let project = Scratch::new("env-use-offline-project");
    let root = Scratch::new("env-use-offline-root");
    let fixtures = Scratch::new("env-use-offline-fixtures");
    let home = Scratch::new("env-use-offline-home");
    write_native_omp_fixture(&fixtures.path);
    fs::write(
        project.join("env.jet"),
        "module env.dev { packages: [\"omp@releases#1.0.0\"] }\n",
    )
    .unwrap();

    let prep = jetpack()
        .args([
            "env",
            "--prep",
            "--yes",
            "--trust",
            "--offline",
            "--no-color",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        prep.status.success(),
        "env --prep failed: {}",
        String::from_utf8_lossy(&prep.stderr)
    );

    fs::remove_file(fixtures.path.join("jetpackage-omp.json")).unwrap();
    fs::remove_file(fixtures.path.join("omp-1.0.0")).unwrap();

    let warmup = run_env(&project.path, &root.path, &home.path, false);
    assert!(
        warmup.status.success(),
        "cached entry failed: {}",
        String::from_utf8_lossy(&warmup.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&warmup.stdout).trim(), "cached");

    let receipt_path = project.join(".jet/receipts/env-entry");
    let receipt_before = fs::read(&receipt_path).unwrap();
    let output_hash = std::str::from_utf8(&receipt_before)
        .unwrap()
        .lines()
        .find(|line| line.starts_with("omp@"))
        .and_then(|line| line.split('\t').nth(3))
        .unwrap()
        .to_string();
    let out_dir = root.path.join("hangar/objects").join(&output_hash);
    let lock_path = project.join(".jet/lock");
    let lock_before = fs::read(&lock_path).ok();
    let root_bytes_before_warm_runs = tree_bytes(&root.path);
    for run in 0..3 {
        let started = Instant::now();
        let output = run_env(&project.path, &root.path, &home.path, run == 0);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "warm env run {run} took {elapsed:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success(),
            "warm env run {run} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("cache hit 100 percent"), "stderr: {stderr}");
        if run == 0 {
            assert!(
                !stderr.contains("TIMING step omp"),
                "warm path realized omp; receipt:\n{}\nstderr:\n{stderr}",
                fs::read_to_string(&receipt_path).unwrap()
            );
        }
        assert!(
            !stderr.contains("continue?"),
            "cached entry prompted: {stderr}"
        );
    }
    assert_eq!(fs::read(&receipt_path).unwrap(), receipt_before);
    assert_eq!(fs::read(&lock_path).ok(), lock_before);
    assert_eq!(
        tree_bytes(&root.path),
        root_bytes_before_warm_runs,
        "warm reuse left net Hangar/lease bytes"
    );

    let env_path = project.join("env.jet");
    let original_env = fs::read(&env_path).unwrap();
    let mut changed_env = original_env.clone();
    changed_env.extend_from_slice(b"\n");
    fs::write(&env_path, changed_env).unwrap();
    let changed_input = run_env(&project.path, &root.path, &home.path, true);
    assert!(
        changed_input.status.success(),
        "changed environment input failed: {}",
        String::from_utf8_lossy(&changed_input.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&changed_input.stdout).trim(),
        "cached"
    );
    assert!(
        String::from_utf8_lossy(&changed_input.stderr).contains("TIMING step omp"),
        "changed environment input was incorrectly warm-reused: {}",
        String::from_utf8_lossy(&changed_input.stderr)
    );
    fs::write(&env_path, original_env).unwrap();
    let restored_input = run_env(&project.path, &root.path, &home.path, false);
    assert!(
        restored_input.status.success(),
        "restored environment input failed: {}",
        String::from_utf8_lossy(&restored_input.stderr)
    );

    let journal_path = fs::read_dir(root.path.join("hangar/closure-db/journal"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("txn"))
        .unwrap();
    let journal_metadata = fs::metadata(&journal_path).unwrap();
    let status = Command::new("touch")
        .args(["-d", "@1"])
        .arg(&journal_path)
        .status()
        .unwrap();
    assert!(status.success(), "could not mutate journal mtime");
    let changed_journal = run_env(&project.path, &root.path, &home.path, true);
    assert!(
        changed_journal.status.success(),
        "changed store journal failed: {}",
        String::from_utf8_lossy(&changed_journal.stderr)
    );
    assert!(
        String::from_utf8_lossy(&changed_journal.stderr).contains("TIMING step omp"),
        "changed store journal was incorrectly warm-reused: {}",
        String::from_utf8_lossy(&changed_journal.stderr)
    );
    restore_mtime(&journal_path, &journal_metadata);
    let restored_journal = run_env(&project.path, &root.path, &home.path, false);
    assert!(
        restored_journal.status.success(),
        "restored store journal failed: {}",
        String::from_utf8_lossy(&restored_journal.stderr)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let seal_path = root.path.join("hangar/seals").join(&output_hash);
        let seal_before = fs::read(&seal_path).unwrap();
        let seal_metadata = fs::metadata(&seal_path).unwrap();
        let mut tampered_seal = seal_before.clone();
        tampered_seal[0] ^= 1;
        fs::write(&seal_path, &tampered_seal).unwrap();
        restore_mtime(&seal_path, &seal_metadata);
        let tampered_metadata = fs::metadata(&seal_path).unwrap();
        assert_eq!(tampered_metadata.len(), seal_metadata.len());
        assert_eq!(tampered_metadata.mtime(), seal_metadata.mtime());
        assert_eq!(tampered_metadata.mtime_nsec(), seal_metadata.mtime_nsec());

        let output = run_env(&project.path, &root.path, &home.path, true);
        assert!(
            output.status.success(),
            "same-stat seal mutation did not cold-fallback: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("TIMING step omp"),
            "same-stat seal mutation was incorrectly warm-reused: {stderr}"
        );

        fs::write(&seal_path, &seal_before).unwrap();
        restore_mtime(&seal_path, &seal_metadata);
        let restored = run_env(&project.path, &root.path, &home.path, false);
        assert!(
            restored.status.success(),
            "restored seal failed: {}",
            String::from_utf8_lossy(&restored.stderr)
        );

        fs::remove_file(&seal_path).unwrap();
        let deleted_seal = run_env(&project.path, &root.path, &home.path, true);
        assert!(
            deleted_seal.status.success(),
            "deleted seal did not cold-fallback: {}",
            String::from_utf8_lossy(&deleted_seal.stderr)
        );
        let stderr = String::from_utf8_lossy(&deleted_seal.stderr);
        assert!(
            stderr.contains("TIMING step omp"),
            "deleted seal was incorrectly warm-reused: {stderr}"
        );
    }

    let receipt = fs::read_to_string(project.join(".jet/receipts/env-entry")).unwrap();
    assert!(
        receipt.starts_with("jetpack-env-entry-v4\n"),
        "normal env entry must publish the verified activation receipt: {receipt}"
    );

    let output = jetpack()
        .args(["use", "omp@releases#1.0.0", "--offline", "--", "omp"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cached entry failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("continue?"),
        "cached entry prompted: {stderr}"
    );

    common::make_tree_writable(&out_dir);
    fs::remove_dir_all(&out_dir).unwrap();
    let deleted_object = run_env(&project.path, &root.path, &home.path, true);
    assert!(
        !deleted_object.status.success(),
        "deleted direct-CAS output was served: {}",
        String::from_utf8_lossy(&deleted_object.stderr)
    );
}

#[test]
fn native_env_and_use_enter_without_network_or_prompt() {
    let project = Scratch::new("native-env-use-offline-project");
    let root = Scratch::new("native-env-use-offline-root");
    let fixtures = Scratch::new("native-env-use-offline-fixtures");
    let home = Scratch::new("native-env-use-offline-home");
    write_native_omp_fixture(&fixtures.path);
    fs::write(
        project.join("env.jet"),
        "module env.dev { packages: [\"omp@releases#1.0.0\"] }\n",
    )
    .unwrap();

    let prep = jetpack()
        .args([
            "env",
            "--prep",
            "--yes",
            "--trust",
            "--offline",
            "--no-color",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        prep.status.success(),
        "env --prep failed: {}",
        String::from_utf8_lossy(&prep.stderr)
    );

    fs::remove_file(fixtures.path.join("jetpackage-omp.json")).unwrap();
    fs::remove_file(fixtures.path.join("omp-1.0.0")).unwrap();

    for args in [
        vec!["env", "--trust", "--offline", "--no-color", "--", "omp"],
        vec![
            "use",
            "omp@releases#1.0.0",
            "--offline",
            "--no-color",
            "--",
            "omp",
        ],
    ] {
        let output = jetpack()
            .args(args)
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("HOME", &home.path)
            .env("JETPACK_DENY_NETWORK", "1")
            .env_remove("JETPACK_FIXTURES")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cached entry failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("continue?"),
            "cached entry prompted: {stderr}"
        );
    }
}
