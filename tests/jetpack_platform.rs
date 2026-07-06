//! U25 (D-JPK-PLATFORM1=A): platform-tier audit tests.
//!
//! These are intentionally narrow. They prove the product contract is encoded
//! in code and that CI has a native lane scaffold for each tier-1 OS without
//! pretending this Linux run executed macOS or Windows behavior.

use jet::Jetpack::Platform;

#[test]
fn platform_tier_audit_contract_names_linux_macos_windows() {
    assert_eq!(
        Platform::TIER_ONE_OSES,
        [Platform::OS_LINUX, Platform::OS_MACOS, Platform::OS_WINDOWS]
    );

    for os in Platform::TIER_ONE_OSES {
        let key = Platform::PlatformKey::new(Platform::ARCH_X64, os).unwrap();
        assert!(key.is_tier_one());
        assert_eq!(key.envelope_key(), format!("x86_64-{os}"));
    }

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
