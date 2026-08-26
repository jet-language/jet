//! U25 (D-JPK-PLATFORM1=A): platform-tier audit tests.
//!
//! These are intentionally narrow. They prove the product contract is encoded
//! in code, then drive one real package through the native CI lane for each
//! tier-1 OS. A local run proves only its host; the CI matrix supplies the
//! native macOS and Windows executions.

use std::fs;
use std::time::{Duration, Instant};

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

use jetpack::Platform;

#[test]
fn platform_tier_audit_contract_names_linux_macos_windows() {
    assert_eq!(
        Platform::TIER_ONE_OSES,
        [Platform::OS_LINUX, Platform::OS_MACOS, Platform::OS_WINDOWS]
    );

    for arch in Platform::TIER_ONE_ARCHES {
        for os in Platform::TIER_ONE_OSES {
            let key = Platform::PlatformKey::new(arch, os).unwrap();
            assert!(key.is_tier_one());
            assert_eq!(key.envelope_key(), format!("{arch}-{os}"));
        }
    }

    let host = Platform::PlatformKey::host();
    assert!(
        host.is_tier_one(),
        "the current CI host is outside Jetpack's tier-1 matrix: {}",
        host.envelope_key()
    );
    assert_eq!(Platform::host_key(), host.envelope_key());

    assert!(Platform::PlatformKey::new("riscv64", Platform::OS_LINUX).is_none());
    assert!(Platform::PlatformKey::new(Platform::ARCH_X64, "freebsd").is_none());

    assert_eq!(Platform::exe_suffix_for_os(Platform::OS_WINDOWS), ".exe");
    assert_eq!(Platform::path_separator_for_os(Platform::OS_WINDOWS), ';');
    assert_eq!(Platform::path_separator_for_os(Platform::OS_LINUX), ':');
    assert_eq!(Platform::path_separator_for_os(Platform::OS_MACOS), ':');
}

#[test]
fn platform_tier_audit_ci_has_native_lane_scaffold() {
    let workflow = std::fs::read_to_string(".github/workflows/ci.yml").unwrap();
    assert!(
        workflow.contains("jetpack-platform"),
        "CI must name the U25 platform audit job"
    );
    for runner in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        assert!(
            workflow.contains(runner),
            "CI must scaffold a jetpack platform lane for {runner}"
        );
    }
    assert!(
        workflow.contains("cargo test --test jetpack_platform"),
        "platform lanes must run the focused U25 audit test"
    );
}

