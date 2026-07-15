//! U19 env/dev split + trust gate (D-JPK-DEVCOMPOSE1=D, card c9jetpackgates).
//!
//! Covers:
//!   * `jet env` (`jetpack enter`) never runs a project function (regression);
//!   * project-level `jet dev` (bare, no file — distinct from the shipped
//!     `jet dev <file.jet>` watch loop) runs `fn dev()` after the U12 no-op
//!     service wait;
//!   * `jet dev` with neither `fn dev()` nor `fn run()` is E1254;
//!   * a trust-sensitive env hitting a non-interactive path with no grant is
//!     a clean E1255, never a hang;
//!   * `--trust` bypasses; `jetpack config trust add` pre-authorizes.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;
use common::jetpack_bin;

/// A throwaway directory under the system temp dir, removed on drop.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jpk-trust-it-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}

/// Write a `nixpkgs:fastfetch` fixture backed by a sealed Hangar object.
/// Store's closure proof accepts a zero-reference Nix realization only when
/// Hangar can re-hash its canonical object (or Nix protects a real
/// `/nix/store` output with a GC root). A generic temp directory is real
/// enough for leasing but has neither proof, so trust tests must place their
/// fixture output at the same content-addressed path production ingest uses.
fn write_fastfetch_fixture(
    fixtures: &std::path::Path,
    root: &std::path::Path,
    staging_dir: &std::path::Path,
) {
    fs::create_dir_all(fixtures).unwrap();
    let bin = staging_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fastfetch = bin.join("fastfetch");
    fs::write(&fastfetch, "#!/bin/sh\necho fastfetch stub\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fastfetch, fs::Permissions::from_mode(0o755)).unwrap();
    }
    jetpack::Store::seal_local_output(staging_dir).unwrap();
    let digest = jetpack::Envelope::try_output_hash_of(&staging_dir.to_string_lossy()).unwrap();
    let out_dir = root.join("hangar").join("objects").join(&digest);
    fs::create_dir_all(out_dir.parent().unwrap()).unwrap();
    let mut staging_permissions = fs::metadata(staging_dir).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staging_permissions.set_mode(staging_permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    staging_permissions.set_readonly(false);
    fs::set_permissions(staging_dir, staging_permissions).unwrap();
    fs::rename(staging_dir, &out_dir).unwrap();
    jetpack::Store::seal_local_output(&out_dir).unwrap();
    assert_eq!(
        jetpack::Envelope::try_output_hash_of(&out_dir.to_string_lossy()).unwrap(),
        digest,
        "published fixture must retain its content-addressed identity"
    );

    let drv_path = fixtures.join("fastfetch.drv");
    fs::write(&drv_path, "fixture derivation identity\n").unwrap();
    let json = format!(
        "[{{\"drvPath\":{:?},\"outputs\":{{\"out\":{:?}}}}}]",
        drv_path.to_string_lossy(),
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-fastfetch.json"), json).unwrap();
}

/// A project with no declared packages — never trust-sensitive, so these
/// tests exercise env/dev composition and entry-fn resolution without the
/// trust gate in the way.
fn write_packageless_project(dir: &std::path::Path, main_src: &str) {
    fs::write(dir.join("env.jet"), "module env.dev { }\n").unwrap();
    fs::write(dir.join("main.jet"), main_src).unwrap();
}

/// A project that declares one real (fixture-realizable) package — trust
/// sensitive.
fn write_package_project(dir: &std::path::Path, main_src: &str) {
    fs::write(
        dir.join("env.jet"),
        "module env.dev { packages: [nixpkgs.fastfetch] }\n",
    )
    .unwrap();
    fs::write(dir.join("main.jet"), main_src).unwrap();
}

/// A project whose env only declares secrets — no packages. U13 still makes
/// this trust-sensitive because entering it may decrypt repo secrets.
fn write_secret_project(dir: &std::path::Path, main_src: &str) {
    fs::write(
        dir.join("env.jet"),
        "module env.dev { secrets: [\"db_password\"] }\n",
    )
    .unwrap();
    fs::write(dir.join("main.jet"), main_src).unwrap();
}

/// `jetpack enter` (`jet env`) realizes the declared env and drops into a
/// shell / runs a one-off command — it must NEVER run a project's own
/// `fn run()`/`fn dev()` (U19's env/dev split). Regression test: the world's
/// only way to notice a project function ran is grep the marker it prints.
#[test]
fn env_enter_runs_no_project_function() {
    let proj = Scratch::new("enter-no-fn");
    let root = Scratch::new("enter-no-fn-root");
    let home = Scratch::new("enter-no-fn-home");
    write_packageless_project(&proj.path, "fn run() { print(\"SHOULD-NOT-RUN\"); }\n");
    let out = jetpack()
        .args(["enter", "--no-color", "--", "echo", "entered"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("entered"), "stdout: {stdout}");
    assert!(
        !stdout.contains("SHOULD-NOT-RUN"),
        "env entry must never run a project function: {stdout}"
    );
}

/// Bare `jetpack dev` (no file — the project-level U19 command, distinct from
/// the already-shipped `jet dev <file.jet>` watch loop) finds the project's
/// `main.jet`, waits for services (U12 no-op today), and runs its `fn dev()`.
#[test]
fn project_dev_runs_fn_dev_after_service_wait() {
    let proj = Scratch::new("dev-runs-fn-dev");
    let root = Scratch::new("dev-runs-fn-dev-root");
    let home = Scratch::new("dev-runs-fn-dev-home");
    write_packageless_project(&proj.path, "fn dev() { print(\"DEV-RAN\"); }\n");
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("DEV-RAN"), "stdout: {stdout}");
}

/// A project with neither `fn dev()` nor `fn run()` in its entry file is a
/// clean E1254, not a confusing compiler error further down the line.
#[test]
fn dev_no_entry_is_e1254() {
    let proj = Scratch::new("dev-no-entry");
    let root = Scratch::new("dev-no-entry-root");
    let home = Scratch::new("dev-no-entry-home");
    write_packageless_project(&proj.path, "fn other() { print(\"nope\"); }\n");
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1254"), "stderr: {stderr}");
}

/// A trust-sensitive env (it declares a package) hitting a non-interactive
/// path (the test harness's stdin is not a TTY) with no `--trust` and no
/// prior grant is a clean E1255 — never a hung prompt. The trust gate runs
/// before package realization, so this needs no fixtures.
#[test]
fn dev_untrusted_non_interactive_is_e1255() {
    let proj = Scratch::new("dev-untrusted");
    let root = Scratch::new("dev-untrusted-root");
    let home = Scratch::new("dev-untrusted-home");
    write_package_project(&proj.path, "fn dev() { print(\"DEV-RAN\"); }\n");
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1255"), "stderr: {stderr}");
    // Never got as far as running the project.
    assert!(!String::from_utf8_lossy(&out.stdout).contains("DEV-RAN"));
}

/// A secrets-only env is trust-sensitive even without packages: non-interactive
/// entry refuses before trying to decrypt or run project code.
#[test]
fn dev_secret_env_untrusted_non_interactive_is_e1255() {
    let proj = Scratch::new("dev-secret-untrusted");
    let root = Scratch::new("dev-secret-untrusted-root");
    let home = Scratch::new("dev-secret-untrusted-home");
    write_secret_project(&proj.path, "fn dev() { print(\"DEV-RAN\"); }\n");
    let out = jetpack()
        .args(["dev", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1255"), "stderr: {stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("DEV-RAN"));
}

/// With trust granted, `jet dev` validates declared secret names before entry.
/// No store exists here, so the check is a clean E1263 and the project never
/// runs.
#[test]
fn dev_declared_missing_secret_is_e1263() {
    let proj = Scratch::new("dev-secret-missing");
    let root = Scratch::new("dev-secret-missing-root");
    let home = Scratch::new("dev-secret-missing-home");
    write_secret_project(&proj.path, "fn dev() { print(\"DEV-RAN\"); }\n");
    let out = jetpack()
        .args(["dev", "--no-color", "--trust"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1263"), "stderr: {stderr}");
    assert!(stderr.contains("db_password"), "stderr: {stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("DEV-RAN"));
}

/// `jet env` uses the same U13 validation before entering the shell or running
/// a one-off command.
#[test]
fn env_declared_missing_secret_is_e1263() {
    let proj = Scratch::new("env-secret-missing");
    let root = Scratch::new("env-secret-missing-root");
    let home = Scratch::new("env-secret-missing-home");
    write_secret_project(&proj.path, "fn run() { print(\"SHOULD-NOT-RUN\"); }\n");
    let out = jetpack()
        .args(["enter", "--no-color", "--trust", "--", "echo", "entered"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1263"), "stderr: {stderr}");
    assert!(stderr.contains("db_password"), "stderr: {stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("entered"));
}

/// A typed env that only declares secrets still counts as the project's own
/// env; passive `flake.nix` auto-detection must not steal the command.
#[test]
fn secret_env_beats_foreign_flake_autodetect() {
    let proj = Scratch::new("secret-env-beats-flake");
    let root = Scratch::new("secret-env-beats-flake-root");
    let home = Scratch::new("secret-env-beats-flake-home");
    write_secret_project(&proj.path, "fn run() { print(\"SHOULD-NOT-RUN\"); }\n");
    fs::write(proj.path.join("flake.nix"), "{ outputs = _: {}; }\n").unwrap();
    let out = jetpack()
        .args(["enter", "--no-color", "--", "echo", "entered"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1255"), "stderr: {stderr}");
    assert!(!stderr.contains("E1256"), "stderr: {stderr}");
}

/// `--trust` is the one-shot bypass: same untrusted project, but the run
/// proceeds all the way through realization to `fn dev()`.
#[test]
fn dev_trust_flag_bypasses() {
    let proj = Scratch::new("dev-trust-flag");
    let root = Scratch::new("dev-trust-flag-root");
    let home = Scratch::new("dev-trust-flag-home");
    let fixtures = Scratch::new("dev-trust-flag-fixtures");
    let fastfetch_out = Scratch::new("dev-trust-flag-fastfetch-out");
    write_package_project(&proj.path, "fn dev() { print(\"DEV-RAN\"); }\n");
    write_fastfetch_fixture(&fixtures.path, &root.path, &fastfetch_out.path);
    let out = jetpack()
        .args(["dev", "--no-color", "--offline", "--trust"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("DEV-RAN"), "stdout: {stdout}");
    // `--trust` is one-shot: it must not have persisted a grant.
    let trust_file = home.path.join(".jet").join("trust");
    assert!(!trust_file.exists(), "`--trust` must never persist a grant");
}

/// `jetpack config trust add <pattern>` pre-authorizes matching projects with
/// no per-run flag and no prompt.
#[test]
fn dev_pattern_trust_preauthorizes() {
    let proj = Scratch::new("dev-pattern-trust");
    let root = Scratch::new("dev-pattern-trust-root");
    let home = Scratch::new("dev-pattern-trust-home");
    let fixtures = Scratch::new("dev-pattern-trust-fixtures");
    let fastfetch_out = Scratch::new("dev-pattern-trust-fastfetch-out");
    write_package_project(&proj.path, "fn dev() { print(\"DEV-RAN\"); }\n");
    write_fastfetch_fixture(&fixtures.path, &root.path, &fastfetch_out.path);

    let pattern = format!("{}*", proj.path.display());
    let add_out = jetpack()
        .args(["config", "trust", "add", &pattern, "--no-color"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        add_out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add_out.stderr)
    );

    let out = jetpack()
        .args(["dev", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("DEV-RAN"), "stdout: {stdout}");
}

/// `jetpack config trust list`/`remove` round-trip.
#[test]
fn config_trust_list_and_remove() {
    let home = Scratch::new("config-trust-list");
    let pattern = "/some/project/*";
    jetpack()
        .args(["config", "trust", "add", pattern, "--no-color"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    let list_out = jetpack()
        .args(["config", "trust", "list", "--no-color"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(list_out.status.success());
    let stderr = String::from_utf8_lossy(&list_out.stderr);
    assert!(stderr.contains(pattern), "stderr: {stderr}");

    let remove_out = jetpack()
        .args(["config", "trust", "remove", pattern, "--no-color"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(remove_out.status.success());
    let trust_file = home.path.join(".jet").join("trust");
    let contents = fs::read_to_string(&trust_file).unwrap_or_default();
    assert!(!contents.contains(pattern), "contents: {contents}");
}

/// `jet trust` is the D-JPK-GRANTCMD1 public front door. It dispatches to
/// Jetpack, but users never need to know the older `jetpack config trust` shape.
#[test]
fn top_level_jet_trust_grant_list_explain_revoke() {
    let home = Scratch::new("top-level-jet-trust");

    let grant = jet()
        .args([
            "trust",
            "grant",
            "github@acme/web:postgres.service",
            "--scope",
            "repo",
            "--color=never",
        ])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&grant.stderr)
    );

    let list = jet()
        .args(["trust", "list", "--json", "--color=never"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains("\"authority\":\"service\""),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"subject\":\"github@acme/web:postgres.service\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"scope\":\"repo\""), "stdout: {stdout}");

    let explain = jet()
        .args([
            "trust",
            "explain",
            "service:github@acme/web:postgres.service",
            "--color=never",
        ])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(explain.status.success());
    let stderr = String::from_utf8_lossy(&explain.stderr);
    assert!(
        stderr.contains("exact authority: service"),
        "stderr: {stderr}"
    );

    let revoke = jet()
        .args([
            "trust",
            "revoke",
            "service:github@acme/web:postgres.service",
            "--color=never",
        ])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(revoke.status.success());

    let list_after = jet()
        .args(["trust", "list", "--json", "--color=never"])
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&list_after.stdout).trim(),
        "{\"grants\":[]}"
    );
}

#[test]
fn top_level_jet_trust_grant_scope_needs_value() {
    let home = Scratch::new("trust-scope-value-home");
    let out = jet()
        .args(["trust", "grant", "service:postgres.service", "--scope"])
        .env("HOME", &home.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("couldn't parse trust grant"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("unknown trust scope"), "stderr: {stderr}");
}
