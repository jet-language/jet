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
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Hash every input that can change the checked activation plan. The prompt
/// hook uses this read-only fingerprint before it emits anything, so changed
/// imported modules, dotenv files, managed-file sources, locks, and preset
/// inputs cannot leave a stale environment active.
pub fn definition_fingerprint(root: &Path, requested_preset: Option<&str>) -> Option<String> {
    definition_fingerprint_with_selections(root, requested_preset, None)
}

/// Hash the activation plan for both kinds of explicit selection: a named
/// environment preset inside `env.jet`, and one declared `env.<name>`
/// environment module.
pub fn definition_fingerprint_with_selections(
    root: &Path,
    requested_preset: Option<&str>,
    requested_environment: Option<&str>,
) -> Option<String> {
    let env_path = root.join(Syntax::ENV_FILE);
    let source = std::fs::read_to_string(&env_path).ok()?;
    let mut entries = Vec::<(String, Vec<u8>)>::new();
    if let Ok(plan) = jet_env_model::ModuleEval::evaluate_env_with_selections(
        &source,
        root,
        requested_preset,
        requested_environment,
    ) {
        for relative in &plan.source_files {
            add_input(root, relative, "source", &mut entries);
        }
        for dotenv in &plan.lifecycle.dotenv {
            add_input(root, &dotenv.file, "dotenv", &mut entries);
        }
        if let jet_env_model::ModuleEval::ReloadPolicy::Watch { paths, .. } = &plan.lifecycle.reload
        {
            for path in paths {
                add_input(root, path, "reload-watch", &mut entries);
            }
        }
        for file in &plan.files {
            if let Some(relative) = &file.source {
                add_input(root, relative, "managed", &mut entries);
            }
            entries.push((
                format!("managed-fact:{}", file.destination),
                file.fingerprint().into_bytes(),
            ));
        }
        entries.push((
            "lifecycle".to_string(),
            plan.lifecycle.fingerprint().into_bytes(),
        ));
        for preset in &plan.presets {
            entries.push((
                format!("preset:{}", preset.name),
                format!(
                    "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                    preset.extends,
                    preset.packages,
                    preset.variables,
                    preset.hostname,
                    preset.user,
                    requested_preset,
                )
                .into_bytes(),
            ));
        }
        entries.push((
            "languages".to_string(),
            plan.languages
                .iter()
                .map(|language| language.fingerprint())
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        ));
    } else {
        // Keep malformed/legacy files observable without allowing an
        // unrelated `.jet` file to become part of a valid environment graph.
        // The activation path still rejects the malformed plan below.
        collect_definition_files(root, root, &mut entries);
    }
    for relative in [Syntax::UNIFIED_LOCK_FILE, "package.jet", "pkg.jet", "jetpack.toml"] {
        add_input(root, relative, "project", &mut entries);
    }
    entries.push((
        "selection".to_string(),
        format!(
            "preset={};environment={};host={};user={}",
            requested_preset.unwrap_or_default(),
            requested_environment.unwrap_or_default(),
            std::env::var("HOSTNAME").unwrap_or_default(),
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default()
        )
        .into_bytes(),
    ));
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut canonical = Vec::new();
    for (name, bytes) in entries {
        canonical.extend_from_slice(name.as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        canonical.extend_from_slice(&bytes);
    }
    Some(crate::SHA256::sha256_hex(&canonical))
}

/// Read the typed lifecycle policy without realizing packages or executing
/// project code. Legacy `pkg.*` env files have the normal prompt policy.
pub fn reload_policy(root: &Path) -> jet_env_model::ModuleEval::ReloadPolicy {
    reload_policy_with_environment(root, None)
}

/// Read reload policy for the selected `env.<name>` environment module.
pub fn reload_policy_with_environment(
    root: &Path,
    requested_environment: Option<&str>,
) -> jet_env_model::ModuleEval::ReloadPolicy {
    let Ok(source) = std::fs::read_to_string(root.join(Syntax::ENV_FILE)) else {
        return jet_env_model::ModuleEval::ReloadPolicy::default();
    };
    jet_env_model::ModuleEval::evaluate_env_with_selections(
        &source,
        root,
        None,
        requested_environment,
    )
        .map(|plan| plan.lifecycle.reload)
        .unwrap_or_default()
}

/// Coalesce a watched definition change until its debounce window expires.
/// State is project-local and contains only the definition hash and a clock
/// value; it never stores environment values or secrets.
pub fn watch_reload_ready(root: &Path, hash: &str, debounce_ms: u64) -> Result<bool, String> {
    if debounce_ms == 0 {
        return Ok(true);
    }
    let state_dir = root.join(Syntax::CONFIG_DEFAULT_DIR);
    let state_path = state_dir.join("env-hook-reload");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if let Ok(state) = std::fs::read_to_string(&state_path) {
        let mut lines = state.lines();
        if lines.next() == Some(hash) {
            if let Some(started) = lines.next().and_then(|value| value.parse::<u64>().ok()) {
                return Ok(now.saturating_sub(started) >= debounce_ms);
            }
        }
    }
    std::fs::create_dir_all(&state_dir)
        .map_err(|error| format!("couldn't create {}: {error}", state_dir.display()))?;
    let temporary = state_dir.join(format!(".env-hook-reload.{}.tmp", std::process::id()));
    std::fs::write(&temporary, format!("{hash}\n{now}\n"))
        .map_err(|error| format!("couldn't write {}: {error}", state_path.display()))?;
    std::fs::rename(&temporary, &state_path)
        .map_err(|error| format!("couldn't commit {}: {error}", state_path.display()))?;
    Ok(false)
}

/// Remove the debounce marker after a watched definition has activated.
pub fn clear_watch_reload(root: &Path) {
    let _ = std::fs::remove_file(root.join(Syntax::CONFIG_DEFAULT_DIR).join("env-hook-reload"));
}

fn add_input(root: &Path, relative: &str, kind: &str, entries: &mut Vec<(String, Vec<u8>)>) {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        entries.push((format!("{kind}:unsafe:{relative}"), Vec::new()));
        return;
    }
    let path = root.join(path);
    let root_real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut visited = BTreeSet::new();
    add_input_path(
        &root_real,
        &path,
        relative,
        kind,
        entries,
        &mut visited,
    );
}

