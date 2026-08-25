mod common;

use std::fs;
use std::process::Command;

use common::Scratch;

#[test]
fn jet_test_outside_project_environment_refuses_without_acquiring() {
    let project = Scratch::new("jet-env-boundary");
    let jetpack_root = project.join("jetpack-root-must-not-be-created");
    fs::write(
        project.join("package.jet"),
        "name: \"jet-env-boundary\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev {\n    sources: { stable: NixOS/nixpkgs/nixos-24.05@github }\n}\n",
    )
    .unwrap();
    fs::write(project.join("run.jet"), "fn run() {}\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["test", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &jetpack_root)
        .env_remove("JETPACK_ENV")
        .env_remove("JETPACK_ENV_DIR")
        .env_remove("JETPACK_ENV_HASH")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        include_str!("fixtures/jetpack-diagnostics/E1355.stderr")
    );
    assert!(
        !jetpack_root.exists(),
        "jet must not create a Jetpack root while refusing the command"
    );
    assert!(
        !project.join(".jet").exists(),
        "jet must not create project state while refusing the command"
    );
}
