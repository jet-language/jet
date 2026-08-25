use std::fs;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;

use jetpack_fixtures::{jetpack, Scratch};

#[test]
fn combined_update_plans_both_scopes_before_one_confirmation() {
    let project = Scratch::new("combined-update-project");
    let root = Scratch::new("combined-update-root");
    let fixtures = Scratch::new("combined-update-fixtures");
    let home = Scratch::new("combined-update-home");
    fs::write(
        project.join("env.jet"),
        "module dev {\n    sources: { default: acme/tools@github#latest }\n    env.dev: Env{ packages: [default.greet] }\n}\n",
    )
    .unwrap();
    fs::create_dir_all(&fixtures.path).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        "github:acme/tools latest github:acme/tools#v1.2.0 240000000\n\
         core:omp latest core:omp#v2 676000000\n",
    )
    .unwrap();
    let manifest_dir = home.join(".jet/tools");
    fs::create_dir_all(&manifest_dir).unwrap();
    fs::write(
        manifest_dir.join("manifest.json"),
        r##"{
  "profile":"tools",
  "schema":"jet-user-tools-v1",
  "sources":[{"name":"core","policy":"#latest","provider":"core","raw":"omp@core#latest","upstream":"core:omp#v1"}],
  "tools":[{"bins":[],"members":[],"name":"omp","reference":"omp@core#latest","resolved":"omp@core#v1","tier":"#latest"}]
}
"##,
    )
    .unwrap();

    let output = jetpack()
        .args(["update", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let deps = stderr.find("deps  default").unwrap();
    let tools = stderr.find("tools core").unwrap();
    let gate = stderr.find("plan only; pass -y or --yes").unwrap();
    assert!(deps < gate, "stderr: {stderr}");
    assert!(tools < gate, "stderr: {stderr}");
    assert!(stderr.contains("1 package, 240 MB"), "stderr: {stderr}");
    assert!(stderr.contains("1 package, 676 MB"), "stderr: {stderr}");
    assert_eq!(stderr.matches("plan only;").count(), 1, "stderr: {stderr}");
    assert!(!project.join(".jet/lock").exists());
    assert!(fs::read_to_string(manifest_dir.join("manifest.json"))
        .unwrap()
        .contains("omp@core#v1"));
}
