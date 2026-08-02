use super::Boundary::NativeBoundary;

#[test]
fn private_integration_has_pinned_product_ready_evaluator() {
    let boundary = NativeBoundary::embedded().expect("committed manifest must validate");
    assert!(boundary.product_ready());
}

#[test]
fn private_integration_projects_lazy_flake_without_external_nix() {
    let boundary = NativeBoundary::embedded().expect("committed manifest must validate");
    let evaluated = boundary
        .evaluate_devshell(
            "let make = packages: pkgs.mkShell { packages = packages; }; in { outputs = { devShells.x86_64-linux.default = make [ pkgs.fd ]; }; }",
            "x86_64-linux",
        )
        .expect("private production boundary must evaluate supported lazy syntax");
    assert_eq!(evaluated.packages(), &["fd".to_string()]);

    let error = boundary
        .evaluate_devshell(
            "{ outputs = { devShells.x86_64-linux.default = pkgs.mkShell { packages = /tmp/package; }; }; }",
            "x86_64-linux",
        )
        .expect_err("private production boundary must reject ambient path authority");
    assert!(error
        .to_string()
        .contains("absolute paths require explicit project-root authority"));
}

#[test]
fn private_integration_materializes_derivation_without_external_nix() {
    let evaluated = super::evaluate_derivation(
        r#"builtins.derivationStrict { name = "hello"; system = "x86_64-linux"; builder = "/bin/sh"; args = [ "-c" "echo hi > $out" ]; }"#,
        "x86_64-linux",
    )
        .expect("private production boundary must materialize a derivation");
    assert_eq!(
        evaluated.drv_path(),
        "/nix/store/76w21n1f03fs5kw8fnffphx7qrqffw6r-hello.drv"
    );
    assert_eq!(
        evaluated.outputs().get("out").map(String::as_str),
        Some("/nix/store/mjs27ix6ig2bkbi3s3sm470vrv4lf7ic-hello")
    );
}

#[test]
fn private_integration_materializes_fixed_output_derivation() {
    let evaluated = super::evaluate_derivation(
        r#"builtins.derivationStrict { name = "fixed"; system = "x86_64-linux"; builder = "/bin/sh"; args = [ "-c" "echo hi > $out" ]; outputHashAlgo = "sha256"; outputHashMode = "flat"; outputHash = "0000000000000000000000000000000000000000000000000000000000000000"; }"#,
        "x86_64-linux",
    )
        .expect("private production boundary must materialize fixed output");
    assert_eq!(
        evaluated.drv_path(),
        "/nix/store/24mj56f9sfhlf0bd7x0h9xgfc709a1fn-fixed.drv"
    );
    assert_eq!(
        evaluated.outputs().get("out").map(String::as_str),
        Some("/nix/store/ap9h69qwrm5060ldi96axyklh3pr3yjn-fixed")
    );
}

#[test]
fn private_derivation_path_is_independent_of_nix_on_path() {
    const CHILD: &str = "JETPACK_NATIVE_DERIVATION_NO_PATH_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "NixEval::Tests::private_derivation_path_is_independent_of_nix_on_path",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let evaluated = super::evaluate_derivation(
        r#"builtins.derivation { name = "no-path"; system = "x86_64-linux"; builder = "/bin/sh"; }"#,
        "x86_64-linux",
    )
        .expect("native derivation evaluator must not need Nix");
    assert!(evaluated.drv_path().ends_with("-no-path.drv"));
}
