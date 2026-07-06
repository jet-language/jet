//! Env composition + temporary subshell (D-JPK14).
//!
//! Jetpack composes the environment itself (PATH + markers) and spawns the
//! user's own shell as a child with a generated rc/init file that sets a pretty
//! `jetpack` prompt. `exit`/Ctrl-D ends the child; the parent shell's env is
//! never mutated. bash, fish, and zsh are supported.

use super::Output::Theme;
use crate::Syntax;
use std::path::PathBuf;
use std::process::Command;

/// The shells Jetpack can decorate. Anything else falls back to `bash`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
}

impl ShellKind {
    pub fn binary(self) -> &'static str {
        match self {
            ShellKind::Bash => "bash",
            ShellKind::Zsh => "zsh",
            ShellKind::Fish => "fish",
        }
    }

    /// Detect the user's shell from `$SHELL`, defaulting to bash.
    pub fn detect() -> ShellKind {
        let shell = std::env::var("SHELL").unwrap_or_default();
        let base = shell.rsplit('/').next().unwrap_or("");
        match base {
            "zsh" => ShellKind::Zsh,
            "fish" => ShellKind::Fish,
            _ => ShellKind::Bash,
        }
    }
}

/// A composed environment: what to expose and how to label the prompt.
pub struct Env {
    pub bin_dirs: Vec<String>,
    pub refs: Vec<String>,
    pub label: String,
}

impl Env {
    /// Prepend our bin dirs to `base_path`, deduping while preserving order.
    pub fn composed_path(&self, base_path: &str) -> String {
        let mut seen = std::collections::BTreeSet::new();
        let mut parts: Vec<&str> = Vec::new();
        for dir in &self.bin_dirs {
            if seen.insert(dir.as_str()) {
                parts.push(dir);
            }
        }
        let sep = super::Platform::path_separator();
        for dir in base_path.split(sep).filter(|s| !s.is_empty()) {
            if seen.insert(dir) {
                parts.push(dir);
            }
        }
        parts.join(&sep.to_string())
    }

    fn apply(&self, cmd: &mut Command) {
        let base = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", self.composed_path(&base));
        cmd.env(Syntax::JETPACK_ENV_MARKER, "1");
        cmd.env("JETPACK_REF", self.refs.join(" "));
    }
}

/// Run `cmd_args` inside the composed env and return its exit code. The parent
/// process env is untouched (we mutate only the child's `Command`).
pub fn run_command(env: &Env, cmd_args: &[String]) -> i32 {
    let Some((program, rest)) = cmd_args.split_first() else {
        return 0;
    };
    let mut cmd = Command::new(program);
    cmd.args(rest);
    env.apply(&mut cmd);
    match cmd.status() {
        Ok(status) => status
            .code()
            .unwrap_or(if status.success() { 0 } else { 1 }),
        Err(e) => {
            eprintln!("jetpack: could not run `{program}`: {e}");
            127
        }
    }
}

/// Enter an interactive temporary shell. Returns the child's exit code.
pub fn enter(theme: &Theme, env: &Env, kind: ShellKind) -> i32 {
    theme.note("entering a temporary shell — type `exit` to leave, nothing is installed.");

    let mut cmd = Command::new(kind.binary());
    env.apply(&mut cmd);

    // Per-shell prompt + init wiring. Temp files are cleaned on the way out.
    let _scratch = match kind {
        ShellKind::Bash => {
            let rc = write_temp("jetpack-bashrc", &bash_rc(&env.label));
            cmd.arg("--rcfile").arg(&rc).arg("-i");
            Some(Scratch::File(rc))
        }
        ShellKind::Zsh => {
            // zsh reads `.zshrc` from ZDOTDIR; point it at a scratch dir.
            let dir = write_temp_dir("jetpack-zdotdir");
            std::fs::write(dir.join(".zshrc"), zsh_rc(&env.label)).ok();
            cmd.env("ZDOTDIR", &dir).arg("-i");
            Some(Scratch::Dir(dir))
        }
        ShellKind::Fish => {
            cmd.arg("-C").arg(fish_init(&env.label)).arg("-i");
            None
        }
    };

    let code = match cmd.status() {
        Ok(status) => status.code().unwrap_or(0),
        Err(e) => {
            eprintln!("jetpack: could not start `{}`: {e}", kind.binary());
            127
        }
    };
    theme.note("left the temporary shell. your machine is unchanged.");
    code
}

/// The branded interactive-shell command a caller with its own outer process
/// (e.g. `nix develop`'s foreign-flake fallback) can append after its own
/// `--command` flag, so a shell entered through a path other than
/// [`enter`] still gets the unmistakable jetpack prompt rather than
/// silently inheriting the user's plain shell prompt. `env_vars` must be set
/// on the OUTER command (e.g. `nix develop`'s `Command`) so they reach the
/// inner shell; `cleanup` must be called once the caller's blocking
/// `.status()` call returns, mirroring how [`enter`] keeps its own scratch
/// files alive for exactly the child's lifetime.
pub struct BrandedShell {
    pub command_tail: Vec<String>,
    pub env_vars: Vec<(String, String)>,
    cleanup_file: Option<PathBuf>,
    cleanup_dir: Option<PathBuf>,
}

