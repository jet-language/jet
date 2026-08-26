//! D-JPK-TOOLRUN1 (card #477): `jetpack tool install|list|uninstall`.
//!
//! Ephemeral package shells use `jetpack use`. Persistent `tool install`
//! projects bins onto `~/.jet/bin` with generation metadata. A bin name that
//! collides with a project `#Job fn` is E1297 (JPK-TOOL-COLLIDE); an
//! external provider prefix with no hangar realization path is E1298
//! (JPK-TOOL-PROVIDER). Split out per the `tests/jetpack.rs` -> per-card-file
//! convention (see `tests/jetpack_tasks.rs`); shared fixtures/helpers come
//! from `tests/support/jetpack_fixtures.rs` (see `tests/jetpack_engine.rs`).

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

mod common;

use jetpack::SHA256;

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

fn write_native_omp_fixture(fixtures: &Path, version: &str, script: &str) {
    fs::create_dir_all(fixtures).unwrap();
    let artifact_name = format!("omp-{version}");
    let artifact = fixtures.join(&artifact_name);
    fs::write(&artifact, script).unwrap();
    let digest = SHA256::sha256_file_hex(&artifact).unwrap();
    fs::write(
        fixtures.join("jetpackage-omp.json"),
        format!(
            "{{\"tag\":\"v{version}\",\"version\":\"{version}\",\"sha256\":\"{digest}\",\"artifact\":\"{artifact_name}\"}}"
        ),
    )
    .unwrap();
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
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
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
        assert!(
            std::time::Instant::now() < deadline,
            "detached cwd proof timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let temp = std::env::temp_dir();
    let root = temp.ancestors().last().unwrap();
    assert_eq!(
        fs::read_to_string(marker).unwrap().trim(),
        root.to_str().unwrap()
    );
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
fn tool_help_lists_install_list_uninstall_and_shell_verbs() {
    let output = jetpack().args(["help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tool install"), "stdout: {stdout}");
    assert!(stdout.contains("tool list"), "stdout: {stdout}");
    assert!(stdout.contains("tool uninstall"), "stdout: {stdout}");
    assert!(stdout.contains("env <name>"), "stdout: {stdout}");
    assert!(stdout.contains("use <package>"), "stdout: {stdout}");
}

#[test]
fn use_ephemeral_execs_inside_and_outside_project_without_path_projection() {
    let root = Scratch::new("use-root");
    let project = Scratch::new("use-project");
    let outside = Scratch::new("use-outside");
    let fixtures = Scratch::new("use-fx");
    write_native_omp_fixture(
        &fixtures.path,
        "1.0.0",
        "#!/bin/sh\necho hello from jetpack use\n",
    );
    // A project package plan proves `use` does not merge the cwd project plan
    // into the explicitly named package set.
    fs::write(
        project.join("env.jet"),
        "module env.dev { packages: [\"missing@releases#1.0.0\"] }\n",
    )
    .unwrap();
    let home = Scratch::new("use-home");
    for cwd in [&project.path, &outside.path] {
        let command = format!(
            "stty rows 30 cols 120; exec {} use omp@releases#1.0.0 -y --no-color --offline --fixtures {}",
            test_shell_quote(common::jetpack_bin()),
            test_shell_quote(&fixtures.path),
        );
        let mut child = Command::new("script")
            .args(["-qfec", &command, "/dev/null"])
            .current_dir(cwd)
            .env("JETPACK_ROOT", &root.path)
            .env("HOME", &home.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                b"if test \"$JETPACK_REF\" = 'omp@releases#1.0.0' && command -v omp >/dev/null; then printf 'JETPACK_USE_OK\\n'; exit 0; else exit 1; fi\n",
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "cwd={}: stdout: {} stderr: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("JETPACK_USE_OK"),
            "cwd={}: interactive shell probe did not run: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    // `use` is ephemeral: it does not publish a tool profile or PATH bin.
    assert!(
        !home.join(".jet/bin").join(physical_bin("omp")).exists(),
        "use must not leave PATH projection"
    );
    assert!(!home.join(".jet/tools/profile.json").exists());
}

#[test]
fn use_prep_materializes_then_entry_reuses_the_package() {
    let root = Scratch::new("use-prep-root");
    let project = Scratch::new("use-prep-project");
    let fixtures = Scratch::new("use-prep-fx");
    let home = Scratch::new("use-prep-home");
    write_native_omp_fixture(&fixtures.path, "1.0.0", "#!/bin/sh\necho prepared omp\n");
    let prep = jetpack()
        .args([
            "use",
            "omp@releases#1.0.0",
            "--prep",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        prep.status.success(),
        "use --prep failed: {}",
        String::from_utf8_lossy(&prep.stderr)
    );
    assert!(prep.stdout.is_empty(), "prep must not enter a shell");
    fs::remove_file(fixtures.path.join("jetpackage-omp.json")).unwrap();
    fs::remove_file(fixtures.path.join("omp-1.0.0")).unwrap();

    let entry = jetpack()
        .args([
            "use",
            "omp@releases#1.0.0",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .args(["--", "omp"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        entry.status.success(),
        "entry after use --prep failed: {}",
        String::from_utf8_lossy(&entry.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&entry.stdout).trim(), "prepared omp");
}

#[test]
fn use_unavailable_provider_is_e1298_not_silent() {
    // D-JPK-REF1 keeps the package first even for a provider that is gated.
    let root = Scratch::new("tool-prov-root");
    let proj = Scratch::new("tool-prov-proj");
    let home = Scratch::new("tool-prov-home");
    let output = jetpack()
        .args(["use", "prettier@npm", "--no-color"])
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
fn tool_install_publishes_real_projection_and_generation() {
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
    assert_eq!(
        fs::read(&link).unwrap(),
        b"#!/bin/sh\necho installed greet\n",
        "install must project the tool bytes, not jetpack"
    );
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
    assert_eq!(
        String::from_utf8_lossy(&invoked.stdout).trim(),
        "installed greet"
    );
    let meta = home.join(".jet/tools/generations/1/meta.json");
    assert!(
        meta.is_file(),
        "missing generation metadata {}",
        meta.display()
    );
    let meta_text = fs::read_to_string(&meta).unwrap();
    assert!(meta_text.contains("\"generation\": 1"), "{meta_text}");
    assert!(meta_text.contains("greet@nixpkgs"), "{meta_text}");
    let output_hash = metadata_output_hash(&meta_text);
    assert!(output_hash.starts_with("sha256-"), "{meta_text}");
    assert!(home.join(".jet/tools/generations/1/complete").is_file());
    let profile = fs::read_to_string(home.join(".jet/tools/profile.json")).unwrap();
    assert!(profile.contains("\"current\": 1"), "{profile}");
    let manifest = fs::read_to_string(home.join(".jet/tools/manifest.json")).unwrap();
    assert!(
        manifest.contains("\"reference\":\"greet@nixpkgs\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("\"resolved\":\"greet@nixpkgs\""),
        "{manifest}"
    );
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
        .args(["hangar", "clean", "--no-color", "--yes"])
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
    assert!(
        link.exists(),
        "lease teardown and clean must preserve rooted tool"
    );

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
    assert!(!link.exists(), "uninstall must remove the managed projection");
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
fn native_omp_recipe_admits_hangar_projects_and_rolls_back() {
    let root = Scratch::new("native-omp-root");
    let project = Scratch::new("native-omp-project");
    let fixtures = Scratch::new("native-omp-fixtures");
    let home = Scratch::new("native-omp-home");
    write_native_omp_fixture(&fixtures.path, "1.0.0", "#!/bin/sh\necho omp version one\n");

    let install = |reference: &str| {
        jetpack()
            .args([
                "tool",
                "install",
                reference,
                "--no-color",
                "--offline",
                "--fixtures",
            ])
            .arg(&fixtures.path)
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("HOME", &home.path)
            .output()
            .unwrap()
    };
    let first = install("omp@releases#auto");
    assert!(
        first.status.success(),
        "first native install failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let link = home.join(".jet/bin/omp");
    let first_run = Command::new(&link)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&first_run.stdout).trim(),
        "omp version one"
    );
    let manifest = fs::read_to_string(home.join(".jet/tools/manifest.json")).unwrap();
    assert!(
        manifest.contains("\"provider\":\"jetpackage\""),
        "{manifest}"
    );
    assert!(manifest.contains("github:can1357/oh-my-pi") || manifest.contains("can1357/oh-my-pi"));
    assert!(manifest.contains("\"tier\":\"#auto\""), "{manifest}");

    let first_generation =
        fs::read_to_string(home.join(".jet/tools/generations/1/meta.json")).unwrap();
    let first_store_root = json_meta_field(&first_generation, "store_root");
    assert!(first_store_root.starts_with(root.path.to_string_lossy().as_ref()));
    assert!(
        !first_generation.contains("/nix/store/"),
        "{first_generation}"
    );
    let mut hangar_objects = Vec::new();
    for entry in fs::read_dir(Path::new(&first_store_root).join("hangar")).unwrap() {
        let path = entry.unwrap().path();
        if path.join("meta.json").is_file() {
            hangar_objects.push(path);
        }
    }
    assert!(
        !hangar_objects.is_empty(),
        "native install did not register Hangar metadata"
    );
    assert!(
        hangar_objects.iter().any(|path| {
            let metadata = fs::read_to_string(path.join("meta.json")).unwrap();
            metadata.contains("omp@releases") && !metadata.contains("/nix/store/")
        }),
        "native package was not recorded as a Hangar-owned output: {hangar_objects:?}"
    );

    write_native_omp_fixture(&fixtures.path, "2.0.0", "#!/bin/sh\necho omp version two\n");
    fs::write(
        fixtures.path.join("channels.txt"),
        "github:can1357/oh-my-pi latest v2.0.0\n",
    )
    .unwrap();
    let second = jetpack()
        .args(["profile", "build", "tools", "--no-color", "--fixtures"])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "native channel update failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_run = Command::new(&link)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&second_run.stdout).trim(),
        "omp version two"
    );

    let rollback = jetpack()
        .args(["profile", "rollback", "tools", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "rollback failed: {}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    let rolled_back = Command::new(&link)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&rolled_back.stdout).trim(),
        "omp version one"
    );
}

#[test]
fn tool_profile_reports_drift_without_prompt_and_yes_moves_pin() {
    let root = Scratch::new("tool-drift-root");
    let project = Scratch::new("tool-drift-project");
    let fixtures = Scratch::new("tool-drift-fixtures");
    let home = Scratch::new("tool-drift-home");
    write_native_omp_fixture(&fixtures.path, "1.0.0", "#!/bin/sh\necho omp version one\n");

    let installed = jetpack()
        .args([
            "tool",
            "install",
            "omp@releases#1.0.0",
            "--no-color",
            "--offline",
            "--fixtures",
        ])
        .arg(&fixtures.path)
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&installed.stderr)
    );

    let manifest_path = home.join(".jet/tools/manifest.json");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    fs::write(
        &manifest_path,
        manifest.replace(
            "\"reference\":\"omp@releases#1.0.0\"",
            "\"reference\":\"omp@releases#0.9.0\"",
        ),
    )
    .unwrap();

    let report = || {
        jetpack()
            .args(["tool", "list", "--no-color"])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("HOME", &home.path)
            .output()
            .unwrap()
    };
    let first = report();
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first.status.success(), "stderr: {first_stderr}");
    assert!(first_stderr.contains("omp drifted"), "stderr: {first_stderr}");
    assert!(first_stderr.contains("installed 1.0.0"), "stderr: {first_stderr}");
    assert!(first_stderr.contains("pinned 0.9.0"), "stderr: {first_stderr}");
    assert!(
        !first_stderr.contains("[Y/n]"),
        "non-interactive drift check prompted: {first_stderr}"
    );
    assert!(
        fs::read_to_string(&manifest_path)
            .unwrap()
            .contains("\"reference\":\"omp@releases#0.9.0\"")
    );

    let second = report();
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "stderr: {second_stderr}");
    assert!(second_stderr.contains("omp drifted"), "stderr: {second_stderr}");

    let reconciled = jetpack()
        .args(["tool", "list", "--no-color", "-y"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        reconciled.status.success(),
        "reconcile failed: {}",
        String::from_utf8_lossy(&reconciled.stderr)
    );
    assert!(
        fs::read_to_string(&manifest_path)
            .unwrap()
            .contains("\"reference\":\"omp@releases#1.0.0\"")
    );
    let tool = Command::new(home.join(".jet/bin/omp")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&tool.stdout).trim(), "omp version one");
}

#[test]
fn declarative_user_tools_realize_outside_repo_and_rollback_runs_previous_generation() {
    let (base, _project, root) = core_hello_project("user-tools-declarative");
    let repo = base.join("jet-pkgs");
    let outside = Scratch::new("user-tools-outside");
    let home = Scratch::new("user-tools-home");
    let manifest_dir = home.join(".jet/tools");
    fs::create_dir_all(&manifest_dir).unwrap();
    let write_manifest = |source: &str, repo: &Path| {
        let upstream = format!("path:{}", repo.to_string_lossy());
        fs::write(
            manifest_dir.join("manifest.json"),
            format!(
                "{{\n  \"profile\":\"tools\",\n  \"schema\":\"jet-user-tools-v1\",\n  \"sources\":[{{\"name\":{:?},\"policy\":\"pinned\",\"provider\":\"core\",\"raw\":{:?},\"upstream\":{:?}}}],\n  \"tools\":[{{\"bins\":[],\"members\":[],\"name\":\"hello\",\"reference\":\"hello@{source}\",\"resolved\":\"hello@{source}\",\"tier\":\"pinned\"}}]\n}}\n",
                source, upstream, upstream,
            ),
        )
        .unwrap();
    };
    write_manifest("mine", &repo);

    let switch = || {
        jetpack()
            .args(["profile", "switch", "tools", "--no-color", "--offline"])
            .current_dir(&outside.path)
            .env("JETPACK_ROOT", &root)
            .env("HOME", &home.path)
            .output()
            .unwrap()
    };
    let first = switch();
    assert!(
        first.status.success(),
        "first declarative switch failed\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let tool = home.join(".jet/bin/hello");
    let run = |tool: &Path| {
        Command::new(tool)
            .env("JETPACK_ROOT", &root)
            .env("HOME", &home.path)
            .output()
            .unwrap()
    };
    let first_run = run(&tool);
    assert_eq!(
        String::from_utf8_lossy(&first_run.stdout).trim(),
        "hello from jet-pkgs"
    );
    assert!(fs::read_to_string(home.join(".jet/tools/current"))
        .unwrap()
        .contains("generation\t1"));
    assert!(fs::read_to_string(home.join(".jet/tools/profile.json"))
        .unwrap()
        .contains("\"current\": 1"));

    let repo_two = base.join("jet-pkgs-two");
    fs::create_dir_all(repo_two.join("pkgs/hello/bin")).unwrap();
    fs::copy(repo.join("package.jet"), repo_two.join("package.jet")).unwrap();
    fs::copy(
        repo.join("pkgs/hello/hello.jet"),
        repo_two.join("pkgs/hello/hello.jet"),
    )
    .unwrap();
    fs::write(
        repo_two.join("pkgs/hello/bin/hello"),
        "#!/bin/sh\necho hello from user-tools generation two\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            repo_two.join("pkgs/hello/bin/hello"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    write_manifest("mine-two", &repo_two);
    let second = switch();
    assert!(
        second.status.success(),
        "second declarative switch failed\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let second_run = run(&tool);
    assert_eq!(
        String::from_utf8_lossy(&second_run.stdout).trim(),
        "hello from user-tools generation two"
    );
    assert!(fs::read_to_string(home.join(".jet/tools/current"))
        .unwrap()
        .contains("generation\t2"));
    assert!(fs::read_to_string(home.join(".jet/tools/profile.json"))
        .unwrap()
        .contains("\"current\": 2"));

    let rollback = jetpack()
        .args(["profile", "rollback", "tools", "--no-color"])
        .current_dir(&outside.path)
        .env("JETPACK_ROOT", &root)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "rollback failed\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&rollback.stdout),
        String::from_utf8_lossy(&rollback.stderr)
    );
    let rolled_back = run(&tool);
    assert_eq!(
        String::from_utf8_lossy(&rolled_back.stdout).trim(),
        "hello from jet-pkgs"
    );
    assert!(fs::read_to_string(home.join(".jet/tools/current"))
        .unwrap()
        .contains("generation\t1"));
    assert!(fs::read_to_string(home.join(".jet/tools/profile.json"))
        .unwrap()
        .contains("\"current\": 1"));

    let generations = jetpack()
        .args(["profile", "generations", "tools", "--no-color", "--json"])
        .current_dir(&outside.path)
        .env("JETPACK_ROOT", &root)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(generations.status.success());
    let report = String::from_utf8_lossy(&generations.stdout);
    assert!(report.contains("\"profile_bytes\":"), "{report}");
    assert!(report.contains("\"generations_bytes\":"), "{report}");
    assert!(report.contains("\"generation\":1"), "{report}");
    assert!(report.contains("\"generation\":2"), "{report}");
    assert!(report.contains("\"current\":true"), "{report}");
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
    let output = tool_install_command(&root.path, &proj.path, &fixtures.path, &home.path, "native")
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
    assert!(
        second.contains("left") && second.contains("right"),
        "{second}"
    );
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
    assert!(
        stdout.contains("left") && stdout.contains("right"),
        "{stdout}"
    );
}

#[test]
fn multi_store_root_commit_crash_recovers_before_pointer_switch() {
    let left_root = Scratch::new("tool-multiroot-left");
    let right_root = Scratch::new("tool-multiroot-right");
    let proj = Scratch::new("tool-multiroot-proj");
    let fixtures = Scratch::new("tool-multiroot-fx");
    let home = Scratch::new("tool-multiroot-home");
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
    .output()
    .unwrap();
    assert!(
        left.status.success(),
        "{}",
        String::from_utf8_lossy(&left.stderr)
    );

    let interrupted = tool_install_command(
        &right_root.path,
        &proj.path,
        &fixtures.path,
        &home.path,
        "right",
    )
    .env(
        "JETPACK_INTERNAL_TEST_PROFILE_FAILPOINT",
        "between-root-commits",
    )
    .output()
    .unwrap();
    assert_eq!(interrupted.status.code(), Some(2));
    assert!(home.join(".jet/tools/generations/2/complete").is_file());
    assert!(fs::read_to_string(home.join(".jet/tools/current"))
        .unwrap()
        .contains("generation\t1"));

    let recovered = jetpack()
        .args(["tool", "list", "--no-color"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &left_root.path)
        .env("HOME", &home.path)
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert!(fs::read_to_string(home.join(".jet/tools/current"))
        .unwrap()
        .contains("generation\t2"));
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
    let installed =
        tool_install_command(&root.path, &proj.path, &fixtures.path, &home.path, "greet")
            .output()
            .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );

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
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    assert!(home
        .join(".jet/tools/legacy-generations/generation-1")
        .is_dir());
    assert!(home
        .join(".jet/tools/generations/2/bin")
        .join(physical_bin("greet"))
        .is_file());
    let current = fs::read_to_string(home.join(".jet/tools/current")).unwrap();
    assert!(current.contains("generation\t2"), "{current}");
    let invoked = Command::new(home.join(".jet/bin").join(physical_bin("greet")))
        .output()
        .unwrap();
    assert!(invoked.status.success());
    assert_eq!(
        String::from_utf8_lossy(&invoked.stdout).trim(),
        "migrated greet"
    );
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

        let failed =
            tool_install_command(&root.path, &proj.path, &fixtures.path, &home.path, "greet")
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
                fs::read_to_string(&pointer)
                    .unwrap()
                    .contains("generation\t1"),
                "phase {phase}"
            );
            assert!(mirror.is_file(), "phase {phase}");
        } else if phase == "after-current-pointer" {
            assert!(fs::read_to_string(&pointer)
                .unwrap()
                .contains("generation\t1"));
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
        assert!(
            pointer.contains("generation\t1"),
            "phase {phase}: {pointer}"
        );
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
fn tool_install_job_collision_is_e1297_snapshot() {
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
    assert_jetos_stderr_snapshot("tool_job_collide", diagnostic);
    assert!(!home.join(".jet/bin").join(physical_bin("serve")).exists());
}

#[test]
fn test_runs_unimported_sibling_jet_tests_and_propagates_failure() {
    let project = Scratch::new("test-project");
    let root = Scratch::new("test-root");
    fs::write(
        project.join("package.jet"),
        "name: \"test_project\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("main.jet"),
        "#Test(\"entry test\") {\n    assert(true)\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("sibling_test.jet"),
        "#Test(\"unimported sibling test\") {\n    assert(false)\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "use jetpack as pkg\npub fn shell() [JSON] -> [pkg.source(\"local\", \"./\", \"core\"), pkg.packages([\"test_project@local\"]), pkg.prompt(\"test\")]\n",
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
        "jet test must report the failing test\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("unimported sibling test") || stderr.contains("unimported sibling test"),
        "test name missing from output:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("FAIL") || stderr.contains("FAIL"),
        "failure missing from output:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn test_check_only_package_without_run_jet_collects_package_tests() {
    let project = Scratch::new("test-check-only-project");
    let root = Scratch::new("test-check-only-root");
    fs::write(
        project.join("package.jet"),
        "name: \"test_check_only\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("check.jet"),
        "#Test(\"check-only package\") {\n    assert(true)\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("env.jet"),
        "use jetpack as pkg\npub fn shell() [JSON] -> [pkg.source(\"local\", \"./\", \"core\"), pkg.packages([\"test_check_only@local\"]), pkg.prompt(\"test\")]\n",
    )
    .unwrap();
    let output = jetpack()
        .args(["test", "--no-color"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "check-only package test failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("check-only package") || stderr.contains("check-only package"),
        "check-only package test missing from output:\nstdout: {stdout}\nstderr: {stderr}"
    );
}
