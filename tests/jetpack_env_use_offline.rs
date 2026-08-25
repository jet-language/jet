//! Card #2191 criterion 2: fully materialized env/use entry is offline and silent.

use std::fs;

mod common;

use jetpack::SHA256;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jetpack, Scratch};

fn write_native_omp_fixture(fixtures: &std::path::Path) {
    fs::create_dir_all(fixtures).unwrap();
    let artifact = fixtures.join("omp-1.0.0");
    fs::write(&artifact, "#!/bin/sh\nprintf '%s\\n' cached\n").unwrap();
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
fn fully_materialized_env_and_use_enter_without_network_or_prompt() {
    let project = Scratch::new("env-use-offline-project");
    let root = Scratch::new("env-use-offline-root");
    let fixtures = Scratch::new("env-use-offline-fixtures");
    let home = Scratch::new("env-use-offline-home");
    write_native_omp_fixture(&fixtures.path);
    fs::write(
        project.join("env.jet"),
        "module env.dev { packages: [\"omp@releases#1.0.0\"] }\n",
    )
    .unwrap();

    let prep = jetpack()
        .args([
            "env",
            "--prep",
            "--yes",
            "--trust",
            "--offline",
            "--no-color",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        prep.status.success(),
        "env --prep failed: {}",
        String::from_utf8_lossy(&prep.stderr)
    );

    fs::remove_file(fixtures.path.join("jetpackage-omp.json")).unwrap();
    fs::remove_file(fixtures.path.join("omp-1.0.0")).unwrap();

    for args in [
        vec!["env", "--trust", "--offline", "--no-color", "--", "omp"],
        vec![
            "use",
            "omp@releases#1.0.0",
            "--offline",
            "--no-color",
            "--",
            "omp",
        ],
    ] {
        let output = jetpack()
            .args(args)
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("HOME", &home.path)
            .env("JETPACK_DENY_NETWORK", "1")
            .env_remove("JETPACK_FIXTURES")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cached entry failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("continue?"), "cached entry prompted: {stderr}");
    }
}
