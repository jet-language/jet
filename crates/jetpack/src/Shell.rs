//! Env composition + temporary subshell (D-JPK14).
//!
//! Jetpack composes the environment itself (PATH + markers) and spawns the
//! user's own shell as a child with a generated rc/init file that sets a pretty
//! `jetpack` prompt. `exit`/Ctrl-D ends the child; the parent shell's env is
//! never mutated. bash, fish, and zsh are supported.

use super::Output::Theme;
use crate::Syntax;
use jet_env_model::ModuleEval::{PromptPathMode, PromptStripMode};
use jet_foundation::Terminal::Theme as SharedTheme;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Help-app prefill widgets for an already-running user shell (`jet env hook`)
/// and for branded jetpack subshells. Idempotent when sourced twice.
pub fn help_prefill_widgets(kind: ShellKind) -> String {
    match kind {
        ShellKind::Bash => "\
if ! type __jetpack_help_prefill >/dev/null 2>&1; then\n\
  __jetpack_help_prefill() { local picked; picked=$(JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty) || return; [ -n \"$picked\" ] || return; READLINE_LINE=$picked; READLINE_POINT=$(printf %s \"$READLINE_LINE\" | wc -c); }\n\
  bind -x '\"\\e?\":__jetpack_help_prefill' 2>/dev/null || true\n\
  jet() { if [ \"$#\" -eq 1 ] && [ \"$1\" = '?' ]; then local picked code line; picked=$(JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty); code=$?; [ -n \"$picked\" ] || return $code; if IFS= read -r -e -i \"$picked\" -p \"${PS1@P}\" line; then [ -n \"$line\" ] || return 0; history -s -- \"$line\"; eval -- \"$line\"; return $?; fi; return 0; fi; command jet \"$@\"; }\n\
fi\n"
        .into(),
        ShellKind::Zsh => "\
if ! typeset -f __jetpack_help_prefill >/dev/null 2>&1; then\n\
  __jetpack_help_prefill() { local picked=$(JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty); [[ -n $picked ]] || return; BUFFER=$picked; CURSOR=$#BUFFER; zle redisplay; }\n\
  zle -N __jetpack_help_prefill 2>/dev/null || true\n\
  bindkey '^[?' __jetpack_help_prefill 2>/dev/null || true\n\
  jet() { if [[ $# -eq 1 && $1 == '?' ]]; then local picked; picked=$(JET_HELP_SHELL_PREFILL=1 command jet '?' </dev/tty) || return; [[ -n $picked ]] || return 0; print -z -- \"$picked\"; return 0; fi; command jet \"$@\"; }\n\
  alias jet='noglob jet'\n\
fi\n"
        .into(),
        ShellKind::Fish => "\
if not functions -q __jetpack_help_prefill\n\
  function __jetpack_help_prefill; set -l picked (begin; set -lx JET_HELP_SHELL_PREFILL 1; command jet '?' </dev/tty; end); test -n \"$picked\"; or return; commandline -r -- \"$picked\"; commandline -C (string length -- \"$picked\"); end\n\
  bind \\e\\? __jetpack_help_prefill\n\
  function jet; if test (count $argv) -eq 1; and test \"$argv[1]\" = '?'; set -e __jetpack_help_pending; set -l picked (begin; set -lx JET_HELP_SHELL_PREFILL 1; command jet '?' </dev/tty; end); set -l code $status; if test -n \"$picked\"; set -g __jetpack_help_pending \"$picked\"; end; return $code; end; command jet $argv; end\n\
  function __jetpack_help_postexec --on-event fish_postexec; set -q __jetpack_help_pending; or return; commandline -r -- \"$__jetpack_help_pending\"; commandline -C (string length -- \"$__jetpack_help_pending\"); set -e __jetpack_help_pending; end\n\
end\n"
        .into(),
    }
}

/// A composed environment: what to expose and how to label the prompt.
pub struct Env {
    pub bin_dirs: Vec<String>,
    /// Provider-owned runtime search paths projected into child processes.
    pub vars: std::collections::BTreeMap<String, String>,
    /// Variables that must not leak from the parent shell into this env.
    pub unset_vars: Vec<String>,
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

    pub(crate) fn apply_to(&self, cmd: &mut Command) {
        let base = std::env::var("PATH").unwrap_or_default();
        self.apply_to_base(cmd, &base);
    }

    /// Apply only the declared environment and realized PATH. Callers use
    /// this after `env_clear` for clean-shell checks and task probes.
    pub(crate) fn apply_clean_to(&self, cmd: &mut Command) {
        let base = super::Platform::clean_path();
        self.apply_to_base(cmd, base);
    }

