//! U26 package discovery process tests.
//!
//! These exercise the CLI against copied fixtures. `search`/`info` may read
//! provider fixture files and `.jet/discovery/index.jsonl`, but they must not
//! realize packages or fetch metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{jetpack_bin, Scratch};

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
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
        stdout.contains("\"reference\":\"ripgrep@default\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"version\":\"14.1.0\""), "{stdout}");
    assert!(
        stdout.contains("\"maintainer_liveness\":\"not-applicable\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"name\":\"enable\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"ready\""), "{stdout}");
}

#[test]
fn jet_inspect_info_shows_registry_maintainer_liveness() {
    let project = Scratch::new("registry-info-project");
    let root = Scratch::new("registry-info-root");
    fs::create_dir_all(project.path.join(".jet")).unwrap();
    fs::write(
        project.path.join(".jet/lock"),
        "version = 1\n\n[[package]]\nname = \"textkit\"\nversion = \"1.2.0\"\nsource = { registry = \"jet\", reference = \"textkit@jet\", output = \"out\", source-hash = \"sha256-tree\", repository = \"https://registry\", authority = \"jet-registry-index\", tier = \"community\", gate-status = \"signature=passed;audit=passed;name=passed;liveness=passed;review=not-required\" }\nfingerprint = \"fp\"\ndependencies = []\n",
    )
    .unwrap();

    let json = jet()
        .args([
            "inspect",
            "info",
            "jet.textkit",
            "--json",
            "--offline",
            "--no-color",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        json.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    let json = String::from_utf8_lossy(&json.stdout);
    assert!(
        json.contains("\"maintainer_liveness\":\"passed\""),
        "{json}"
    );

    let human = jet()
        .args(["inspect", "info", "jet.textkit", "--offline", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        human.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("maintainer liveness: passed"), "{human}");
}

#[test]
fn search_matches_typed_service_options_offline() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    copy_project(&project.path);

    let out = jetpack()
        .args([
            "search",
            "ready",
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
}

#[test]
fn top_level_jet_search_routes_typed_option_queries_through_main() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    copy_project(&project.path);

    let out = jet()
        .args([
            "search",
            "ready",
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
}

#[test]
fn search_without_local_index_fails_closed() {
    let project = Scratch::new("empty-project");
    let root = Scratch::new("root");

    let out = jetpack()
        .args(["search", "ready", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no local discovery index"),
        "stderr: {stderr}"
    );
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

#[test]
fn top_level_jet_search_help_is_flat() {
    let out = jet().args(["search", "--help"]).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("jet search <query>"), "stdout: {stdout}");
    assert!(!stdout.contains("jet inspect search"), "stdout: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("E2101"), "stderr: {stderr}");
}

#[test]
fn jet_inspect_search_is_no_longer_a_canonical_route() {
    let project = Scratch::new("project");
    let root = Scratch::new("root");
    copy_project(&project.path);

    let out = jet()
        .args([
            "inspect",
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
    assert_eq!(
        out.status.code(),
        Some(2),
        "grouped search must be rejected"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "stderr: {stderr}");
    assert!(
        stderr.contains("isn't a jet inspect command"),
        "stderr: {stderr}"
    );
}