fn add_input_path(
    root: &Path,
    path: &Path,
    relative: &str,
    kind: &str,
    entries: &mut Vec<(String, Vec<u8>)>,
    visited: &mut BTreeSet<PathBuf>,
) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        entries.push((format!("{kind}:missing:{relative}"), Vec::new()));
        return;
    };
    if metadata.file_type().is_symlink() {
        let target = match std::fs::canonicalize(path) {
            Ok(target) if target.starts_with(root) => target,
            Ok(target) => {
                entries.push((
                    format!("{kind}:unsafe-link:{relative}"),
                    target.to_string_lossy().as_bytes().to_vec(),
                ));
                return;
            }
            Err(_) => {
                entries.push((format!("{kind}:unsafe-link:{relative}"), Vec::new()));
                return;
            }
        };
        entries.push((
            format!("{kind}:link:{relative}"),
            target.to_string_lossy().as_bytes().to_vec(),
        ));
        add_input_path(root, &target, relative, kind, entries, visited);
        return;
    }
    if let Ok(real) = path.canonicalize() {
        if !real.starts_with(root) {
            entries.push((
                format!("{kind}:unsafe:{relative}"),
                real.to_string_lossy().as_bytes().to_vec(),
            ));
            return;
        }
        if !visited.insert(real) {
            entries.push((format!("{kind}:cycle:{relative}"), Vec::new()));
            return;
        }
    }
    if metadata.is_dir() {
        entries.push((
            format!("{kind}:directory:{relative}"),
            format!(
                "readonly={};modified={:?}",
                metadata.permissions().readonly(),
                metadata.modified().ok()
            )
            .into_bytes(),
        ));
        let Ok(read_dir) = std::fs::read_dir(&path) else {
            entries.push((format!("{kind}:unreadable:{relative}"), Vec::new()));
            return;
        };
        let mut children = read_dir.filter_map(Result::ok).collect::<Vec<_>>();
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let name = child.file_name().to_string_lossy().into_owned();
            let child_relative = if relative.is_empty() || relative == "." {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            add_input_path(root, &path.join(&name), &child_relative, kind, entries, visited);
        }
    } else if metadata.is_file() {
        match std::fs::read(path) {
            Ok(bytes) => entries.push((format!("{kind}:file:{relative}"), bytes)),
            Err(_) => entries.push((format!("{kind}:unreadable:{relative}"), Vec::new())),
        }
    } else {
        entries.push((
            format!("{kind}:special:{relative}"),
            format!("file-type={:?}", metadata.file_type()).into_bytes(),
        ));
    }
}

