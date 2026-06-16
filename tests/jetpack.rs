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

/// The committed example fixtures dir.
fn example_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/jetpack/fixtures")
}

/// The committed example project dir (`env.jet` + `jet-pkgs/`).
fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/jetpack")
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
        "import jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"nixpkgs\");\n        pkg.packages([\"fastfetch\"]);\n    ];\n}\n",
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
fn named_source_env_resolves_with_pin() {
    // An env that declares a named source `stable` and references it inline as
    // `stable:ripgrep` resolves via the nix provider against the pin. The
    // fixture is keyed by the source name (`stable-ripgrep.json`).
    let proj = Scratch::new("proj");
    let root = Scratch::new("root");
    fs::write(
        proj.join("env.jet"),
        "import jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"stable\", \"github:NixOS/nixpkgs/nixos-24.05\");\n        pkg.packages([\"stable:ripgrep\"]);\n    ];\n}\n",
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
        "import jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [\n        pkg.source(\"stable\", \"github:NixOS/nixpkgs/nixos-24.05\");\n        pkg.packages([\"beta:neovim\"]);\n    ];\n}\n",
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

#[test]
fn core_provider_runs_first_party_package_without_nix() {
    // R2: a `core` named source realizes a first-party Jet package with no nix
    // anywhere. We strip nix from PATH to prove it.
    let base = Scratch::new("core");
    let repo = base.join("jet-pkgs");
    let proj = base.join("proj");
    let root = base.join("root");
    let hello_bin = repo.join("pkgs/hello/bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        repo.join("env.jet"),
        "pkg.package(\"hello\", \"./pkgs/hello\");\n",
    )
    .unwrap();
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
            "import jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"path:{}\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
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
fn committed_example_builds_offline_end_to_end() {
    // I5: the committed `examples/jetpack/` project is the executable spec for
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
    assert!(
        stderr.contains("built 3 package(s)"),
        "stderr: {stderr}"
    );
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
    let hello_bin = repo.join("pkgs/hello/bin");
    fs::create_dir_all(&hello_bin).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        repo.join("env.jet"),
        "pkg.package(\"hello\", \"./pkgs/hello\");\n",
    )
    .unwrap();
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
            "import jetpack as pkg;\npub fn shell() -> [JSON] {{\n    return [\n        pkg.source(\"mine\", \"file://{}#HEAD\", \"core\");\n        pkg.packages([\"mine:hello\"]);\n    ];\n}}\n",
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
    assert!(
        stderr.contains("offline mode needs fixtures"),
        "stderr: {stderr}"
    );
}
