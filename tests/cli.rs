mod common;
include!("cli_parts/support.rs");
#[path = "cli_parts/core.rs"]
mod cli_core;
#[path = "cli_parts/inspect.rs"]
mod cli_inspect;

#[cfg(unix)]
#[test]
fn direct_launch_shebang_runs_executable_file() {
    use std::os::unix::fs::PermissionsExt;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/cli/direct_launch.jet");
    let original_mode = fs::metadata(&path).unwrap().permissions().mode();
    let mut executable = fs::metadata(&path).unwrap().permissions();
    executable.set_mode(original_mode | 0o111);
    fs::set_permissions(&path, executable).unwrap();

    let jet_path = jet();
    let path_dir = jet_path.parent().unwrap();
    let path_env = format!(
        "{}:{}",
        path_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let cwd = isolated_cwd("direct_launch_shebang");
    let direct = Command::new(&path)
        .current_dir(&cwd)
        .env("PATH", &path_env)
        .output()
        .unwrap();
    let via_jet = Command::new(&jet_path)
        .arg(&path)
        .current_dir(&cwd)
        .env("PATH", &path_env)
        .output()
        .unwrap();

    let mut restored = fs::metadata(&path).unwrap().permissions();
    restored.set_mode(original_mode);
    fs::set_permissions(&path, restored).unwrap();

    assert!(
        direct.status.success(),
        "direct launch failed: {}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert!(
        via_jet.status.success(),
        "jet file launch failed: {}",
        String::from_utf8_lossy(&via_jet.stderr)
    );
    assert_eq!(direct.status.code(), via_jet.status.code());
    assert_eq!(direct.stdout, via_jet.stdout);
    assert_eq!(direct.stderr, via_jet.stderr);
}