fn collect_definition_files(root: &Path, current: &Path, entries: &mut Vec<(String, Vec<u8>)>) {
    let Ok(read_dir) = std::fs::read_dir(current) else {
        return;
    };
    let mut paths = read_dir.filter_map(Result::ok).map(|entry| entry.path()).collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        if path.file_name().is_some_and(|name| name == ".jet") {
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            if path.extension().is_some_and(|extension| extension == Syntax::FILE_EXT) {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                add_input(root, &relative, "source", entries);
            }
        } else if metadata.is_dir() {
            collect_definition_files(root, &path, entries);
        } else if metadata.is_file()
            && path.extension().is_some_and(|extension| extension == Syntax::FILE_EXT)
        {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            add_input(root, &relative, "source", entries);
        }
    }
}

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
    pub vars: std::collections::BTreeMap<String, String>,
    pub unset: Vec<String>,
    pub plan_hash: String,
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
pub fn render_activate(kind: ShellKind, act: &Activation) -> Result<String, String> {
    validate_activation(act)?;
    let old = Syntax::ENV_HOOK_OLD_PATH_VAR;
    let marker = Syntax::JETPACK_ENV_MARKER;
    let refs = Syntax::JETPACK_REF_VAR;
    let dir = Syntax::ENV_HOOK_ACTIVE_DIR_VAR;
    let hash = Syntax::ENV_HOOK_ACTIVE_HASH_VAR;
    let vars = render_vars(kind, &act.vars);
    let unset = render_unset(kind, &act.unset);
    Ok(match kind {
        ShellKind::Bash | ShellKind::Zsh => format!(
            "export {old}={base}\n\
             export PATH={path}\n\
             export {marker}=1\n\
             export {refs}={refval}\n\
             export {dir}={root}\n\
             export {hash}={plan_hash}\n\
             {vars}{unset}",
            base = sh_quote(&act.base_path),
            path = sh_quote(&act.composed_path),
            refval = sh_quote(&act.refs),
            root = sh_quote(&act.root),
            plan_hash = sh_quote(&act.plan_hash),
            vars = vars,
            unset = unset,
        ),
        ShellKind::Fish => format!(
            "set -gx {old} {base}\n\
             set -gx PATH (string split : {path})\n\
             set -gx {marker} 1\n\
             set -gx {refs} {refval}\n\
             set -gx {dir} {root}\n\
             set -gx {hash} {plan_hash}\n\
             {vars}{unset}",
            base = fish_quote(&act.base_path),
            path = fish_quote(&act.composed_path),
            refval = fish_quote(&act.refs),
            root = fish_quote(&act.root),
            plan_hash = fish_quote(&act.plan_hash),
            vars = vars,
            unset = unset,
        ),
    })
}

/// Render the statements that unload the active env, restoring `base_path`
/// (the `PATH` from before the env loaded) and clearing every marker.
pub fn render_unload(kind: ShellKind, base_path: &str) -> String {
    let old = Syntax::ENV_HOOK_OLD_PATH_VAR;
    let marker = Syntax::JETPACK_ENV_MARKER;
    let refs = Syntax::JETPACK_REF_VAR;
    let dir = Syntax::ENV_HOOK_ACTIVE_DIR_VAR;
    let hash = Syntax::ENV_HOOK_ACTIVE_HASH_VAR;
    match kind {
        ShellKind::Bash | ShellKind::Zsh => format!(
            "export PATH={path}\n\
             unset {marker}\n\
             unset {refs}\n\
             unset {dir}\n\
             unset {hash}\n\
             unset {old}\n",
            path = sh_quote(base_path),
            hash = hash,
        ),
        ShellKind::Fish => format!(
            "set -gx PATH (string split : {path})\n\
             set -e {marker}\n\
             set -e {refs}\n\
             set -e {dir}\n\
             set -e {hash}\n\
             set -e {old}\n",
            path = fish_quote(base_path),
            hash = hash,
        ),
    }
}