    fn apply_to_base(&self, cmd: &mut Command, base_path: &str) {
        cmd.env("PATH", self.composed_path(base_path));
        for (name, value) in &self.vars {
            cmd.env(name, value);
        }
        for name in &self.unset_vars {
            cmd.env_remove(name);
        }
        cmd.env(Syntax::JETPACK_ENV_MARKER, "1");
        cmd.env(Syntax::JETPACK_REF_VAR, self.refs.join(" "));
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
    run_command_in(env, cmd_args, None)
}

/// Run a command with the composed environment and an explicit working
/// directory. The directory belongs to the child, never to this process.
pub fn run_command_in(env: &Env, cmd_args: &[String], cwd: Option<&Path>) -> i32 {
    run_command_in_mode(env, cmd_args, cwd, false, false)
}

/// Run a composed command while keeping its stdout out of a generated shell
/// script. Stderr remains visible so a failed task still explains itself.
pub fn run_command_in_silent(env: &Env, cmd_args: &[String], cwd: Option<&Path>) -> i32 {
    run_command_in_mode(env, cmd_args, cwd, false, true)
}

fn run_command_in_mode(
    env: &Env,
    cmd_args: &[String],
    cwd: Option<&Path>,
    clean: bool,
    silent: bool,
) -> i32 {
    if !env.validate_cache(&Theme::resolve_choice(jet_foundation::Terminal::ColorChoice::Never)) {
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
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    if clean {
        cmd.env_clear();
        env.apply_clean_to(&mut cmd);
    } else {
        env.apply_to(&mut cmd);
    }
    if silent {
        cmd.stdout(Stdio::null());
    }
    let code = match cmd.status() {
        Ok(status) => status
            .code()
            .unwrap_or(if status.success() { 0 } else { 1 }),
        Err(e) => {
            let suffix = if clean { " in a clean env" } else { "" };
            eprintln!("jetpack: could not run `{program}`{suffix}: {e}");
            127
        }
    };
    if !env.validate_cache(&Theme::resolve_choice(jet_foundation::Terminal::ColorChoice::Never)) {
        return 126;
    }
    code
}

/// Run a command with no inherited host variables. Only the composed PATH,
/// declared variables, Jet markers, and declared unsets enter the child.
pub fn run_clean_command(env: &Env, cmd_args: &[String]) -> i32 {
    run_clean_command_in(env, cmd_args, None)
}

/// Run a command with no inherited host variables and an explicit working
/// directory. This is the clean-shell counterpart to `run_command_in`.
pub fn run_clean_command_in(env: &Env, cmd_args: &[String], cwd: Option<&Path>) -> i32 {
    run_command_in_mode(env, cmd_args, cwd, true, false)
}

/// Run a clean-shell command without letting task stdout corrupt a generated
/// activation script.
pub fn run_clean_command_in_silent(
    env: &Env,
    cmd_args: &[String],
    cwd: Option<&Path>,
) -> i32 {
    run_command_in_mode(env, cmd_args, cwd, true, true)
}

/// Enter an interactive temporary shell. Returns the child's exit code.
pub fn enter(theme: &Theme, env: &Env, kind: ShellKind) -> i32 {
    enter_with_mode(theme, env, kind, false)
}

/// Enter an interactive temporary shell with only the composed environment.
/// This is the explicit `--pure` foreign-environment path; ordinary Jetpack
/// shells keep the host variables that users expect for interactive work.
pub fn enter_clean(theme: &Theme, env: &Env, kind: ShellKind) -> i32 {
    enter_with_mode(theme, env, kind, true)
}

fn enter_with_mode(theme: &Theme, env: &Env, kind: ShellKind, clean: bool) -> i32 {
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
    if clean {
        cmd.env_clear();
        env.apply_clean_to(&mut cmd);
    } else {
        env.apply_to(&mut cmd);
    }
    if theme.color {
        cmd.env_remove("NO_COLOR");
    } else {
        cmd.env("NO_COLOR", "");
    }

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
/// can append after its own `--command` flag, so a shell entered through a
/// path other than [`enter`] still gets the unmistakable jetpack prompt rather
/// than silently inheriting the user's plain shell prompt. `env_vars` must be
/// set on the outer command so they reach the inner shell; `cleanup` must be
/// called once the caller's blocking `.status()` call returns, mirroring how
/// [`enter`] keeps its own scratch files alive for exactly the child's
/// lifetime.
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
    let accent = SharedTheme::ACCENT_SGR;
    let border = SharedTheme::BORDER_SGR;
    let success = SharedTheme::SUCCESS_SGR;
    let warn = SharedTheme::WARN_SGR;
    let error = SharedTheme::ERROR_SGR;
    let mut rc = format!(
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
         __jetpack_spinner() {{ local frames='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏' i=0; while :; do if [ \"${{NO_COLOR+x}}\" = x ]; then printf '\\r%s running %s · %ss' \"${{frames:i++%10:1}}\" \"$1\" \"$(( $(date +%s) - $2 ))\" >&2; else printf '\\r\\033[{accent}m%s\\033[0m running %s · %ss' \"${{frames:i++%10:1}}\" \"$1\" \"$(( $(date +%s) - $2 ))\" >&2; fi; sleep .1; done; }}\n\
         __jetpack_preexec() {{\n\
           [ \"$__jetpack_active\" = 0 ] || return\n\
           case \"$1\" in jet\\ build*) __jetpack_kind=build;; jet\\ test*) __jetpack_kind=test;; *) return;; esac\n\
           __jetpack_active=1; __jetpack_command=$1; __jetpack_started=$(date +%s)\n\
           if [ -t 2 ]; then __jetpack_spinner \"$__jetpack_kind\" \"$__jetpack_started\" & __jetpack_spinner_pid=$!; fi\n\
         }}\n\
         __jetpack_precmd() {{\n\
           local code=$?\n\
           if [ \"$__jetpack_active\" = 1 ]; then\n\
             [ -z \"$__jetpack_spinner_pid\" ] || {{ kill \"$__jetpack_spinner_pid\" 2>/dev/null; wait \"$__jetpack_spinner_pid\" 2>/dev/null; if [ \"${{NO_COLOR+x}}\" = x ]; then printf '\\r                                        \\r' >&2; else printf '\\r\\033[2K' >&2; fi; }}\n\
             local elapsed=$(( $(date +%s) - __jetpack_started )) result\n\
             if [ \"$code\" = 0 ]; then result=ok; else result=\"failed ($code)\"; fi\n\
             if [ \"$__jetpack_kind\" = build ]; then __jetpack_build_status=\"$result · ${{elapsed}}s\"; else __jetpack_test_status=\"$result · ${{elapsed}}s\"; fi\n\
             if [ \"${{NO_COLOR+x}}\" = x ]; then printf '%s %s · %ss\\n' \"$__jetpack_kind\" \"$result\" \"$elapsed\"; else [ \"$code\" = 0 ] && printf '\\033[{success}m✓\\033[0m %s ok · %ss\\n' \"$__jetpack_kind\" \"$elapsed\" || printf '\\033[{error}m✗\\033[0m %s failed (%s) · %ss\\n' \"$__jetpack_kind\" \"$code\" \"$elapsed\"; fi\n\
             [ \"$code\" = 0 ] || {{ [ \"${{NO_COLOR+x}}\" = x ] && printf '%s\\n' \"-> $__jetpack_kind failed. Rerun: $__jetpack_command\" || printf '\\033[{warn}m→\\033[0m %s\\n' \"$__jetpack_kind failed. Rerun: $__jetpack_command\"; }}\n\
             __jetpack_active=0; __jetpack_spinner_pid=''\n\
           fi\n\
           return \"$code\"\n\
         }}\n\
         trap '__jetpack_preexec \"$BASH_COMMAND\"' DEBUG\n\
         PROMPT_COMMAND=\"__jetpack_precmd${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\"\n\
         if [ \"${{NO_COLOR+x}}\" = x ]; then\n\
           PS1='{status_prefix}{label} {path_escape} > '\n\
         else\n\
           PS1='{status_prefix}{S}\u{1b}[{accent}m{E}{label}{S}\u{1b}[0m{E} {S}\u{1b}[{border}m{E}{path_escape}{S}\u{1b}[0m{E} {S}\u{1b}[{success}m{E}❯{S}\u{1b}[0m{E} '\n\
         fi\n"
    );
    rc.push_str(&help_prefill_widgets(ShellKind::Bash));
    rc
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
    let accent = SharedTheme::ACCENT_SGR;
    let border = SharedTheme::BORDER_SGR;
    let success = SharedTheme::SUCCESS_SGR;
    let warn = SharedTheme::WARN_SGR;
    let error = SharedTheme::ERROR_SGR;
    let mut rc = format!(
        "[ -f \"$HOME/.zshrc\" ] && source \"$HOME/.zshrc\"\n\
         setopt prompt_subst\n\
         setopt NO_NOMATCH\n\
         zmodload zsh/datetime\n\
         typeset -g __jetpack_build_status='never run' __jetpack_test_status='never run' __jetpack_kind='' __jetpack_command='' __jetpack_started=0 __jetpack_spinner_pid=''\n\
         __jetpack_git_status() {{ if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then local branch=$(git branch --show-current 2>/dev/null); [[ -n $branch ]] || branch=detached; if git diff --quiet --ignore-submodules -- 2>/dev/null && git diff --cached --quiet --ignore-submodules -- 2>/dev/null; then print -n -- \"$branch clean\"; else print -n -- \"$branch changed\"; fi; else print -n -- 'not a git worktree'; fi; }}\n\
         __jetpack_status_words() {{ printf 'build %s · test %s · git %s' \"$__jetpack_build_status\" \"$__jetpack_test_status\" \"$(__jetpack_git_status)\"; }}\n\
         __jetpack_status_glance() {{ printf '\\n%s\\n' \"$(__jetpack_status_words)\"; zle reset-prompt; }}\n\
         zle -N __jetpack_status_glance 2>/dev/null || true\n\
         bindkey '^G' __jetpack_status_glance 2>/dev/null || true\n\
         __jetpack_spinner() {{ local -a frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏); local i=1; while true; do if [[ ${{+NO_COLOR}} = 1 ]]; then printf '\\r%s running %s · %ss' $frames[$i] $1 $(( $EPOCHSECONDS - $2 )) >&2; else printf '\\r\\033[{accent}m%s\\033[0m running %s · %ss' $frames[$i] $1 $(( $EPOCHSECONDS - $2 )) >&2; fi; (( i = i % 10 + 1 )); sleep .1; done; }}\n\
         __jetpack_preexec() {{ case $1 in 'jet build'*) __jetpack_kind=build;; 'jet test'*) __jetpack_kind=test;; *) __jetpack_kind=''; return;; esac; __jetpack_command=$1; __jetpack_started=$EPOCHSECONDS; if [[ -t 2 ]]; then __jetpack_spinner $__jetpack_kind $__jetpack_started &!; __jetpack_spinner_pid=$!; fi; }}\n\
         __jetpack_precmd() {{ local code=$?; [[ -n $__jetpack_kind ]] || return; if [[ -n $__jetpack_spinner_pid ]]; then kill $__jetpack_spinner_pid 2>/dev/null; wait $__jetpack_spinner_pid 2>/dev/null; if [[ ${{+NO_COLOR}} = 1 ]]; then printf '\\r                                        \\r' >&2; else printf '\\r\\033[2K' >&2; fi; fi; local elapsed=$(( EPOCHSECONDS - __jetpack_started )) result; [[ $code = 0 ]] && result=ok || result=\"failed ($code)\"; [[ $__jetpack_kind = build ]] && __jetpack_build_status=\"$result · ${{elapsed}}s\" || __jetpack_test_status=\"$result · ${{elapsed}}s\"; if [[ ${{+NO_COLOR}} = 1 ]]; then printf '%s %s · %ss\\n' $__jetpack_kind \"$result\" $elapsed; else [[ $code = 0 ]] && printf '\\033[{success}m✓\\033[0m %s ok · %ss\\n' $__jetpack_kind $elapsed || printf '\\033[{error}m✗\\033[0m %s failed (%s) · %ss\\n' $__jetpack_kind $code $elapsed; fi; if [[ $code != 0 ]]; then [[ ${{+NO_COLOR}} = 1 ]] && printf '%s\\n' \"-> $__jetpack_kind failed. Rerun: $__jetpack_command\" || printf '\\033[{warn}m→\\033[0m %s\\n' \"$__jetpack_kind failed. Rerun: $__jetpack_command\"; fi; __jetpack_kind=''; __jetpack_spinner_pid=''; }}\n\
         autoload -Uz add-zsh-hook; add-zsh-hook preexec __jetpack_preexec; add-zsh-hook precmd __jetpack_precmd\n\
         if [[ ${{+NO_COLOR}} = 1 ]]; then\n\
           PROMPT='{status_prefix}{label} {path_escape} > '\n\
         else\n\
           PROMPT=$'{status_prefix}%{{\\033[{accent}m%}}{label}%{{\\033[0m%}} %{{\\033[{border}m%}}{path_escape}%{{\\033[0m%}} %{{\\033[{success}m%}}❯%{{\\033[0m%}} '\n\
         fi\n"
    );
    rc.push_str(&help_prefill_widgets(ShellKind::Zsh));
    rc
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
    let accent = SharedTheme::ACCENT_SGR;
    let border = SharedTheme::BORDER_SGR;
    let success = SharedTheme::SUCCESS_SGR;
    let warn = SharedTheme::WARN_SGR;
    let error = SharedTheme::ERROR_SGR;
    let mut rc = format!(
        "set -g __jetpack_build_status 'never run'; set -g __jetpack_test_status 'never run'; set -g __jetpack_kind ''; set -g __jetpack_command ''; set -g __jetpack_started 0; set -g __jetpack_spinner_pid ''; \
         function __jetpack_git_status; if git rev-parse --is-inside-work-tree >/dev/null 2>&1; set -l branch (git branch --show-current 2>/dev/null); test -n \"$branch\"; or set branch detached; if git diff --quiet --ignore-submodules -- 2>/dev/null; and git diff --cached --quiet --ignore-submodules -- 2>/dev/null; echo -n \"$branch clean\"; else; echo -n \"$branch changed\"; end; else; echo -n 'not a git worktree'; end; end; \
         function __jetpack_status_words; printf 'build %s · test %s · git %s' \"$__jetpack_build_status\" \"$__jetpack_test_status\" (__jetpack_git_status); end; \
         function __jetpack_status_glance; echo; __jetpack_status_words; echo; commandline -f repaint; end; \
         bind \\cg __jetpack_status_glance; \
         function __jetpack_spinner; set -l frames ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏; set -l i 1; while true; if set -q NO_COLOR; printf '\\r%s running %s · %ss' $frames[$i] $argv[1] (math (date +%s) - $argv[2]) >&2; else; printf '\\r\\033[{accent}m%s\\033[0m running %s · %ss' $frames[$i] $argv[1] (math (date +%s) - $argv[2]) >&2; end; set i (math $i % 10 + 1); sleep .1; end; end; \
         function __jetpack_preexec --on-event fish_preexec; switch $argv[1]; case \"jet build*\"; set -g __jetpack_kind build; case \"jet test*\"; set -g __jetpack_kind test; case '*'; set -g __jetpack_kind ''; return; end; set -g __jetpack_command $argv[1]; set -g __jetpack_started (date +%s); if isatty stderr; command sh -c 'while :; do if [ \"${{NO_COLOR+x}}\" = x ]; then printf \"\\r⠹ running %s · %ss\" \"$0\" \"$(( $(date +%s) - $1 ))\" >&2; else printf \"\\r\\033[{accent}m⠹\\033[0m running %s · %ss\" \"$0\" \"$(( $(date +%s) - $1 ))\" >&2; fi; sleep .1; done' $__jetpack_kind $__jetpack_started &\nset -g __jetpack_spinner_pid $last_pid; end; end; \
         function __jetpack_postexec --on-event fish_postexec; set -l code $status; test -n \"$__jetpack_kind\"; or return; if test -n \"$__jetpack_spinner_pid\"; kill $__jetpack_spinner_pid 2>/dev/null; wait $__jetpack_spinner_pid 2>/dev/null; if set -q NO_COLOR; printf '\\r                                        \\r' >&2; else; printf '\\r\\033[2K' >&2; end; end; set -l elapsed (math (date +%s) - $__jetpack_started); set -l result ok; test $code -eq 0; or set result \"failed ($code)\"; if test $__jetpack_kind = build; set -g __jetpack_build_status \"$result · \"$elapsed\"s\"; else; set -g __jetpack_test_status \"$result · \"$elapsed\"s\"; end; if set -q NO_COLOR; printf '%s %s · %ss\\n' $__jetpack_kind \"$result\" $elapsed; else if test $code -eq 0; printf '\\033[{success}m✓\\033[0m %s ok · %ss\\n' $__jetpack_kind $elapsed; else; printf '\\033[{error}m✗\\033[0m %s failed (%s) · %ss\\n' $__jetpack_kind $code $elapsed; end; if test $code -ne 0; if set -q NO_COLOR; echo \"-> $__jetpack_kind failed. Rerun: $__jetpack_command\"; else; printf '\\033[{warn}m→\\033[0m %s\\n' \"$__jetpack_kind failed. Rerun: $__jetpack_command\"; end; end; set -g __jetpack_kind ''; set -g __jetpack_spinner_pid ''; end; \
         function fish_prompt; {strip_line}\
         if set -q NO_COLOR; echo -n '{label} '; echo -n ({path_expr}); echo -n ' > '; \
         else; printf '\\033[{accent}m%s\\033[0m ' '{label}'; \
         printf '\\033[{border}m%s\\033[0m' ({path_expr}); \
         printf ' \\033[{success}m❯\\033[0m '; end; end\n"
    );
    rc.push_str(&help_prefill_widgets(ShellKind::Fish));
    rc
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
            vars: std::collections::BTreeMap::new(),
            unset_vars: Vec::new(),
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
    fn run_command_projects_provider_runtime_paths() {
        let mut env = env_with(&[]);
        env.vars.insert("JET_PROVIDER_PATH_TEST".to_string(), "locked-path".to_string());
        let args = vec![
            "sh".to_string(),
            "-c".to_string(),
            "test \"$JET_PROVIDER_PATH_TEST\" = locked-path".to_string(),
        ];
        assert_eq!(run_command(&env, &args), 0);
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
    fn generated_shell_palettes_use_shared_role_codes() {
        let expected = [
            SharedTheme::ACCENT_SGR,
            SharedTheme::BORDER_SGR,
            SharedTheme::SUCCESS_SGR,
            SharedTheme::WARN_SGR,
            SharedTheme::ERROR_SGR,
        ];
        for init in [
            bash_rc("jetpack", PromptPathMode::Short, PromptStripMode::Off),
            zsh_rc("jetpack", PromptPathMode::Short, PromptStripMode::Off),
            fish_init("jetpack", PromptPathMode::Short, PromptStripMode::Off),
        ] {
            for sgr in expected {
                assert!(
                    init.contains(&format!("\\033[{sgr}m"))
                        || init.contains(&format!("\x1b[{sgr}m")),
                    "missing SGR {sgr}"
                );
            }
        }
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
        assert!(bash.contains("jet() {"));
        assert!(bash.contains("read -r -e -i"));

        let zsh = zsh_rc("web-api", PromptPathMode::Short, PromptStripMode::Off);
        assert!(zsh.contains("BUFFER=$picked"));
        assert!(zsh.contains("bindkey '^[?' __jetpack_help_prefill"));
        assert!(zsh.contains("jet() {"));
        assert!(zsh.contains("print -z --"));
        assert!(zsh.contains("alias jet='noglob jet'"));
        assert!(zsh.contains("setopt NO_NOMATCH"));

        let fish = fish_init("web-api", PromptPathMode::Short, PromptStripMode::Off);
        assert!(fish.contains("commandline -r -- \"$picked\""));
        assert!(fish.contains("bind \\e\\? __jetpack_help_prefill"));
        assert!(fish.contains("function jet;"));
        assert!(fish.contains("command jet '?'"));
        assert!(fish.contains("set -lx JET_HELP_SHELL_PREFILL 1"));
        assert!(fish.contains("set -e __jetpack_help_pending"));
        assert!(fish.contains("function __jetpack_help_postexec --on-event fish_postexec"));

        for rc in [&bash, &zsh, &fish] {
            assert!(!rc.contains("eval $picked"), "help selection must never execute");
        }
    }

    #[test]
    fn help_prefill_widgets_preserve_edit_buffer_in_real_shells() {
        let dir = write_temp_dir("jetpack-help-prefill");
        let marker = dir.join("selected-bytes");
        let fake_jet = dir.join("jet");
        std::fs::write(
            &fake_jet,
            format!(
                "#!/bin/sh\nif [ \"$JET_HELP_SHELL_PREFILL\" = 1 ]; then printf \"printf 'JET_SELECTED_BYTES' > '{}'\"; elif [ \"$1\" = --version ]; then printf 'JET_DELEGATED\\n'; else exit 9; fi\n",
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

        let bash_file = write_temp("jetpack-help-bashrc", &bash_rc("help", PromptPathMode::Short, PromptStripMode::Off));
        let bash = format!("PATH={} bash --noprofile --rcfile {} -i", shell_single_quote(&path), bash_file.display());
        let bash_out = pty_prefill_oracle(&bash, &marker, b"\x1b?");
        let _ = std::fs::remove_file(bash_file);
        assert!(bash_out.contains("JET_HELP_WIDGET_RETURNED"), "{bash_out}");
        assert!(marker.exists(), "bash widget did not preserve selected command:\n{bash_out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "JET_SELECTED_BYTES");
        std::fs::remove_file(&marker).unwrap();

        let zdir = write_temp_dir("jetpack-help-zdotdir");
        std::fs::write(zdir.join(".zshrc"), zsh_rc("help", PromptPathMode::Short, PromptStripMode::Off)).unwrap();
        let zsh = format!("PATH={} ZDOTDIR={} zsh -d -i", shell_single_quote(&path), zdir.display());
        let zsh_out = pty_prefill_oracle(&zsh, &marker, b"\x1b?");
        let _ = std::fs::remove_dir_all(zdir);
        assert!(zsh_out.contains("JET_HELP_WIDGET_RETURNED"), "{zsh_out}");
        assert!(marker.exists(), "zsh widget did not preserve selected command:\n{zsh_out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "JET_SELECTED_BYTES");
        std::fs::remove_file(&marker).unwrap();

        let fish = format!(
            "PATH={} fish -C {} -i",
            shell_single_quote(&path),
            shell_single_quote(&fish_init("help", PromptPathMode::Short, PromptStripMode::Off))
        );
        let fish_out = pty_prefill_oracle(&fish, &marker, b"\x1b?");
        assert!(fish_out.contains("JET_HELP_WIDGET_RETURNED"), "{fish_out}");
        assert!(marker.exists(), "fish widget did not preserve selected command:\n{fish_out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "JET_SELECTED_BYTES");
        std::fs::remove_file(&marker).unwrap();

        let fish_literal_out = pty_prefill_oracle(&fish, &marker, b"jet ?\n");
        assert!(fish_literal_out.contains("JET_HELP_WIDGET_RETURNED"), "{fish_literal_out}");
        assert!(marker.exists(), "literal `jet ?` did not prefill fish:\n{fish_literal_out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "JET_SELECTED_BYTES");
        std::fs::remove_file(&marker).unwrap();

        let bash_file = write_temp("jetpack-help-bashrc-literal", &bash_rc("help", PromptPathMode::Short, PromptStripMode::Off));
        let bash = format!("PATH={} bash --noprofile --rcfile {} -i", shell_single_quote(&path), bash_file.display());
        let bash_literal_out = pty_prefill_oracle(&bash, &marker, b"jet ?\n");
        let _ = std::fs::remove_file(bash_file);
        assert!(bash_literal_out.contains("JET_HELP_WIDGET_RETURNED"), "{bash_literal_out}");
        assert!(marker.exists(), "literal `jet ?` did not prefill bash:\n{bash_literal_out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "JET_SELECTED_BYTES");
        std::fs::remove_file(&marker).unwrap();

        let zdir = write_temp_dir("jetpack-help-zdotdir-literal");
        // Hostile: a one-character pathname must not steal bare `jet ?` via zsh glob.
        std::fs::write(zdir.join("x"), "").unwrap();
        std::fs::write(zdir.join(".zshrc"), zsh_rc("help", PromptPathMode::Short, PromptStripMode::Off)).unwrap();
        let zsh = format!(
            "cd {} && PATH={} ZDOTDIR={} zsh -d -i",
            shell_single_quote(&zdir.display().to_string()),
            shell_single_quote(&path),
            shell_single_quote(&zdir.display().to_string())
        );
        let zsh_literal_out = pty_prefill_oracle(&zsh, &marker, b"jet ?\n");
        let _ = std::fs::remove_dir_all(zdir);
        assert!(zsh_literal_out.contains("JET_HELP_WIDGET_RETURNED"), "{zsh_literal_out}");
        assert!(marker.exists(), "literal `jet ?` did not prefill zsh:\n{zsh_literal_out}");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "JET_SELECTED_BYTES");
    }

    #[test]
    fn prompt_rc_has_plain_no_color_fallback() {
        let rc = bash_rc("web-api", PromptPathMode::Full, PromptStripMode::Off);
        assert!(rc.contains("NO_COLOR"));
        assert!(rc.contains("web-api \\w > "));
    }

    fn pty(shell_command: &str, input: &str, no_color: bool) -> String {
        let mut command = Command::new("script");
        command.args(["-qec", shell_command, "/dev/null"])
            .env_remove("NO_COLOR")
            .env_remove("FORCE_COLOR");
        if no_color {
            command.env("NO_COLOR", "").env("TERM", "dumb");
        } else {
            command.env("TERM", "xterm-256color");
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

    fn pty_prefill_oracle(shell_command: &str, marker: &std::path::Path, trigger: &[u8]) -> String {
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
        stdin.write_all(b"echo JET_HELP_WIDGET_START\n").unwrap();
        stdin.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        stdin.write_all(b"jet --version\n").unwrap();
        stdin.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        stdin.write_all(trigger).unwrap();
        stdin.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(350));
        assert!(!marker.exists(), "help selection executed before explicit Enter");
        stdin.write_all(b"; printf 'JET_EXPLICIT_ACCEPT\\n'\n").unwrap();
        stdin.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        stdin.write_all(b"echo JET_HELP_WIDGET_RETURNED\nexit 0\n").unwrap();
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let transcript = String::from_utf8_lossy(&out.stdout).replace('\r', "");
        assert!(transcript.contains("JET_DELEGATED"), "{transcript}");
        assert!(transcript.contains("JET_EXPLICIT_ACCEPT"), "{transcript}");
        transcript
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

    fn assert_receipt_colors_after_marker(output: &str) {
        let captured = output
            .rsplit_once("JETPACK_CAPTURE_START")
            .map(|(_, captured)| captured)
            .expect("PTY reached color capture marker");
        assert!(
            captured.contains(&format!("\x1b[{}m✓\x1b[0m", SharedTheme::SUCCESS_SGR)),
            "{captured:?}"
        );
        assert!(
            captured.contains(&format!("\x1b[{}m✗\x1b[0m", SharedTheme::ERROR_SGR)),
            "{captured:?}"
        );
    }

    #[test]
    fn bash_prompt_receipts_are_colored_in_a_real_pty() {
        const LABEL: &str = "JETPACK_BASH_GENERATED";
        let home = write_temp_dir("jetpack-prompt-color-bash-home");
        std::fs::write(home.join(".bashrc"), "# isolated test startup\n").unwrap();
        let bash_rc = write_temp(
            "jetpack-prompt-color-bashrc",
            &bash_rc(LABEL, PromptPathMode::Short, PromptStripMode::Off),
        );
        assert!(bash_rc.is_file(), "generated bash rc was not written");
        let path = std::env::var("PATH").unwrap_or_default();
        let bash = pty(
            &format!(
                "env -i HOME={} PATH={} TERM=xterm-256color SHELL=/bin/bash HISTFILE=/dev/null bash --noprofile --rcfile {} -i",
                shell_single_quote(&home.display().to_string()),
                shell_single_quote(&path),
                shell_single_quote(&bash_rc.display().to_string()),
            ),
            "bind 'set enable-bracketed-paste off'\necho JETPACK_CAPTURE_START\njet() { sleep .1; [ \"$1\" = build ]; }\njet build\njet test\nexit 0\n",
            false,
        );
        let _ = std::fs::remove_file(bash_rc);
        let _ = std::fs::remove_dir_all(home);
        assert_receipt_colors_after_marker(&bash);
        let captured = bash.rsplit_once("JETPACK_CAPTURE_START").unwrap().1;
        assert!(
            captured.contains(&format!(
                "\x1b[{}m{LABEL}\x1b[0m",
                SharedTheme::ACCENT_SGR
            )),
            "generated bash prompt sentinel missing: {captured:?}"
        );
    }

    #[test]
    fn fish_prompt_receipts_are_colored_in_a_real_pty() {
        let fish = pty(
            &format!(
                "TERM=dumb fish -C {} -i",
                shell_single_quote(&fish_init("web-api", PromptPathMode::Short, PromptStripMode::Off))
            ),
            "echo JETPACK_CAPTURE_START\nfunction jet; sleep .3; test $argv[1] = build; end\njet build\njet test\necho JETPACK_FISH_COLOR_DONE\nexit 0\n",
            false,
        );
        assert_receipt_colors_after_marker(&fish);
    }

    #[test]
    fn zsh_prompt_receipts_are_colored_in_a_real_pty() {
        let zdir = write_temp_dir("jetpack-prompt-color-zdotdir");
        std::fs::write(
            zdir.join(".zshrc"),
            zsh_rc("web-api", PromptPathMode::Short, PromptStripMode::Off),
        )
        .unwrap();
        let zsh = pty(
            &format!("ZDOTDIR={} zsh -d -i", zdir.display()),
            "unset zle_bracketed_paste\necho JETPACK_CAPTURE_START\nunalias jet 2>/dev/null\nunfunction jet 2>/dev/null\njet() { [[ $1 = build ]] }\njet build\njet test\nexit 0\n",
            false,
        );
        let _ = std::fs::remove_dir_all(zdir);
        assert_receipt_colors_after_marker(&zsh);
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
            "unset zle_bracketed_paste\necho JETPACK_CAPTURE_START\nunalias jet 2>/dev/null\nunfunction jet 2>/dev/null\njet() { sleep .3; [[ $1 = build ]] }\njet build\njet test\n\x07\n__jetpack_status_words\necho JETPACK_ZSH_PTY_DONE\nexit 0\n",
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
