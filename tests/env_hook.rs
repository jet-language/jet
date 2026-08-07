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
use std::process::{Command, Stdio};

mod common;
use common::{jetpack_bin, Scratch};

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
        "module env.dev {\n  prompt: \"smoke\"\n}\n",
    )
    .unwrap();
}

fn activation_hash(stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(stdout);
    text
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
fn enter_from_nested_directory_projects_allowlisted_dotenv_and_unsets() {
    let scratch = Scratch::new("dotenv-nested");
    let inner = scratch.path.join("src").join("inner");
    fs::create_dir_all(&inner).unwrap();
    fs::write(
        scratch.path.join(".env"),
        "VISIBLE=from-dotenv\nSECRET=hidden\nUNLISTED=must-not-load\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("env.jet"),
        "module env.dev {\n  dotenv: Dotenv.{ file: \".env\", allow: [\"VISIBLE\", \"SECRET\"], secrets: [\"SECRET\"] }\n  unset: [\"REMOVE_ME\"]\n}\n",
    )
    .unwrap();
    let out = export_cmd(&inner)
        .args([
            "enter",
            "--trust",
            "--no-color",
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s' \"$VISIBLE\" \"$SECRET\" \"${UNLISTED:-unset}\" \"${REMOVE_ME:-unset}\"",
        ])
        .env("VISIBLE", "from-parent")
        .env("REMOVE_ME", "remove-me")
        .env("UNLISTED", "from-parent")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "from-dotenv|hidden|from-parent|unset"
    );
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
        "module env.dev {\n  prompt: \"changed\"\n}\n",
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

#[test]
fn enter_runs_a_trusted_lifecycle_hook_before_the_child() {
    let scratch = Scratch::new("trusted-hook");
    fs::write(
        scratch.path.join("env.jet"),
        r#"module env.dev {
    on_enter: [.{
        name: "marker",
        command: "printf entered > .hook-marker",
        trusted: true,
    }]
}
"#,
    )
    .unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&scratch.path)
        .args([
            "enter",
            "--trust",
            "--no-color",
            "--",
            "/bin/sh",
            "-c",
            "test -f .hook-marker",
        ])
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "trusted lifecycle hook must run before enter: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(scratch.path.join(".hook-marker")).unwrap(),
        "entered"
    );
}

