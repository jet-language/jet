//! Top-level `jet` → `jetpack` command dispatch tests (Tower card #367
//! slice 6 split).
//!
//! These cover the CLI delegation surface — `jet clean`/`jet env`/
//! `jet outdated`/`jet run nixpkgs:tool` routing through to the `jetpack`
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
    let stale = write_hangar_meta(&root.path, "old-top", "oldtop", "1.0", "", Some(1));

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
fn top_level_jet_outdated_dispatches_to_jetpack() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: { default: github@acme/tools#latest }
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
    // D-DEV4 (ratified 2026-06-17): `jet env` is the friendly Scale-2 front door
    // into the project dev shell — it delegates straight to `jetpack enter`,
    // forwarding flags and the trailing `-- cmd`. (`jet dev` is now reserved for
    // the E2-M4 watch/interpret loop.) Running through the `jet` binary must
    // reach the same composed env.
    let (base, proj, root) = core_hello_project("jet-env");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        // U19: same trust gate reached through `jet env`; `--trust` bypasses.
        .args(["env", "--no-color", "--trust", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
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

// ── U16: -p ad-hoc packages / --flake foreign-flake fallback ──


#[test]
fn top_level_jet_run_nixpkgs_colon_tool_execs_tool() {
    // U16: `nix run nixpkgs#tool` parity at the public `jet` front door. The
    // top-level spelling uses CLI refs (`nixpkgs:tool`) and lowers to the
    // same jetpack realization path as `jetpack run nixpkgs:tool -- tool`.
    let root = Scratch::new("jet-run-nixpkgs-root");
    let proj = Scratch::new("jet-run-nixpkgs-proj");
    let fixtures = Scratch::new("jet-run-nixpkgs-fx");
    let out = Scratch::new("jet-run-nixpkgs-out");
    write_runnable_fixture(&fixtures.path, &out.path);
    let output = jet()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
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
        stderr.matches("running nixpkgs:greet -> greet").count(),
        1,
        "stderr: {stderr}"
    );
}

// ── U16: `jetpack bridge flake` ──