#[test]
fn platform_tier_gate_runs_native_package_offline_and_cleans_store() {
    let root = Scratch::new("platform-gate-root");
    let fixtures = Scratch::new("platform-gate-fixtures");
    let staging = Scratch::new("platform-gate-staging");
    let missing_tools = Scratch::new("platform-gate-missing-tools");
    write_native_jetpack_fixture(&fixtures.path, &root.path, &staging.path);
    let program = format!("jetpack{}", std::env::consts::EXE_SUFFIX);

    let run = jetpack()
        .args([
            "use",
            "native-jetpack@nixpkgs",
            "--no-color",
            "--offline",
            "--",
            &program,
            "--help",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("jetpack"),
        "native package did not run: {}",
        String::from_utf8_lossy(&run.stdout)
    );

    let stale = write_hangar_meta(
        &root.path,
        "platform-stale",
        "platform-stale",
        "1.0",
        Some(1),
    )
    .0;
    let clean = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(
        !stale.exists(),
        "production clean left stale platform state"
    );
}

#[test]
fn platform_gate_reaches_store_authenticated_lease_service() {
    let root = Scratch::new("platform-authenticated-lease-root");
    let fixtures = Scratch::new("platform-authenticated-lease-fixtures");
    let staging = Scratch::new("platform-authenticated-lease-staging");
    let missing_tools = Scratch::new("platform-authenticated-lease-missing-tools");
    write_native_jetpack_fixture(&fixtures.path, &root.path, &staging.path);
    let program = format!("omp{}", std::env::consts::EXE_SUFFIX);

    let mut run = jetpack()
        .args([
            "use",
            "omp@releases#1.0.0",
            "-y",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .args(["--", &program, "--hold"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .env("PATH", &missing_tools.path)
        .spawn()
        .unwrap();

    let records = root.path.join("lease-service/leases");
    let deadline = Instant::now() + Duration::from_secs(5);
    let found = loop {
        let found = fs::read_dir(&records).ok().and_then(|entries| {
            entries.flatten().find_map(|entry| {
                let generation = entry.path().join("generations/1");
                let receipt = generation.join("receipt");
                let complete = generation.join("complete");
                if receipt.is_file() && complete.is_file() {
                    fs::read_to_string(&receipt)
                        .ok()
                        .map(|contents| (entry.path(), contents))
                } else {
                    None
                }
            })
        });
        if found.is_some() || Instant::now() >= deadline {
            break found;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let Some((record, receipt)) = found else {
        let _ = run.kill();
        let output = run.wait_with_output().unwrap();
        panic!(
            "production run did not publish a complete authenticated lease receipt: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    assert!(receipt.starts_with("JET-EXECUTABLE-LEASE/1 receipt\n"));
    assert!(receipt.contains("\ngeneration=1\n"), "receipt: {receipt}");
    let mac = receipt
        .lines()
        .find_map(|line| line.strip_prefix("mac="))
        .expect("authenticated lease receipt tag");
    assert_eq!(mac.len(), 64);
    assert!(mac.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let witness = fs::read_to_string(record.join("generations/1/complete")).unwrap();
    assert_eq!(witness.trim().len(), 71);
    assert!(witness.trim().starts_with("sha256-"));

    let recover = jetpack()
        .args(["hangar", "recover", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        recover.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recover.stderr)
    );
    assert!(
        record.join("generations/1/receipt").is_file(),
        "recovery removed a live authenticated lease"
    );

    let run_output = run.wait_with_output().unwrap();
    assert!(
        run_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run_output.stderr)
    );
    let recover_after_exit = jetpack()
        .args(["hangar", "recover", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        recover_after_exit.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recover_after_exit.stderr)
    );
    assert!(
        fs::read_dir(&records)
            .map(|entries| entries.flatten().next().is_none())
            .unwrap_or(true),
        "idle authenticated lease record survived recovery"
    );
}

#[test]
fn platform_tier_gate_recovers_hostile_partial_lease_without_losing_good_output() {
    let root = Scratch::new("platform-hostile-lease-root");
    let fixtures = Scratch::new("platform-hostile-lease-fixtures");
    let staging = Scratch::new("platform-hostile-lease-staging");
    let missing_tools = Scratch::new("platform-hostile-lease-missing-tools");
    write_native_jetpack_fixture(&fixtures.path, &root.path, &staging.path);
    let (good_object, _) = write_hangar_meta(
        &root.path,
        "platform-hostile-good",
        "platform-hostile-good",
        "1.0",
        None,
    );
    let interrupted = root
        .join("leases")
        .join("4294967294-1-platform-hostile-good");
    fs::create_dir_all(interrupted.join("snapshot/bin")).unwrap();
    fs::write(interrupted.join("snapshot/bin/partial"), "interrupted").unwrap();

    let recover = jetpack()
        .args(["hangar", "recover", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        recover.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recover.stderr)
    );
    assert!(
        !interrupted.exists(),
        "recovery left an interrupted hostile lease"
    );
    assert!(
        good_object.join("meta.json").is_file(),
        "recovery removed the last good Hangar object"
    );
}

#[test]
fn platform_tier_gate_exercises_native_lease_diagnostics_and_audit() {
    let root = Scratch::new("platform-diagnostics-root");
    let fixtures = Scratch::new("platform-diagnostics-fixtures");
    let staging = Scratch::new("platform-diagnostics-staging");
    let missing_tools = Scratch::new("platform-diagnostics-missing-tools");
    write_native_jetpack_fixture(&fixtures.path, &root.path, &staging.path);
    let program = format!("omp{}", std::env::consts::EXE_SUFFIX);
    let run = jetpack()
        .args([
            "use",
            "omp@releases#1.0.0",
            "--no-color",
            "--offline",
            "--",
            &program,
            "--help",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "native lease handoff failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("jetpack"),
        "native lease child did not run: {}",
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(
        fs::read_dir(root.join("leases"))
            .unwrap()
            .next()
            .is_none(),
        "production child exit left an executable lease behind"
    );

    let (good_object, _) = write_hangar_meta(
        &root.path,
        "platform-diagnostics-good",
        "platform-diagnostics-good",
        "1.0",
        None,
    );
    let stale = root
        .join("leases")
        .join("4294967294-1-platform-diagnostics-good");
    fs::create_dir_all(stale.join("snapshot")).unwrap();
    fs::write(stale.join("snapshot/partial"), "interrupted").unwrap();

    let doctor = jetpack()
        .args(["doctor", "--json", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    let doctor_json = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        doctor_json.contains("stale executable lease(s) found"),
        "doctor missed stale lease: {doctor_json}"
    );
    assert!(stale.is_dir(), "doctor changed lease state");

    let audit = jetpack()
        .args(["audit", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        audit.status.success(),
        "audit stderr: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit_text = String::from_utf8_lossy(&audit.stderr);
    assert!(audit_text.contains("Leases:"), "audit omitted lease state");
    assert!(
        audit_text.contains("0 active, 1 stale"),
        "audit missed stale lease: {audit_text}"
    );

    let journal = root.path.join("hangar/closure-db/journal");
    fs::create_dir_all(&journal).unwrap();
    let partial = journal.join("00000000000000000099-corrupt.txn.partial");
    fs::write(&partial, "interrupted").unwrap();
    let broken = jetpack()
        .args(["audit", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(2));
    let broken_text = String::from_utf8_lossy(&broken.stderr);
    assert!(broken_text.contains("Error [E1340]:"), "{broken_text}");
    assert!(broken_text.contains("Why:"), "{broken_text}");
    assert!(broken_text.contains("Fix:"), "{broken_text}");
    assert!(
        broken_text.contains("More: jet-lang.dev/e/E1340"),
        "{broken_text}"
    );
    assert!(partial.is_file(), "audit repaired partial journal");
    assert!(good_object.join("meta.json").is_file());

    fs::remove_file(&partial).unwrap();
    let recover = jetpack()
        .args(["hangar", "recover", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(
        recover.status.success(),
        "lease recovery stderr: {}",
        String::from_utf8_lossy(&recover.stderr)
    );
    assert!(!stale.exists(), "production recovery left stale lease");
    assert!(
        good_object.join("meta.json").is_file(),
        "production recovery removed the last good Hangar object"
    );
}

#[test]
fn platform_store_lease_projection_rejects_invalid_identity_in_doctor_and_audit() {
    // Criterion 6: a raw lease-directory scan would silently classify this
    // node. The production Store projection must reject it in both readers.
    let root = Scratch::new("platform-invalid-lease-identity");
    let malformed = root.join("leases").join("not-a-lease");
    fs::create_dir_all(&malformed).unwrap();

    let doctor = jetpack()
        .args(["doctor", "--json", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(doctor.status.code(), Some(2));
    let doctor_json = String::from_utf8_lossy(&doctor.stdout);
    assert!(doctor_json.contains("\"name\":\"leases\""), "{doctor_json}");
    assert!(doctor_json.contains("\"status\":\"broken\""), "{doctor_json}");
    assert!(doctor_json.contains("invalid identity"), "{doctor_json}");

    let audit = jetpack()
        .args(["audit", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(audit.status.code(), Some(2));
    let audit_stderr = String::from_utf8_lossy(&audit.stderr);
    assert!(audit_stderr.contains("Error [E1340]:"), "{audit_stderr}");
    assert!(audit_stderr.contains("invalid identity"), "{audit_stderr}");
    assert!(audit_stderr.contains("More: jet-lang.dev/e/E1340"), "{audit_stderr}");
    assert!(malformed.is_dir(), "diagnostics mutated the lease projection");
}

#[test]
fn audit_reports_stale_executable_lease_golden() {
    let root = Scratch::new("audit-stale-lease-golden");
    let stale = root.join("leases").join("4294967294-1-audit-stale");
    fs::create_dir_all(stale.join("snapshot")).unwrap();
    fs::write(stale.join("snapshot/partial"), "interrupted").unwrap();

    let audit = jetpack()
        .args(["audit", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        audit.status.success(),
        "audit stderr: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit_text = String::from_utf8_lossy(&audit.stderr);
    let lease_lines = audit_text
        .lines()
        .filter(|line| line.contains("Leases:") || line.contains("Lease Note:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        format!("{lease_lines}\n"),
        include_str!("cli/jetpack_audit_stale_lease.txt")
    );
    assert!(stale.is_dir(), "audit repaired stale lease state");
}

#[test]
fn platform_tier_gate_reports_missing_offline_component() {
    let root = Scratch::new("platform-failure-root");
    let fixtures = Scratch::new("platform-failure-fixtures");
    let missing_tools = Scratch::new("platform-failure-missing-tools");
    let missing = root.join("missing-component-output");
    fs::create_dir_all(&fixtures.path).unwrap();
    fs::write(
        fixtures.join("nixpkgs-native-jetpack.json"),
        format!(
            "[{{\"drvPath\":\"/nix/store/0fixture-native-jetpack.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
            missing.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["build", "native-jetpack@nixpkgs", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .env("PATH", &missing_tools.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E1315]:"), "stderr: {stderr}");
    assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    assert_no_hangar_entry(&root.path, "native-jetpack-");
}
