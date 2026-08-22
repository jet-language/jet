//! Top-level `jet` → `jetpack` command dispatch tests (Tower card #367
//! slice 6 split).
//!
//! These cover the CLI delegation surface — `jet clean`/`jet env`/
//! `jet outdated`/`jet run tool@nixpkgs` routing through to the `jetpack`
//! engine — as distinct from the engine mechanics themselves (see
//! `tests/jetpack_engine.rs`). Split out of the former `tests/jetpack.rs`.

use std::fs;
use std::process::Command;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

#[test]
fn jet_clean_delegates_to_jetpack_clean() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-top", "oldtop", "1.0", Some(1)).0;

    let out = jet()
        .args(["clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stale.exists(), "`jet clean` should collect via jetpack");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cleaned hangar"), "stderr: {stderr}");
}

#[test]
fn jet_shared_store_status_does_not_default_to_install() {
    let home = Scratch::new("shared-store-status-home");
    let data = Scratch::new("shared-store-status-data");
    let state = Scratch::new("shared-store-status-state");
    let legacy_hangar = state.join("jet/hangar");
    fs::create_dir_all(&legacy_hangar).unwrap();
    fs::write(legacy_hangar.join("marker"), "legacy").unwrap();

    let output = jet()
        .args(["shared-store", "status", "--no-color"])
        .env_remove("JETPACK_ROOT")
        .env("HOME", &home.path)
        .env("XDG_DATA_HOME", &data.path)
        .env("XDG_STATE_HOME", &state.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("shared-store broker is not installed."),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("Error [E1340]"), "stderr: {stderr}");
    assert!(
        !stderr.contains("shared-store install failed"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("sudo"), "stderr: {stderr}");
    assert!(
        !stderr.contains("jetpack shared-store install"),
        "stderr: {stderr}"
    );
    assert!(legacy_hangar.join("marker").is_file());
    assert!(!data.path.join("jet/hangar").exists());
    assert!(!data.path.join("jet/shared-store").exists());
}

/// D-CLI-SURFACE3=B: `outdated` moved under `jet inspect` — bare
/// `jet outdated` is now a teaching error (E2101) naming the new spelling.
#[test]
fn bare_jet_outdated_is_a_teaching_error_naming_jet_inspect_outdated() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: acme/tools#latest@github }
    env.dev: Env.{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(proj.join(".jet")).unwrap();
    fs::write(
        proj.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"default\"\nchannel = \"latest\"\nexact = \"github:acme/tools#v1.2.0\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.3.0",
    );

    let out = jet()
        .args(["outdated", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "bare `jet outdated` must be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "stderr: {stderr}");
    assert!(stderr.contains("jet inspect outdated"), "stderr: {stderr}");
}

/// `jet inspect outdated` is the canonical top-level spelling and still
/// dispatches through to the jetpack engine.
#[test]
fn jet_inspect_outdated_dispatches_to_jetpack() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: acme/tools#latest@github }
    env.dev: Env.{ packages: [default.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(proj.join(".jet")).unwrap();
    fs::write(
        proj.join(".jet/lock"),
        "version = 1\n\n[[source_channel]]\nname = \"default\"\nchannel = \"latest\"\nexact = \"github:acme/tools#v1.2.0\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.3.0",
    );

    let out = jet()
        .args(["inspect", "outdated", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("v1.2.0"), "stderr: {stderr}");
    assert!(stderr.contains("v1.3.0"), "stderr: {stderr}");
}


#[test]
fn jet_env_delegates_to_jetpack_enter() {
    // Card #955 / D-DEV4 (ratified 2026-06-17): `jet env` is the friendly Scale-2 front door
    // into the project dev shell — it delegates straight to `jetpack enter`,
    // forwarding flags and the trailing `-- cmd`. (`jet dev` is now reserved for
    // the E2-M4 watch/interpret loop.) Running through the `jet` binary must
    // reach the same composed env with no installed tool or network fallback.
    let (base, proj, root) = core_hello_project("jet-env");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        // U19: same trust gate reached through `jet env`; `--trust` bypasses.
        .args(["env", "--no-color", "--trust", "--offline", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "") // no installed tools, including nix, on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jet-pkgs"
    );
}

#[test]
fn jet_run_without_nix_compatibility_output_reports_e1272() {
    let root = Scratch::new("jet-run-no-nix-root");
    let output = jet()
        .args(["run", "postgres@nixpkgs", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("postgres@nixpkgs"), "stderr: {stderr}");
    assert!(
        stderr.contains("does not invoke an installed Nix executable"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
}

#[test]
fn jet_env_sync_delegates_to_typed_managed_file_sync() {
    let project = Scratch::new("jet-env-sync");
    let root = Scratch::new("jet-env-sync-root");
    let home = Scratch::new("jet-env-sync-home");
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    files: ["config/generated.txt": File{ content: "generated\n", mode: .Copy }]
}
"#,
    )
    .unwrap();

    let output = jet()
        .args(["env", "sync", "--trust", "--yes", "--offline", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("config/generated.txt")).unwrap(),
        "generated\n"
    );
    assert!(project.join(".jet/files/state").is_file());
}

#[test]
fn jet_env_info_discloses_typed_summary_without_realizing_or_starting_services() {
    let project = Scratch::new("jet-env-info");
    let root = Scratch::new("jet-env-info-root");
    fs::create_dir_all(project.join("scripts/githooks")).unwrap();
    fs::write(
        project.join("env.jet"),
        r#"module env.dev {
    prompt: $HOME
    packages: [nixpkgs.ripgrep]
    git_hooks_path: "scripts/githooks"
    services: {
        fixture: {
            enable: false,
            ports: [8080],
            run: ["fixture", "--port", "8080"],
            ready: "fixture --ready",
            data_dir: "state/fixture",
            watch: ["src"],
            after: ["database"],
            before_start: ["lint"],
            sockets: ["run/fixture.sock"]
        }
    }
    files: ["config/generated.txt": File{ content: "generated\n", mode: .Copy }]
}
module env.full {
    prompt: $FULL_ONLY
    packages: [nixpkgs.fd]
    services: {
        sibling: { enable: false, ports: [9090] }
    }
    checks: [{
        name: "full-only-check",
        command: "true",
    }]
    files: ["config/full.txt": File{ content: "full\n", mode: .Copy }]
}
"#,
    )
    .unwrap();
    fs::write(project.join("run.jet"), "#Job\nfn lint() {}\n").unwrap();

    let output = jet()
        .args(["env", "info", "--json", "--offline", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", "/test/home")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        jetpack::JSON::parse(&stdout).is_ok(),
        "info JSON must parse: {stdout}"
    );
    for fact in [
        "\"packages\":[\"ripgrep@nixpkgs\"]",
        "\"name\":\"fixture\",\"enabled\":false",
        "\"run\":[\"fixture\",\"--port\",\"8080\"]",
        "\"ready\":\"fixture --ready\"",
        "\"after\":[\"database\"]",
        "\"before_start\":[\"lint\"]",
        "\"sockets\":[\"run/fixture.sock\"]",
        "\"checks\":[],\"jobs\":[\"lint\"]",
        "\"variables\":[{\"name\":\"HOME\",\"sources\":[\"environment\"]}]",
        "\"files\":[\"config/generated.txt\"]",
        "\"git_hooks_path\":\"scripts/githooks\"",
    ] {
        assert!(
            stdout.contains(fact),
            "missing {fact}; status={:?}; stderr={}; stdout={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
            stdout
        );
    }
    assert!(!stdout.contains("\"name\":\"sibling\""));
    assert!(!stdout.contains("full-only-check"));
    assert!(!stdout.contains("FULL_ONLY"));
    assert!(!stdout.contains("config/full.txt"));
    assert!(
        !project.join(".jet/services").exists(),
        "env info must not start a service supervisor"
    );

    let human = jet()
        .args(["env", "info", "--offline", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", "/test/home")
        .output()
        .unwrap();
    assert!(
        human.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human_output = format!(
        "{}{}",
        String::from_utf8_lossy(&human.stdout),
        String::from_utf8_lossy(&human.stderr)
    );
    for fact in [
        "fixture (disabled",
        "ports=8080",
        "run=fixture --port 8080",
        "ready=fixture --ready",
        "data_dir=state/fixture",
        "watch=src",
        "after=database",
        "before_start=lint",
        "sockets=run/fixture.sock",
        "git hooks path: scripts/githooks",
    ] {
        assert!(
            human_output.contains(fact),
            "missing {fact}; output={human_output}"
        );
    }
    assert!(
        !project.join(".jet/services").exists(),
        "human env info must not start a service supervisor"
    );

    let full = jet()
        .args(["env", "info", "--json", "--offline", "--no-color", "--env", "full"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", "/test/home")
        .output()
        .unwrap();
    assert!(
        full.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&full.stderr)
    );
    let full_stdout = String::from_utf8_lossy(&full.stdout);
    assert!(full_stdout.contains("\"active_environment\":\"full\""));
    assert!(full_stdout.contains("\"packages\":[\"fd@nixpkgs\"]"));
    assert!(full_stdout.contains("\"name\":\"sibling\",\"enabled\":false"));
    assert!(full_stdout.contains("\"checks\":[\"full-only-check\"]"));
    assert!(full_stdout.contains("\"name\":\"FULL_ONLY\""));
    assert!(full_stdout.contains("\"files\":[\"config/full.txt\"]"));
    assert!(!full_stdout.contains("\"name\":\"fixture\""));
    assert!(!full_stdout.contains("config/generated.txt"));

    let missing = jet()
        .args(["env", "info", "--offline", "--no-color", "--env", "missing"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(
        missing.status.code(),
        Some(2),
        "missing environment must fail"
    );
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("E1337"),
        "missing environment lost its diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("environment module `missing` is not declared"),
        "missing environment failure is not actionable: {stderr}"
    );
    assert!(
        !project.join(".jet/services").exists(),
        "failed env info must not leave a service supervisor"
    );
}

// ── U16: -p ad-hoc packages / --flake foreign-flake fallback ──

#[test]
fn top_level_jet_run_nixpkgs_suffix_tool_execs_tool() {
    // U16: `nix run nixpkgs#tool` parity at the public `jet` front door. The
    // top-level spelling uses CLI refs (`tool@nixpkgs`) and lowers to the
    // same jetpack realization path as `jetpack run tool@nixpkgs -- tool`.
    let root = Scratch::new("jet-run-nixpkgs-root");
    let proj = Scratch::new("jet-run-nixpkgs-proj");
    let fixtures = Scratch::new("jet-run-nixpkgs-fx");
    let out = Scratch::new("jet-run-nixpkgs-out");
    write_runnable_fixture(&fixtures.path, &root.path, &out.path);
    let output = jet()
        .args([
            "run",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from jetpack"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("running greet@nixpkgs -> greet").count(),
        1,
        "stderr: {stderr}"
    );
}

// ── U16: `jet os bridge flake` → `jetpack bridge flake` ──
#[test]
fn jet_os_bridge_flake_delegates_to_jetpack() {
    let project = Scratch::new("jet-bridge-flake");
    let root = Scratch::new("jet-bridge-flake-root");
    fs::write(
        project.join("flake.nix"),
        "{ devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.fd ]; }; }",
    )
    .unwrap();

    let output = jet()
        .args(["os", "bridge", "flake", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("packages: [fd]"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        project.join(".jet/lock").is_file(),
        "the production bridge must commit its typed foreign graph"
    );
}

#[test]
fn jet_os_bridge_flake_reports_missing_foreign_input() {
    let project = Scratch::new("jet-bridge-flake-missing");
    let root = Scratch::new("jet-bridge-flake-missing-root");

    let output = jet()
        .args(["os", "bridge", "flake", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no foreign flake"), "stderr: {stderr}");
    assert!(
        !project.join(".jet/lock").exists(),
        "a failed bridge must not publish a misleading lock"
    );
}
