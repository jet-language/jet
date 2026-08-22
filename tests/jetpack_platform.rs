//! U25 (D-JPK-PLATFORM1=A): platform-tier audit tests.
//!
//! These are intentionally narrow. They prove the product contract is encoded
//! in code, then drive one real package through the native CI lane for each
//! tier-1 OS. A local run proves only its host; the CI matrix supplies the
//! native macOS and Windows executions.

use std::fs;

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
            "run",
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
        .args(["clean", "--no-color", "--yes"])
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
    assert!(stderr.contains("error[E1315]"), "stderr: {stderr}");
    assert!(stderr.contains("does not exist"), "stderr: {stderr}");
    assert_no_hangar_entry(&root.path, "native-jetpack-");
}
