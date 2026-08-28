mod common;

use std::fs;
use std::process::Command;

use common::Scratch;

#[test]
fn toolchain_only_run_and_dev_skip_environment_realization() {
    let project = Scratch::new("jet-env-toolchain-only");
    let jetpack_root = project.join("jetpack-root-must-not-be-created");
    fs::write(
        project.join("package.jet"),
        "name: \"jet-env-toolchain-only\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev { packages: [nixpkgs.git] }\n",
    )
    .unwrap();
    fs::write(
        project.join("run.jet"),
        "use core.text as text\npub fn count() Int -> text.scalar_count(\"toolchain\")\nfn run() {}\n",
    )
    .unwrap();

    for active in [false, true] {
        for args in [
            vec!["run", "run.jet"],
            vec!["dev", "run.jet", "--watch=off"],
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
            command
                .args(args.iter())
                .current_dir(&project.path)
                .env("JETPACK_ROOT", &jetpack_root)
                .env_remove("JETPACK_ENV")
                .env_remove("JETPACK_ENV_DIR")
                .env_remove("JETPACK_ENV_HASH");
            if active {
                command
                    .env("JETPACK_ENV", "1")
                    .env("JETPACK_ENV_DIR", &project.path);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "toolchain-only {} failed (active={active}):\n{}",
                args[0],
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "",
                "toolchain-only {} output (active={active})",
                args[0]
            );
        }
    }
    assert!(
        !jetpack_root.exists(),
        "toolchain-only run/dev must not create a Jetpack root"
    );
}

#[test]
fn declared_package_import_gates_run_dev_and_test_without_acquiring() {
    let project = Scratch::new("jet-env-boundary");
    let jetpack_root = project.join("jetpack-root-must-not-be-created");
    fs::write(
        project.join("package.jet"),
        "name: \"jet-env-boundary\"\nversion: \"0.1.0\"\ndeps: { helper: ./helper }\n",
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev {\n    sources: { stable: NixOS/nixpkgs/nixos-24.05@github }\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("run.jet"),
        "use helper\nfn run() {}\n",
    )
    .unwrap();
    fs::write(project.join("helper.jet"), "fn help() {}\n").unwrap();

    for args in [
        vec!["run", "run.jet"],
        vec!["dev", "run.jet", "--watch=off"],
        vec!["test", "run.jet"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(args.iter())
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &jetpack_root)
            .env_remove("JETPACK_ENV")
            .env_remove("JETPACK_ENV_DIR")
            .env_remove("JETPACK_ENV_HASH")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(1), "{} should be gated", args[0]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Error [E1355]"), "{} stderr: {stderr}", args[0]);
        assert!(stderr.contains("Import: `use helper`"), "{} stderr: {stderr}", args[0]);
        if args[0] == "test" {
            assert_eq!(
                stderr,
                include_str!("fixtures/jetpack-diagnostics/E1355.stderr")
            );
        }
    }
    assert!(
        !jetpack_root.exists(),
        "jet must not create a Jetpack root while refusing the commands"
    );
    assert!(
        !project.join(".jet").exists(),
        "jet must not create project state while refusing the command"
    );
}
