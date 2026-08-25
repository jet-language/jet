//! Card #2191 criterion 4: non-interactive acquisition needs explicit consent.

use std::fs;

mod common;

use jetpack::SHA256;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jetpack, Scratch};

fn write_native_omp_fixture(fixtures: &std::path::Path) {
    fs::create_dir_all(fixtures).unwrap();
    let artifact = fixtures.join("omp-1.0.0");
    fs::write(&artifact, "#!/bin/sh\nprintf '%s\\n' downloaded\n").unwrap();
    let digest = SHA256::sha256_file_hex(&artifact).unwrap();
    fs::write(
        fixtures.join("jetpackage-omp.json"),
        format!(
            "{{\"tag\":\"v1.0.0\",\"version\":\"1.0.0\",\"sha256\":\"{digest}\",\"artifact\":\"omp-1.0.0\"}}"
        ),
    )
    .unwrap();
}

#[test]
fn non_tty_download_without_yes_fails_before_realization() {
    let root = Scratch::new("acquisition-non-tty-root");
    let project = Scratch::new("acquisition-non-tty-project");
    let fixtures = Scratch::new("acquisition-non-tty-fixtures");
    let home = Scratch::new("acquisition-non-tty-home");
    write_native_omp_fixture(&fixtures.path);

    let output = jetpack()
        .args([
            "use",
            "omp@releases#1.0.0",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .args(["--", "omp"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("-y"), "stderr: {stderr}");
    assert!(output.stdout.is_empty());
    assert!(jetpack::Store::list(&jetpack::Store::Roots::at(root.path.clone())).is_empty());
}
