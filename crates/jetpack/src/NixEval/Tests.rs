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
