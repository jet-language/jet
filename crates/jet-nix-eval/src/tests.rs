use super::*;
use alloc::format;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;

const ZERO_SRI: &str = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const X86_64_LINUX_BUILD: &str = "sha256-CM7sAhHMOi6XW7Ly1Z3ASuPzMWyz828sLRpLyt7r47Y=";
const X86_64_LINUX_EXECUTABLE: &str = "sha256-VdvAPujI/c6bKKSPQK+Q1dtICYEqzljvR19Ld+Ht3sQ=";
const STAGE_A_FIXTURE: &str =
    include_str!("../../../tests/fixtures/nix-compat/stage-a.json");
const STAGE_A_AUTHORITY_FIXTURE: &str =
    include_str!("../../../tests/fixtures/nix-compat/stage-a-authority.json");
const STAGE_A_DERIVATION_FIXTURE: &str =
    include_str!("../../../tests/fixtures/nix-compat/stage-a-derivation.json");

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
        .replace("\"status\": \"ready\"", &format!("\"status\": \"{status}\""))
        .replace("bit_exact", corpus_status)
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
    assert!(manifest.product_ready());

    for system in REQUIRED_SYSTEMS {
        let observed = OracleBuildIdentity::new(system, NIXPKGS_NAR_HASH, ZERO_SRI).unwrap();
        assert!(matches!(
            manifest.verify_oracle(&observed),
            Err(BoundaryError::OracleIdentityMismatch { .. })
        ));
    }
}

#[test]
fn exact_ready_identity_verifies() {
    let manifest = ready_manifest();
    let verified = manifest
        .verify_oracle(&identity(X86_64_LINUX_BUILD, X86_64_LINUX_EXECUTABLE))
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
        manifest.verify_oracle(&identity(X86_64_LINUX_BUILD, NIXPKGS_NAR_HASH)),
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
        "bit_exact",
    ))
    .expect("blocked manifest with identities must validate");
    assert!(matches!(
        manifest.verify_oracle(&identity(X86_64_LINUX_BUILD, X86_64_LINUX_EXECUTABLE)),
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
        "\"status\": \"ready\"",
        "\"status\": \"ready\", \"status\": \"blocked\"",
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
            "\"build_nar_hash\": \"sha256-CM7sAhHMOi6XW7Ly1Z3ASuPzMWyz828sLRpLyt7r47Y=\"",
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
fn native_devshell_keeps_unused_thunks_lazy_and_reports_forced_cycles() {
    let evaluated = evaluate_devshell(
        "let unused = unused; in { devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.fd ]; }; }",
        "x86_64-linux",
    )
    .expect("unused recursive thunks must remain lazy");
    assert_eq!(evaluated.packages(), &["fd".to_string()]);

    let error = evaluate_devshell(
        "let cycle = cycle; in { devShells.x86_64-linux.default = pkgs.mkShell { packages = [ cycle ]; }; }",
        "x86_64-linux",
    )
    .expect_err("forcing a recursive thunk must fail closed");
    assert!(matches!(error, EvaluationError::Invalid(reason) if reason.contains("cyclic foreign flake evaluation")));
}

#[test]
fn native_devshell_bounds_lazy_thunk_chains() {
    let mut source = String::from("let ");
    for index in 0..300 {
        source.push_str(&format!("v{index} = v{}; ", index + 1));
    }
    source.push_str(
        "v300 = pkgs.fd; in { devShells.x86_64-linux.default = pkgs.mkShell { packages = [ v0 ]; }; }",
    );
    let error = evaluate_devshell(&source, "x86_64-linux")
        .expect_err("deep lazy thunk chains must hit the evaluator budget");
    assert!(matches!(error, EvaluationError::ResourceLimit(reason) if reason.contains("expression steps")));
}

#[test]
fn native_devshell_releases_lazy_scopes_after_each_evaluation() {
    let source =
        "let make = packages: pkgs.mkShell { packages = packages; }; in { outputs = { devShells.x86_64-linux.default = make [ pkgs.fd ]; }; }";
    for _ in 0..256 {
        let evaluated = evaluate_devshell(source, "x86_64-linux")
            .expect("repeated bounded evaluations must remain valid");
        assert_eq!(evaluated.packages(), &["fd".to_string()]);
    }
}

#[test]
fn native_devshell_rejects_truncated_expressions_without_panicking() {
    for source in ["let", "let value =", "[", "{ outputs =", "(pkgs.mkShell"] {
        assert!(evaluate_devshell(source, "x86_64-linux").is_err());
    }
}

#[test]
fn native_evaluator_rejects_oversized_input_before_parsing() {
    let source = "x".repeat((1 << 20) + 1);
    assert!(matches!(
        evaluate_devshell(&source, "x86_64-linux"),
        Err(EvaluationError::InputTooLarge)
    ));
    assert!(matches!(
        evaluate_derivation(&source, "x86_64-linux"),
        Err(EvaluationError::InputTooLarge)
    ));
    assert!(matches!(
        evaluate_derivation_output(&source, "x86_64-linux", "out"),
        Err(EvaluationError::InputTooLarge)
    ));
}

#[test]
fn native_devshell_evaluates_indented_string_interpolation() {
    let evaluated = evaluate_devshell(
        r#"{ devShells.x86_64-linux.default = pkgs.mkShell { packages = [ ''${pkgs.fd}'' ]; }; }"#,
        "x86_64-linux",
    )
    .expect("indented string interpolation must evaluate");
    assert_eq!(evaluated.packages(), &["fd".to_string()]);
}

#[test]
fn native_devshell_preserves_string_contexts_during_projection() {
    let evaluated = evaluate_devshell(
        r#"{
          devShells.x86_64-linux.default = pkgs.mkShell {
            packages = [ "${pkgs.fd}" (builtins.toString pkgs.ripgrep) ];
            shellHook = "echo ${pkgs.fd}";
          };
        }"#,
        "x86_64-linux",
    )
    .expect("bounded string contexts must evaluate");
    assert_eq!(
        evaluated.packages(),
        &["fd".to_string(), "ripgrep".to_string()]
    );
    assert_eq!(evaluated.unsupported(), &["shellHook".to_string()]);
}

#[test]
fn native_devshell_rejects_path_and_import_without_authority() {
    let path_error = evaluate_devshell(
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = [ /tmp/package ]; }; }",
        "x86_64-linux",
    )
    .expect_err("absolute paths must not gain ambient authority");
    assert!(matches!(path_error, EvaluationError::Unsupported(reason) if reason.contains("absolute paths")));

    let import_error = evaluate_devshell(
        "{ devShells.x86_64-linux.default = import ./shell.nix; }",
        "x86_64-linux",
    )
    .expect_err("imports must require explicit authority");
    assert!(matches!(import_error, EvaluationError::Unsupported(reason) if reason.contains("explicit project-root authority")));
}

