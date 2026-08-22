//! Focused production-path proof for Package System/Fleet projections.

use std::fs;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jet, Scratch};

#[test]
fn package_fleet_output_reaches_the_immutable_generation_and_retries_cleanly() {
    let project = Scratch::new("package-fleet-output");
    let root = Scratch::new("package-fleet-output-root");
    fs::write(
        project.join("package.jet"),
        r#"name: "demo"
outputs: .{
    workstation: System{
        target: linux.x64
        options: .{ boot: .{ kernel: Linux }, init: .{ path: "/bin/init" } }
    }
    prod: Fleet{
        name: "prod"
        hosts: .{ edge: system.workstation }
    }
}"#,
    )
    .unwrap();

    let run = || {
        jet()
            .args([
                "os",
                "build",
                "workstation",
                "--name",
                "fleet-output",
                "--no-color",
                "--offline",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let deploy_plan = root.join("systems/generations/fleet-output/fleet/deploy-plan.json");
    let script = root.join("systems/generations/fleet-output/fleet/deploy-edge.sh");
    let system_plan = root.join("systems/generations/fleet-output/plan.json");
    let first_plan = fs::read_to_string(&deploy_plan).unwrap();
    let first_system_plan = fs::read_to_string(&system_plan).unwrap();
    assert!(first_system_plan.contains("\"graph_identity\":\""));
    let graph_identity = first_system_plan
        .split_once("\"graph_identity\":\"")
        .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value))
        .expect("system plan graph identity");
    assert!(
        first_plan.contains(&format!("\"graph_identity\":\"{graph_identity}\"")),
        "{first_plan}"
    );
    assert!(first_plan.contains("\"fleet\":\"prod\""), "{first_plan}");
    assert!(first_plan.contains("\"host\":\"edge\""), "{first_plan}");
    assert!(
        first_plan.contains("\"system\":\"workstation\""),
        "{first_plan}"
    );
    assert!(script.is_file(), "missing generated Fleet host script");
    let script_text = fs::read_to_string(&script).unwrap();
    assert!(
        script_text.contains(&format!("\"graph_identity\":\"{graph_identity}\"")),
        "{script_text}"
    );

    let root_proof = fs::read_to_string(
        root.join("systems/generations/fleet-output/generation-root.json"),
    )
    .unwrap();
    assert!(
        root_proof.contains("\"kind\":\"jetos.generation-root.v1\"")
            && root_proof.contains("\"source_proof_sha256\":\"")
            && root_proof.contains("\"files_proof_sha256\":\"")
            && root_proof.contains("\"witness\":\""),
        "{root_proof}"
    );
    let ledger = fs::read_to_string(root.join("systems/generations.log")).unwrap();
    let ledger_row = ledger
        .lines()
        .find(|line| line.contains("\tfleet-output\t"))
        .expect("generation ledger row");
    let witness = root_proof
        .split_once("\"witness\":\"")
        .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value))
        .expect("generation root witness");
    assert!(ledger_row.ends_with(&format!("\t{witness}")), "{ledger}");

    let second = run();
    assert!(
        second.status.success(),
        "retry stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(fs::read_to_string(&deploy_plan).unwrap(), first_plan);
    assert_eq!(fs::read_to_string(&system_plan).unwrap(), first_system_plan);
}

#[test]
fn package_fleet_path_escape_fails_before_generation_publication() {
    let project = Scratch::new("package-fleet-path-escape");
    let root = Scratch::new("package-fleet-path-escape-root");
    fs::write(
        project.join("package.jet"),
        r#"name: "demo"
outputs: .{
    workstation: System{
        target: linux.x64
        options: .{ boot: .{ kernel: Linux }, init: .{ path: "/bin/init" } }
    }
    prod: Fleet{ hosts: .{ "../escape": system.workstation } }
}"#,
    )
    .unwrap();

    let output = jet()
        .args([
            "os",
            "plan",
            "workstation",
            "--json",
            "--no-color",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("safe path component"), "{stderr}");
    assert!(!root.join("systems").exists());
}