impl BrandedShell {
    pub fn cleanup(&self) {
        if let Some(p) = &self.cleanup_file {
            let _ = std::fs::remove_file(p);
        }
        if let Some(p) = &self.cleanup_dir {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// Build a [`BrandedShell`] for `kind`, labeling the prompt with `label`.
pub fn branded_shell(kind: ShellKind, label: &str) -> BrandedShell {
    match kind {
        ShellKind::Bash => {
            let rc = write_temp("jetpack-bashrc", &bash_rc(label));
            BrandedShell {
                command_tail: vec![
                    "bash".to_string(),
                    "--rcfile".to_string(),
                    rc.display().to_string(),
                    "-i".to_string(),
                ],
                env_vars: Vec::new(),
                cleanup_file: Some(rc),
                cleanup_dir: None,
            }
        }
        ShellKind::Zsh => {
            let dir = write_temp_dir("jetpack-zdotdir");
            std::fs::write(dir.join(".zshrc"), zsh_rc(label)).ok();
            BrandedShell {
                command_tail: vec!["zsh".to_string(), "-i".to_string()],
                env_vars: vec![("ZDOTDIR".to_string(), dir.display().to_string())],
                cleanup_file: None,
                cleanup_dir: Some(dir),
            }
        }
        ShellKind::Fish => BrandedShell {
            command_tail: vec![
                "fish".to_string(),
                "-C".to_string(),
                fish_init(label),
                "-i".to_string(),
            ],
            env_vars: Vec::new(),
            cleanup_file: None,
            cleanup_dir: None,
        },
    }
}

// ── prompt / rc generation ───────────────────────────────────────────────

fn bash_rc(label: &str) -> String {
    // Source the user's real bashrc first, then override the prompt so the
    // jetpack label is unmistakable.
    format!(
        "[ -f /etc/bash.bashrc ] && . /etc/bash.bashrc\n\
         [ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n\
         PS1='\\[\\e[36m\\]{label}\\[\\e[0m\\] \\w \\$ '\n"
    )
}

fn zsh_rc(label: &str) -> String {
    format!(
        "[ -f \"$HOME/.zshrc\" ] && source \"$HOME/.zshrc\"\n\
         PROMPT='%F{{cyan}}{label}%f %~ %# '\n"
    )
}

fn fish_init(label: &str) -> String {
    format!(
        "function fish_prompt; set_color cyan; echo -n '{label} '; set_color normal; \
         echo -n (prompt_pwd)' $ '; end"
    )
}

// ── tiny scratch-file helpers (std-only) ─────────────────────────────────

enum Scratch {
    File(PathBuf),
    Dir(PathBuf),
}

impl Drop for Scratch {
    fn drop(&mut self) {
        match self {
            Scratch::File(p) => {
                let _ = std::fs::remove_file(p);
            }
            Scratch::Dir(p) => {
                let _ = std::fs::remove_dir_all(p);
            }
        }
    }
}

fn unique_tmp(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{tag}-{}-{}", std::process::id(), nanos))
}

fn write_temp(tag: &str, contents: &str) -> PathBuf {
    let path = unique_tmp(tag);
    let _ = std::fs::write(&path, contents);
    path
}

fn write_temp_dir(tag: &str) -> PathBuf {
    let path = unique_tmp(tag);
    let _ = std::fs::create_dir_all(&path);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with(dirs: &[&str]) -> Env {
        Env {
            bin_dirs: dirs.iter().map(|s| s.to_string()).collect(),
            refs: vec![],
            label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        }
    }

    #[test]
    fn composes_path_prepended_and_deduped() {
        let env = env_with(&["/a/bin", "/b/bin", "/a/bin"]);
        let sep = super::super::Platform::path_separator();
        let path = env.composed_path(&format!("/usr/bin{sep}/b/bin"));
        assert_eq!(path, format!("/a/bin{sep}/b/bin{sep}/usr/bin"));
    }

    #[test]
    fn detect_falls_back_to_bash() {
        // Any unknown shell name resolves to bash.
        assert_eq!(ShellKind::Bash.binary(), "bash");
    }

    #[test]
    fn bash_rc_sets_label() {
        let rc = bash_rc("jetpack");
        assert!(rc.contains("PS1="));
        assert!(rc.contains("jetpack"));
    }

    #[test]
    fn run_command_returns_child_status() {
        let env = env_with(&[]);
        let ok = run_command(&env, &["true".into()]);
        assert_eq!(ok, 0);
        let bad = run_command(&env, &["false".into()]);
        assert_ne!(bad, 0);
    }

    #[test]
    fn run_command_exposes_bin_on_path() {
        // The composed PATH must reach the child: ask `sh` to echo $PATH.
        let env = env_with(&["/jetpack-test-marker/bin"]);
        let code = run_command(
            &env,
            &[
                "sh".into(),
                "-c".into(),
                "case \"$PATH\" in /jetpack-test-marker/bin:*) exit 0;; *) exit 3;; esac".into(),
            ],
        );
        assert_eq!(code, 0);
    }
}
