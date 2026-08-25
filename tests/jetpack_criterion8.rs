//! Criterion 8: an imported locked package stays on Jetpack after import.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jetpack, write_executable, Scratch};

#[test]
fn imported_locked_package_runs_builds_and_restores_without_nix() {
    let base = Scratch::new("criterion8-import-reuse");
    let project = base.join("project");
    let root = base.join("root");
    let restored_root = base.join("restored-root");
    let fixtures = base.join("fixtures");
    let staging = base.join("staging");
    let tools = base.join("tools");
    let marker = base.join("nix-invoked");
    fs::create_dir_all(project.join(".jet")).unwrap();
    fs::create_dir_all(staging.join("bin")).unwrap();
    fs::create_dir_all(&fixtures).unwrap();
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        project.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"jetpack\"\nchannel = \"jetpack\"\nexact = \"github:NixOS/nixpkgs#0123456789abcdef0123456789abcdef01234567\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_executable(
        &staging.join("bin/greet"),
        "#!/bin/sh\nprintf '%s\\n' 'hello from imported jetpack'\n",
    );
    jetpack::Store::seal_local_output(&staging).unwrap();
    let output_hash = jetpack::Envelope::try_output_hash_of(&staging.to_string_lossy()).unwrap();
    fs::write(
        fixtures.join("jetpack-greet.json"),
        format!(
            "[{{\"drvPath\":\"/nix/store/0fixture00000000000000000000-greet.drv\",\"outputs\":{{\"out\":{:?}}},\"jetpackImport\":{{\"closedGraph\":{{\"root\":{{\"dependencies\":[]}}}},\"selectedOutputs\":{{\"out\":{:?}}},\"dependencies\":[],\"sources\":[],\"hashes\":{{\"out\":{:?}}},\"losses\":[],\"proof\":{{\"source\":\"criterion-8\"}},\"recipe\":{{\"kind\":\"compatibility\"}},\"lock\":{{\"system\":\"x86_64-linux\"}}}}}}]",
            staging.to_string_lossy(),
            staging.to_string_lossy(),
            output_hash
        ),
    )
    .unwrap();
    write_executable(
        &tools.join("nix"),
        &format!(
            "#!/bin/sh\nprintf '%s' invoked > {:?}\nexit 91\n",
            marker.to_string_lossy()
        ),
    );

    let imported = jetpack()
        .args(["build", "greet@jetpack", "--offline", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env("JETPACK_FIXTURES", &fixtures)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "import stderr: {}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let imported_roots = jetpack::Store::Roots::at(root.clone());
    let imported_entry = jetpack::Store::find_by_reference(&imported_roots, "greet@jetpack")
        .expect("successful import must publish the Jetpack package");
    assert!(Path::new(&imported_entry.out).starts_with(root.join("hangar/objects")));
    let producer = jetpack::Store::ProducerRecord::decode(&imported_entry.producer_record).unwrap();
    assert_eq!(
        producer.facts.get("nix.fallback.schema").map(String::as_str),
        Some("jetpack.nix-fallback.v1")
    );
    assert!(fs::read_to_string(project.join(".jet/lock")).unwrap().contains("greet@jetpack"));

    let build = jetpack()
        .args(["build", "greet@jetpack", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env_remove("JETPACK_FIXTURES")
        .env("PATH", &tools)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "cached build stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = jetpack()
        .args(["run", "greet@jetpack", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env_remove("JETPACK_FIXTURES")
        .env("PATH", &tools)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "cached run stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "hello from imported jetpack"
    );
    assert!(!marker.exists(), "cached Jetpack operations invoked Nix");

    let dumped = jetpack()
        .args(["hangar", "dump", "greet@jetpack", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .env("PATH", &tools)
        .output()
        .unwrap();
    assert!(
        dumped.status.success(),
        "dump stderr: {}",
        String::from_utf8_lossy(&dumped.stderr)
    );
    let key = root.join("trust/hangar.key");
    assert!(key.is_file(), "Jetpack dump must publish its Hangar key");
    let mut restore = jetpack()
        .args([
            "hangar",
            "restore",
            "--key",
            key.to_str().unwrap(),
            "--yes",
            "--no-color",
        ])
        .current_dir(&project)
        .env("JETPACK_ROOT", &restored_root)
        .env("PATH", &tools)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    restore
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&dumped.stdout)
        .unwrap();
    drop(restore.stdin.take());
    let restored = restore.wait_with_output().unwrap();
    assert!(
        restored.status.success(),
        "restore stderr: {}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let restored_entry = jetpack::Store::find_by_reference(
        &jetpack::Store::Roots::at(restored_root.clone()),
        "greet@jetpack",
    )
    .expect("Jetpack restore must preserve the locked package ref");
    assert_eq!(
        restored_entry.envelope.output_hash,
        imported_entry.envelope.output_hash
    );
    assert!(!marker.exists(), "Jetpack restore invoked Nix");

    let restored_run = jetpack()
        .args(["run", "greet@jetpack", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &restored_root)
        .env_remove("JETPACK_FIXTURES")
        .env("PATH", &tools)
        .output()
        .unwrap();
    assert!(
        restored_run.status.success(),
        "restored run stderr: {}",
        String::from_utf8_lossy(&restored_run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&restored_run.stdout).trim(),
        "hello from imported jetpack"
    );
    assert!(!marker.exists(), "restored package execution invoked Nix");
}
