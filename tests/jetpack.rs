//! End-to-end tests for the `jetpack` binary (Phase 1, D-JPK*).
//!
//! These drive the compiled `jetpack` binary the way a user would, using
//! offline provider fixtures so they need neither network nor Nix (the Forge
//! fixture pattern from forge-salvage.md). They cover the golden exit criteria
//! for JPK-1 / JPK-2 / JPK-3:
//!   * resolve a fixture ref → store path, exit 0
//!   * `-- cmd` runs in the composed env and returns the child's status
//!   * the parent environment is unchanged afterwards
//!   * a bad ref produces a friendly diagnostic and exit 2
//!   * add/remove edit `env.jet`

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jetpack() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetpack"))
}

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}

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
            "jpk-it-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
    fn join(&self, p: &str) -> PathBuf {
        self.path.join(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn example_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-project/fixtures")
}

/// The committed jetpack project fixture (`env.jet` + `jet-pkgs/`).
fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-project")
}

/// Write a provider fixture whose `out` points at a real dir we control, so a
/// `-- cmd` invocation can actually execute a binary from the realized env.
fn write_runnable_fixture(fixtures: &Path, out_dir: &Path) {
    fs::create_dir_all(fixtures).unwrap();
    let bin = out_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let greet = bin.join("greet");
    fs::write(&greet, "#!/bin/sh\necho hello from jetpack\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let json = format!(
        "[{{\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-greet.json"), json).unwrap();
}

fn write_channel_fixture(fixtures: &Path, base: &str, channel: &str, exact: &str) {
    fs::create_dir_all(fixtures).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        format!("{base} {channel} {exact}\n"),
    )
    .unwrap();
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn write_hangar_meta(
    root: &Path,
    id: &str,
    name: &str,
    version: &str,
    output_hash: &str,
    last_used_at: Option<u64>,
) -> PathBuf {
    let dir = root.join("hangar").join(id);
    fs::create_dir_all(&dir).unwrap();
    let timestamps = last_used_at
        .map(|ts| format!("  \"realized_at\": \"{ts}\",\n  \"last_used_at\": \"{ts}\",\n"))
        .unwrap_or_default();
    fs::write(
        dir.join("meta.json"),
        format!(
            "{{\n  \"name\": \"{name}\",\n  \"version\": \"{version}\",\n  \"ref\": \"nixpkgs:{name}\",\n  \"out\": \"/nix/store/{name}\",\n  \"bin\": \"/nix/store/{name}/bin\",\n  \"rlib\": \"\",\n  \"output_hash\": \"{output_hash}\",\n  \"platform\": \"test\",\n  \"signature\": \"\",\n  \"provenance\": \"fixture\",\n{timestamps}  \"end\": \"meta\"\n}}"
        ),
    )
    .unwrap();
    dir
}

fn write_lock_with_live_output(project: &Path, name: &str, version: &str, output_hash: &str) {
    let dot = project.join(".jet");
    fs::create_dir_all(&dot).unwrap();
    fs::write(
        dot.join("lock"),
        format!(
            "version = 1\n\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = {{ path = \".\" }}\nfingerprint = \"sha256-test\"\ndependencies = []\noutput-hash = \"{output_hash}\"\n\n[root]\ndependencies = [\"{name}\"]\n"
        ),
    )
    .unwrap();
}

#[test]
fn build_resolves_fixture_ref() {
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "nixpkgs:fastfetch", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fastfetch"), "stderr: {stderr}");
    assert!(stderr.contains("/nix/store/"), "stderr: {stderr}");
}

