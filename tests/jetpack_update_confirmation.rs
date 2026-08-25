use std::fs;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;

use jetpack_fixtures::{jetpack, write_channel_fixture, Scratch};

#[test]
fn update_noninteractive_requires_yes_and_yes_applies() {
    let project = Scratch::new("update-confirmation-project");
    let root = Scratch::new("update-confirmation-root");
    let fixtures = Scratch::new("update-confirmation-fixtures");
    fs::write(
        project.join("env.jet"),
        "module dev {\n    sources: { default: acme/tools@github#latest }\n    env.dev: Env{ packages: [default.greet] }\n}\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.2.0",
    );

    let plan_only = jetpack()
        .args(["update", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(plan_only.status.code(), Some(2));
    let plan_stderr = String::from_utf8_lossy(&plan_only.stderr);
    assert!(
        plan_stderr.contains("plan only; pass -y or --yes to apply in a non-interactive shell"),
        "stderr: {plan_stderr}"
    );
    assert!(!project.join(".jet/lock").exists());

    let applied = jetpack()
        .args(["update", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let applied_stderr = String::from_utf8_lossy(&applied.stderr);
    assert!(applied_stderr.contains("applying plan (--yes)"));
    assert!(!applied_stderr.contains("Apply?"));
    assert!(fs::read_to_string(project.join(".jet/lock"))
        .unwrap()
        .contains("exact = \"github:acme/tools#v1.2.0\""));
}
