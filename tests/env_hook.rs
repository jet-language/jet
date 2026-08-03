//! D-ENVHOOK1=A: direnv-style opt-in env auto-activation.
//!
//! Covers the front-door subverbs of `jet env` (which route through `jetpack
//! enter`, D-JPK-DISPATCH1):
//!   * `jet env hook <shell>` prints an installable per-prompt hook for each
//!     supported shell, and rejects an unknown shell with a clean error;
//!   * `jet env export <shell>` is silent outside any `env.jet`;
//!   * it activates the nearest `env.jet` (exporting PATH + markers + the
//!     active-env-dir state), from the root and from a nested subdirectory;
//!   * `JET_ENV_DISABLE` unloads an active env; and an unchanged directory is a
//!     silent no-op (never re-realizes).
//!
//! These drive `jetpack` directly (same as `env_dev_trust.rs`) so the intercept
//! in `cmd_enter` is exercised without depending on engine-binary discovery.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

mod common;
use common::jetpack_bin;

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
            "jpk-envhook-it-{tag}-{nanos}-{:?}",
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

/// A `jetpack` command with the env-hook state variables cleared, stdin nulled
/// (non-interactive), and a fixed `PATH` baseline so activation output is
/// deterministic.
fn export_cmd(dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(jetpack_bin());
    cmd.current_dir(dir)
        .stdin(Stdio::null())
        .env_remove("JET_ENV_DISABLE")
        .env_remove("JETPACK_ENV")
        .env_remove("JETPACK_ENV_DIR")
        .env_remove("JETPACK_ENV_HASH")
        .env_remove("JETPACK_ENV_OLD_PATH")
        .env_remove("JETPACK_REF")
        .env("PATH", "/usr/bin:/bin");
    cmd
}

fn write_prompt_only_env(dir: &std::path::Path) {
    fs::write(
        dir.join("env.jet"),
        "module env.dev {\n  env.dev: Env {\n    prompt: \"smoke\"\n  }\n}\n",
    )
    .unwrap();
}

fn activation_hash(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .find_map(|line| {
            line.strip_prefix("export JETPACK_ENV_HASH='")
                .and_then(|value| value.strip_suffix('\''))
                .map(str::to_string)
        })
        .expect("activation must export its definition hash")
}

#[test]
fn hook_prints_installable_snippet_per_shell() {
    for (shell, needle) in [
        ("bash", "PROMPT_COMMAND"),
        ("zsh", "add-zsh-hook precmd __jet_env_hook"),
        ("fish", "--on-event fish_prompt"),
    ] {
        let out = Command::new(jetpack_bin())
            .args(["enter", "hook", shell])
            .output()
            .unwrap();
        assert!(out.status.success(), "`enter hook {shell}` must exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(needle),
            "`hook {shell}` output missing `{needle}`:\n{stdout}"
        );
        assert!(
            stdout.contains(&format!("command jet env export {shell}")),
            "`hook {shell}` must call back into `jet env export {shell}`:\n{stdout}"
        );
        assert!(
            stdout.contains("__jetpack_help_prefill"),
            "`hook {shell}` must install help-app prefill widgets:\n{stdout}"
        );
    }
}

#[test]
fn hook_unknown_shell_is_a_clean_error() {
    let out = Command::new(jetpack_bin())
        .args(["enter", "hook", "tcsh"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown shell"), "{stderr}");
    assert!(stderr.contains("bash, zsh, fish"), "{stderr}");
}

#[test]
fn export_outside_any_env_is_silent() {
    let scratch = Scratch::new("noenv");
    let out = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "no env.jet in the tree must emit nothing:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn export_activates_nearest_env_from_root_and_subdir() {
    let scratch = Scratch::new("activate");
    write_prompt_only_env(&scratch.path);
    let inner = scratch.path.join("src").join("inner");
    fs::create_dir_all(&inner).unwrap();
    let root = scratch.path.to_string_lossy().into_owned();

    for dir in [&scratch.path, &inner] {
        let out = export_cmd(dir)
            .args(["enter", "export", "bash"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The activation is anchored to the env.jet root even from a subdir.
        assert!(
            stdout.contains(&format!("export JETPACK_ENV_DIR='{root}'")),
            "activation from {dir:?} must anchor to the env root:\n{stdout}"
        );
        assert!(stdout.contains("export JETPACK_ENV=1"), "{stdout}");
        assert!(stdout.contains("export JETPACK_ENV_OLD_PATH="), "{stdout}");
        assert!(stdout.contains("export PATH="), "{stdout}");
    }
}

#[test]
fn export_disable_unloads_active_env() {
    let scratch = Scratch::new("disable");
    write_prompt_only_env(&scratch.path);
    let root = scratch.path.to_string_lossy().into_owned();
    let out = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .env("JET_ENV_DISABLE", "1")
        .env("JETPACK_ENV_DIR", &root)
        .env("JETPACK_ENV_OLD_PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("export PATH='/usr/bin:/bin'"), "{stdout}");
    assert!(stdout.contains("unset JETPACK_ENV_DIR"), "{stdout}");
    assert!(stdout.contains("unset JETPACK_ENV\n"), "{stdout}");
}

#[test]
fn export_unchanged_directory_is_silent() {
    let scratch = Scratch::new("stable");
    write_prompt_only_env(&scratch.path);
    let root = scratch.path.to_string_lossy().into_owned();
    let first = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .output()
        .unwrap();
    assert!(first.status.success());
    let hash = activation_hash(&first.stdout);
    let out = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .env("JETPACK_ENV_DIR", &root)
        .env("JETPACK_ENV_HASH", hash)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "already-active env must be a no-op:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn export_changed_definition_reactivates_at_next_prompt() {
    let scratch = Scratch::new("changed");
    write_prompt_only_env(&scratch.path);
    let root = scratch.path.to_string_lossy().into_owned();
    let first = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .output()
        .unwrap();
    assert!(first.status.success());
    let hash = activation_hash(&first.stdout);
    fs::write(
        scratch.path.join("env.jet"),
        "module env.dev {\n  env.dev: Env { prompt: \"changed\" }\n}\n",
    )
    .unwrap();
    let out = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .env("JETPACK_ENV_DIR", &root)
        .env("JETPACK_ENV_HASH", hash)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        !out.stdout.is_empty(),
        "a changed definition must emit replacement activation"
    );
    let replacement = String::from_utf8_lossy(&out.stdout);
    assert!(replacement.contains("changed"), "replacement facts missing:\n{replacement}");
    assert!(!replacement.contains("smoke"), "stale facts survived reload:\n{replacement}");
    assert!(replacement.contains("export JETPACK_ENV_HASH='"));
}

#[test]
fn export_failed_reload_keeps_the_previous_activation() {
    let scratch = Scratch::new("failed-reload");
    write_prompt_only_env(&scratch.path);
    let root = scratch.path.to_string_lossy().into_owned();
    let first = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .output()
        .unwrap();
    assert!(first.status.success());
    let hash = activation_hash(&first.stdout);
    fs::write(scratch.path.join("env.jet"), "module env.dev {\n").unwrap();
    let out = export_cmd(&scratch.path)
        .args(["enter", "export", "bash"])
        .env("JETPACK_ENV_DIR", &root)
        .env("JETPACK_ENV_HASH", hash)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "failed reload must not unload the previous activation"
    );
}
