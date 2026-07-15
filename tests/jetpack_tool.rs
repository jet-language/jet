//! D-JPK-TOOLRUN1 (card #477): `jetpack tool run|install|list|uninstall`.
//!
//! Ephemeral `tool run` realizes a ref through a built-in provider and execs
//! its binary once (nothing stays on PATH). Persistent `tool install`
//! projects bins onto `~/.jet/bin` with generation metadata. A bin name that
//! collides with a project `#Task fn` is E1297 (JPK-TOOL-COLLIDE); an
//! external provider prefix with no hangar realization path is E1298
//! (JPK-TOOL-PROVIDER). Split out per the `tests/jetpack.rs` -> per-card-file
//! convention (see `tests/jetpack_tasks.rs`); shared fixtures/helpers come
//! from `tests/support/jetpack_fixtures.rs` (see `tests/jetpack_engine.rs`).

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

fn write_tool_bin_fixture(fixtures: &Path, out_dir: &Path, pkg: &str, bin: &str, script: &str) {
    fs::create_dir_all(fixtures).unwrap();
    let bin_dir = out_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let path = bin_dir.join(bin);
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let json = format!(
        "[{{\"drvPath\":\"/nix/store/fixture-{pkg}.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join(format!("nixpkgs-{pkg}.json")), json).unwrap();
}

fn tool_install_command(
    root: &Path,
    project: &Path,
    fixtures: &Path,
    home: &Path,
    package: &str,
) -> Command {
    let mut command = jetpack();
    command
        .args([
            "tool",
            "install",
            &format!("nixpkgs:{package}"),
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(fixtures)
        .current_dir(project)
        .env("JETPACK_ROOT", root)
        .env("HOME", home);
    command
}

fn install_tool(
    root: &Path,
    project: &Path,
    fixtures: &Path,
    home: &Path,
    package: &str,
) -> Output {
    tool_install_command(root, project, fixtures, home, package)
        .output()
        .unwrap()
}

fn lifecycle_wire(root: &Path) -> String {
    let journal = root.join("hangar/lifecycle-db/journal");
    let mut paths = fs::read_dir(journal)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .collect()
}

fn ascii_hex(value: &str) -> String {
    value
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn metadata_output_hash(metadata: &str) -> String {
    metadata
        .split_once("\"output_hash\": \"")
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(digest, _)| digest.to_string())
        .unwrap()
}

#[test]
fn tool_help_lists_run_install_list_uninstall() {
    let output = jetpack().args(["help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tool run"), "stdout: {stdout}");
    assert!(stdout.contains("tool install"), "stdout: {stdout}");
    assert!(stdout.contains("tool list"), "stdout: {stdout}");
    assert!(stdout.contains("tool uninstall"), "stdout: {stdout}");
}

#[test]
fn tool_run_ephemeral_execs_builtin_provider_fixture() {
    let root = Scratch::new("tool-run-root");
    let proj = Scratch::new("tool-run-proj");
    let fixtures = Scratch::new("tool-run-fx");
    let out = Scratch::new("tool-run-out");
    write_tool_bin_fixture(
        &fixtures.path,
        &out.path,
        "greet",
        "greet",
        "#!/bin/sh\necho hello from tool run\n",
    );
    let home = Scratch::new("tool-run-home");
    let output = jetpack()
        .args([
            "tool",
            "run",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
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
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from tool run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ephemeral"),
        "stderr should mark ephemeral: {stderr}"
    );
    // Nothing projected onto ~/.jet/bin for a one-shot run.
    assert!(
        !home.join(".jet/bin/greet").exists(),
        "tool run must not leave PATH projection"
    );
}

#[test]
fn tool_run_unavailable_provider_is_e1298_not_silent() {
    let root = Scratch::new("tool-prov-root");
    let proj = Scratch::new("tool-prov-proj");
    let home = Scratch::new("tool-prov-home");
    let output = jetpack()
        .args(["tool", "run", "npm:prettier", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostic = stderr
        .find("\n  error[E1298]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("tool_provider_unavailable", diagnostic);
}

#[test]
fn tool_install_projects_real_bin_symlink_with_generation() {
    let root = Scratch::new("tool-inst-root");
    let proj = Scratch::new("tool-inst-proj");
    let fixtures = Scratch::new("tool-inst-fx");
    let out = Scratch::new("tool-inst-out");
    let home = Scratch::new("tool-inst-home");
    write_tool_bin_fixture(
        &fixtures.path,
        &out.path,
        "greet",
        "greet",
        "#!/bin/sh\necho installed greet\n",
    );
    let output = jetpack()
        .args([
            "tool",
            "install",
            "nixpkgs:greet",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let link = home.join(".jet/bin/greet");
    assert!(
        link.exists() || link.is_symlink(),
        "missing PATH projection at {}",
        link.display()
    );
    #[cfg(unix)]
    {
        assert!(
            link.symlink_metadata().unwrap().file_type().is_symlink(),
            "install must create a real symlink, not a copy"
        );
    }
    let meta = home.join(".jet/tools/generations/1/meta.json");
    assert!(meta.is_file(), "missing generation metadata {}", meta.display());
    let meta_text = fs::read_to_string(&meta).unwrap();
    assert!(meta_text.contains("\"generation\": 1"), "{meta_text}");
    assert!(meta_text.contains("nixpkgs:greet"), "{meta_text}");
    let output_hash = metadata_output_hash(&meta_text);
    assert!(output_hash.starts_with("sha256-"), "{meta_text}");
    assert!(home.join(".jet/tools/generations/1/complete").is_file());
    let profile = fs::read_to_string(home.join(".jet/tools/profile.json")).unwrap();
    assert!(profile.contains("\"current\": 1"), "{profile}");
    assert!(profile.contains("tools"), "{profile}");
    let rooted = lifecycle_wire(&root.path);
    assert!(rooted.contains(&output_hash), "{rooted}");
    assert!(
        rooted.contains(&ascii_hex("profile-generation:user:tools:1")),
        "{rooted}"
    );

    let cleaned = jetpack()
        .args(["clean", "--no-color", "--yes"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        cleaned.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    assert!(link.exists(), "lease teardown and clean must preserve rooted tool");

    let listed = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(listed.status.success());
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(stdout.contains("greet"), "stdout: {stdout}");

    let removed = jetpack()
        .args(["tool", "uninstall", "greet", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        removed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!link.exists(), "uninstall must remove PATH projection");
    let empty_meta = fs::read_to_string(home.join(".jet/tools/generations/2/meta.json")).unwrap();
    assert!(empty_meta.contains("\"tools\": [\n  ]"), "{empty_meta}");
    assert!(home.join(".jet/tools/generations/2/complete").is_file());
    let profile = fs::read_to_string(home.join(".jet/tools/profile.json")).unwrap();
    assert!(profile.contains("\"current\": 2"), "{profile}");
    let roots_after_empty = lifecycle_wire(&root.path);
    assert!(
        !roots_after_empty.contains(&ascii_hex("profile-generation:user:tools:2")),
        "empty generation must not create a root: {roots_after_empty}"
    );
}

#[test]
fn concurrent_installs_serialize_and_retain_two_rooted_generations() {
    let root = Scratch::new("tool-concurrent-root");
    let proj = Scratch::new("tool-concurrent-proj");
    let fixtures = Scratch::new("tool-concurrent-fx");
    let left_out = Scratch::new("tool-concurrent-left-out");
    let right_out = Scratch::new("tool-concurrent-right-out");
    let home = Scratch::new("tool-concurrent-home");
    write_tool_bin_fixture(
        &fixtures.path,
        &left_out.path,
        "left",
        "left",
        "#!/bin/sh\necho left\n",
    );
    write_tool_bin_fixture(
        &fixtures.path,
        &right_out.path,
        "right",
        "right",
        "#!/bin/sh\necho right\n",
    );

    let left = tool_install_command(
        &root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "left",
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let right = tool_install_command(
        &root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "right",
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let left = left.wait_with_output().unwrap();
    let right = right.wait_with_output().unwrap();
    assert!(
        left.status.success(),
        "left stderr: {}",
        String::from_utf8_lossy(&left.stderr)
    );
    assert!(
        right.status.success(),
        "right stderr: {}",
        String::from_utf8_lossy(&right.stderr)
    );

    let profile = fs::read_to_string(home.join(".jet/tools/profile.json")).unwrap();
    assert!(profile.contains("\"current\": 2"), "{profile}");
    let first = fs::read_to_string(home.join(".jet/tools/generations/1/meta.json")).unwrap();
    let second = fs::read_to_string(home.join(".jet/tools/generations/2/meta.json")).unwrap();
    assert!(first.contains("left") || first.contains("right"), "{first}");
    assert!(second.contains("left") && second.contains("right"), "{second}");
    assert!(home.join(".jet/tools/generations/1/complete").is_file());
    assert!(home.join(".jet/tools/generations/2/complete").is_file());
    let roots = lifecycle_wire(&root.path);
    for generation in [1, 2] {
        assert!(
            roots.contains(&ascii_hex(&format!(
                "profile-generation:user:tools:{generation}"
            ))),
            "missing retained generation {generation}: {roots}"
        );
    }

    let listed = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(listed.status.success());
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(stdout.contains("left") && stdout.contains("right"), "{stdout}");
}

#[test]
fn profile_failpoints_recover_conservatively_around_root_and_pointer_publish() {
    for phase in ["after-generation", "after-root-commit", "after-pointer"] {
        let root = Scratch::new(&format!("tool-fail-{phase}-root"));
        let proj = Scratch::new(&format!("tool-fail-{phase}-proj"));
        let fixtures = Scratch::new(&format!("tool-fail-{phase}-fx"));
        let out = Scratch::new(&format!("tool-fail-{phase}-out"));
        let home = Scratch::new(&format!("tool-fail-{phase}-home"));
        write_tool_bin_fixture(
            &fixtures.path,
            &out.path,
            "greet",
            "greet",
            "#!/bin/sh\necho failpoint greet\n",
        );

        let failed = tool_install_command(
            &root.path,
            &proj.path,
            &fixtures.path,
            &home.path,
            "greet",
        )
        .env("JETPACK_INTERNAL_TEST_PROFILE_FAILPOINT", phase)
        .output()
        .unwrap();
        assert_eq!(
            failed.status.code(),
            Some(2),
            "phase {phase}: {}",
            String::from_utf8_lossy(&failed.stderr)
        );
        assert!(home.join(".jet/tools/generations/1/complete").is_file());
        let pointer = home.join(".jet/tools/profile.json");
        if phase == "after-pointer" {
            assert!(
                fs::read_to_string(&pointer).unwrap().contains("\"current\": 1"),
                "phase {phase}"
            );
        } else {
            assert!(!pointer.exists(), "phase {phase}");
        }

        let retried = install_tool(
            &root.path,
            &proj.path,
            &fixtures.path,
            &home.path,
            "greet",
        );
        assert!(
            retried.status.success(),
            "phase {phase}: {}",
            String::from_utf8_lossy(&retried.stderr)
        );
        let pointer = fs::read_to_string(&pointer).unwrap();
        assert!(pointer.contains("\"current\": 2"), "phase {phase}: {pointer}");
        assert!(home.join(".jet/tools/generations/1").is_dir());
        assert!(home.join(".jet/tools/generations/2/complete").is_file());
        let roots = lifecycle_wire(&root.path);
        assert!(
            roots.contains(&ascii_hex("profile-generation:user:tools:2")),
            "phase {phase}: {roots}"
        );
        if phase == "after-generation" {
            assert!(
                !roots.contains(&ascii_hex("profile-generation:user:tools:1")),
                "unpublished unrooted generation gained a root: {roots}"
            );
        } else {
            assert!(
                roots.contains(&ascii_hex("profile-generation:user:tools:1")),
                "durable pre-pointer root was lost: {roots}"
            );
        }
    }
}

#[test]
fn tool_install_task_collision_is_e1297_snapshot() {
    let root = Scratch::new("tool-collide-root");
    let proj = Scratch::new("tool-collide-proj");
    let fixtures = Scratch::new("tool-collide-fx");
    let out = Scratch::new("tool-collide-out");
    let home = Scratch::new("tool-collide-home");
    write_tool_bin_fixture(
        &fixtures.path,
        &out.path,
        "serve",
        "serve",
        "#!/bin/sh\necho serve tool\n",
    );
    fs::write(
        proj.join("main.jet"),
        "#Task fn serve() {\n    print(\"task\")\n}\n\nfn run() {\n    print(\"run\")\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args([
            "tool",
            "install",
            "nixpkgs:serve",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostic = stderr
        .find("\n  error[E1297]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("tool_task_collide", diagnostic);
    assert!(!home.join(".jet/bin/serve").exists());
}