fn render_vars(kind: ShellKind, vars: &std::collections::BTreeMap<String, String>) -> String {
    vars.iter()
        .map(|(name, value)| match kind {
            ShellKind::Bash | ShellKind::Zsh => {
                format!("export {name}={}\n", sh_quote(value))
            }
            ShellKind::Fish => format!("set -gx {name} {}\n", fish_quote(value)),
        })
        .collect()
}

fn render_unset(kind: ShellKind, names: &[String]) -> String {
    names
        .iter()
        .map(|name| match kind {
            ShellKind::Bash | ShellKind::Zsh => format!("unset {name}\n"),
            ShellKind::Fish => format!("set -e {name}\n"),
        })
        .collect()
}

fn validate_activation(act: &Activation) -> Result<(), String> {
    for name in act.vars.keys().chain(act.unset.iter()) {
        if !jet_env_model::ModuleEval::valid_env_name(name) {
            return Err(format!("activation variable '{name}' is not a valid environment name"));
        }
    }
    Ok(())
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
            refs: "ripgrep@nixpkgs jq@nixpkgs".to_string(),
            root: "/home/dev/router".to_string(),
            vars: std::collections::BTreeMap::new(),
            unset: Vec::new(),
            plan_hash: "hash".to_string(),
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
        let bash = render_activate(ShellKind::Bash, &act()).unwrap();
        assert!(bash.contains("export JETPACK_ENV_OLD_PATH='/usr/bin:/bin'"));
        assert!(bash.contains("export PATH='/nix/store/pkg/bin:/usr/bin:/bin'"));
        assert!(bash.contains("export JETPACK_ENV=1"));
        assert!(bash.contains("export JETPACK_ENV_DIR='/home/dev/router'"));
        assert!(bash.contains("export JETPACK_REF='ripgrep@nixpkgs jq@nixpkgs'"));

        let fish = render_activate(ShellKind::Fish, &act()).unwrap();
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

    #[test]
    fn definition_fingerprint_tracks_explicit_reload_paths() {
        let root = std::env::temp_dir().join(format!(
            "jpk-envhook-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(Syntax::ENV_FILE),
            "module env.dev { reload: Reload.{ watch: [\"tracked.txt\"], debounce: 250 } }\n",
        )
        .unwrap();
        std::fs::write(root.join("tracked.txt"), "one\n").unwrap();
        let first = definition_fingerprint(&root, None).unwrap();
        std::fs::write(root.join("untracked.txt"), "one\n").unwrap();
        assert_eq!(definition_fingerprint(&root, None), Some(first.clone()));
        std::fs::write(root.join("tracked.txt"), "two\n").unwrap();
        assert_ne!(definition_fingerprint(&root, None), Some(first));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn definition_fingerprint_tracks_environment_module_selection() {
        let root = std::env::temp_dir().join(format!(
            "jpk-envhook-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(Syntax::ENV_FILE),
            "module env.dev { packages: [nixpkgs.ripgrep] }\nmodule env.full { packages: [nixpkgs.fd] }\n",
        )
        .unwrap();
        let dev = definition_fingerprint_with_selections(&root, None, Some("dev")).unwrap();
        let full = definition_fingerprint_with_selections(&root, None, Some("full")).unwrap();
        assert_ne!(dev, full);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn activation_rejects_shell_tokens_at_render_boundary() {
        let mut activation = act();
        activation.unset.push("BAD; echo injected".to_string());
        assert!(render_activate(ShellKind::Bash, &activation).is_err());
    }
}
