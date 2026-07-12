//! U26 package discovery process tests.
//!
//! These exercise the CLI against copied fixtures. `search`/`info` may read
//! provider fixture files and `.jet/discovery/index.jsonl`, but they must not
//! realize packages or fetch metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

fn jetpack_bin() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(path) = option_env!("CARGO_BIN_EXE_jetpack") {
            return PathBuf::from(path);
        }
        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
        let bin = target_dir
            .join("debug")
            .join(format!("jetpack{}", std::env::consts::EXE_SUFFIX));
        if !bin.is_file() {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "jetpack", "--bin", "jetpack"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .unwrap();
            assert!(status.success(), "building jetpack test binary failed");
        }
        bin
    })
}

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
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
            "jpk-discovery-{tag}-{nanos}-{:?}",
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

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-typed")
}

fn copy_project(dst: &Path) {
    copy_dir(&fixture_root(), dst);
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

#[test]
fn search_reads_local_fixture_metadata_offline() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    copy_project(&project.path);

    let out = jetpack()
        .args([
            "search",
            "rip",
            "--no-color",
            "--offline",
            "--fixtures",
            "fixtures",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("default.ripgrep"), "stdout: {stdout}");
    assert!(stdout.contains("14.1.0"), "stdout: {stdout}");
    assert!(
        project.path.join(".jet/discovery/index.jsonl").is_file(),
        "search should persist the local discovery index"
    );
}

#[test]
fn info_json_is_stable_and_includes_service_options() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    copy_project(&project.path);

    let out = jetpack()
        .args([
            "info",
            "default.ripgrep",
            "--json",
            "--no-color",
            "--offline",
            "--fixtures",
            "fixtures",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"reference\":\"default:ripgrep\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"version\":\"14.1.0\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"enable\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"ready\""), "{stdout}");
}

#[test]
fn top_level_jet_search_dispatches_to_jetpack() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    copy_project(&project.path);

    let out = jet()
        .args([
            "search",
            "jq",
            "--no-color",
            "--offline",
            "--fixtures",
            "fixtures",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("default.jq"), "stdout: {stdout}");
}
