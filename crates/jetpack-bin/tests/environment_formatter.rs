//! D-ECO12 / D-FMTPROJECT1: the environment formatter must use the real
//! jetpack plan, package realization, composed process, and zero-write batch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jet-environment-formatter-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_executable(path: &Path, source: &str) {
    fs::write(path, source).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn fixture(project: &Path, root: &Path, fixtures: &Path, scratch: &Path, fail: bool) -> Output {
    fixture_with_args(project, root, fixtures, scratch, fail, &[])
}

fn fixture_with_args(
    project: &Path,
    root: &Path,
    fixtures: &Path,
    scratch: &Path,
    fail: bool,
    extra_args: &[&str],
) -> Output {
    fs::create_dir_all(fixtures).unwrap();
    let out = root.join("hangar/provider-fixture-nixfmt");
    let bin = out.join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_executable(
        &bin.join("nixfmt"),
        "#!/bin/sh\nset -e\nif test \"x$JET_FMT_FAIL\" = x1; then exit 7; fi\nfor path do\n    printf 'formatted\\n' > \"$path\"\ndone\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin.join("nixfmt"), fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(&out, fs::Permissions::from_mode(0o555)).unwrap();
    }
    fs::write(
        fixtures.join("nixpkgs-nixfmt.json"),
        format!(
            "[{{\"drvPath\":\"/nix/store/fixture-nixfmt.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
            out.to_string_lossy()
        ),
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev { formatter: pkgs.nixfmt }\n",
    )
    .unwrap();
    fs::write(project.join("flake.nix"), "unformatted\n").unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_jetpack"));
    command
        .args(["fmt", "--lang", "nix", "--trust", "--offline", "--no-color"])
        .args(extra_args)
        .arg("--fixtures")
        .arg(fixtures)
        .current_dir(project)
        .env("JETPACK_ROOT", root)
        .env("HOME", root.join("home"))
        .env("TMPDIR", scratch);
    if fail {
        command.env("JET_FMT_FAIL", "1");
    }
    command.output().unwrap()
}

#[test]
fn formatter_realizes_typed_environment_and_writes_only_after_success() {
    let tree = TempTree::new("success");
    let project = tree.join("project");
    let root = tree.join("root");
    let fixtures = tree.join("fixtures");
    let scratch = tree.join("scratch");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&scratch).unwrap();

    let output = fixture(&project, &root, &fixtures, &scratch, false);
    assert!(
        output.status.success(),
        "formatter failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("flake.nix")).unwrap(),
        "formatted\n"
    );
    assert!(
        fs::read_dir(&scratch).unwrap().next().is_none(),
        "formatter staging directory leaked"
    );
}

#[test]
fn environment_info_discloses_the_typed_formatter_fact() {
    let tree = TempTree::new("info");
    let project = tree.join("project");
    let root = tree.join("root");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(
        project.join("env.jet"),
        "module env.dev { formatter: pkgs.nixfmt }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jetpack"))
        .args(["enter", "info", "--json", "--no-color"])
        .current_dir(&project)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "info failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("\"formatter\":\"nixfmt@nixpkgs\""),
        "formatter fact missing from info: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn formatter_failure_keeps_sources_and_cleans_staging() {
    let tree = TempTree::new("failure");
    let project = tree.join("project");
    let root = tree.join("root");
    let fixtures = tree.join("fixtures");
    let scratch = tree.join("scratch");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&scratch).unwrap();

    let output = fixture(&project, &root, &fixtures, &scratch, true);
    assert_eq!(output.status.code(), Some(7));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("E1340"),
        "missing formatter failure diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("flake.nix")).unwrap(),
        "unformatted\n"
    );
    assert!(
        fs::read_dir(&scratch).unwrap().next().is_none(),
        "formatter staging directory leaked after failure"
    );
}

#[test]
fn formatter_dry_run_reports_a_diff_without_writing_sources() {
    let tree = TempTree::new("dry-run");
    let project = tree.join("project");
    let root = tree.join("root");
    let fixtures = tree.join("fixtures");
    let scratch = tree.join("scratch");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&scratch).unwrap();

    let output = fixture_with_args(&project, &root, &fixtures, &scratch, false, &["--dry-run"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("--- flake.nix"),
        "dry-run must report a unified diff: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fs::read_to_string(project.join("flake.nix")).unwrap(),
        "unformatted\n"
    );
    assert!(
        fs::read_dir(&scratch).unwrap().next().is_none(),
        "formatter staging directory leaked after dry-run"
    );
}
