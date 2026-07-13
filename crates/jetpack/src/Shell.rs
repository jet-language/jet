//! Env composition + temporary subshell (D-JPK14).
//!
//! Jetpack composes the environment itself (PATH + markers) and spawns the
//! user's own shell as a child with a generated rc/init file that sets a pretty
//! `jetpack` prompt. `exit`/Ctrl-D ends the child; the parent shell's env is
//! never mutated. bash, fish, and zsh are supported.

use super::Output::Theme;
use jet_env_model::ModuleEval::{PromptPathMode, PromptStripMode};
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
    pub prompt_path: PromptPathMode,
    pub prompt_strip: PromptStripMode,
    /// Cache verification leases stay live through child process handoff.
    pub cache_leases: Vec<super::Store::CacheLease>,
}

impl Env {
    /// Prepend our bin dirs to `base_path`, deduping while preserving order.
    pub fn composed_path(&self, base_path: &str) -> String {
        let mut seen = std::collections::BTreeSet::new();
        let mut parts: Vec<&str> = Vec::new();
        for dir in self
            .cache_leases
            .iter()
            .filter_map(|lease| lease.wrapper_dir())
            .filter_map(|path| path.to_str())
        {
            if seen.insert(dir) {
                parts.push(dir);
            }
        }
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

    fn validate_cache(&self, theme: &Theme) -> bool {
        for lease in &self.cache_leases {
            if let Some(failure) = lease.integrity_failure() {
                super::Store::report_integrity(theme, &failure);
                return false;
            }
        }
        true
    }
}

/// Run `cmd_args` inside the composed env and return its exit code. The parent
/// process env is untouched (we mutate only the child's `Command`).
pub fn run_command(env: &Env, cmd_args: &[String]) -> i32 {
    if !env.validate_cache(&Theme::resolve(true)) {
        return 126;
    }
    let Some((program, rest)) = cmd_args.split_first() else {
        return 0;
    };
    let stable_program = env
        .cache_leases
        .iter()
        .find_map(|lease| lease.executable(program));
    let mut cmd = stable_program
        .as_ref()
        .map_or_else(|| Command::new(program), Command::new);
    cmd.args(rest);
    env.apply(&mut cmd);
    let code = match cmd.status() {
        Ok(status) => status
            .code()
            .unwrap_or(if status.success() { 0 } else { 1 }),
        Err(e) => {
            eprintln!("jetpack: could not run `{program}`: {e}");
            127
        }
    };
    if !env.validate_cache(&Theme::resolve(true)) {
        return 126;
    }
    code
}

/// Enter an interactive temporary shell. Returns the child's exit code.
pub fn enter(theme: &Theme, env: &Env, kind: ShellKind) -> i32 {
    if !env.validate_cache(theme) {
        return 126;
    }
    // The threshold rule — the signature moment of `jet env`. One quiet line
    // in, one mirrored line out, so the temporary shell reads as a room you
    // walked into and back out of, not a mode you might be stuck in.
    let count = match env.refs.len() {
        0 => "temporary shell".to_string(),
        1 => "1 package".to_string(),
        n => format!("{n} packages"),
    };
    theme.rule(&[
        env.label.as_str(),
        count.as_str(),
        "exit to leave",
        "nothing is installed",
    ]);

    let mut cmd = Command::new(kind.binary());
    env.apply(&mut cmd);

    // Per-shell prompt + init wiring. Temp files are cleaned on the way out.
    let _scratch = match kind {
        ShellKind::Bash => {
            let rc = write_temp("jetpack-bashrc", &bash_rc(&env.label, env.prompt_path, env.prompt_strip));
            cmd.arg("--rcfile").arg(&rc).arg("-i");
            Some(Scratch::File(rc))
        }
        ShellKind::Zsh => {
            // zsh reads `.zshrc` from ZDOTDIR; point it at a scratch dir.
            let dir = write_temp_dir("jetpack-zdotdir");
            std::fs::write(dir.join(".zshrc"), zsh_rc(&env.label, env.prompt_path, env.prompt_strip)).ok();
            cmd.env("ZDOTDIR", &dir).arg("-i");
            Some(Scratch::Dir(dir))
        }
        ShellKind::Fish => {
            cmd.arg("-C").arg(fish_init(&env.label, env.prompt_path, env.prompt_strip)).arg("-i");
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
    if !env.validate_cache(theme) {
        return 126;
    }
    let left = format!("left {}", env.label);
    theme.rule(&[left.as_str(), "your machine is unchanged"]);
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
    let path = PromptPathMode::Short;
    let strip = PromptStripMode::Off;
    match kind {
        ShellKind::Bash => {
            let rc = write_temp("jetpack-bashrc", &bash_rc(label, path, strip));
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
            std::fs::write(dir.join(".zshrc"), zsh_rc(label, path, strip)).ok();
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
                fish_init(label, path, strip),
                "-i".to_string(),
            ],
            env_vars: Vec::new(),
            cleanup_file: None,
            cleanup_dir: None,
        },
    }
}

// ── prompt / rc generation ───────────────────────────────────────────────

fn bash_rc(label: &str, path: PromptPathMode, strip: PromptStripMode) -> String {
    // Source the user's real bashrc first, then override the prompt so the
    // jetpack label is unmistakable: bold cyan label, blue path, green ❯.
    //
    // The zero-width color runs are wrapped in raw \x01/\x02 bytes (readline's
    // invisible-marker pair) rather than `\[`/`\]`: bash 5.3 stopped rewriting
    // `\[`/`\]` on this path, so they printed literally inside `nix develop`-
    // wrapped shells (nixpkgs bash 5.3.9). The raw markers work on every bash
    // and are invisible control bytes when readline isn't displaying at all.
    const S: char = '\u{1}';
    const E: char = '\u{2}';
    let path_escape = match path {
        PromptPathMode::Short => "\\W",
        PromptPathMode::Full => "\\w",
    };
    let status_prefix = if strip == PromptStripMode::On {
        "$(__jetpack_status_words)\\n"
    } else {
        ""
    };
    format!(
        "[ -f /etc/bash.bashrc ] && . /etc/bash.bashrc\n\
         [ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"\n\
         __jetpack_build_status='never run'\n\
         __jetpack_test_status='never run'\n\
         __jetpack_active=0\n\
         __jetpack_spinner_pid=''\n\
         __jetpack_git_status() {{\n\
           if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then\n\
             local branch dirty\n\
             branch=$(git branch --show-current 2>/dev/null); [ -n \"$branch\" ] || branch='detached'\n\
             git diff --quiet --ignore-submodules -- 2>/dev/null && git diff --cached --quiet --ignore-submodules -- 2>/dev/null && dirty='clean' || dirty='changed'\n\
             printf '%s %s' \"$branch\" \"$dirty\"\n\
           else printf 'not a git worktree'; fi\n\
         }}\n\
         __jetpack_status_words() {{ printf 'build %s · test %s · git %s' \"$__jetpack_build_status\" \"$__jetpack_test_status\" \"$(__jetpack_git_status)\"; }}\n\
         __jetpack_status_glance() {{ printf '\\n%s\\n' \"$(__jetpack_status_words)\"; }}\n\
         bind -x '\"\\C-g\":__jetpack_status_glance' 2>/dev/null || true\n\
         __jetpack_help_prefill() {{ local picked; picked=$(JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty) || return; [ -n \"$picked\" ] || return; READLINE_LINE=$picked; READLINE_POINT=$(printf %s \"$READLINE_LINE\" | wc -c); }}\n\
         bind -x '\"\\e?\":__jetpack_help_prefill' 2>/dev/null || true\n\
         __jetpack_spinner() {{ local frames='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏' i=0; while :; do printf '\\r%s running %s · %ss' \"${{frames:i++%10:1}}\" \"$1\" \"$(( $(date +%s) - $2 ))\" >&2; sleep .1; done; }}\n\
         __jetpack_preexec() {{\n\
           [ \"$__jetpack_active\" = 0 ] || return\n\
           case \"$1\" in jet\\ build*) __jetpack_kind=build;; jet\\ test*) __jetpack_kind=test;; *) return;; esac\n\
           __jetpack_active=1; __jetpack_command=$1; __jetpack_started=$(date +%s)\n\
           if [ -t 2 ]; then __jetpack_spinner \"$__jetpack_kind\" \"$__jetpack_started\" & __jetpack_spinner_pid=$!; fi\n\
         }}\n\
         __jetpack_precmd() {{\n\
           local code=$?\n\
           if [ \"$__jetpack_active\" = 1 ]; then\n\
             [ -z \"$__jetpack_spinner_pid\" ] || {{ kill \"$__jetpack_spinner_pid\" 2>/dev/null; wait \"$__jetpack_spinner_pid\" 2>/dev/null; if [ -n \"${{NO_COLOR:-}}\" ]; then printf '\\r                                        \\r' >&2; else printf '\\r\\033[2K' >&2; fi; }}\n\
             local elapsed=$(( $(date +%s) - __jetpack_started )) result\n\
             if [ \"$code\" = 0 ]; then result=ok; else result=\"failed ($code)\"; fi\n\
             if [ \"$__jetpack_kind\" = build ]; then __jetpack_build_status=\"$result · ${{elapsed}}s\"; else __jetpack_test_status=\"$result · ${{elapsed}}s\"; fi\n\
             if [ -n \"${{NO_COLOR:-}}\" ]; then printf '%s %s · %ss\\n' \"$__jetpack_kind\" \"$result\" \"$elapsed\"; else [ \"$code\" = 0 ] && printf '✓ %s ok · %ss\\n' \"$__jetpack_kind\" \"$elapsed\" || printf '✗ %s failed (%s) · %ss\\n' \"$__jetpack_kind\" \"$code\" \"$elapsed\"; fi\n\
             [ \"$code\" = 0 ] || {{ [ -n \"${{NO_COLOR:-}}\" ] && printf '%s\\n' \"-> $__jetpack_kind failed. Rerun: $__jetpack_command\" || printf '%s\\n' \"→ $__jetpack_kind failed. Rerun: $__jetpack_command\"; }}\n\
             __jetpack_active=0; __jetpack_spinner_pid=''\n\
           fi\n\
           return \"$code\"\n\
         }}\n\
         trap '__jetpack_preexec \"$BASH_COMMAND\"' DEBUG\n\
         PROMPT_COMMAND=\"__jetpack_precmd${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\"\n\
         if [ -n \"${{NO_COLOR:-}}\" ]; then\n\
           PS1='{status_prefix}{label} {path_escape} > '\n\
         else\n\
           PS1='{status_prefix}{S}\u{1b}[1;36m{E}{label}{S}\u{1b}[0m{E} {S}\u{1b}[34m{E}{path_escape}{S}\u{1b}[0m{E} {S}\u{1b}[32m{E}❯{S}\u{1b}[0m{E} '\n\
         fi\n"
    )
}

fn zsh_rc(label: &str, path: PromptPathMode, strip: PromptStripMode) -> String {
    let path_escape = match path {
        PromptPathMode::Short => "%1~",
        PromptPathMode::Full => "%~",
    };
    let status_prefix = if strip == PromptStripMode::On {
        "$(__jetpack_status_words)\n"
    } else {
        ""
    };
    format!(
        "[ -f \"$HOME/.zshrc\" ] && source \"$HOME/.zshrc\"\n\
         setopt prompt_subst\n\
         zmodload zsh/datetime\n\
         typeset -g __jetpack_build_status='never run' __jetpack_test_status='never run' __jetpack_kind='' __jetpack_command='' __jetpack_started=0 __jetpack_spinner_pid=''\n\
         __jetpack_git_status() {{ if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then local branch=$(git branch --show-current 2>/dev/null); [[ -n $branch ]] || branch=detached; if git diff --quiet --ignore-submodules -- 2>/dev/null && git diff --cached --quiet --ignore-submodules -- 2>/dev/null; then print -n -- \"$branch clean\"; else print -n -- \"$branch changed\"; fi; else print -n -- 'not a git worktree'; fi; }}\n\
         __jetpack_status_words() {{ printf 'build %s · test %s · git %s' \"$__jetpack_build_status\" \"$__jetpack_test_status\" \"$(__jetpack_git_status)\"; }}\n\
         __jetpack_status_glance() {{ printf '\\n%s\\n' \"$(__jetpack_status_words)\"; zle reset-prompt; }}\n\
         zle -N __jetpack_status_glance 2>/dev/null || true\n\
         bindkey '^G' __jetpack_status_glance 2>/dev/null || true\n\
         __jetpack_help_prefill() {{ local picked=$(JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty); [[ -n $picked ]] || return; BUFFER=$picked; CURSOR=$#BUFFER; zle redisplay; }}\n\
         zle -N __jetpack_help_prefill 2>/dev/null || true\n\
         bindkey '^[?' __jetpack_help_prefill 2>/dev/null || true\n\
         __jetpack_spinner() {{ local -a frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏); local i=1; while true; do printf '\\r%s running %s · %ss' $frames[$i] $1 $(( $EPOCHSECONDS - $2 )) >&2; (( i = i % 10 + 1 )); sleep .1; done; }}\n\
         __jetpack_preexec() {{ case $1 in 'jet build'*) __jetpack_kind=build;; 'jet test'*) __jetpack_kind=test;; *) __jetpack_kind=''; return;; esac; __jetpack_command=$1; __jetpack_started=$EPOCHSECONDS; if [[ -t 2 ]]; then __jetpack_spinner $__jetpack_kind $__jetpack_started &!; __jetpack_spinner_pid=$!; fi; }}\n\
         __jetpack_precmd() {{ local code=$?; [[ -n $__jetpack_kind ]] || return; if [[ -n $__jetpack_spinner_pid ]]; then kill $__jetpack_spinner_pid 2>/dev/null; wait $__jetpack_spinner_pid 2>/dev/null; if [[ -n $NO_COLOR ]]; then printf '\\r                                        \\r' >&2; else printf '\\r\\033[2K' >&2; fi; fi; local elapsed=$(( EPOCHSECONDS - __jetpack_started )) result; [[ $code = 0 ]] && result=ok || result=\"failed ($code)\"; [[ $__jetpack_kind = build ]] && __jetpack_build_status=\"$result · ${{elapsed}}s\" || __jetpack_test_status=\"$result · ${{elapsed}}s\"; if [[ -n $NO_COLOR ]]; then printf '%s %s · %ss\\n' $__jetpack_kind \"$result\" $elapsed; else [[ $code = 0 ]] && printf '✓ %s ok · %ss\\n' $__jetpack_kind $elapsed || printf '✗ %s failed (%s) · %ss\\n' $__jetpack_kind $code $elapsed; fi; if [[ $code != 0 ]]; then [[ -n $NO_COLOR ]] && printf '%s\\n' \"-> $__jetpack_kind failed. Rerun: $__jetpack_command\" || printf '%s\\n' \"→ $__jetpack_kind failed. Rerun: $__jetpack_command\"; fi; __jetpack_kind=''; __jetpack_spinner_pid=''; }}\n\
         autoload -Uz add-zsh-hook; add-zsh-hook preexec __jetpack_preexec; add-zsh-hook precmd __jetpack_precmd\n\
         if [ -n \"${{NO_COLOR:-}}\" ]; then\n\
           PROMPT='{status_prefix}{label} {path_escape} > '\n\
         else\n\
           PROMPT='{status_prefix}%B%F{{cyan}}{label}%f%b %F{{blue}}{path_escape}%f %F{{green}}❯%f '\n\
         fi\n"
    )
}

fn fish_init(label: &str, path: PromptPathMode, strip: PromptStripMode) -> String {
    let path_expr = match path {
        PromptPathMode::Short => "prompt_pwd",
        PromptPathMode::Full => "pwd",
    };
    let strip_line = if strip == PromptStripMode::On {
        "__jetpack_status_words; echo; "
    } else {
        ""
    };
    format!(
        "set -g __jetpack_build_status 'never run'; set -g __jetpack_test_status 'never run'; set -g __jetpack_kind ''; set -g __jetpack_command ''; set -g __jetpack_started 0; set -g __jetpack_spinner_pid ''; \
         function __jetpack_git_status; if git rev-parse --is-inside-work-tree >/dev/null 2>&1; set -l branch (git branch --show-current 2>/dev/null); test -n \"$branch\"; or set branch detached; if git diff --quiet --ignore-submodules -- 2>/dev/null; and git diff --cached --quiet --ignore-submodules -- 2>/dev/null; echo -n \"$branch clean\"; else; echo -n \"$branch changed\"; end; else; echo -n 'not a git worktree'; end; end; \
         function __jetpack_status_words; printf 'build %s · test %s · git %s' \"$__jetpack_build_status\" \"$__jetpack_test_status\" (__jetpack_git_status); end; \
         function __jetpack_status_glance; echo; __jetpack_status_words; echo; commandline -f repaint; end; \
         bind \\cg __jetpack_status_glance; \
         function __jetpack_help_prefill; set -l picked (env JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty); test -n \"$picked\"; or return; commandline -r -- \"$picked\"; commandline -C (string length -- \"$picked\"); end; \
         bind \\e\\? __jetpack_help_prefill; \
         function __jetpack_spinner; set -l frames ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏; set -l i 1; while true; printf '\\r%s running %s · %ss' $frames[$i] $argv[1] (math (date +%s) - $argv[2]) >&2; set i (math $i % 10 + 1); sleep .1; end; end; \
         function __jetpack_preexec --on-event fish_preexec; switch $argv[1]; case \"jet build*\"; set -g __jetpack_kind build; case \"jet test*\"; set -g __jetpack_kind test; case '*'; set -g __jetpack_kind ''; return; end; set -g __jetpack_command $argv[1]; set -g __jetpack_started (date +%s); if isatty stderr; command sh -c 'while :; do printf \"\\r⠹ running %s · %ss\" \"$0\" \"$(( $(date +%s) - $1 ))\" >&2; sleep .1; done' $__jetpack_kind $__jetpack_started &\nset -g __jetpack_spinner_pid $last_pid; end; end; \
         function __jetpack_postexec --on-event fish_postexec; set -l code $status; test -n \"$__jetpack_kind\"; or return; if test -n \"$__jetpack_spinner_pid\"; kill $__jetpack_spinner_pid 2>/dev/null; wait $__jetpack_spinner_pid 2>/dev/null; if set -q NO_COLOR; printf '\\r                                        \\r' >&2; else; printf '\\r\\033[2K' >&2; end; end; set -l elapsed (math (date +%s) - $__jetpack_started); set -l result ok; test $code -eq 0; or set result \"failed ($code)\"; if test $__jetpack_kind = build; set -g __jetpack_build_status \"$result · \"$elapsed\"s\"; else; set -g __jetpack_test_status \"$result · \"$elapsed\"s\"; end; if set -q NO_COLOR; printf '%s %s · %ss\\n' $__jetpack_kind \"$result\" $elapsed; else if test $code -eq 0; printf '✓ %s ok · %ss\\n' $__jetpack_kind $elapsed; else; printf '✗ %s failed (%s) · %ss\\n' $__jetpack_kind $code $elapsed; end; if test $code -ne 0; if set -q NO_COLOR; echo \"-> $__jetpack_kind failed. Rerun: $__jetpack_command\"; else; echo \"→ $__jetpack_kind failed. Rerun: $__jetpack_command\"; end; end; set -g __jetpack_kind ''; set -g __jetpack_spinner_pid ''; end; \
         function fish_prompt; {strip_line}\
         if set -q NO_COLOR; echo -n '{label} '; echo -n ({path_expr}); echo -n ' > '; \
         else; set_color -o cyan; echo -n '{label} '; \
         set_color blue; echo -n ({path_expr}); \
         set_color green; echo -n ' ❯ '; set_color normal; end; end"
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
    use std::io::{Read, Write};
    use std::process::Stdio;

    fn env_with(dirs: &[&str]) -> Env {
        Env {
            bin_dirs: dirs.iter().map(|s| s.to_string()).collect(),
            refs: vec![],
            label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
            prompt_path: PromptPathMode::Short,
            prompt_strip: PromptStripMode::Off,
            cache_leases: Vec::new(),
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
        let rc = bash_rc("jetpack", PromptPathMode::Short, PromptStripMode::Off);
        assert!(rc.contains("PS1="));
        assert!(rc.contains("jetpack"));
    }

    #[test]
    fn prompt_rc_supports_glance_and_optional_strip() {
        let bash = bash_rc("web-api", PromptPathMode::Short, PromptStripMode::On);
        assert!(bash.contains("__jetpack_status_words"));
        assert!(bash.contains("\\C-g"));
        assert!(bash.contains("$(__jetpack_status_words)\\n"));
        assert!(bash.contains("\\W"));

        let zsh = zsh_rc("web-api", PromptPathMode::Full, PromptStripMode::On);
        assert!(zsh.contains("bindkey '^G' __jetpack_status_glance"));
        assert!(zsh.contains("%~"));

        let fish = fish_init("web-api", PromptPathMode::Short, PromptStripMode::On);
        assert!(fish.contains("bind \\cg __jetpack_status_glance"));
        assert!(fish.contains("__jetpack_status_words; echo;"));
        assert!(fish.contains("prompt_pwd"));
    }

    #[test]
    fn prompt_rc_prefills_help_selection_without_accepting_line() {
        let bash = bash_rc("web-api", PromptPathMode::Short, PromptStripMode::Off);
        assert!(bash.contains("JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty"));
        assert!(bash.contains("READLINE_LINE=$picked"));
        assert!(bash.contains("__jetpack_help_prefill"));

        let zsh = zsh_rc("web-api", PromptPathMode::Short, PromptStripMode::Off);
        assert!(zsh.contains("BUFFER=$picked"));
        assert!(zsh.contains("bindkey '^[?' __jetpack_help_prefill"));

        let fish = fish_init("web-api", PromptPathMode::Short, PromptStripMode::Off);
        assert!(fish.contains("commandline -r -- \"$picked\""));
        assert!(fish.contains("bind \\e\\? __jetpack_help_prefill"));

        for rc in [&bash, &zsh, &fish] {
            assert!(!rc.contains("eval $picked"), "help selection must never execute");
        }
    }

    #[test]
    fn help_prefill_widgets_preserve_edit_buffer_in_real_shells() {
        let dir = write_temp_dir("jetpack-help-prefill");
        let marker = dir.join("executed");
        let fake_jet = dir.join("jet");
        std::fs::write(
            &fake_jet,
            format!(
                "#!/bin/sh\n[ \"$JET_HELP_SHELL_PREFILL\" = 1 ] || exit 9\nprintf 'touch {}'\n",
                marker.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fake_jet).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake_jet, permissions).unwrap();
        }
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let steps = [
            ("echo JET_HELP_WIDGET_START\n", 250),
            ("\x1b?", 250),
            ("\x03", 100),
            ("echo JET_HELP_WIDGET_RETURNED\nexit 0\n", 50),
        ];

        let bash_file = write_temp("jetpack-help-bashrc", &bash_rc("help", PromptPathMode::Short, PromptStripMode::Off));
        let bash = format!("PATH={} bash --noprofile --rcfile {} -i", shell_single_quote(&path), bash_file.display());
        let bash_out = pty_steps(&bash, &steps);
        let _ = std::fs::remove_file(bash_file);
        assert!(bash_out.contains("JET_HELP_WIDGET_RETURNED"), "{bash_out}");
        assert!(!marker.exists(), "bash help selection executed");

        let zdir = write_temp_dir("jetpack-help-zdotdir");
        std::fs::write(zdir.join(".zshrc"), zsh_rc("help", PromptPathMode::Short, PromptStripMode::Off)).unwrap();
        let zsh = format!("PATH={} ZDOTDIR={} zsh -d -i", shell_single_quote(&path), zdir.display());
        let zsh_out = pty_steps(&zsh, &steps);
        let _ = std::fs::remove_dir_all(zdir);
        assert!(zsh_out.contains("JET_HELP_WIDGET_RETURNED"), "{zsh_out}");
        assert!(!marker.exists(), "zsh help selection executed");

        let fish = format!(
            "PATH={} TERM_PROGRAM=ghostty fish -C {} -i",
            shell_single_quote(&path),
            shell_single_quote(&fish_init("help", PromptPathMode::Short, PromptStripMode::Off))
        );
        let fish_out = pty_steps(&fish, &steps);
        assert!(fish_out.contains("JET_HELP_WIDGET_RETURNED"), "{fish_out}");
        assert!(!marker.exists(), "fish help selection executed");
    }

    #[test]
    fn prompt_rc_has_plain_no_color_fallback() {
        let rc = bash_rc("web-api", PromptPathMode::Full, PromptStripMode::Off);
        assert!(rc.contains("NO_COLOR"));
        assert!(rc.contains("web-api \\w > "));
    }

    fn pty(shell_command: &str, input: &str, no_color: bool) -> String {
        let mut command = Command::new("script");
        command.args(["-qec", shell_command, "/dev/null"]);
        if no_color {
            command.env("NO_COLOR", "1").env("TERM", "dumb");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn PTY through script(1)");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("PTY shell did not reach exit sentinel within 10s");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        child.stdout.take().unwrap().read_to_end(&mut stdout).unwrap();
        child.stderr.take().unwrap().read_to_end(&mut stderr).unwrap();
        assert!(
            status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        String::from_utf8_lossy(&stdout).replace('\r', "")
    }

    fn pty_steps(shell_command: &str, steps: &[(&str, u64)]) -> String {
        let mut child = Command::new("script")
            .args(["-qec", shell_command, "/dev/null"])
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn PTY through script(1)");
        let mut stdin = child.stdin.take().unwrap();
        for (input, delay_ms) in steps {
            stdin.write_all(input.as_bytes()).unwrap();
            stdin.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(*delay_ms));
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).replace('\r', "")
    }

    fn shell_single_quote(text: &str) -> String {
        format!("'{}'", text.replace('\'', "'\\''"))
    }

    fn assert_no_esc_after_marker(output: &str) {
        let captured = output
            .rsplit_once("JETPACK_CAPTURE_START")
            .map(|(_, captured)| captured)
            .expect("PTY reached NO_COLOR capture marker");
        assert!(!captured.as_bytes().contains(&0x1b), "{captured:?}");
    }

    #[test]
    fn bash_prompt_pty_reports_real_command_and_git_facts_without_color() {
        let rc = write_temp(
            "jetpack-prompt-test-bashrc",
            &bash_rc("web-api", PromptPathMode::Short, PromptStripMode::On),
        );
        let shell = format!("bash --noprofile --rcfile {} -i", rc.display());
        let output = pty(
            &shell,
            "bind 'set enable-bracketed-paste off'\necho JETPACK_CAPTURE_START\njet() { sleep .3; [ \"$1\" = build ]; }\njet build\njet test\n\x07\n__jetpack_status_words\nexit\n",
            true,
        );
        let _ = std::fs::remove_file(rc);
        assert!(output.contains("build ok"), "{output}");
        assert!(output.contains("test failed (1)"), "{output}");
        assert!(output.contains("-> test failed. Rerun: jet test"), "{output}");
        let branch = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .expect("git rev-parse")
                .stdout,
        )
        .expect("utf8 branch")
        .trim()
        .to_string();
        assert!(
            output.contains(&format!("git {branch} changed"))
                || output.contains(&format!("git {branch} clean")),
            "expected git {branch} changed|clean in: {output}"
        );
        assert!(output.contains("running build"), "{output}");
        assert_no_esc_after_marker(&output);
        assert!(!output.contains("unknown"), "{output}");
    }

    #[test]
    fn fish_prompt_pty_reports_real_command_receipts_when_available() {
        Command::new("fish")
            .arg("--version")
            .output()
            .expect("fish must be present for prompt PTY coverage");
        let init = fish_init("web-api", PromptPathMode::Short, PromptStripMode::On);
        let shell = format!("fish -C {} -i", shell_single_quote(&init));
        let output = pty(
            &shell,
            "echo JETPACK_CAPTURE_START\nfunction jet; sleep .3; test $argv[1] = build; end\njet build\njet test\n\x07\n__jetpack_status_words\necho JETPACK_FISH_PTY_DONE\nexit 0\n",
            true,
        );
        assert!(output.contains("build ok"), "{output}");
        assert!(output.contains("test failed (1)"), "{output}");
        assert!(output.contains("-> test failed. Rerun: jet test"), "{output}");
        assert!(output.contains("JETPACK_FISH_PTY_DONE"), "{output}");
        assert!(output.contains("running build"), "{output}");
        assert_no_esc_after_marker(&output);
        assert!(!output.contains("unknown"), "{output}");
    }

    #[test]
    fn zsh_prompt_pty_exercises_native_hooks_glance_strip_and_no_color() {
        Command::new("zsh")
            .arg("--version")
            .output()
            .expect("zsh must be present for prompt PTY coverage");
        let dir = write_temp_dir("jetpack-prompt-test-zdotdir");
        std::fs::write(
            dir.join(".zshrc"),
            zsh_rc("web-api", PromptPathMode::Short, PromptStripMode::On),
        )
        .unwrap();
        let shell = format!("ZDOTDIR={} zsh -d -i", dir.display());
        let output = pty(
            &shell,
            "unset zle_bracketed_paste\necho JETPACK_CAPTURE_START\njet() { sleep .3; [[ $1 = build ]] }\njet build\njet test\n\x07\n__jetpack_status_words\necho JETPACK_ZSH_PTY_DONE\nexit 0\n",
            true,
        );
        let _ = std::fs::remove_dir_all(dir);
        assert!(output.contains("build ok"), "{output}");
        assert!(output.contains("test failed (1)"), "{output}");
        assert!(output.contains("-> test failed. Rerun: jet test"), "{output}");
        assert!(output.contains("running build"), "{output}");
        assert!(output.contains("JETPACK_ZSH_PTY_DONE"), "{output}");
        assert_no_esc_after_marker(&output);
        assert!(!output.contains("unknown"), "{output}");
    }

    #[test]
    fn prompt_strip_off_omits_status_line_in_all_shells() {
        let bash = bash_rc("web-api", PromptPathMode::Short, PromptStripMode::Off);
        let zsh = zsh_rc("web-api", PromptPathMode::Short, PromptStripMode::Off);
        let fish = fish_init("web-api", PromptPathMode::Short, PromptStripMode::Off);
        assert!(!bash.contains("$(__jetpack_status_words)\\nweb-api"));
        assert!(!zsh.contains("$(__jetpack_status_words)\nweb-api"));
        assert!(!fish.contains("function fish_prompt; __jetpack_status_words; echo;"));
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
