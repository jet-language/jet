use super::*;
use alloc::string::String;

const ZERO_SRI: &str = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

#[test]
fn partial_stage_authority_is_minted_inside_seam_tests() {
    let harness = Authority::test_harness();
    for stage in [
        Authority::InternalStage::Syntax,
        Authority::InternalStage::Values,
        Authority::InternalStage::Evaluation,
        Authority::InternalStage::Authority,
        Authority::InternalStage::Derivation,
        Authority::InternalStage::Flakes,
    ] {
        assert_eq!(Authority::authorize_internal(&harness, stage).stage(), stage);
    }
}

fn manifest_with_identities(status: &str, corpus_status: &str) -> String {
    ORACLE_JSON
        .replace(
            "\"build_nar_hash\": null",
            &format!("\"build_nar_hash\": \"{NIXPKGS_NAR_HASH}\""),
        )
        .replace(
            "\"executable_nar_hash\": null",
            &format!("\"executable_nar_hash\": \"{ZERO_SRI}\""),
        )
        .replace("\"status\": \"blocked\"", &format!("\"status\": \"{status}\""))
        .replace("blocked_on_oracle_build_hashes", corpus_status)
}

fn ready_manifest() -> ValidatedOracleManifest {
    ValidatedOracleManifest::parse_and_validate(&manifest_with_identities("ready", "bit_exact"))
        .expect("ready manifest must validate")
}

fn identity(build: &str, executable: &str) -> OracleBuildIdentity {
    OracleBuildIdentity::new("x86_64-linux", build, executable)
        .expect("test identity must use canonical SRI")
}

#[test]
fn embedded_pin_is_exact_and_missing_build_identity_blocks() {
    let manifest = ValidatedOracleManifest::embedded().expect("embedded manifest must validate");
    assert_eq!(manifest.nix_version(), NIX_VERSION);
    assert_eq!(manifest.nix_tag_object(), NIX_TAG_OBJECT);
    assert_eq!(manifest.nix_source_commit(), NIX_SOURCE_COMMIT);
    assert_eq!(manifest.nixpkgs_revision(), NIXPKGS_REVISION);
    assert_eq!(manifest.nixpkgs_last_modified(), NIXPKGS_LAST_MODIFIED);
    assert_eq!(manifest.nixpkgs_nar_hash(), NIXPKGS_NAR_HASH);
    assert_eq!(manifest.systems(), REQUIRED_SYSTEMS);
    assert!(!manifest.product_ready());

    for system in REQUIRED_SYSTEMS {
        let observed = OracleBuildIdentity::new(system, NIXPKGS_NAR_HASH, ZERO_SRI).unwrap();
        assert!(matches!(
            manifest.verify_oracle(&observed),
            Err(BoundaryError::MissingOracleIdentity { .. })
        ));
    }
}

#[test]
fn exact_ready_identity_verifies() {
    let manifest = ready_manifest();
    let verified = manifest
        .verify_oracle(&identity(NIXPKGS_NAR_HASH, ZERO_SRI))
        .expect("exact ready identity must verify");
    assert_eq!(verified.system(), "x86_64-linux");
    assert!(manifest.product_ready());
}

#[test]
fn build_and_executable_mismatches_fail_closed() {
    let manifest = ready_manifest();
    assert!(matches!(
        manifest.verify_oracle(&identity(ZERO_SRI, ZERO_SRI)),
        Err(BoundaryError::OracleIdentityMismatch {
            field: "build_nar_hash",
            ..
        })
    ));
    assert!(matches!(
        manifest.verify_oracle(&identity(NIXPKGS_NAR_HASH, NIXPKGS_NAR_HASH)),
        Err(BoundaryError::OracleIdentityMismatch {
            field: "executable_nar_hash",
            ..
        })
    ));
}

#[test]
fn matching_identity_remains_blocked_when_build_status_is_blocked() {
    let manifest = ValidatedOracleManifest::parse_and_validate(&manifest_with_identities(
        "blocked",
        "blocked_on_oracle_build_hashes",
    ))
    .expect("blocked manifest with identities must validate");
    assert!(matches!(
        manifest.verify_oracle(&identity(NIXPKGS_NAR_HASH, ZERO_SRI)),
        Err(BoundaryError::OracleBuildBlocked { .. })
    ));
}

