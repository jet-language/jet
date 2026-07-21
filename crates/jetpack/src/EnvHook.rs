//! D-ENVHOOK1=A: direnv-style opt-in env auto-activation.
//!
//! `jet env hook <shell>` prints a one-line shell hook the user installs once
//! (`jet env hook fish | source`, or a line in their shell config). After that,
//! each prompt runs the hook, which `eval`s the output of `jet env export
//! <shell>` — the private per-prompt callback that realizes the nearest
//! `env.jet` and emits the shell statements to activate it (or, when leaving
//! its directory tree, to deactivate and restore the prior `PATH`).
//!
//! Split by responsibility: this module owns the *pure* string generation and
//! the ancestor `env.jet` walk (both trivially unit-testable, std-only). The
//! realize/trust orchestration — which reads `env.jet`, gates on trust, and
//! composes the env — lives in `CLI::run_enter_dev`, reusing the exact same
//! `compose_env` + `Trust::gate` path as `jet env` itself (I8: one env engine).

use crate::Shell::ShellKind;
use crate::Syntax;
use std::path::{Path, PathBuf};

/// Parse the shell argument the user (or the installed hook) passed to
/// `jet env hook <shell>` / `jet env export <shell>`. `None` falls back to
/// auto-detection from `$SHELL`.
pub fn parse_shell(arg: Option<&str>) -> Option<ShellKind> {
    match arg {
        None => Some(ShellKind::detect()),
        Some("bash") => Some(ShellKind::Bash),
        Some("zsh") => Some(ShellKind::Zsh),
        Some("fish") => Some(ShellKind::Fish),
        Some(_) => None,
    }
}

/// The nearest directory at or above `start` that contains an `env.jet`, or
/// `None`. This is what makes `cd` into a project subdirectory keep the env
/// active (direnv semantics): the whole subtree under an `env.jet` root counts
/// as inside that env.
pub fn find_env_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(Syntax::ENV_FILE).is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// The activation facts a load emits into the shell.
pub struct Activation {
    /// The `PATH` the shell had before this env loaded — saved so unload can
    /// restore it verbatim.
    pub base_path: String,
    /// The composed `PATH` (env bin dirs prepended to `base_path`).
    pub composed_path: String,
    /// The realized package refs, space-joined (mirrors `Shell::Env::apply`).
    pub refs: String,
    /// The absolute `env.jet` root directory this activation is anchored to.
    pub root: String,
}

/// Render the opt-in shell hook the user installs once. Idempotent to install
/// (re-sourcing never double-registers). Also installs `jet ?` / Alt-?
/// help-app prefill widgets so a normal bash/zsh/fish session gets the same
/// editable-buffer behavior as a branded jetpack subshell.
pub fn render_hook(kind: ShellKind) -> String {
    use crate::Shell::help_prefill_widgets;
    let export = Syntax::ENV_EXPORT_VERB;
    let env_hook = match kind {
        ShellKind::Bash => format!(
            "__jet_env_hook() {{\n  \
               local __jet_out\n  \
               __jet_out=\"$(command jet env {export} bash)\" || return 0\n  \
               [ -n \"$__jet_out\" ] && eval \"$__jet_out\"\n\
             }}\n\
             case \";${{PROMPT_COMMAND:-}};\" in\n  \
               *\";__jet_env_hook;\"*) ;;\n  \
               *) PROMPT_COMMAND=\"__jet_env_hook${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\" ;;\n\
             esac\n"
        ),
        ShellKind::Zsh => format!(
            "__jet_env_hook() {{\n  \
               local __jet_out\n  \
               __jet_out=\"$(command jet env {export} zsh)\" || return 0\n  \
               [[ -n \"$__jet_out\" ]] && eval \"$__jet_out\"\n\
             }}\n\
             autoload -Uz add-zsh-hook\n\
             add-zsh-hook precmd __jet_env_hook\n"
        ),
        ShellKind::Fish => format!(
            "function __jet_env_hook --on-event fish_prompt --on-variable PWD\n  \
               set -l __jet_out (command jet env {export} fish | string collect)\n  \
               test -n \"$__jet_out\"; and eval \"$__jet_out\"\n\
             end\n"
        ),
    };
    format!("{env_hook}{}", help_prefill_widgets(kind))
}

/// Render the statements that load an env into the current shell.
pub fn render_activate(kind: ShellKind, act: &Activation) -> String {
    let old = Syntax::ENV_HOOK_OLD_PATH_VAR;
    let marker = Syntax::JETPACK_ENV_MARKER;
    let refs = Syntax::JETPACK_REF_VAR;
    let dir = Syntax::ENV_HOOK_ACTIVE_DIR_VAR;
    match kind {
        ShellKind::Bash | ShellKind::Zsh => format!(
            "export {old}={base}\n\
             export PATH={path}\n\
             export {marker}=1\n\
             export {refs}={refval}\n\
             export {dir}={root}\n",
            base = sh_quote(&act.base_path),
            path = sh_quote(&act.composed_path),
            refval = sh_quote(&act.refs),
            root = sh_quote(&act.root),
        ),
        ShellKind::Fish => format!(
            "set -gx {old} {base}\n\
             set -gx PATH (string split : {path})\n\
             set -gx {marker} 1\n\
             set -gx {refs} {refval}\n\
             set -gx {dir} {root}\n",
            base = fish_quote(&act.base_path),
            path = fish_quote(&act.composed_path),
            refval = fish_quote(&act.refs),
            root = fish_quote(&act.root),
        ),
    }
}

