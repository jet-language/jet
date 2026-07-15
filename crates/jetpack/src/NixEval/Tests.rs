use super::Boundary::{
    BoundaryError, InternalStage, NativeBoundary, OracleBuildIdentity, OracleManifest,
};

const SYSTEMS: [&str; 4] = [
    "aarch64-darwin",
    "aarch64-linux",
    "x86_64-darwin",
    "x86_64-linux",
];

#[test]
fn pinned_oracle_manifest_is_exact_and_blocked_without_build_identity() {
    let manifest = OracleManifest::embedded().expect("committed manifest must parse");
    assert_eq!(manifest.nix_version(), "2.34.8");
    assert_eq!(
        manifest.nix_tag_object(),
        "b6769c588f60b3e762f73d3a8cf60294df078ccd"
    );
    assert_eq!(
        manifest.nix_source_commit(),
        "f3f1c3c5b8ad91850e0f7c590cf177f7ab022024"
    );
    assert_eq!(
        manifest.nixpkgs_revision(),
        "b5aa0fbd538984f6e3d201be0005b4463d8b09f8"
    );
    assert_eq!(manifest.nixpkgs_last_modified(), 1_782_723_713);
    assert_eq!(
        manifest.nixpkgs_nar_hash(),
        "sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ="
    );
    assert_eq!(manifest.systems(), SYSTEMS);

    for system in SYSTEMS {
        let observed = OracleBuildIdentity::new(
            system,
            "sha256-observed-build",
            "sha256-observed-executable",
        );
        assert!(matches!(
            manifest.verify_oracle(&observed),
            Err(BoundaryError::MissingOracleIdentity { .. })
        ));
    }
}

#[test]
fn partial_stage_has_only_test_harness_authority() {
    let boundary = NativeBoundary::embedded().expect("committed manifest must parse");
    let harness = boundary.internal_test_harness();
    assert_eq!(harness.engine(), "native-jetpack");
    for stage in [
        InternalStage::Syntax,
        InternalStage::Values,
        InternalStage::Evaluation,
        InternalStage::Authority,
        InternalStage::Derivation,
        InternalStage::Flakes,
    ] {
        assert_eq!(boundary.authorize_internal(&harness, stage).stage(), stage);
    }
    assert!(!boundary.product_ready());
}

#[test]
fn unknown_or_mismatched_oracle_identity_fails_closed() {
    let manifest = OracleManifest::embedded().expect("committed manifest must parse");
    let unknown = OracleBuildIdentity::new(
        "riscv64-linux",
        "sha256-build",
        "sha256-executable",
    );
    assert!(matches!(
        manifest.verify_oracle(&unknown),
        Err(BoundaryError::UnsupportedOracleSystem(_))
    ));

    let ready = include_str!("../../../../tests/fixtures/nix-compat/oracle.json")
        .replace("\"build_nar_hash\": null", "\"build_nar_hash\": \"sha256-build\"")
        .replace(
            "\"executable_nar_hash\": null",
            "\"executable_nar_hash\": \"sha256-executable\"",
        )
        .replace("\"status\": \"blocked\"", "\"status\": \"ready\"");
    let ready = OracleManifest::parse(&ready).expect("ready test manifest must parse");
    let mismatch = OracleBuildIdentity::new(
        "x86_64-linux",
        "sha256-wrong-build",
        "sha256-executable",
    );
    assert!(matches!(
        ready.verify_oracle(&mismatch),
        Err(BoundaryError::OracleIdentityMismatch {
            field: "build_nar_hash",
            ..
        })
    ));
}
