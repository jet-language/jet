//! U29 (D-JPK-OFFLINE1=A): offline guarantee process tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::jetpack_bin;

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

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
            "jpk-offline-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        make_tree_writable(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn make_tree_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            make_tree_writable(&entry.unwrap().path());
        }
    }
    if !meta.file_type().is_symlink() {
        let mode = if meta.is_dir() { 0o755 } else { meta.permissions().mode() | 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
}

#[cfg(not(unix))]
fn make_tree_writable(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    let mut permissions = meta.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_runnable_fixture(
    fixtures: &Path,
    root: &Path,
    staging_dir: &Path,
    lua_path: Option<&str>,
) -> PathBuf {
    fs::create_dir_all(fixtures).unwrap();
    let bin = staging_dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let greet = bin.join(if cfg!(windows) { "greet.bat" } else { "greet" });
    #[cfg(windows)]
    fs::write(&greet, "@echo hello from offline cache\r\n").unwrap();
    #[cfg(not(windows))]
    {
        fs::write(&greet, "#!/bin/sh\necho hello from offline cache\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&greet, fs::Permissions::from_mode(0o755)).unwrap();
    }
    if let Some(lua_path) = lua_path {
        fs::write(staging_dir.join("lua-path"), lua_path).unwrap();
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
    let json = format!(
        "[{{\"drvPath\":\"/nix/store/0fixture00000000000000000000-greet.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-greet.json"), json).unwrap();
    out_dir
}

#[test]
fn offline_build_and_run_use_hangar_cache_with_network_denied() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    let fixtures = Scratch::new("fixtures");
    let out_dir = Scratch::new("out");
    write_runnable_fixture(&fixtures.path, &root.path, &out_dir.path, None);

    let first = jetpack()
        .args([
            "build",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = jetpack()
        .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("cached"), "stderr: {stderr}");
    assert!(!stderr.contains("fixture"), "stderr: {stderr}");

    let run = jetpack()
        .args([
            "run",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--",
            "greet",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("hello from offline cache"),
        "stdout: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn selected_workspace_run_propagates_unsafe_metadata_failure_before_command() {
    let project = Scratch::new("unsafe-metadata-project");
    let root = Scratch::new("unsafe-metadata-root");
    let fixtures = Scratch::new("unsafe-metadata-fixtures");
    let out_dir = Scratch::new("unsafe-metadata-out");
    write_runnable_fixture(
        &fixtures.path,
        &root.path,
        &out_dir.path,
        Some("../outside\n"),
    );

    let member = project.path.join("packages/hello");
    fs::create_dir_all(&member).unwrap();
    fs::write(
        project.path.join("workspace.jet"),
        "module workspace { members: find(\"./packages\") }\n",
    )
    .unwrap();
    fs::write(
        member.join("pkg.jet"),
        "payload: { name: \"hello\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(
        member.join("hello.jet"),
        "module hello { pub fn greeting() => String { return \"hello\" } }\n",
    )
    .unwrap();
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [nixpkgs.greet] }\n",
    )
    .unwrap();

    let sentinel = project.path.join("command-ran");
    let output = jetpack()
        .args(["run", "-p", "hello", "--offline", "--fixtures"])
        .arg(&fixtures.path)
        .args(["--", "sh", "-c", "printf executed > \"$1\"", "jet-test"])
        .arg(&sentinel)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!sentinel.exists(), "requested command ran after compose failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("couldn't compose package environment").count(),
        1,
        "stderr: {stderr}"
    );
}

/// D-JPK-OFFLINE2=B tamper gate: after a first build records the locked
/// identity + closure digest, corrupting the realized closure on disk must make
/// the offline reuse refuse loudly (integrity), naming the artifact — never
/// silently serve the stale/mismatched copy (card #418 trust hole stays closed).
#[test]
fn offline_reuse_refuses_a_tampered_closure() {
    let project = Scratch::new("tamper-project");
    let root = Scratch::new("tamper-root");
    let fixtures = Scratch::new("tamper-fixtures");
    let out_dir = Scratch::new("tamper-out");
    let seeded_out = write_runnable_fixture(&fixtures.path, &root.path, &out_dir.path, None);

    let first = jetpack()
        .args([
            "build",
            "greet@nixpkgs",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    // Corrupt the realized closure on disk (a partial GC / bit-rot / tamper
    // stand-in): drop the executable the recorded digest covers.
    let greet = seeded_out
        .join("bin")
        .join(if cfg!(windows) { "greet.bat" } else { "greet" });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(
            seeded_out.join("bin"),
            fs::Permissions::from_mode(0o755),
        );
    }
    fs::remove_file(&greet).unwrap();

    let second = jetpack()
        .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert!(
        !second.status.success(),
        "a tampered closure must refuse, not serve stale; stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("greet"),
        "the refusal must name the artifact; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("hello from offline cache"),
        "stale bytes must never be served; stderr: {stderr}"
    );
}

#[test]
fn offline_missing_object_is_loud_e1276() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    let out = jetpack()
        .args(["build", "greet@nixpkgs", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_DENY_NETWORK", "1")
        .env_remove("JETPACK_FIXTURES")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1276"), "stderr: {stderr}");
    assert!(stderr.contains("greet@nixpkgs"), "stderr: {stderr}");
}

#[test]
fn network_class_verbs_refuse_offline_immediately() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    for args in [
        &["update", "--no-color", "--offline"][..],
        &["outdated", "--no-color", "--offline"][..],
        &["add", "ripgrep@nixpkgs", "--no-color", "--offline"][..],
    ] {
        let out = jetpack()
            .args(args)
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("JETPACK_DENY_NETWORK", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "args: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E1276"), "args: {args:?}\nstderr: {stderr}");
        assert!(
            stderr.contains("network-class command"),
            "args: {args:?}\nstderr: {stderr}"
        );
    }
}