#[test]
fn list_shows_realized_package() {
    let root = Scratch::new("root");
    jetpack()
        .args(["build", "nixpkgs:ripgrep", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    let out = jetpack()
        .args(["list", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ripgrep"), "stderr: {stderr}");
}

#[test]
fn clean_removes_only_stale_unreferenced_hangar_objects() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-1", "old", "1.0", "sha256-old", Some(1));
    let fresh = write_hangar_meta(
        &root.path,
        "fresh-1",
        "fresh",
        "1.0",
        "sha256-fresh",
        Some(now_secs()),
    );
    fs::write(stale.join("payload"), "old bytes").unwrap();
    fs::write(fresh.join("payload"), "fresh bytes").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stale.exists(), "stale object should be collected");
    assert!(fresh.exists(), "fresh object should be kept");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("removed 1 stale object"),
        "stderr: {stderr}"
    );
}

#[test]
fn clean_keeps_lock_reachable_and_legacy_unknown_hangar_objects() {
    let root = Scratch::new("root");
    let project = Scratch::new("proj");
    let live = write_hangar_meta(&root.path, "live-1", "live", "1.0", "sha256-live", Some(1));
    let legacy = write_hangar_meta(&root.path, "legacy-1", "legacy", "1.0", "", None);
    write_lock_with_live_output(&project.path, "live", "1.0", "sha256-live");

    let out = jetpack()
        .args(["clean", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(live.exists(), "lock-reachable object should be kept");
    assert!(
        legacy.exists(),
        "legacy object without timestamps should be kept"
    );
}

#[test]
fn clean_sweeps_orphan_build_scratch_but_keeps_active_scratch() {
    let root = Scratch::new("root");
    let scratch = root.path.join("hangar/build-scratch");
    let orphan = scratch.join("orphan");
    let active = scratch.join("active");
    fs::create_dir_all(&orphan).unwrap();
    fs::create_dir_all(&active).unwrap();
    fs::write(orphan.join("tmp"), "dead").unwrap();
    fs::write(active.join(".active"), "").unwrap();
    fs::write(active.join("tmp"), "live").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!orphan.exists(), "orphan scratch should be swept");
    assert!(active.exists(), "active scratch marker protects scratch");
}

#[test]
fn clean_optimizes_duplicate_files_inside_hangar_only() {
    let root = Scratch::new("root");
    let first = write_hangar_meta(
        &root.path,
        "dup-a",
        "dupa",
        "1.0",
        "sha256-a",
        Some(now_secs()),
    );
    let second = write_hangar_meta(
        &root.path,
        "dup-b",
        "dupb",
        "1.0",
        "sha256-b",
        Some(now_secs()),
    );
    fs::write(first.join("blob"), "same payload").unwrap();
    fs::write(second.join("blob"), "same payload").unwrap();

    let out = jetpack()
        .args(["clean", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("optimized 1 file"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(first.join("blob")).unwrap(),
        "same payload"
    );
    assert_eq!(
        fs::read_to_string(second.join("blob")).unwrap(),
        "same payload"
    );
}

#[test]
fn jet_clean_delegates_to_jetpack_clean() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-top", "oldtop", "1.0", "", Some(1));

    let out = jet()
        .args(["clean", "--no-color"])
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
fn build_runs_opportunistic_clean_after_success() {
    let root = Scratch::new("root");
    let stale = write_hangar_meta(&root.path, "old-auto", "oldauto", "1.0", "", Some(1));

    let out = jetpack()
        .args(["build", "nixpkgs:fastfetch", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .env("JETPACK_AUTO_CLEAN_ALWAYS", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stale.exists(),
        "successful build should run opportunistic clean"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("auto-cleaned hangar"), "stderr: {stderr}");
}

#[test]
fn run_dash_dash_executes_in_env_and_returns_status() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let output = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "greet",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello from jetpack");
}

#[test]
fn run_explicit_package_without_command_runs_package_visibly() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let output = jetpack()
        .args(["run", "nixpkgs:greet", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
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
    assert!(
        stderr.contains("running nixpkgs:greet -> greet"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("(no args)"), "stderr: {stderr}");
}

#[test]
fn run_dash_dash_propagates_failure_status() {
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);

    let output = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "false",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn parent_env_unchanged_after_run() {
    // The composed PATH only reaches the child. Ask the child to echo PATH and
    // confirm our bin dir leads; the test process's own PATH is unaffected
    // because we never mutate it.
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &out_dir.path);
    let before = std::env::var("PATH").unwrap_or_default();

    let output = jetpack()
        .args([
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--",
            "sh",
            "-c",
            "printf %s \"$PATH\"",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    let child_path = String::from_utf8_lossy(&output.stdout);
    let want = format!("{}/bin", out_dir.path.to_string_lossy());
    assert!(child_path.starts_with(&want), "child PATH was {child_path}");
    assert_eq!(std::env::var("PATH").unwrap_or_default(), before);
}

#[test]
fn bad_ref_is_friendly_and_exits_2() {
    let out = jetpack()
        .args(["run", "fastfetch", "--no-color"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing a source"), "stderr: {stderr}");
    assert!(stderr.contains("<source>:<package>"), "stderr: {stderr}");
}

#[test]
fn unknown_source_is_friendly() {
    let out = jetpack()
        .args(["build", "brew:wget", "--no-color"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a known source"), "stderr: {stderr}");
}

#[test]
fn add_then_remove_edits_env_file() {
    let proj = Scratch::new("proj");
    let add = jetpack()
        .args(["add", "nixpkgs:ripgrep", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(add.status.success());
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(env.contains("ripgrep"), "env.jet: {env}");
    assert!(env.contains("pkg.packages"), "env.jet: {env}");

    let remove = jetpack()
        .args(["remove", "nixpkgs:ripgrep", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(remove.status.success());
    let env = fs::read_to_string(proj.join("env.jet")).unwrap();
    assert!(
        !env.contains("\"ripgrep\""),
        "env.jet still has ripgrep: {env}"
    );
}

#[test]
fn run_with_project_env_file_resolves_declared_packages() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    // Declare one package, then run with no ref → it resolves from env.jet.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"nixpkgs\");\n        pkg.packages([\"fastfetch\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["run", "--no-color", "--offline", "--", "true"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fastfetch"), "stderr: {stderr}");
}

#[test]
fn typed_env_copy_adapter_realizes_local_source() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let vendor = proj.join("vendor/tool");
    fs::create_dir_all(vendor.join("share")).unwrap();
    fs::write(vendor.join("share/readme.txt"), "adapted\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: path@vendor/tool,
                recipe: Recipe.copy()
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries = fs::read_dir(root.path.join("hangar"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    assert!(
        entries.iter().any(
            |p| fs::read_to_string(p.join("share/readme.txt")).unwrap_or_default() == "adapted\n"
        ),
        "adapter output missing copied file: {entries:?}"
    );
}

#[test]
fn typed_env_prebuilt_adapter_runs_from_path() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let vendor = proj.join("vendor/weirdctl");
    fs::create_dir_all(&vendor).unwrap();
    let bin = vendor.join("weirdctl");
    fs::write(&bin, "#!/bin/sh\necho weird ok\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "weirdctl",
                source: path@vendor/weirdctl,
                recipe: Recipe.prebuilt(bin: "weirdctl", as: "weirdctl")
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["run", "--no-color", "--", "weirdctl"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "weird ok");
}

#[test]
fn no_nix_nixpkgs_package_reports_e1272() {
    let root = Scratch::new("root");
    let output = jetpack()
        .args(["build", "nixpkgs:postgres", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:postgres"), "stderr: {stderr}");
    assert!(stderr.contains("install Nix"), "stderr: {stderr}");
    assert!(stderr.contains("--adapt"), "stderr: {stderr}");
    assert!(!stderr.contains("E1256"), "stderr: {stderr}");
    assert!(!stderr.contains("couldn't run `nix`"), "stderr: {stderr}");
}

#[test]
fn no_nix_ad_hoc_package_reports_e1272() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let output = jetpack()
        .args([
            "enter",
            "-p",
            "postgres",
            "--no-color",
            "--trust",
            "--",
            "true",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:postgres"), "stderr: {stderr}");
}

#[test]
fn no_nix_mixed_env_realizes_core_then_reports_nix_hole() {
    let (base, proj, root) = core_hello_project("no-nix-mixed");
    fs::write(
        proj.join("env.jet"),
        fs::read_to_string(proj.join("env.jet")).unwrap().replace(
            "pkg.packages([\"mine:hello\"])",
            "pkg.packages([\"mine:hello\", \"nixpkgs:postgres\"])",
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hello built"), "stderr: {stderr}");
    assert!(stderr.contains("E1272"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:postgres"), "stderr: {stderr}");
    let metas = fs::read_dir(root.join("hangar"))
        .unwrap()
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path().join("meta.json")).ok())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(metas.contains("\"name\": \"hello\""), "metas: {metas}");
}

#[test]
fn no_nix_json_lists_realized_refs_and_holes() {
    let (base, proj, root) = core_hello_project("no-nix-json");
    fs::write(
        proj.join("env.jet"),
        fs::read_to_string(proj.join("env.jet")).unwrap().replace(
            "pkg.packages([\"mine:hello\"])",
            "pkg.packages([\"mine:hello\", \"nixpkgs:postgres\"])",
        ),
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--json"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"E1272\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"realized\":[\"mine:hello\"]"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"holes\":[\"nixpkgs:postgres\"]"),
        "stdout: {stdout}"
    );
}

#[test]
fn typed_env_bad_adapter_is_e1270() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "broken",
                source: path@vendor/broken,
                recipe: Recipe.build()
            )
        ],
    }
}
"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1270"), "stderr: {stderr}");
}

#[test]
fn channel_update_writes_exact_lock_and_build_uses_it_offline() {
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
    write_channel_fixture(
        &fixtures.path,
        "github:acme/tools",
        "latest",
        "github:acme/tools#v1.2.0",
    );
    fs::write(
        fixtures.join("default-greet.json"),
        r#"[{"outputs":{"out":"/nix/store/0000000000000000000000000000000a-greet-1.2.0"}}]"#,
    )
    .unwrap();

    let update = jetpack()
        .args(["update", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        update.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&update.stderr)
    );
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("[[source_channel]]"), "lock: {lock}");
    assert!(lock.contains("channel = \"latest\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#v1.2.0\""),
        "lock: {lock}"
    );

    let build = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", &fixtures.path)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn channel_build_without_lock_is_e1271() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
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
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1271"), "stderr: {stderr}");
    assert!(
        stderr.contains("jetpack update default"),
        "stderr: {stderr}"
    );
}

#[test]
fn channel_update_accepts_main_and_semver_mask() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fx");
    fs::write(
        proj.join("env.jet"),
        r#"
module dev {
    sources: {
        trunk: github@acme/tools#main,
        stable: github@acme/tools#v0.x,
    }
    env.dev: Env.{ packages: [trunk.greet, stable.greet] }
}
"#,
    )
    .unwrap();
    fs::create_dir_all(&fixtures.path).unwrap();
    fs::write(
        fixtures.join("channels.txt"),
        "github:acme/tools main github:acme/tools#abc123\n\
         github:acme/tools v0.x github:acme/tools#v0.9.4\n",
    )
    .unwrap();

    let out = jetpack()
        .args(["update", "--no-color", "--fixtures"])
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
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(lock.contains("name = \"trunk\""), "lock: {lock}");
    assert!(lock.contains("channel = \"main\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#abc123\""),
        "lock: {lock}"
    );
    assert!(lock.contains("name = \"stable\""), "lock: {lock}");
    assert!(lock.contains("channel = \"v0.x\""), "lock: {lock}");
    assert!(
        lock.contains("exact = \"github:acme/tools#v0.9.4\""),
        "lock: {lock}"
    );
}

#[test]
fn outdated_reports_newer_channel_without_mutating_lock() {
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

    let out = jetpack()
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
    let lock = fs::read_to_string(proj.join(".jet/lock")).unwrap();
    assert!(
        lock.contains("exact = \"github:acme/tools#v1.2.0\""),
        "lock mutated: {lock}"
    );
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
fn add_adapt_prints_snippet_without_editing_env() {
    let proj = Scratch::new("proj");
    let output = jetpack()
        .args(["add", "path:vendor/weirdctl", "--adapt", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Pkg.adapt("), "stdout: {stdout}");
    assert!(
        stdout.contains("source: path@vendor/weirdctl"),
        "stdout: {stdout}"
    );
    assert!(!proj.join("env.jet").exists());
}

#[test]
fn named_source_env_resolves_with_pin() {
    // An env that declares a named source `stable` and references it inline as
    // `stable:ripgrep` resolves via the nix provider against the pin. The
    // fixture is keyed by the source name (`stable-ripgrep.json`).
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"stable\", \"github:NixOS/nixpkgs/nixos-24.05\");\n        pkg.packages([\"stable:ripgrep\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ripgrep"), "stderr: {stderr}");
}

#[test]
fn unknown_named_source_in_env_is_friendly() {
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    // References `beta:neovim` but only declares `stable`.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"stable\", \"github:NixOS/nixpkgs/nixos-24.05\");\n        pkg.packages([\"beta:neovim\"]);\n    ];\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a known source"), "stderr: {stderr}");
    assert!(
        stderr.contains("stable"),
        "should list declared names: {stderr}"
    );
}

/// Build a scratch project whose `env.jet` pulls a first-party `core` package
/// (`hello`) from a local repo. Returns `(base, proj, root)` so a test can run
/// a jetpack command in `proj` with `JETPACK_ROOT=root` and no nix on PATH.
fn core_hello_project(tag: &str) -> (Scratch, PathBuf, PathBuf) {
    let base = Scratch::new(tag);
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"path:{}\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();
    (base, proj, root)
}

#[test]
fn jetpack_enter_runs_command_in_project_env() {
    // Gap #6 / U §8 (Scale-2): `jetpack enter` is the project-env command — it
    // never takes an explicit ref, it always composes the env declared by the
    // project `env.jet`. The `-- cmd` form runs a one-off command in the
    // realized env, which is how we prove `enter` put the package on PATH.
    let (base, proj, root) = core_hello_project("enter");
    let output = jetpack()
        // U19: `enter` trust-gates a project that declares packages; `--trust`
        // is the one-shot bypass so this test can assert on PATH composition
        // without exercising the interactive prompt.
        .args(["enter", "--no-color", "--trust", "--", "hello"])
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
fn enter_dash_p_adds_adhoc_package_with_no_manifest_at_all() {
    // U16: `jet env -p <pkg>... -- cmd` needs no env.jet/pkg.jet at all — the
    // ad-hoc package becomes an ordinary nixpkgs RefSpec, folded into an
    // otherwise-empty plan, trust-gated and realized exactly like a
    // manifest-declared ref.
    let root = Scratch::new("dashp-root");
    let proj = Scratch::new("dashp-proj");
    let fixtures = Scratch::new("dashp-fx");
    let out = Scratch::new("dashp-out");
    write_runnable_fixture(&fixtures.path, &out.path);
    let output = jetpack()
        .args(["enter", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures.path)
        .args(["-p", "greet", "--", "greet"])
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
}

#[test]
fn enter_dash_p_merges_with_project_declared_packages() {
    // The project's own declared package (`hello`, a `core` ref) and the
    // ad-hoc `-p greet` (nixpkgs) both land on PATH in the same shell.
    let (base, proj, root) = core_hello_project("dashp-merge");
    let fixtures = base.join("fixtures");
    let out = base.join("greet-out");
    write_runnable_fixture(&fixtures, &out);
    let output = jetpack()
        .args(["enter", "--no-color", "--trust", "--offline", "--fixtures"])
        .arg(&fixtures)
        .args(["-p", "greet", "--", "sh", "-c", "hello && greet"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello from jet-pkgs"), "stdout: {stdout}");
    assert!(stdout.contains("hello from jetpack"), "stdout: {stdout}");
}

#[test]
fn enter_without_env_jet_or_packages_is_still_nothing_to_do() {
    // The pre-U16 refusal is unchanged when there is truly nothing: no
    // env.jet and no `-p`.
    let root = Scratch::new("nothing-root");
    let proj = Scratch::new("nothing-proj");
    let output = jetpack()
        .args(["enter", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nothing to do"), "stderr: {stderr}");
}

#[test]
fn enter_flake_detection_ordering_project_env_wins_without_flag() {
    // U16's ordering rule: a project that declares `env.*` (here the
    // Phase-1 directive surface) is never silently swapped for a foreign
    // flake.nix, even when one is present — only `--flake` forces it. Proven
    // here by an offline realize of the *declared* `hello` package
    // succeeding with no `nix` on PATH and no flake.nix ever being touched
    // (a bad flake.nix would fail loudly if `nix develop` ran against it).
    let (base, proj, root) = core_hello_project("flake-ordering");
    fs::write(proj.join("flake.nix"), "this is not valid nix").unwrap();
    let output = jetpack()
        .args(["enter", "--no-color", "--trust", "--", "hello"])
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

#[test]
fn enter_flake_flag_forces_foreign_flake_and_reports_missing_nix() {
    // `--flake` forces the foreign-flake fallback even though the project
    // declares `env.*`; with no `nix` on PATH this is a clean E1256, not a
    // panic or a raw spawn error.
    let (base, proj, root) = core_hello_project("flake-forced");
    fs::write(proj.join("flake.nix"), "{ }").unwrap();
    let output = jetpack()
        .args(["enter", "--no-color", "--flake"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("HOME", base.join("home"))
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
    assert!(stderr.contains("nix"), "stderr: {stderr}");
}

#[test]
fn enter_flake_with_no_foreign_flake_present_is_friendly() {
    let root = Scratch::new("flake-none-root");
    let proj = Scratch::new("flake-none-proj");
    let output = jetpack()
        .args(["enter", "--no-color", "--flake"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no foreign flake"), "stderr: {stderr}");
}

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

#[test]
fn bridge_flake_missing_nix_is_e1256_not_a_panic() {
    let dir = Scratch::new("bridge-nonix");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E1256"), "stderr: {stderr}");
}

#[test]
fn bridge_flake_prints_shim_and_warns_on_unmapped_shell_hook() {
    // The best-effort translation: buildInputs become a plain env.dev
    // packages list on stdout; a non-empty shellHook (no env.* equivalent)
    // fires L0204 on stderr without blocking the print.
    let dir = Scratch::new("bridge-shim");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let fixtures = Scratch::new("bridge-shim-fx");
    fs::write(
        fixtures.join("flake-devshell.json"),
        r#"{"buildInputs": ["ripgrep", "fd"], "shellHook": "export FOO=1"}"#,
    )
    .unwrap();
    let output = jetpack()
        .args(["bridge", "flake", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("module env.dev {"), "stdout: {stdout}");
    assert!(
        stdout.contains("packages: [fd, ripgrep]"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("L0204"), "stderr: {stderr}");
    assert!(stderr.contains("shellHook"), "stderr: {stderr}");
}

#[test]
fn bridge_flake_twice_produces_identical_shim_stdout() {
    // Drift-check (U16 plan doc): the bridge is a pure function of the
    // flake's facts, so two runs against the same fixture print
    // byte-identical shims.
    let dir = Scratch::new("bridge-drift");
    fs::write(dir.join("flake.nix"), "{ }").unwrap();
    let fixtures = Scratch::new("bridge-drift-fx");
    fs::write(
        fixtures.join("flake-devshell.json"),
        r#"{"buildInputs": ["nodejs", "ripgrep"], "shellHook": ""}"#,
    )
    .unwrap();
    let run = || {
        jetpack()
            .args(["bridge", "flake", "--no-color", "--fixtures"])
            .arg(&fixtures.path)
            .current_dir(&dir.path)
            .output()
            .unwrap()
    };
    let a = run();
    let b = run();
    assert!(a.status.success());
    assert!(b.status.success());
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn bridge_flake_no_flake_nix_here_is_friendly() {
    let dir = Scratch::new("bridge-noflake");
    let output = jetpack()
        .args(["bridge", "flake", "--no-color"])
        .current_dir(&dir.path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no flake.nix"), "stderr: {stderr}");
}

#[test]
fn core_provider_runs_first_party_package_without_nix() {
    // R2/U10: a `core` named source realizes a first-party Jet package with no
    // nix anywhere. Package is discovered by module name — no env.jet index.
    let base = Scratch::new("core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The project declares a `core` named source pointing at the local repo.
    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"path:{}\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
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

#[test]
fn typed_core_source_inferred_from_pack_jet() {
    // U9/U10: a typed `module { … }` env declares `sources: { mine: path@<dir> }`
    // with no provider marker. The kind is *inferred* from `pkg.jet` in the
    // target → realizes through the first-party `core` provider. U10 Chunk 3:
    // the package is discovered by module name — `module hello` in the source tree
    // — with no `env.jet` index. No nix on PATH proves no nix is involved.
    let base = Scratch::new("typed-core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    // `pkg.jet` is both the U9 probe marker and the U10 package index.
    fs::write(
        repo.join("pkg.jet"),
        "payload: {\n    name: \"jet-pkgs\",\n    version: \"0.1.0\",\n}\npackages: {\n    hello: executable,\n}\n",
    )
    .unwrap();
    // The `module hello` declaration is the U10 Chunk 3 discovery target — no
    // `env.jet` pkg.package index needed anymore (dual marker retired).
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The typed env declares the source with no `via`/`core` marker — just
    // `provider@target`. `mine.hello` is the Pkg sugar → `mine:hello`.
    fs::write(
        proj.join("env.jet"),
        format!(
            "module dev {{\n    sources: {{ mine: path@{} }}\n    env.dev: Env.{{\n        packages: [mine.hello],\n    }}\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
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

#[test]
fn core_provider_builds_library_package_without_nix() {
    // U10 Chunk 4: a `library` package realizes through the `core` provider
    // (no nix), staging its module source. Unlike an `executable`, it puts no
    // `bin/` on PATH — but `jetpack build` realizes it just the same. The kind
    // comes from the repo's `pkg.jet` `packages:` index.
    let base = Scratch::new("core-library");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let lib_pkg = repo.join("lib/mathlib");
    fs::create_dir_all(&lib_pkg).unwrap();
    fs::create_dir_all(&proj).unwrap();
    // `pkg.jet` declares the package as a `library` (the kind index).
    fs::write(
        repo.join("pkg.jet"),
        "payload: {\n    name: \"jet-pkgs\",\n    version: \"0.1.0\",\n}\npackages: {\n    mathlib: library,\n}\n",
    )
    .unwrap();
    // The library's source: a `module mathlib` discovered by name (Chunk 3),
    // with no `bin/` — it is imported for its code, not installed on PATH.
    fs::write(
        lib_pkg.join("mathlib.jet"),
        "module mathlib {\n    pub fn add(a: Int, b: Int) -> Int { return a + b }\n}\n",
    )
    .unwrap();
    // A typed env references the library package; the source kind is inferred
    // from `pkg.jet` → core.
    fs::write(
        proj.join("env.jet"),
        format!(
            "module dev {{\n    sources: {{ mine: path@{} }}\n    env.dev: Env.{{\n        packages: [mine.mathlib],\n    }}\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("built 1 package(s)"),
        "expected build success status, got: {stderr}"
    );
}

#[test]
fn committed_example_builds_offline_end_to_end() {
    // I5: the committed jetpack project fixture is the executable spec for
    // a real env.jet. `jetpack build` with no ref reads env.jet and realizes
    // everything it declares — nix-backed named sources (`stable:ripgrep`,
    // `unstable:neovim`) resolved from the committed fixtures, plus a
    // first-party `mine:hello` realized through the `core` provider with no
    // nix. The whole thing runs fully offline. The store lives under a scratch
    // JETPACK_ROOT, so nothing is written back into the example dir.
    let root = Scratch::new("example-e2e");
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for pkg in ["ripgrep", "neovim", "hello"] {
        assert!(
            stderr.contains(pkg),
            "expected `{pkg}` in build output: {stderr}"
        );
    }
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn typed_module_example_builds_offline_end_to_end() {
    // I5: the committed jetpack-typed fixture is the executable spec
    // for the typed `module { … }` env surface (U3/U6/U8) including U4 import-tree
    // discovery. `jetpack build` with no ref evaluates env.jet through `modeval`:
    // the `default` source merges to its pinned nixpkgs upstream,
    // `default.[ripgrep, fd]` expands to two `Pkg` refs, and `imports:
    // find("./modules")` walks `modules/tools.jet` and folds its `default.jq`
    // into the same merge. All three realize from the committed fixtures, fully
    // offline. The store lives under a scratch JETPACK_ROOT, so nothing is
    // written back.
    let typed_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-typed");
    let root = Scratch::new("typed-e2e");
    let output = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&typed_dir)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", typed_dir.join("fixtures"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for pkg in ["ripgrep", "fd", "jq"] {
        assert!(
            stderr.contains(pkg),
            "expected `{pkg}` in build output: {stderr}"
        );
    }
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn core_provider_fetches_remote_git_package_from_env() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("note: skipping remote core provider integration test (git not found)");
        return;
    }

    let base = Scratch::new("core-remote");
    let repo = base.join("remote");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from remote jet-pkgs\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }

    for args in [
        vec!["init"],
        vec!["config", "user.email", "jetpack@example.invalid"],
        vec!["config", "user.name", "Jetpack Test"],
        vec!["add", "."],
        vec!["commit", "-m", "init"],
    ] {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fs::write(
        proj.join("env.jet"),
        format!(
            "use jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"file://{}#HEAD\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
            repo.to_string_lossy()
        ),
    )
    .unwrap();

    let output = jetpack()
        .args(["run", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from remote jet-pkgs"
    );
    assert!(
        root.join("sources").is_dir(),
        "remote source cache was not created"
    );

    let offline = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        offline.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&offline.stderr)
    );
}

// ── gap #4: `jetpack os <verb> [<config-path>]@<host>` (U15/U16) ─────────

/// The committed jetpack-config fixture dir.
fn config_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-config")
}

#[test]
fn os_build_realizes_selected_system_offline() {
    // I5/U15/U16: `jetpack os build <path>@<host>` loads the config, selects the
    // System named <host>, and realizes its packages into a system generation —
    // fully offline (the packages come from a first-party `core` source repo, so
    // no nix). The store lives under a scratch JETPACK_ROOT.
    let root = Scratch::new("os-build-root");
    let config = config_example_dir().join("config.jet");
    let out = jetpack()
        .args([
            "os",
            "build",
            &format!("{}@halcyon", config.to_string_lossy()),
            "--no-color",
            "--offline",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin") // no nix on PATH
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    for pkg in ["hello", "btop"] {
        assert!(stderr.contains(pkg), "expected `{pkg}` in output: {stderr}");
    }
    assert!(stderr.contains("halcyon"), "stderr: {stderr}");
    assert!(stderr.contains("generation"), "stderr: {stderr}");
    // A generation directory was assembled under the managed system store.
    assert!(
        root.join("systems").is_dir(),
        "expected a systems dir under the root"
    );
}

#[test]
fn os_switch_activates_and_sets_current() {
    // U15: `switch` builds the generation, then activates it — flips a `current`
    // pointer (and a boot `default`). The internal mechanic is a symlink in the
    // managed system store; the user sees a clear "activated" line.
    let root = Scratch::new("os-switch-root");
    let config = config_example_dir().join("config.jet");
    let out = jetpack()
        .args([
            "os",
            "switch",
            &format!("{}@halcyon", config.to_string_lossy()),
            "--no-color",
            "--offline",
        ])
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("activated"), "stderr: {stderr}");
    // The `current` pointer now exists.
    let current = root.join("systems").join("current");
    assert!(
        current.exists(),
        "expected a `current` generation pointer at {}",
        current.display()
    );
}

#[test]
fn os_build_default_config_path_uses_home_dot_jet() {
    // U16: with no path prefix (`jetpack os build @host`), the config defaults to
    // `~/.jet/config.jet`. We point HOME at a scratch dir holding that file.
    let home = Scratch::new("os-home");
    let root = Scratch::new("os-default-root");
    let jet_dir = home.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap();
    // A minimal self-contained system (no packages → realizes trivially offline).
    fs::write(
        jet_dir.join("config.jet"),
        "module box {\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["os", "build", "@box", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("box"), "stderr: {stderr}");
}

#[test]
fn os_missing_host_is_friendly_and_exits_2() {
    let root = Scratch::new("os-no-host");
    let out = jetpack()
        .args(["os", "build", "./config.jet", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E0979"), "stderr: {stderr}");
    assert!(stderr.contains("@host"), "stderr: {stderr}");
}

#[test]
fn os_unknown_host_lists_available_systems() {
    let root = Scratch::new("os-bad-host");
    let config = config_example_dir().join("config.jet");
    let out = jetpack()
        .args([
            "os",
            "build",
            &format!("{}@nope", config.to_string_lossy()),
            "--no-color",
            "--offline",
        ])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E0980"), "stderr: {stderr}");
    assert!(stderr.contains("nope"), "stderr: {stderr}");
    assert!(stderr.contains("halcyon"), "should list systems: {stderr}");
}

#[test]
fn os_missing_config_file_is_friendly() {
    let root = Scratch::new("os-no-config");
    let out = jetpack()
        .args([
            "os",
            "build",
            "/definitely/not/here/config.jet@box",
            "--no-color",
        ])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E0981"), "stderr: {stderr}");
}

#[test]
fn offline_without_fixtures_errors() {
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "nixpkgs:fastfetch", "--no-color", "--offline"])
        .env("JETPACK_ROOT", &root.path)
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1276"), "stderr: {stderr}");
    assert!(stderr.contains("nixpkgs:fastfetch"), "stderr: {stderr}");
}

// ── D-JPK-FILES Phase 2b: jetpack.toml wiring ─────────────────────────────

/// The committed multi-package monorepo example dir.
fn mono_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-mono")
}

#[test]
fn malformed_jetpack_toml_fires_e1214_from_cli() {
    // I4/D-JPK-FILES Phase 2b: E1214 must be reachable from real `jetpack`
    // usage, not just the in-module unit test. Create a scratch project whose
    // jetpack.toml has a malformed line, run `jetpack build`, and verify that
    // E1214 appears in stderr with exit code 2.
    let proj = Scratch::new("bad-toml-e1214");
    let root = Scratch::new("bad-toml-root");
    // Write a jetpack.toml with a malformed line (not a key="value" or [table]).
    fs::write(
        proj.join("jetpack.toml"),
        "[repo]\nname = \"test\"\nbad line here\n",
    )
    .unwrap();
    // Also write a minimal env.jet so the `nothing to do` error isn't hit first.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1214"),
        "expected E1214 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("jetpack.toml"),
        "expected jetpack.toml in error: {stderr}"
    );
}

#[test]
fn malformed_jetpack_toml_fires_e1215_from_cli() {
    // I4/D-JPK-FILES Phase 2b: E1215 must be reachable from real `jetpack`
    // usage. An unknown table name fires E1215 with did-you-mean.
    let proj = Scratch::new("bad-toml-e1215");
    let root = Scratch::new("bad-toml-root2");
    fs::write(proj.join("jetpack.toml"), "[workspace]\nfoo = \"bar\"\n").unwrap();
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1215"),
        "expected E1215 in stderr: {stderr}"
    );
}

#[test]
fn jetpack_toml_packages_fires_e1225_from_cli() {
    // D-WORKSPACE1: the old `[packages]` monorepo index moved to
    // `workspace.jet`; keep a real CLI test so the migration diagnostic is
    // reachable from user commands.
    let proj = Scratch::new("bad-toml-e1225");
    let root = Scratch::new("bad-toml-root3");
    fs::write(
        proj.join("jetpack.toml"),
        "[packages]\ngreeter = \"packages/greeter/pkg.jet\"\n",
    )
    .unwrap();
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.source(\"nixpkgs\"), pkg.packages([\"ripgrep\"])];\n}\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FIXTURES", example_fixtures())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1225"),
        "expected E1225 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("workspace.jet"),
        "expected workspace.jet migration hint: {stderr}"
    );
}

#[test]
fn jetpack_toml_sources_merge_into_cwd_table() {
    // D-JPK-FILES Phase 2b: `[sources]` declared in jetpack.toml are folded
    // into the source table so env.jet can reference them by name. Create a
    // project whose jetpack.toml declares a named source and whose env.jet
    // references it — the build should resolve via the folded table.
    let base = Scratch::new("toml-sources");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_pkg = repo.join("pkgs/hello");
    let hello_bin = hello_pkg.join("bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(hello_pkg.join("hello.jet"), "module hello { }\n").unwrap();
    let greet = hello_bin.join("hello");
    fs::write(&greet, "#!/bin/sh\necho hello from toml-source\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // jetpack.toml declares `mine` as a path source (no via — inferred as core).
    fs::write(
        proj.join("jetpack.toml"),
        format!("[sources]\nmine = \"path@{}\"\n", repo.to_string_lossy()),
    )
    .unwrap();
    // env.jet references `mine:hello` — the source name is resolved from jetpack.toml.
    fs::write(
        proj.join("env.jet"),
        "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"mine\", \"path:PLACEHOLDER\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}\n".replace(
            "path:PLACEHOLDER",
            &format!("path:{}", repo.to_string_lossy()),
        ),
    )
    .unwrap();
    let out = jetpack()
        .args(["build", "--no-color", "--", "hello"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn mono_example_has_two_pkg_jet_members() {
    // D-WORKSPACE1: the committed monorepo example now uses workspace.jet
    // instead of the retired jetpack.toml [packages] index.
    let mono = mono_example_dir();
    assert!(
        mono.join("workspace.jet").exists(),
        "workspace.jet missing from mono example"
    );
    let greeter_pkg = mono.join("packages/greeter/pkg.jet");
    let logger_pkg = mono.join("packages/logger/pkg.jet");
    assert!(
        greeter_pkg.exists(),
        "packages/greeter/pkg.jet missing: {greeter_pkg:?}"
    );
    assert!(
        logger_pkg.exists(),
        "packages/logger/pkg.jet missing: {logger_pkg:?}"
    );
    let workspace_src = fs::read_to_string(mono.join("workspace.jet")).unwrap();
    assert!(
        workspace_src.contains("find(\"./packages\")"),
        "workspace.jet should use find-based member discovery"
    );
}

// ── Card #99 T4: build-from-source surface (build states / vendor / audit) ────

#[test]
fn jet_build_reports_source_states() {
    // T4: `jetpack build` reports how each package was satisfied. A first build
    // of a core package is `built`; the content-addressed re-build is `cached`.
    let (_base, proj, root) = core_hello_project("t4-build");
    let run = || {
        jetpack()
            .args(["build", "--no-color"])
            .current_dir(&proj)
            .env("JETPACK_ROOT", &root)
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
    let out1 = String::from_utf8_lossy(&first.stderr);
    assert!(
        out1.contains("built"),
        "first build must report `built`: {out1}"
    );
    assert!(
        out1.contains("1 built"),
        "summary must count the built package: {out1}"
    );

    let second = run();
    assert!(second.status.success());
    let out2 = String::from_utf8_lossy(&second.stderr);
    assert!(
        out2.contains("cached"),
        "re-build of the same content must report `cached`: {out2}"
    );
    assert!(
        out2.contains("1 cached"),
        "summary must count the cache hit: {out2}"
    );
}

#[test]
fn jet_vendor_writes_pinned_sources() {
    // T4 / D-BFS1: `jetpack vendor` copies each source-built package and writes a
    // `<name>.sha256` pin (the A4 output hash) so a later build is reproducible.
    let (_base, proj, root) = core_hello_project("t4-vendor");
    // Realize first so the hangar has a source-built object.
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["vendor", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pin = proj.join("vendor/hello.sha256");
    assert!(pin.is_file(), "vendor must write a per-package sha256 pin");
    let hash = fs::read_to_string(&pin).unwrap();
    assert!(
        hash.trim().starts_with("sha256-"),
        "the pin must be a content hash: {hash}"
    );
    assert!(
        proj.join("vendor/hello").is_dir(),
        "vendor must copy the package source tree"
    );
}

#[test]
fn jet_audit_reads_without_exec() {
    // T4 / D-BUILDSCOPE1: `jetpack audit` reads build provenance and executes
    // nothing — no "resolving …" / "built" build activity, just a read-only
    // report of the realized objects' provenance.
    let (_base, proj, root) = core_hello_project("t4-audit");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["audit", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("read-only, no build ran"),
        "audit is read-only: {report}"
    );
    assert!(
        report.contains("provenance"),
        "audit reports provenance: {report}"
    );
    // Audit must not run a build: it never prints the realize progress line.
    assert!(
        !report.contains("resolving"),
        "audit must not realize anything: {report}"
    );
}

#[test]
fn jet_hangar_du_counts_source_built_objects() {
    // T0 exit: `jetpack hangar du` counts realized objects honestly, marking
    // source-built ones. A first-party core build shows up as a `(built)` object.
    let (_base, proj, root) = core_hello_project("t0-du");
    let built = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(built.status.success());

    let out = jetpack()
        .args(["hangar", "du", "--no-color"])
        .current_dir(&proj)
        .env("JETPACK_ROOT", &root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("built"),
        "du must mark source-built objects: {report}"
    );
    assert!(
        report.contains("1 built from source"),
        "du summary must count source-built objects honestly: {report}"
    );
}