#[test]
fn enter_resolves_a_bare_lifecycle_name_to_a_typed_task() {
    let scratch = Scratch::new("typed-lifecycle-task");
    fs::write(
        scratch.path.join("env.jet"),
        r#"module env.dev {
    on_enter: [seed_config]
}
"#,
    )
    .unwrap();
    fs::write(
        scratch.path.join("main.jet"),
        r#"#Job
fn seed_config() {
    print("typed lifecycle task")
}
fn run() {}
"#,
    )
    .unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&scratch.path)
        .args([
            "enter",
            "--trust",
            "--no-color",
            "--",
            "true",
        ])
        // `enter` keeps the caller's toolchain visible. The nested `jet run`
        // task therefore needs the same Rust toolchain that runs this test;
        // the clean-shell proof below still uses the fixed minimal PATH.
        .env("PATH", std::env::var_os("PATH").expect("tests run with Rust on PATH"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "typed lifecycle task must run through the task path: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("typed lifecycle task"),
        "task output missing: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn enter_rejects_an_undeclared_lifecycle_task_before_launch() {
    let scratch = Scratch::new("undeclared-lifecycle-task");
    fs::write(
        scratch.path.join("env.jet"),
        "module env.dev {\n  on_enter: [missing_task]\n}\n",
    )
    .unwrap();
    fs::write(scratch.path.join("main.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&scratch.path)
        .args(["enter", "--trust", "--no-color", "--", "true"])
        .env("PATH", std::env::var_os("PATH").expect("tests run with Rust on PATH"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1294"), "missing lifecycle task diagnostic:\n{stderr}");
    assert!(stderr.contains("missing_task"), "missing task name:\n{stderr}");
    assert!(
        !stderr.contains("running task missing_task"),
        "undeclared lifecycle task must be rejected before child launch:\n{stderr}"
    );
}

#[test]
fn export_runs_typed_lifecycle_tasks_without_stdout_pollution() {
    let scratch = Scratch::new("typed-lifecycle-export");
    let home = Scratch::new("typed-lifecycle-export-home");
    fs::write(
        scratch.path.join("env.jet"),
        "module env.dev {\n  on_enter: [seed_config]\n}\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("main.jet"),
        "#Job\nfn seed_config() { print(\"typed lifecycle task\") }\nfn run() {}\n",
    )
    .unwrap();
    let selector = format!("env:{}", scratch.path.display());
    let trusted = Command::new(jetpack_bin())
        .current_dir(&scratch.path)
        .args(["trust", "grant", &selector, "--scope", "user"])
        .env("HOME", &home.path)
        .env("PATH", std::env::var_os("PATH").expect("tests run with Rust on PATH"))
        .output()
        .unwrap();
    assert!(
        trusted.status.success(),
        "trust setup failed: {}",
        String::from_utf8_lossy(&trusted.stderr)
    );
    let out = export_cmd(&scratch.path)
        .args(["enter", "--no-color", "export", "bash"])
        .env("HOME", &home.path)
        .env("PATH", std::env::var_os("PATH").expect("tests run with Rust on PATH"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("export JETPACK_ENV=1"),
        "missing activation; stderr: {}\nstdout:\n{stdout}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stdout.contains("typed lifecycle task"),
        "task output must not corrupt shell activation:\n{stdout}"
    );
}

#[test]
fn enter_requires_trust_for_a_trusted_lifecycle_hook() {
    let project = Scratch::new("trusted-hook-gate");
    let home = Scratch::new("trusted-hook-gate-home");
    fs::write(
        project.path.join("env.jet"),
        r#"module env.dev {
    on_enter: [.{
        name: "marker",
        command: "printf entered > .hook-marker",
        trusted: true,
    }]
}
"#,
    )
    .unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&project.path)
        .args(["enter", "--no-color", "--", "/bin/true"])
        .env("HOME", &home.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1255"), "missing trust diagnostic:\n{stderr}");
    assert!(!project.path.join(".hook-marker").exists());
}

#[test]
fn env_test_rejects_an_untrusted_lifecycle_hook() {
    let scratch = Scratch::new("untrusted-hook");
    fs::write(
        scratch.path.join("env.jet"),
        r#"module env.dev {
    on_enter: [.{
        name: "needs-review",
        command: "false",
    }]
}
"#,
    )
    .unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&scratch.path)
        .args(["enter", "--trust", "--no-color", "test"])
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1329"), "missing trust diagnostic:\n{stderr}");
    assert!(stderr.contains("needs-review"), "missing hook name:\n{stderr}");
}

#[test]
fn env_test_runs_hooks_checks_and_command_in_a_clean_child() {
    let scratch = Scratch::new("clean-hook");
    fs::write(
        scratch.path.join("env.jet"),
        r#"module env.dev {
    on_enter: [.{
        name: "clean-enter",
        command: "test -z \"$JET_PARENT_SENTINEL\" && printf entered > .clean-marker",
        trusted: true,
    }]
    checks: [.{
        name: "clean-check",
        command: "test -z \"$JET_PARENT_SENTINEL\" && test -f .clean-marker",
        trusted: true,
    }]
}
"#,
    )
    .unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&scratch.path)
        .args([
            "enter",
            "--trust",
            "--no-color",
            "test",
            "--",
            "/bin/sh",
            "-c",
            "test -z \"$JET_PARENT_SENTINEL\" && test -f .clean-marker",
        ])
        .env("JET_PARENT_SENTINEL", "must-not-leak")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clean env test must run every lifecycle phase without host leakage: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(scratch.path.join(".clean-marker")).unwrap(),
        "entered"
    );
}

#[cfg(unix)]
#[test]
fn enter_rejects_a_trusted_hook_cwd_that_escapes_through_a_symlink() {
    use std::os::unix::fs::symlink;

    let project = Scratch::new("trusted-hook-symlink");
    let outside = Scratch::new("trusted-hook-outside");
    symlink(&outside.path, project.path.join("link")).unwrap();
    fs::write(
        project.path.join("env.jet"),
        r#"module env.dev {
    on_enter: [.{
        name: "escape",
        cwd: "link",
        command: "printf escaped > escaped",
        trusted: true,
    }]
}
"#,
    )
    .unwrap();
    let out = Command::new(jetpack_bin())
        .current_dir(&project.path)
        .args(["enter", "--trust", "--no-color", "--", "/bin/true"])
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unsafe cwd"), "missing boundary diagnostic:\n{stderr}");
    assert!(!outside.path.join("escaped").exists());
}
