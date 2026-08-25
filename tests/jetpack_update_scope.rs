use std::fs;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;

use jetpack_fixtures::{jetpack, write_channel_fixture, Scratch};

#[test]
fn update_from_nested_cwd_reports_and_uses_project_scope() {
    let project = Scratch::new("update-scope-project");
    let root = Scratch::new("update-scope-root");
    let fixtures = Scratch::new("update-scope-fixtures");
    let home = Scratch::new("update-scope-home");
    let nested = project.join("packages/app");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        project.join("env.jet"),
        "module dev { sources: { default: acme/tools@github#latest } env.dev: Env{ packages: [default.greet] } }\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.2.0",
    );

    let output = jetpack()
        .args(["update", "--no-color", "--yes", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&nested)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("scope: project dependencies"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("scope: user tools"), "stderr: {stderr}");
    assert!(
        stderr.contains(project.path.to_string_lossy().as_ref()),
        "stderr: {stderr}"
    );
    assert!(
        fs::read_to_string(project.join(".jet/lock"))
            .unwrap()
            .contains("exact = \"github:acme/tools#v1.2.0\"")
    );
}
