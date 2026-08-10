//! D-JPK-TOOLRUN1 (card #477): `jetpack tool run|install|list|uninstall`.
//!
//! Ephemeral `tool run` realizes a ref through a built-in provider and execs
//! its binary once (nothing stays on PATH). Persistent `tool install`
//! projects bins onto `~/.jet/bin` with generation metadata. A bin name that
//! collides with a project `#Job fn` is E1297 (JPK-TOOL-COLLIDE); an
//! external provider prefix with no hangar realization path is E1298
//! (JPK-TOOL-PROVIDER). Split out per the `tests/jetpack.rs` -> per-card-file
//! convention (see `tests/jetpack_tasks.rs`); shared fixtures/helpers come
//! from `tests/support/jetpack_fixtures.rs` (see `tests/jetpack_engine.rs`).

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

fn physical_bin(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn write_tool_bin_fixture(root: &Path, fixtures: &Path, pkg: &str, bin: &str, script: &str) {
    fs::create_dir_all(fixtures).unwrap();
    let out_dir = root.join("hangar").join(format!("provider-fixture-{pkg}"));
    let bin_dir = out_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let path = bin_dir.join(bin);
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(&bin_dir, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(&out_dir, fs::Permissions::from_mode(0o555)).unwrap();
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
            &format!("{package}@nixpkgs"),
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
fn command_helpers_use_stable_filesystem_root() {
    let temp = std::env::temp_dir();
    let root = temp.ancestors().last().unwrap();
    assert!(root.has_root());
    for command in [jetpack(), jet(), jetos()] {
        assert_eq!(command.get_current_dir(), Some(root));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn stable_cwd_survives_abrupt_parent_and_detached_descendant() {
    let proof = Scratch::new("stable-cwd-proof");
    let marker = proof.join("cwd");
    let status = neutral_command(Path::new("sh"))
        .args([
            "-c",
            "setsid sh -c 'sleep 0.05; pwd > \"$1\"' sh \"$1\" </dev/null >/dev/null 2>&1 & kill -KILL $$",
            "sh",
            marker.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !marker.is_file() {
        assert!(std::time::Instant::now() < deadline, "detached cwd proof timed out");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let temp = std::env::temp_dir();
    let root = temp.ancestors().last().unwrap();
    assert_eq!(fs::read_to_string(marker).unwrap().trim(), root.to_str().unwrap());
}

fn json_meta_field(metadata: &str, key: &str) -> String {
    let needle = format!("\"{key}\": \"");
    metadata
        .split_once(&needle)
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(value, _)| value.to_string())
        .unwrap_or_else(|| panic!("missing {key} in {metadata}"))
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
    write_tool_bin_fixture(
        &root.path,
        &fixtures.path,
        "greet",
        "greet",
        "#!/bin/sh\necho hello from tool run\n",
    );
    let home = Scratch::new("tool-run-home");
    let output = jetpack()
        .args([
            "tool",
            "run",
            "greet@nixpkgs",
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
        !home.join(".jet/bin").join(physical_bin("greet")).exists(),
        "tool run must not leave PATH projection"
    );
}

#[test]
fn tool_run_unavailable_provider_is_e1298_not_silent() {
    // D-JPK-REF1 keeps the package first even for a provider that is gated.
    let root = Scratch::new("tool-prov-root");
    let proj = Scratch::new("tool-prov-proj");
    let home = Scratch::new("tool-prov-home");
    let output = jetpack()
        .args(["tool", "run", "prettier@npm", "--no-color"])
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
fn tool_install_publishes_stable_dispatcher_and_generation() {
    let root = Scratch::new("tool-inst-root");
    let proj = Scratch::new("tool-inst-proj");
    let fixtures = Scratch::new("tool-inst-fx");
    let home = Scratch::new("tool-inst-home");
    write_tool_bin_fixture(
        &root.path,
        &fixtures.path,
        "greet",
        "greet",
        "#!/bin/sh\necho installed greet\n",
    );
    let output = jetpack()
        .args([
            "tool",
            "install",
            "greet@nixpkgs",
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
    let link = home.join(".jet/bin").join(physical_bin("greet"));
    assert!(
        link.exists() || link.is_symlink(),
        "missing PATH projection at {}",
        link.display()
    );
    #[cfg(unix)]
    {
        assert!(
            link.symlink_metadata().unwrap().file_type().is_file(),
            "install must create a stable dispatcher"
        );
    }
    let path = env::join_paths(
        std::iter::once(link.parent().unwrap().to_path_buf()).chain(
            env::var_os("PATH")
                .into_iter()
                .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>()),
        ),
    )
    .unwrap();
    let invoked = Command::new(&link)
        .env("HOME", &home.path)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        invoked.status.success(),
        "status={:?}, stderr={}",
        invoked.status.code(),
        String::from_utf8_lossy(&invoked.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&invoked.stdout).trim(), "installed greet");
    let meta = home.join(".jet/tools/generations/1/meta.json");
    assert!(meta.is_file(), "missing generation metadata {}", meta.display());
    let meta_text = fs::read_to_string(&meta).unwrap();
    assert!(meta_text.contains("\"generation\": 1"), "{meta_text}");
    assert!(meta_text.contains("greet@nixpkgs"), "{meta_text}");
    let output_hash = metadata_output_hash(&meta_text);
    assert!(output_hash.starts_with("sha256-"), "{meta_text}");
    assert!(home.join(".jet/tools/generations/1/complete").is_file());
    let profile = fs::read_to_string(home.join(".jet/tools/profile.json")).unwrap();
    assert!(profile.contains("\"current\": 1"), "{profile}");
    let current = fs::read_to_string(home.join(".jet/tools/current")).unwrap();
    assert!(current.contains("generation\t1"), "{current}");
    assert!(current.contains("checksum\tsha256-"), "{current}");
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

    let projection = home
        .join(".jet/tools/generations/1/bin")
        .join(physical_bin("greet"));
    let original_projection = fs::read(&projection).unwrap();
    let original_permissions = fs::metadata(&projection).unwrap().permissions();
    let mut writable_permissions = original_permissions.clone();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        writable_permissions.set_mode(writable_permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    writable_permissions.set_readonly(false);
    fs::set_permissions(&projection, writable_permissions).unwrap();
    fs::write(&projection, b"#!/bin/sh\necho corrupted\n").unwrap();
    let rejected = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("projection proof mismatch"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    fs::write(&projection, original_projection).unwrap();
    fs::set_permissions(&projection, original_permissions).unwrap();

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
    assert!(link.exists(), "stable dispatcher must remain after uninstall");
    let rejected = Command::new(&link).output().unwrap();
    assert_eq!(rejected.status.code(), Some(127));
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

#[cfg(windows)]
#[test]
fn windows_logical_alias_executes_native_exe_with_exact_arguments_and_exit() {
    let root = Scratch::new("tool-win-root");
    let proj = Scratch::new("tool-win-proj");
    let fixtures = Scratch::new("tool-win-fx");
    let home = Scratch::new("tool-win-home");
    fs::create_dir_all(&fixtures.path).unwrap();
    let out_dir = root.path.join("hangar/provider-fixture-native");
    let bin_dir = out_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let system_root = env::var_os("SystemRoot").unwrap();
    fs::copy(
        Path::new(&system_root).join("System32/cmd.exe"),
        bin_dir.join("cmd.exe"),
    )
    .unwrap();
    let json = format!(
        "[{{\"drvPath\":\"C:\\\\fixture-native.drv\",\"outputs\":{{\"out\":{:?}}}}}]",
        out_dir.to_string_lossy()
    );
    fs::write(fixtures.join("nixpkgs-native.json"), json).unwrap();
    let output = tool_install_command(
        &root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "native",
    )
    .args(["--as", "native-alias"])
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let alias = home.join(".jet/bin/native-alias.exe");
    assert!(alias.is_file());
    let path = env::join_paths(
        std::iter::once(alias.parent().unwrap().to_path_buf()).chain(
            env::var_os("PATH")
                .into_iter()
                .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>()),
        ),
    )
    .unwrap();
    let status = Command::new("native-alias")
        .args([
            "/d",
            "/s",
            "/c",
            "if \"a&b\"==\"a&b\" (exit /b 23) else (exit /b 9)",
        ])
        .env("HOME", &home.path)
        .env("PATH", path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(23));
}

#[test]
fn concurrent_installs_serialize_and_retain_two_rooted_generations() {
    let left_root = Scratch::new("tool-concurrent-left-root");
    let right_root = Scratch::new("tool-concurrent-right-root");
    let proj = Scratch::new("tool-concurrent-proj");
    let fixtures = Scratch::new("tool-concurrent-fx");
    let home = Scratch::new("tool-concurrent-home");
    write_tool_bin_fixture(
        &left_root.path,
        &fixtures.path,
        "left",
        "left",
        "#!/bin/sh\necho left\n",
    );
    write_tool_bin_fixture(
        &right_root.path,
        &fixtures.path,
        "right",
        "right",
        "#!/bin/sh\necho right\n",
    );

    let left = tool_install_command(
        &left_root.path,
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
        &right_root.path,
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
    let left_roots = lifecycle_wire(&left_root.path);
    let right_roots = lifecycle_wire(&right_root.path);
    let generation_one = ascii_hex("profile-generation:user:tools:1");
    let generation_two = ascii_hex("profile-generation:user:tools:2");
    assert!(
        left_roots.contains(&generation_one) || right_roots.contains(&generation_one),
        "missing retained generation 1: left={left_roots}, right={right_roots}"
    );
    assert!(
        left_roots.contains(&generation_two) && right_roots.contains(&generation_two),
        "generation 2 must bind both Store authorities: left={left_roots}, right={right_roots}"
    );

    let listed = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &left_root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(listed.status.success());
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(stdout.contains("left") && stdout.contains("right"), "{stdout}");
}

#[test]
fn multi_store_root_commit_crash_recovers_before_pointer_switch() {
    let left_root = Scratch::new("tool-multiroot-left");
    let right_root = Scratch::new("tool-multiroot-right");
    let proj = Scratch::new("tool-multiroot-proj");
    let fixtures = Scratch::new("tool-multiroot-fx");
    let home = Scratch::new("tool-multiroot-home");
    write_tool_bin_fixture(&left_root.path, &fixtures.path, "left", "left", "#!/bin/sh\necho left\n");
    write_tool_bin_fixture(&right_root.path, &fixtures.path, "right", "right", "#!/bin/sh\necho right\n");
    let left = tool_install_command(
        &left_root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "left",
    )
    .output()
    .unwrap();
    assert!(left.status.success(), "{}", String::from_utf8_lossy(&left.stderr));

    let interrupted = tool_install_command(
        &right_root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "right",
    )
    .env("JETPACK_INTERNAL_TEST_PROFILE_FAILPOINT", "between-root-commits")
    .output()
    .unwrap();
    assert_eq!(interrupted.status.code(), Some(2));
    assert!(home.join(".jet/tools/generations/2/complete").is_file());
    assert!(fs::read_to_string(home.join(".jet/tools/current")).unwrap().contains("generation\t1"));

    let recovered = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &left_root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(recovered.status.success(), "{}", String::from_utf8_lossy(&recovered.stderr));
    assert!(fs::read_to_string(home.join(".jet/tools/current")).unwrap().contains("generation\t2"));
    let generation_two = ascii_hex("profile-generation:user:tools:2");
    assert!(lifecycle_wire(&left_root.path).contains(&generation_two));
    assert!(lifecycle_wire(&right_root.path).contains(&generation_two));
}

#[test]
fn legacy_generation_is_verified_and_migrated_to_owned_projection() {
    let root = Scratch::new("tool-legacy-root");
    let proj = Scratch::new("tool-legacy-proj");
    let fixtures = Scratch::new("tool-legacy-fx");
    let home = Scratch::new("tool-legacy-home");
    write_tool_bin_fixture(
        &root.path,
        &fixtures.path,
        "greet",
        "greet",
        "#!/bin/sh\necho migrated greet\n",
    );
    let installed = tool_install_command(
        &root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "greet",
    )
    .output()
    .unwrap();
    assert!(installed.status.success(), "{}", String::from_utf8_lossy(&installed.stderr));

    let canonical = fs::read_to_string(home.join(".jet/tools/generations/1/meta.json")).unwrap();
    let output_hash = metadata_output_hash(&canonical);
    let store_root = json_meta_field(&canonical, "store_root");
    let mut store_meta = fs::read_dir(Path::new(&store_root).join("hangar"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("meta.json"))
        .filter(|path| path.is_file())
        .map(|path| fs::read_to_string(path).unwrap())
        .find(|metadata| metadata.contains("greet@nixpkgs"))
        .expect("greet Store metadata");
    let bin = json_meta_field(&store_meta, "bin");
    store_meta.clear();
    let target = Path::new(&bin).join("greet").to_string_lossy().into_owned();
    let legacy = format!(
        "{{\n  \"generation\": 1,\n  \"profile\": \"tools\",\n  \"created_at\": 1,\n  \"tools\": [\n    {{\n      \"name\": \"greet\",\n      \"version\": \"\",\n      \"source\": \"nixpkgs\",\n      \"reference\": \"greet@nixpkgs\",\n      \"output_hash\": {output_hash:?},\n      \"bins\": [\"greet\"],\n      \"targets\": [{target:?}]\n    }}\n  ]\n}}\n"
    );
    fs::write(home.join(".jet/tools/generations/1/meta.json"), legacy).unwrap();
    fs::write(home.join(".jet/tools/generations/1/complete"), "complete\n").unwrap();
    fs::remove_file(home.join(".jet/tools/current")).unwrap();
    fs::remove_file(home.join(".jet/tools/profile.json")).unwrap();

    let migrated = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(migrated.status.success(), "{}", String::from_utf8_lossy(&migrated.stderr));
    assert!(home.join(".jet/tools/legacy-generations/generation-1").is_dir());
    assert!(
        home.join(".jet/tools/generations/2/bin")
            .join(physical_bin("greet"))
            .is_file()
    );
    let current = fs::read_to_string(home.join(".jet/tools/current")).unwrap();
    assert!(current.contains("generation\t2"), "{current}");
    let invoked = Command::new(home.join(".jet/bin").join(physical_bin("greet")))
        .output()
        .unwrap();
    assert!(invoked.status.success());
    assert_eq!(String::from_utf8_lossy(&invoked.stdout).trim(), "migrated greet");
}

#[test]
fn profile_failpoints_recover_conservatively_around_root_and_pointer_publish() {
    for phase in [
        "after-generation",
        "after-root-prepare",
        "after-root-commit",
        "before-pointer",
        "after-current-pointer",
        "after-pointer",
    ] {
        let root = Scratch::new(&format!("tool-fail-{phase}-root"));
        let proj = Scratch::new(&format!("tool-fail-{phase}-proj"));
        let fixtures = Scratch::new(&format!("tool-fail-{phase}-fx"));
        let home = Scratch::new(&format!("tool-fail-{phase}-home"));
        write_tool_bin_fixture(
            &root.path,
            &fixtures.path,
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
        let pointer = home.join(".jet/tools/current");
        let mirror = home.join(".jet/tools/profile.json");
        if phase == "after-pointer" {
            assert!(
                fs::read_to_string(&pointer).unwrap().contains("generation\t1"),
                "phase {phase}"
            );
            assert!(mirror.is_file(), "phase {phase}");
        } else if phase == "after-current-pointer" {
            assert!(fs::read_to_string(&pointer).unwrap().contains("generation\t1"));
            assert!(!mirror.exists(), "phase {phase}");
        } else {
            assert!(!pointer.exists(), "phase {phase}");
        }

        let recovered = jetpack()
            .args(["tool", "list", "--no-color"])
            .current_dir(&proj.path)
            .env("JETPACK_ROOT", &root.path)
            .env("HOME", &home.path)
            .output()
            .unwrap();
        assert!(
            recovered.status.success(),
            "phase {phase}: {}",
            String::from_utf8_lossy(&recovered.stderr)
        );
        let pointer = fs::read_to_string(&pointer).unwrap();
        assert!(pointer.contains("generation\t1"), "phase {phase}: {pointer}");
        assert!(home.join(".jet/tools/generations/1").is_dir());
        assert!(!home.join(".jet/tools/generations/2").exists());
        let roots = lifecycle_wire(&root.path);
        assert!(
            roots.contains(&ascii_hex("profile-generation:user:tools:1")),
            "phase {phase}: {roots}"
        );
    }
}

#[test]
fn tool_install_task_collision_is_e1297_snapshot() {
    // The collision guidance must teach the canonical email-order ref.
    let root = Scratch::new("tool-collide-root");
    let proj = Scratch::new("tool-collide-proj");
    let fixtures = Scratch::new("tool-collide-fx");
    let home = Scratch::new("tool-collide-home");
    write_tool_bin_fixture(
        &root.path,
        &fixtures.path,
        "serve",
        "serve",
        "#!/bin/sh\necho serve tool\n",
    );
    fs::write(
        proj.join("main.jet"),
        "#Job fn serve() {\n    print(\"task\")\n}\n\nfn run() {\n    print(\"run\")\n}\n",
    )
    .unwrap();
    let output = jetpack()
        .args([
            "tool",
            "install",
            "serve@nixpkgs",
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
    assert!(
        !home
            .join(".jet/bin")
            .join(physical_bin("serve"))
            .exists()
    );
}

#[test]
fn test_runs_jet_tests_and_propagates_failure() {
    let project = Scratch::new("test-project");
    let root = Scratch::new("test-root");
    fs::write(
        project.join("package.jet"),
        "name: \"test_project\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("main.jet"),
        "#Test(\"failing test\") {\n    require(false)\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "use jetpack as pkg\npub fn shell() => [JSON] = [pkg.source(\"local\", \"./\", \"core\"), pkg.packages([\"test_project@local\"]), pkg.prompt(\"test\")]\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["test", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "jetpack test must report the failing test\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("failing test") || stderr.contains("failing test"),
        "test name missing from output:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("FAIL") || stderr.contains("FAIL"),
        "failure missing from output:\nstdout: {stdout}\nstderr: {stderr}"
    );
}