#[test]
fn every_fixed_pin_field_is_validated() {
    let mutations = [
        (NIX_VERSION, "2.34.7"),
        (
            NIX_TAG_OBJECT,
            "a6769c588f60b3e762f73d3a8cf60294df078ccd",
        ),
        (
            NIX_SOURCE_COMMIT,
            "a3f1c3c5b8ad91850e0f7c590cf177f7ab022024",
        ),
        (
            NIXPKGS_REVISION,
            "a5aa0fbd538984f6e3d201be0005b4463d8b09f8",
        ),
        ("1782723713", "1782723712"),
        (NIXPKGS_NAR_HASH, ZERO_SRI),
    ];
    for (exact_pin, mutation) in mutations {
        let changed = ORACLE_JSON.replacen(exact_pin, mutation, 1);
        assert_ne!(changed, ORACLE_JSON);
        assert!(ValidatedOracleManifest::parse_and_validate(&changed).is_err());
    }
}

#[test]
fn duplicate_keys_are_rejected_at_every_depth() {
    let root_duplicate = ORACLE_JSON.replacen(
        "\"schema\": 1,",
        "\"schema\": 1, \"schema\": 1,",
        1,
    );
    assert!(matches!(
        ValidatedOracleManifest::parse_and_validate(&root_duplicate),
        Err(BoundaryError::Manifest(reason)) if reason.contains("duplicate object key `schema`")
    ));

    let build_duplicate = ORACLE_JSON.replacen(
        "\"status\": \"blocked\"",
        "\"status\": \"blocked\", \"status\": \"ready\"",
        1,
    );
    assert!(matches!(
        ValidatedOracleManifest::parse_and_validate(&build_duplicate),
        Err(BoundaryError::Manifest(reason)) if reason.contains("duplicate object key `status`")
    ));
}

#[test]
fn malformed_or_noncanonical_sha256_sri_is_rejected() {
    for malformed in [
        "sha256-build",
        "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/=",
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    ] {
        assert!(matches!(
            OracleBuildIdentity::new("x86_64-linux", malformed, ZERO_SRI),
            Err(BoundaryError::MalformedSRI {
                field: "build_nar_hash"
            })
        ));
        let changed = ORACLE_JSON.replacen(
            "\"build_nar_hash\": null",
            &format!("\"build_nar_hash\": \"{malformed}\""),
            1,
        );
        assert!(ValidatedOracleManifest::parse_and_validate(&changed).is_err());
    }
}

#[test]
fn unknown_oracle_system_fails_closed() {
    let manifest = ready_manifest();
    let observed = OracleBuildIdentity::new("riscv64-linux", NIXPKGS_NAR_HASH, ZERO_SRI).unwrap();
    assert!(matches!(
        manifest.verify_oracle(&observed),
        Err(BoundaryError::UnsupportedOracleSystem(_))
    ));
}

#[test]
fn native_devshell_projects_literal_packages_and_loss_facts() {
    let evaluated = evaluate_devshell(
        r#"
        {
          outputs = { devShells.x86_64-linux.default = pkgs.mkShell {
            packages = [ pkgs.ripgrep pkgs.fd ];
            buildInputs = with pkgs; [ nodejs ];
            shellHook = "export FOO=1";
          }; };
        }
        "#,
        "x86_64-linux",
    )
    .expect("literal devShell must evaluate");
    assert_eq!(evaluated.system(), "x86_64-linux");
    assert_eq!(
        evaluated.packages(),
        &["fd".to_string(), "nodejs".to_string(), "ripgrep".to_string()]
    );
    assert_eq!(evaluated.unsupported(), &["shellHook".to_string()]);
}

#[test]
fn native_devshell_rejects_dynamic_package_expressions() {
    let error = evaluate_devshell(
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = pkgs.lib.optionals true [ pkgs.fd ]; }; }",
        "x86_64-linux",
    )
    .expect_err("dynamic package expressions must not be guessed");
    assert!(matches!(error, EvaluationError::Unsupported(reason) if reason.contains("literal package list")));
}

#[test]
fn native_devshell_rejects_unknown_systems_and_empty_flakes() {
    assert!(matches!(
        evaluate_devshell("{ }", "x86_64-windows"),
        Err(EvaluationError::UnsupportedSystem(_))
    ));
    assert!(matches!(
        evaluate_devshell("{ }", "x86_64-linux"),
        Err(EvaluationError::Unsupported(reason)) if reason.contains("devShell")
    ));
}

#[test]
fn native_devshell_treats_indented_hooks_as_unsupported_loss() {
    let evaluated = evaluate_devshell(
        r#"{
          devShells.x86_64-linux.default = pkgs.mkShell {
            packages = [ pkgs.fd ];
            shellHook = ''
              # packages = [ pkgs.must-not-leak ];
              export FOO=1
            '';
          };
        }"#,
        "x86_64-linux",
    )
    .expect("indented shell hooks must not confuse package extraction");
    assert_eq!(evaluated.packages(), &["fd".to_string()]);
    assert_eq!(evaluated.unsupported(), &["shellHook".to_string()]);
}