/// Render the statements that unload the active env, restoring `base_path`
/// (the `PATH` from before the env loaded) and clearing every marker.
pub fn render_unload(kind: ShellKind, base_path: &str) -> String {
    let old = Syntax::ENV_HOOK_OLD_PATH_VAR;
    let marker = Syntax::JETPACK_ENV_MARKER;
    let refs = Syntax::JETPACK_REF_VAR;
    let dir = Syntax::ENV_HOOK_ACTIVE_DIR_VAR;
    match kind {
        ShellKind::Bash | ShellKind::Zsh => format!(
            "export PATH={path}\n\
             unset {marker}\n\
             unset {refs}\n\
             unset {dir}\n\
             unset {old}\n",
            path = sh_quote(base_path),
        ),
        ShellKind::Fish => format!(
            "set -gx PATH (string split : {path})\n\
             set -e {marker}\n\
             set -e {refs}\n\
             set -e {dir}\n\
             set -e {old}\n",
            path = fish_quote(base_path),
        ),
    }
}

/// POSIX single-quote (bash/zsh): wrap in `'…'`, closing/escaping/reopening for
/// any embedded `'`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// fish single-quote: inside `'…'` fish only treats `\` and `'` specially.
fn fish_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act() -> Activation {
        Activation {
            base_path: "/usr/bin:/bin".to_string(),
            composed_path: "/nix/store/pkg/bin:/usr/bin:/bin".to_string(),
            refs: "nixpkgs:ripgrep nixpkgs:jq".to_string(),
            root: "/home/dev/router".to_string(),
        }
    }

    #[test]
    fn parse_shell_maps_known_and_rejects_unknown() {
        assert_eq!(parse_shell(Some("bash")), Some(ShellKind::Bash));
        assert_eq!(parse_shell(Some("zsh")), Some(ShellKind::Zsh));
        assert_eq!(parse_shell(Some("fish")), Some(ShellKind::Fish));
        assert_eq!(parse_shell(Some("tcsh")), None);
        assert!(parse_shell(None).is_some());
    }

    #[test]
    fn find_env_root_walks_up_to_the_nearest_env_jet() {
        let base = std::env::temp_dir().join(format!("jpk-envhook-{}", std::process::id()));
        let root = base.join("proj");
        let deep = root.join("src").join("inner");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(root.join(Syntax::ENV_FILE), "// env\n").unwrap();
        assert_eq!(find_env_root(&deep), Some(root.clone()));
        assert_eq!(find_env_root(&root), Some(root.clone()));
        // A sibling tree with no env.jet above it resolves to None.
        let orphan = base.join("orphan");
        std::fs::create_dir_all(&orphan).unwrap();
        // Only None if no ancestor has env.jet; temp_dir has none.
        assert_eq!(find_env_root(&orphan), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hook_registers_a_prompt_callback_per_shell() {
        let bash = render_hook(ShellKind::Bash);
        assert!(bash.contains("PROMPT_COMMAND"));
        assert!(bash.contains("command jet env export bash"));
        assert!(bash.contains("__jet_env_hook;")); // dedup guard

        let zsh = render_hook(ShellKind::Zsh);
        assert!(zsh.contains("add-zsh-hook precmd __jet_env_hook"));
        assert!(zsh.contains("command jet env export zsh"));

        let fish = render_hook(ShellKind::Fish);
        assert!(fish.contains("--on-event fish_prompt"));
        assert!(fish.contains("--on-variable PWD"));
        assert!(fish.contains("command jet env export fish"));

        // Help prefill ships with the hook so normal shells (not only jetpack
        // enter) get Alt-? / literal `jet ?` editable-buffer insertion.
        for hook in [&bash, &zsh, &fish] {
            assert!(hook.contains("__jetpack_help_prefill"));
            assert!(hook.contains("JET_HELP_SHELL_PREFILL"));
        }
        assert!(bash.contains("READLINE_LINE=$picked"));
        assert!(zsh.contains("print -z --"));
        assert!(fish.contains("fish_postexec"));
    }

    #[test]
    fn activate_exports_path_markers_and_saves_old_path() {
        let bash = render_activate(ShellKind::Bash, &act());
        assert!(bash.contains("export JETPACK_ENV_OLD_PATH='/usr/bin:/bin'"));
        assert!(bash.contains("export PATH='/nix/store/pkg/bin:/usr/bin:/bin'"));
        assert!(bash.contains("export JETPACK_ENV=1"));
        assert!(bash.contains("export JETPACK_ENV_DIR='/home/dev/router'"));
        assert!(bash.contains("export JETPACK_REF='nixpkgs:ripgrep nixpkgs:jq'"));

        let fish = render_activate(ShellKind::Fish, &act());
        assert!(fish.contains("set -gx PATH (string split : '/nix/store/pkg/bin:/usr/bin:/bin')"));
        assert!(fish.contains("set -gx JETPACK_ENV_DIR '/home/dev/router'"));
    }

    #[test]
    fn unload_restores_base_path_and_clears_markers() {
        let bash = render_unload(ShellKind::Bash, "/usr/bin:/bin");
        assert!(bash.contains("export PATH='/usr/bin:/bin'"));
        assert!(bash.contains("unset JETPACK_ENV\n"));
        assert!(bash.contains("unset JETPACK_ENV_DIR"));
        assert!(bash.contains("unset JETPACK_ENV_OLD_PATH"));

        let fish = render_unload(ShellKind::Fish, "/usr/bin:/bin");
        assert!(fish.contains("set -gx PATH (string split : '/usr/bin:/bin')"));
        assert!(fish.contains("set -e JETPACK_ENV_DIR"));
    }

    #[test]
    fn quoting_survives_single_quotes_and_backslashes() {
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
        assert_eq!(fish_quote("a'b"), "'a\\'b'");
        assert_eq!(fish_quote("a\\b"), "'a\\\\b'");
    }
}