#[test]
fn native_devshell_imports_bounded_relative_sources() {
    let mut files = BTreeMap::new();
    files.insert(
        "sub/shell.nix".to_string(),
        "{ pkgs }: pkgs.mkShell { packages = [ \"${pkgs.fd}\" ]; buildInputs = import ../inputs.nix pkgs; }".to_string(),
    );
    files.insert(
        "inputs.nix".to_string(),
        "pkgs: [ (builtins.toString pkgs.ripgrep) ]".to_string(),
    );
    let authority = Rc::new(move |path: &str| {
        files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("missing test import `{path}`"))
    });
    let evaluated = evaluate_devshell_with_import_authority(
        "{ outputs = { devShells.x86_64-linux.default = (import ./sub/shell.nix) { pkgs = pkgs; }; }; }",
        "x86_64-linux",
        Some(authority),
    )
    .expect("authorized relative imports must evaluate");
    assert_eq!(
        evaluated.packages(),
        &["fd".to_string(), "ripgrep".to_string()]
    );
}

#[test]
fn native_devshell_does_not_coerce_path_contexts_into_packages() {
    let authority = Rc::new(|_: &str| Ok("pkgs.fd".to_string()));
    let error = evaluate_devshell_with_import_authority(
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = [ \"${./package}\" ]; }; }",
        "x86_64-linux",
        Some(authority),
    )
    .expect_err("path string contexts must not become package names");
    assert!(matches!(error, EvaluationError::Unsupported(reason) if reason.contains("path string contexts")));
}

#[test]
fn native_devshell_import_authority_rejects_escape_and_cycles() {
    let escape_authority = Rc::new(|path: &str| Ok(format!("{path}: not used")));
    let escape = evaluate_devshell_with_import_authority(
        "{ devShells.x86_64-linux.default = import ../outside.nix; }",
        "x86_64-linux",
        Some(escape_authority),
    )
    .expect_err("parent imports must not escape the project root");
    assert!(matches!(escape, EvaluationError::Unsupported(reason) if reason.contains("escapes")));

    let cycle_authority = Rc::new(|path: &str| {
        Ok(match path {
            "a.nix" => "import ./a.nix".to_string(),
            _ => "pkgs.fd".to_string(),
        })
    });
    let cycle = evaluate_devshell_with_import_authority(
        "{ devShells.x86_64-linux.default = import ./a.nix; }",
        "x86_64-linux",
        Some(cycle_authority),
    )
    .expect_err("cyclic imports must fail closed");
    assert!(matches!(cycle, EvaluationError::Invalid(reason) if reason.contains("cyclic")));
}

#[test]
fn stage_a_authority_fixture_matches_native_projection() {
    let fixture = JSON::parse(STAGE_A_AUTHORITY_FIXTURE).expect("authority fixture must parse");
    let root = fixture.as_object().expect("authority fixture root");
    let values = root
        .get("values")
        .expect("authority fixture values")
        .as_array()
        .expect("authority fixture values array");
    for value in values {
        let value = value.as_object().expect("authority value object");
        let source = value.get("source").unwrap().as_str().unwrap();
        let system = value.get("system").unwrap().as_str().unwrap();
        let files = value.get("files").unwrap().as_object().unwrap();
        let mut imports = BTreeMap::new();
        for (path, source) in files {
            imports.insert(path.clone(), source.as_str().unwrap().to_string());
        }
        let authority = Rc::new(move |path: &str| {
            imports
                .get(path)
                .cloned()
                .ok_or_else(|| format!("fixture has no `{path}`"))
        });
        let evaluated = evaluate_devshell_with_import_authority(
            source,
            system,
            Some(authority),
        )
        .expect("authority fixture source must evaluate");
        assert_eq!(
            evaluated.packages(),
            &fixture_strings(value.get("jet_packages").unwrap())
        );
        assert_eq!(
            evaluated.unsupported(),
            &fixture_strings(value.get("jet_unsupported").unwrap())
        );
    }
}

#[test]
fn stage_a_differential_fixture_matches_native_projection() {
    let fixture = JSON::parse(STAGE_A_FIXTURE).expect("Stage A fixture must parse");
    let root = fixture.as_object().expect("Stage A fixture root object");
    let values = root
        .get("values")
        .expect("Stage A value cases")
        .as_array()
        .expect("Stage A values array");
    for value in values {
        let value_case = value.as_object().expect("Stage A value case object");
        let value_source = value_case
            .get("source")
            .expect("Stage A value source")
            .as_str()
            .expect("Stage A value source string");
        let system = value_case
            .get("system")
            .expect("Stage A value system")
            .as_str()
            .expect("Stage A value system string");
        let evaluated = evaluate_devshell(value_source, system)
            .expect("pinned Stage A devShell value must evaluate");
        let fixture_packages = fixture_strings(value_case.get("jet_packages").unwrap());
        let fixture_unsupported = fixture_strings(value_case.get("jet_unsupported").unwrap());
        assert_eq!(evaluated.packages(), fixture_packages.as_slice());
        assert_eq!(evaluated.unsupported(), fixture_unsupported.as_slice());
    }

    let value_case = values[0].as_object().expect("Stage A value case object");
    let nix_value = value_case
        .get("nix_value")
        .expect("Stage A reference value")
        .as_object()
        .expect("Stage A reference value object");
    assert_eq!(
        fixture_strings(nix_value.get("packages").unwrap()),
        vec!["ripgrep".to_string(), "fd".to_string()]
    );
    assert_eq!(
        fixture_strings(nix_value.get("buildInputs").unwrap()),
        vec!["nodejs".to_string()]
    );

    let errors = root
        .get("errors")
        .expect("Stage A error cases")
        .as_array()
        .expect("Stage A errors array");
    let error_case = errors[0].as_object().expect("Stage A error case object");
    let error_source = error_case
        .get("source")
        .expect("Stage A error source")
        .as_str()
        .expect("Stage A error source string");
    let error = evaluate_devshell(error_source, "x86_64-linux")
        .expect_err("dynamic package fixture must fail closed");
    assert_eq!(
        error.to_string(),
        error_case
            .get("jet_error")
            .expect("Stage A error projection")
            .as_str()
            .expect("Stage A error projection string")
    );
    let error_value = error_case
        .get("nix_value")
        .expect("Stage A reference error value")
        .as_object()
        .expect("Stage A reference error value object");
    assert_eq!(
        fixture_strings(error_value.get("packages").unwrap()),
        vec!["fd".to_string()]
    );

    let locks = root
        .get("locks")
        .expect("Stage A lock cases")
        .as_array()
        .expect("Stage A locks array");
    let lock_case = locks[0].as_object().expect("Stage A lock case object");
    let lock = lock_case
        .get("nix_value")
        .expect("Stage A lock value")
        .as_object()
        .expect("Stage A lock value object");
    assert!(matches!(lock.get("version"), Some(JSONValue::Num(value)) if *value == 7.0));
    let lock_nodes = lock
        .get("nodes")
        .expect("Stage A lock nodes")
        .as_object()
        .expect("Stage A lock nodes object");
    let nixpkgs = lock_nodes
        .get("nixpkgs")
        .expect("Stage A nixpkgs node")
        .as_object()
        .expect("Stage A nixpkgs node object");
    let locked = nixpkgs
        .get("locked")
        .expect("Stage A locked nixpkgs")
        .as_object()
        .expect("Stage A locked nixpkgs object");
    assert_eq!(
        locked.get("rev").unwrap().as_str().unwrap(),
        NIXPKGS_REVISION
    );
    assert_eq!(
        locked.get("narHash").unwrap().as_str().unwrap(),
        NIXPKGS_NAR_HASH
    );

    let identities = root
        .get("output_identities")
        .expect("Stage A output identities")
        .as_object()
        .expect("Stage A output identities object");
    let manifest = ValidatedOracleManifest::embedded().expect("embedded manifest");
    for system in REQUIRED_SYSTEMS {
        let fixture_identity = identities
            .get(system)
            .expect("required fixture system")
            .as_object()
            .expect("fixture system object");
        let manifest_identity = manifest.builds.get(system).expect("required manifest system");
        assert_eq!(
            fixture_identity.get("build_nar_hash").unwrap().as_str().unwrap(),
            manifest_identity.build_nar_hash.as_deref().unwrap()
        );
        assert_eq!(
            fixture_identity
                .get("executable_nar_hash")
                .unwrap()
                .as_str()
                .unwrap(),
            manifest_identity.executable_nar_hash.as_deref().unwrap()
        );
    }
}

fn fixture_strings(value: &JSONValue) -> Vec<String> {
    value
        .as_array()
        .expect("fixture string array")
        .iter()
        .map(|item| item.as_str().expect("fixture string").to_string())
        .collect()
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
fn pinned_inventory_has_no_implicit_skip_reason() {
    let inventory = pinned_inventory();
    assert!(inventory.iter().any(|entry| {
        entry.surface == "overlays-devshells-multi-output-packages"
            && entry.status == InventoryStatus::Covered
    }));
    assert!(inventory.iter().all(|entry| !entry.reason.trim().is_empty()));
    assert_eq!(evaluator_budget().input_bytes, 1 << 20);
}

#[test]
fn native_derivation_builds_a_pure_request_with_required_builtins() {
    let evaluated = evaluate_derivation(
        r#"
        builtins.derivationStrict {
          name = builtins.concatStringsSep "-" [ "hello" "native" ];
          system = builtins.currentSystem;
          builder = "/bin/sh";
          args = builtins.map (value: value) [ "-c" "echo hi > $out" ];
          message = builtins.toJSON (builtins.fromJSON "{\"ok\":true,\"count\":2}");
          source = builtins.storePath "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-source";
        }
        "#,
        "x86_64-linux",
    )
    .expect("required derivation builtins must evaluate");
    assert_eq!(evaluated.name(), "hello-native");
    assert_eq!(evaluated.system(), "x86_64-linux");
    assert_eq!(evaluated.builder(), "/bin/sh");
    assert_eq!(
        evaluated.args(),
        &["-c".to_string(), "echo hi > $out".to_string()]
    );
    assert_eq!(
        evaluated.env().get("message"),
        Some(&"{\"count\":2,\"ok\":true}".to_string())
    );
    assert_eq!(
        evaluated.input_sources(),
        &["/nix/store/0123456789abcdfghijklmnpqrsvwxyz-source".to_string()]
    );
    assert_eq!(evaluated.outputs()[0].name(), "out");
    assert_eq!(evaluated.outputs()[0].method_algo(), "");
    assert_eq!(evaluated.outputs()[0].hash_hex(), "");
}

#[test]
fn native_derivation_keeps_lazy_fields_lazy_until_strict() {
    let evaluated = evaluate_devshell(
        r#"
        let d = builtins.derivation {
          name = "lazy";
          system = "x86_64-linux";
          builder = "/bin/sh";
          unused = unused;
        };
        in { devShells.x86_64-linux.default = pkgs.mkShell { packages = [ d.type ]; }; }
        "#,
        "x86_64-linux",
    )
    .expect("derivation must not force unused lazy fields");
    assert_eq!(evaluated.packages(), &["derivation".to_string()]);

    let error = evaluate_devshell(
        r#"
        let d = builtins.derivationStrict {
          name = "strict";
          system = "x86_64-linux";
          builder = "/bin/sh";
          unused = unused;
        };
        in { devShells.x86_64-linux.default = pkgs.mkShell { packages = [ d.type ]; }; }
        "#,
        "x86_64-linux",
    )
    .expect_err("derivationStrict must force unused fields");
    assert!(matches!(error, EvaluationError::Invalid(reason) if reason.contains("missing foreign flake value `unused`")));
}

#[test]
fn native_required_string_builtins_match_nix_edge_ordering() {
    let evaluated = evaluate_derivation(
        r#"builtins.derivation { name = builtins.replaceStrings [ "a" "ab" ] [ "X" "Y" ] "ab"; system = "x86_64-linux"; builder = "/bin/sh"; }"#,
        "x86_64-linux",
    )
    .expect("replaceStrings must use Nix list order");
    assert_eq!(evaluated.name(), "Xb");

    let evaluated = evaluate_derivation(
        r#"builtins.derivation { name = builtins.substring 1 (-1) "abc"; system = "x86_64-linux"; builder = "/bin/sh"; }"#,
        "x86_64-linux",
    )
    .expect("negative substring length must mean the remainder");
    assert_eq!(evaluated.name(), "bc");

    let error = evaluate_derivation(
        r#"builtins.derivation { name = builtins.substring (-1) 2 "abc"; system = "x86_64-linux"; builder = "/bin/sh"; }"#,
        "x86_64-linux",
    )
    .expect_err("negative substring start must fail");
    assert!(matches!(error, EvaluationError::Invalid(reason) if reason.contains("negative start")));
}

#[test]
fn native_derivation_rejects_noncanonical_inputs_and_preserves_multiple_outputs() {
    let path_error = evaluate_derivation(
        r#"builtins.derivation { name = "bad"; system = "x86_64-linux"; builder = "/bin/sh"; src = "${pkgs.fd}"; }"#,
        "x86_64-linux",
    )
    .expect_err("package placeholders must not become derivation inputs");
    assert!(matches!(path_error, EvaluationError::Unsupported(reason) if reason.contains("canonical store-path context")));

    let evaluated = evaluate_derivation(
        r#"builtins.derivation { name = "many"; system = "x86_64-linux"; builder = "/bin/sh"; outputs = [ "out" "dev" ]; }"#,
        "x86_64-linux",
    )
    .expect("bounded evaluator must preserve non-fixed output declarations");
    assert_eq!(
        evaluated
            .outputs()
            .iter()
            .map(|output| output.name())
            .collect::<Vec<_>>(),
        vec!["out", "dev"]
    );
    assert_eq!(evaluated.env().get("dev"), Some(&String::new()));
}

#[test]
fn native_derivation_rejects_duplicate_or_invalid_outputs() {
    for source in [
        r#"builtins.derivation { name = "duplicate"; system = "x86_64-linux"; builder = "/bin/sh"; outputs = [ "out" "out" ]; }"#,
        r#"builtins.derivation { name = "invalid"; system = "x86_64-linux"; builder = "/bin/sh"; outputs = [ "out" "bad/name" ]; }"#,
    ] {
        let error = evaluate_derivation(source, "x86_64-linux")
            .expect_err("invalid output declarations must fail before materialization");
        assert!(matches!(error, EvaluationError::Invalid(reason) if reason.contains("output")));
    }
}

#[test]
fn native_derivation_fixture_matches_pure_request() {
    let fixture = JSON::parse(STAGE_A_DERIVATION_FIXTURE).expect("derivation fixture must parse");
    let root = fixture.as_object().expect("derivation fixture root");
    let values = root
        .get("values")
        .expect("derivation fixture values")
        .as_array()
        .expect("derivation fixture values array");
    for value in values {
        let value = value.as_object().expect("derivation value object");
        let source = value.get("source").unwrap().as_str().unwrap();
        let system = value.get("system").unwrap().as_str().unwrap();
        let evaluated = evaluate_derivation(source, system)
            .expect("fixture derivation must evaluate natively");
        let expected = value.get("jet_request").unwrap().as_object().unwrap();
        assert_eq!(evaluated.name(), expected.get("name").unwrap().as_str().unwrap());
        assert_eq!(evaluated.system(), expected.get("system").unwrap().as_str().unwrap());
        assert_eq!(
            evaluated.builder(),
            expected.get("builder").unwrap().as_str().unwrap()
        );
        assert_eq!(
            evaluated.args(),
            &fixture_strings(expected.get("args").unwrap())
        );
        assert_eq!(
            evaluated.input_sources(),
            &fixture_strings(expected.get("input_sources").unwrap())
        );
        assert_eq!(evaluated.outputs().len(), 1);
        let output = evaluated.outputs().first().unwrap();
        let expected_output = expected.get("output").unwrap().as_object().unwrap();
        assert_eq!(output.name(), expected_output.get("name").unwrap().as_str().unwrap());
        assert_eq!(
            output.method_algo(),
            expected_output.get("method_algo").unwrap().as_str().unwrap()
        );
        assert_eq!(
            output.hash_hex(),
            expected_output.get("hash_hex").unwrap().as_str().unwrap()
        );
    }
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

#[test]
fn native_devshell_applies_bounded_package_overlays() {
    let evaluated = evaluate_devshell(
        r#"
        let overlay = final: prev: { custom = prev.fd; };
        in {
          devShells.x86_64-linux.default =
              (import pkgs { overlays = [ overlay ]; }).mkShell {
              packages = [ (import pkgs { overlays = [ overlay ]; }).custom ];
            };
        }
        "#,
        "x86_64-linux",
    )
    .expect("bounded package overlays must project package identities");
    assert_eq!(evaluated.packages(), &["fd".to_string()]);
}

#[test]
fn native_devshell_overlay_final_sees_following_override_and_prev_stays_stable() {
    let evaluated = evaluate_devshell(
        r#"
        let first = final: prev: {
          from_prev = prev.fd;
          from_final = final.future;
        };
        second = final: prev: {
          future = prev.ripgrep;
          second_view = final.future;
        };
        in {
          devShells.x86_64-linux.default =
            (import pkgs { overlays = [ first second ]; }).mkShell {
              packages = [
                (import pkgs { overlays = [ first second ]; }).from_prev
                (import pkgs { overlays = [ first second ]; }).from_final
                (import pkgs { overlays = [ first second ]; }).second_view
              ];
            };
        }
        "#,
        "x86_64-linux",
    )
    .expect("overlay final/prev projections must remain deterministic");
    assert_eq!(evaluated.packages(), &["fd", "ripgrep"]);
}

#[test]
fn native_devshell_merge_keeps_overlay_packages_lazy() {
    let evaluated = evaluate_devshell(
        r#"
        let overlayPkgs = pkgs // { custom = pkgs.ripgrep; };
        in { devShells.x86_64-linux.default = overlayPkgs.mkShell { packages = [ overlayPkgs.custom ]; }; }
        "#,
        "x86_64-linux",
    )
    .expect("package-set merge must preserve explicit overlay fields");
    assert_eq!(evaluated.packages(), &["ripgrep".to_string()]);
}

#[test]
fn native_evaluator_rejects_fetchers_and_cross_system_packages_without_authority() {
    let fetch_error = evaluate_devshell(
        r#"{
          devShells.x86_64-linux.default = pkgs.mkShell {
            packages = [ (builtins.fetchurl { url = "https://example.invalid/source"; }) ];
          };
        }"#,
        "x86_64-linux",
    )
    .expect_err("fetchers must not gain implicit network authority");
    assert!(matches!(
        fetch_error,
        EvaluationError::Unsupported(reason)
            if reason.contains("explicit fetch authority")
    ));

    let cross_error = evaluate_devshell(
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.pkgsCross.aarch64-multiplatform.foo ]; }; }",
        "x86_64-linux",
    )
    .expect_err("cross-system packages need an explicit target boundary");
    assert!(matches!(
        cross_error,
        EvaluationError::Unsupported(reason)
            if reason.contains("explicit target")
    ));
}
