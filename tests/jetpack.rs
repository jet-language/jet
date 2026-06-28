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
    let (_base, proj, root) = core_hello_project("enter");
    let output = jetpack()
        .args(["enter", "--no-color", "--", "hello"])
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
fn jet_env_delegates_to_jetpack_enter() {
    // D-DEV4 (ratified 2026-06-17): `jet env` is the friendly Scale-2 front door
    // into the project dev shell — it delegates straight to `jetpack enter`,
    // forwarding flags and the trailing `-- cmd`. (`jet dev` is now reserved for
    // the E2-M4 watch/interpret loop.) Running through the `jet` binary must
    // reach the same composed env.
    let (_base, proj, root) = core_hello_project("jet-env");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["env", "--no-color", "--", "hello"])
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
    assert!(stderr.contains("built 3 package(s)"), "stderr: {stderr}");
}

#[test]
fn typed_module_example_builds_offline_end_to_end() {
    // I5: the committed `examples/jetpack-typed/` project is the executable spec
    // for the typed `module { … }` env surface (U3/U6/U8) including U4 import-tree
    // discovery. `jetpack build` with no ref evaluates env.jet through `modeval`:
    // the `default` source merges to its pinned nixpkgs upstream,
    // `default.[ripgrep, fd]` expands to two `Pkg` refs, and `imports:
    // find("./modules")` walks `modules/tools.jet` and folds its `default.jq`
    // into the same merge. All three realize from the committed fixtures, fully
    // offline. The store lives under a scratch JETPACK_ROOT, so nothing is
    // written back.
    let typed_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/jetpack-typed");
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

/// The committed `examples/jetpack-config/` config dir.
fn config_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/jetpack-config")
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
    assert!(
        stderr.contains("offline mode needs fixtures"),
        "stderr: {stderr}"
    );
}

// ── D-JPK-FILES Phase 2b: jetpack.toml wiring ─────────────────────────────

/// The committed multi-package monorepo example dir.
fn mono_example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/jetpack-mono")
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
