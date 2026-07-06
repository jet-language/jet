//! U28 (D-JPK-NODAEMON1=A): no daemon / no root runtime policy.

use jet::Jetpack::RuntimePolicy;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jetpack() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetpack"))
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
        let path = std::env::temp_dir().join(format!("jpk-nodaemon-{tag}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn runtime_policy_names_no_daemon_no_root_contract() {
    for policy in RuntimePolicy::all_verb_policies() {
        assert!(
            !policy.resident_daemon,
            "{} must not require a resident daemon",
            policy.verb
        );
        assert!(
            !policy.requires_root,
            "{} must not require root",
            policy.verb
        );
        assert!(
            !policy.transient_sudo,
            "{} must not use transient sudo by default",
            policy.verb
        );
    }

    let os_switch = RuntimePolicy::verb_policy(jet::Syntax::OS_SUBCOMMAND, &["switch"]);
    assert!(os_switch.transient_sudo);
    assert!(!os_switch.requires_root);
}

#[test]
fn sandbox_fallback_warns_and_require_mode_errors() {
    let cwd = Scratch::new("cwd");
    let home = Scratch::new("home");
    let root = Scratch::new("root");

    let warn = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&cwd.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FAKE_SANDBOX", "unavailable")
        .output()
        .unwrap();
    let warn_err = String::from_utf8_lossy(&warn.stderr);
    assert!(warn_err.contains("L0205"), "stderr: {warn_err}");
    assert!(
        warn_err.contains("jetpack config sandbox require"),
        "stderr: {warn_err}"
    );

    let warn_json = jetpack()
        .args(["build", "--json", "--no-color"])
        .current_dir(&cwd.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FAKE_SANDBOX", "unavailable")
        .output()
        .unwrap();
    let warn_json_err = String::from_utf8_lossy(&warn_json.stderr);
    assert!(
        warn_json_err.contains("\"code\": \"L0205\""),
        "stderr: {warn_json_err}"
    );
    assert!(
        warn_json_err.contains("\"severity\": \"warning\""),
        "stderr: {warn_json_err}"
    );

    let require = jetpack()
        .args(["config", "sandbox", "require", "--no-color"])
        .current_dir(&cwd.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(require.status.success());

    let fail = jetpack()
        .args(["build", "--no-color"])
        .current_dir(&cwd.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FAKE_SANDBOX", "unavailable")
        .output()
        .unwrap();
    assert_eq!(fail.status.code(), Some(2));
    let fail_err = String::from_utf8_lossy(&fail.stderr);
    assert!(fail_err.contains("E1275"), "stderr: {fail_err}");
    assert!(
        fail_err.contains("jetpack config sandbox allow"),
        "stderr: {fail_err}"
    );

    let fail_json = jetpack()
        .args(["build", "--json", "--no-color"])
        .current_dir(&cwd.path)
        .env("HOME", &home.path)
        .env("JETPACK_ROOT", &root.path)
        .env("JETPACK_FAKE_SANDBOX", "unavailable")
        .output()
        .unwrap();
    let fail_json_err = String::from_utf8_lossy(&fail_json.stderr);
    assert!(
        fail_json_err.contains("\"code\": \"E1275\""),
        "stderr: {fail_json_err}"
    );
    assert!(
        fail_json_err.contains("\"severity\": \"error\""),
        "stderr: {fail_json_err}"
    );
}

#[test]
fn jetpack_runtime_code_has_no_privileged_helper_tokens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/jet-driver/src/Jetpack");
    let forbidden = [
        "Command::new(\"sudo\")",
        "shell_command(\"sudo",
        " setuid",
        "systemctl",
        "launchctl",
        "/etc/jet",
        "jetpackd",
        "daemon socket",
    ];
    for path in rust_files(&root) {
        if path.file_name().and_then(|n| n.to_str()) == Some("JetOS.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("//!") {
                continue;
            }
            for needle in forbidden {
                assert!(
                    !line.contains(needle),
                    "{}:{} contains forbidden privileged token `{needle}`",
                    path.display(),
                    idx + 1
                );
            }
        }
    }
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for ent in rd.flatten() {
            let path = ent.path();
            if path.is_dir() {
                out.extend(rust_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out
}
