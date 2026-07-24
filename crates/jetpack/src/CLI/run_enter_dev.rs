use super::package_hangar_vendor::auto_clean_after_success;
use super::parse::Parsed;
use super::realize::{apply_locked_channels, classify_or_report, load_project_plan, RunPlan};
use super::services_secrets_config::{
    find_jet_binary, find_project_entry, has_dev_or_run_entry, list_project_tasks,
    validate_declared_secrets, wait_for_services_ready,
};
use super::trust_env_build::compose_env;
use super::workspace_sources::{cwd_table, load_workspace};
use crate::EnvFile;
use crate::EnvHook;
use crate::MemberSelect::{self, SelectRequest};
use jet_env_model::ModuleEval;
use crate::Output::Theme;
use crate::Provider;
use crate::RefSpec;
use crate::Shell::{self, Env, ShellKind};
use crate::Store;
use crate::Syntax;
use crate::Trust;
use std::path::{Path, PathBuf};

/// `jetpack run [<ref>|<task>] [-- cmd…]`
///
/// D-JPK-TASKRUN1: a bare first positional that names a `#Task fn` in the
/// project entry runs that task (via `jet run --task <name> <entry>`). Package
/// refs (`source:pkg`, workspace members) keep the existing realize path.
pub(super) fn cmd_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
    }

    let project_dir = std::env::current_dir().unwrap_or_default();
    let select_req = SelectRequest {
        packages: parsed.flags.workspace_members.clone(),
        affected: parsed.flags.affected,
        affected_since: parsed.flags.affected_since.clone(),
    };
    // D-JPK-SELECTOR1=C: workspace + selection flags → realize only those members.
    if project_dir.join(Syntax::WORKSPACE_FILE).exists() && select_req.is_restricting() {
        if let Some(result) = load_workspace(&project_dir) {
            return match result {
                Err(code) => code,
                Ok(plan) => match MemberSelect::select_members(&project_dir, &plan, &select_req) {
                    Ok(selected) if selected.is_empty() => {
                        theme.status("no workspace members matched the selection.");
                        0
                    }
                    Ok(_) => {
                        // Realize selected members through the build path.
                        let code = super::trust_env_build::cmd_build(theme, parsed);
                        if code != 0 {
                            return code;
                        }
                        match &parsed.command {
                            Some(cmd) if !cmd.is_empty() => {
                                let mut plan = match load_project_plan(theme) {
                                    Ok(plan) => plan,
                                    Err(code) => return code,
                                };
                                if let Err(code) =
                                    apply_locked_channels(theme, &project_dir, &mut plan.table)
                                {
                                    return code;
                                }
                                let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                                    Ok(env) => env,
                                    Err(code) => return code,
                                };
                                run_visible_command(theme, &env, &plan.refs, cmd)
                            }
                            _ => 0,
                        }
                    }
                    Err(d) => {
                        theme.error_coded(&d.code, &d.what, &d.why, &d.fix);
                        2
                    }
                },
            };
        }
    }

    let entry = find_project_entry(&project_dir);
    let declared_tasks = list_project_tasks(&entry);

    // Prefer a project `#Task` over package-ref classification when the first
    // positional is a bare name (no `@source` suffix).
    if let Some(raw) = parsed.positional.first() {
        if !raw.contains(Syntax::REF_PROVIDER_AT) && declared_tasks.iter().any(|t| t == raw) {
            return run_project_task(theme, parsed, &roots, &project_dir, &entry, raw);
        }
    }

    // Collect the refs to realize plus the source table that resolves any
    // named sources: an explicit CLI ref (built-ins only), or the project pack.
    let mut explicit_package: Option<String> = None;
    let mut plan = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(spec) => {
                explicit_package = Some(spec.short_name().to_string());
                RunPlan {
                    refs: vec![spec],
                    adapters: Vec::new(),
                    table: cwd_table(),
                    label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
                    prompt_path: ModuleEval::PromptPathMode::default(),
                    prompt_strip: ModuleEval::PromptStripMode::default(),
                    dev_services: Vec::new(),
                    secrets: Vec::new(),
                }
            }
            Err(_) => {
                // Bare unknown name + declared tasks → E1290 (list them).
                if !raw.contains(Syntax::REF_PROVIDER_AT) && !declared_tasks.is_empty() {
                    let list = declared_tasks.join(", ");
                    theme.error_coded(
                        "E1294",
                        &format!("no task named `{raw}`"),
                        "`jetpack run <name>` invokes a `#Task fn` in the project entry (D-JPK-TASKRUN1).",
                        "mark a function `#Task` to make it runnable, or check the spelling.",
                    );
                    theme.detail(&format!("declared tasks: {list}"));
                    return 2;
                }
                return 2;
            }
        },
        None => match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }

    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };

    let code = match &parsed.command {
        Some(cmd) if !cmd.is_empty() => run_visible_command(theme, &env, &plan.refs, cmd),
        _ => {
            if let Some(program) = explicit_package {
                let cmd = vec![program];
                run_visible_command(theme, &env, &plan.refs, &cmd)
            } else {
                Shell::enter(theme, &env, ShellKind::detect())
            }
        }
    };
    if code == 0 {
        auto_clean_after_success(theme, &roots);
    }
    code
}

/// D-JPK-TASKRUN1: realize the project env (when present), then shell out to
/// `jet run --task <name> <entry> -- <task-args>` (D-JPK-DISPATCH1).
fn run_project_task(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    task: &str,
) -> i32 {
    // Env optional for task-only projects (no env.jet) — host PATH still works.
    // Probe the file first so a missing env doesn't print load_project_plan's
    // "nothing to do" error before we fall through to an empty Env.
    let env = if EnvFile::path_in(project_dir).is_file() {
        match load_project_plan(theme) {
            Ok(mut plan) => {
                if let Err(code) = apply_locked_channels(theme, project_dir, &mut plan.table) {
                    return code;
                }
                if let Err(code) = Trust::gate(
                    theme,
                    &Trust::store_path(),
                    project_dir,
                    &plan.refs,
                    &plan.table,
                    &plan.secrets,
                    parsed.flags.trust,
                ) {
                    return code;
                }
                if let Err(code) = validate_declared_secrets(theme, project_dir, &plan.secrets) {
                    return code;
                }
                match compose_env(theme, roots, &parsed.flags, &plan) {
                    Ok(env) => env,
                    Err(code) => return code,
                }
            }
            Err(code) => return code,
        }
    } else {
        Env {
            bin_dirs: Vec::new(),
            vars: std::collections::BTreeMap::new(),
            refs: Vec::new(),
            label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
            prompt_path: ModuleEval::PromptPathMode::default(),
            prompt_strip: ModuleEval::PromptStripMode::default(),
            cache_leases: Vec::new(),
        }
    };

    theme.status(&format!(
        "running task {} ({})",
        theme.bold(task),
        theme.gray(&entry.display().to_string())
    ));

    let mut task_args: Vec<String> = parsed.positional.iter().skip(1).cloned().collect();
    if let Some(cmd) = &parsed.command {
        task_args.extend(cmd.iter().cloned());
    }

    let mut argv = vec![
        find_jet_binary(),
        "run".to_string(),
        format!("--task={task}"),
        entry.to_string_lossy().into_owned(),
    ];
    if !task_args.is_empty() {
        argv.push("--".to_string());
        argv.extend(task_args);
    }
    let code = Shell::run_command(&env, &argv);
    if code == 0 {
        auto_clean_after_success(theme, roots);
    }
    code
}

fn run_visible_command(theme: &Theme, env: &Env, refs: &[RefSpec::RefSpec], cmd: &[String]) -> i32 {
    if let Some(program) = cmd.first() {
        let ref_label = refs
            .first()
            .map(|r| r.raw.as_str())
            .unwrap_or("project env");
        let arg_note = if cmd.len() == 1 { " (no args)" } else { "" };
        theme.status(&format!(
            "running {} -> {}{}",
            theme.bold(ref_label),
            theme.bold(program),
            theme.gray(arg_note)
        ));
    }
    Shell::run_command(env, cmd)
}

/// `jetpack enter [-- cmd]` — realize the project environment and drop into its
/// shell (Scale-2; U §8). Unlike `run`, `enter` is project-scoped: it never
/// takes an explicit ref, it always composes the env declared by the project
/// `env.jet`. The `-- cmd` form runs a one-off command in that env, then exits.
///
/// U16 additions: `-p <pkg>...` folds ad-hoc nixpkgs packages into the plan
/// (same trust gate, same realize path, as a manifest ref); `--flake` forces
/// (and the absence of any declared `env.*` module otherwise triggers) a
/// foreign `flake.nix`/`devenv.nix` fallback that shells straight to `nix
/// develop` instead of composing jetpack's own env.
pub(super) fn cmd_enter(theme: &Theme, parsed: &Parsed) -> i32 {
    // D-ENVHOOK1=A: `jet env hook <shell>` / `jet env export <shell>` route
    // through `jetpack enter` (D-JPK-DISPATCH1) as reserved first-positional
    // subverbs of `jet env`. The bare `jet env` shell-entry (no positional, or
    // a `-p`/`--flake`/`-- cmd` form) is untouched.
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::ENV_HOOK_VERB => return cmd_env_hook(theme, parsed),
        Some(v) if v == Syntax::ENV_EXPORT_VERB => return cmd_env_export(theme, parsed),
        _ => {}
    }

    let project_dir = std::env::current_dir().unwrap_or_default();

    // U16: a project's own `env.*` always wins; the foreign-flake fallback
    // only kicks in when it declares none, or when `--flake` forces it. An
    // explicit `-p` request is an active signal of intent and must never be
    // silently discarded by the passive auto-detect fallback — only
    // `--flake` (an equally explicit signal) can still force the foreign
    // shell over ad-hoc packages.
    let foreign = foreign_flake_path(&project_dir);
    let auto_detect_wants_foreign = foreign.is_some()
        && !project_declares_env(&project_dir)
        && parsed.flags.packages.is_empty();
    if parsed.flags.flake || auto_detect_wants_foreign {
        let Some(flake_path) = foreign else {
            theme.error(
                "no foreign flake here",
                &format!(
                    "`--flake` was passed but no {}/{} was found in this directory.",
                    Syntax::FOREIGN_FLAKE_FILE,
                    Syntax::FOREIGN_DEVENV_FILE
                ),
                "remove --flake to use the project's own env.*, or add a flake.nix.",
            );
            return 2;
        };
        return enter_foreign_flake(theme, &project_dir, &flake_path, parsed);
    }

    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
    }

    // U16: `-p` needs no manifest at all — a project with no `env.jet` and at
    // least one ad-hoc package still gets a (package-only) shell instead of
    // the usual "nothing to do" refusal, which only applies when neither is
    // present.
    let has_env_file = EnvFile::path_in(&project_dir).is_file();
    let mut plan = if has_env_file || parsed.flags.packages.is_empty() {
        match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        }
    } else {
        RunPlan {
            refs: Vec::new(),
            adapters: Vec::new(),
            table: RefSpec::SourceTable::empty(),
            label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
            prompt_path: ModuleEval::PromptPathMode::default(),
            prompt_strip: ModuleEval::PromptStripMode::default(),
            dev_services: Vec::new(),
            secrets: Vec::new(),
        }
    };

    // U16: ad-hoc `-p` packages become ordinary nixpkgs `RefSpec`s, folded
    // into the same plan as any manifest-declared package — same realize
    // path, same trust gate, no separate machinery.
    for name in &parsed.flags.packages {
        plan.refs.push(RefSpec::RefSpec {
            source: RefSpec::Source::Nixpkgs,
            package: name.clone(),
            raw: format!(
                "{}{}{}",
                name,
                Syntax::REF_PROVIDER_AT,
                Syntax::REF_SOURCE_NIXPKGS
            ),
        });
    }
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }

    // U19: `jet env` never runs a project function (the invariant this card
    // confirms), but it DOES realize the project's own declared packages —
    // first entry to a repo whose env is trust-sensitive gates on it.
    if let Err(code) = Trust::gate(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        parsed.flags.trust,
    ) {
        return code;
    }

    if let Err(code) = validate_declared_secrets(theme, &project_dir, &plan.secrets) {
        return code;
    }

    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };

    let code = match &parsed.command {
        Some(cmd) if !cmd.is_empty() => Shell::run_command(&env, cmd),
        _ => Shell::enter(theme, &env, ShellKind::detect()),
    };
    if code == 0 {
        auto_clean_after_success(theme, &roots);
    }
    code
}

/// D-ENVHOOK1=A: `jet env hook <shell>` — print the opt-in shell hook the user
/// installs once. Pure text (no realize, no trust): installing the hook is a
/// safe editor action; the trust gate only fires later, on the first activation
/// of an untrusted env.
fn cmd_env_hook(theme: &Theme, parsed: &Parsed) -> i32 {
    match EnvHook::parse_shell(parsed.positional.get(1).map(String::as_str)) {
        Some(kind) => {
            print!("{}", EnvHook::render_hook(kind));
            0
        }
        None => {
            theme.error(
                "unknown shell for `jet env hook`",
                &format!(
                    "the auto-activation hook is available for: {}.",
                    Syntax::ENV_HOOK_SHELLS.join(", ")
                ),
                "try `jet env hook bash`, `jet env hook zsh`, or `jet env hook fish`.",
            );
            2
        }
    }
}

/// D-ENVHOOK1=A: `jet env export <shell>` — the hook's private per-prompt
/// callback. Emits (to stdout) the shell statements that load the nearest
/// `env.jet` into the current shell, or unload it when the shell has left that
/// directory tree. Realize/trust/compose reuse the exact same path as `jet env`
/// (`compose_env` + `Trust::gate`), so there is one env engine (I8). All
/// human-facing output (ledger rows, the trust prompt) goes to stderr via
/// `Theme`; stdout carries only shell code for the hook to `eval`.
fn cmd_env_export(theme: &Theme, parsed: &Parsed) -> i32 {
    use std::io::IsTerminal;

    let Some(kind) = EnvHook::parse_shell(parsed.positional.get(1).map(String::as_str)) else {
        // An unknown shell from an installed hook is not worth a diagnostic on
        // every prompt — emit nothing and let the shell keep its environment.
        return 0;
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let disabled = std::env::var_os(Syntax::ENV_DISABLE_VAR)
        .is_some_and(|v| !v.is_empty());
    let target = if disabled {
        None
    } else {
        EnvHook::find_env_root(&cwd)
    };
    let target_s = target.as_ref().map(|p| p.to_string_lossy().into_owned());
    let active_s = std::env::var(Syntax::ENV_HOOK_ACTIVE_DIR_VAR)
        .ok()
        .filter(|s| !s.is_empty());

    // Nothing changed since the last prompt — stay silent so the hook is a
    // no-op on the vast majority of prompts (and never re-realizes).
    if target_s == active_s {
        return 0;
    }

    // The PATH to restore on unload / build on top of when activating: the
    // saved pre-env PATH if an env is currently live, else the live PATH.
    let base_path = if active_s.is_some() {
        std::env::var(Syntax::ENV_HOOK_OLD_PATH_VAR)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default())
    } else {
        std::env::var("PATH").unwrap_or_default()
    };

    let mut script = String::new();
    if active_s.is_some() {
        script.push_str(&EnvHook::render_unload(kind, &base_path));
    }

    if let Some(root_s) = &target_s {
        let root = PathBuf::from(root_s);
        // Realize the target env with `root` as cwd so the existing
        // cwd-relative plan/realize path composes it exactly like `jet env`
        // would from inside it. This process exits immediately after, so
        // changing its own cwd affects nothing else.
        let _ = std::env::set_current_dir(&root);
        let roots = Store::resolve();
        let mut plan = match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(_) => {
                // Malformed / foreign-only env here: don't activate, but the
                // unload (if any) still stands.
                print!("{script}");
                return 0;
            }
        };
        if apply_locked_channels(theme, &root, &mut plan.table).is_err() {
            print!("{script}");
            return 0;
        }

        // D-JPK-GRANTCMD1 trust law: the first activation of an untrusted,
        // trust-sensitive env prompts (interactive) or is refused with a hint
        // (non-interactive). A trusted or non-sensitive env activates silently.
        let store = Trust::store_path();
        let hash =
            Trust::env_definition_hash(&plan.refs, &plan.table, &plan.secrets);
        let sensitive =
            Trust::is_trust_sensitive_ext(&plan.refs, !plan.secrets.is_empty());
        let trusted = !sensitive
            || Trust::is_env_trusted(&store, &root, &hash, &plan.refs, &plan.secrets);
        if !trusted {
            if std::io::stdin().is_terminal() {
                if Trust::gate(
                    theme,
                    &store,
                    &root,
                    &plan.refs,
                    &plan.table,
                    &plan.secrets,
                    parsed.flags.trust,
                )
                .is_err()
                {
                    print!("{script}");
                    return 0;
                }
            } else {
                theme.detail(&format!(
                    "{} here is not trusted — run `jet env` to approve it once",
                    Syntax::ENV_FILE
                ));
                print!("{script}");
                return 0;
            }
        }

        let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
            Ok(env) => env,
            Err(_) => {
                print!("{script}");
                return 0;
            }
        };
        let composed_path = env.composed_path(&base_path);
        script.push_str(&EnvHook::render_activate(
            kind,
            &EnvHook::Activation {
                base_path,
                composed_path,
                refs: env.refs.join(" "),
                root: root_s.clone(),
            },
        ));
    }

    print!("{script}");
    0
}

/// The foreign flake/devenv file in `dir`, if either exists. `flake.nix` wins
/// when both are present (Nix's own name for the concept jetpack is bridging
/// from; `devenv.nix` is devenv's flake-backed variant of the same file).
pub(super) fn foreign_flake_path(dir: &Path) -> Option<PathBuf> {
    let flake = dir.join(Syntax::FOREIGN_FLAKE_FILE);
    if flake.is_file() {
        return Some(flake);
    }
    let devenv = dir.join(Syntax::FOREIGN_DEVENV_FILE);
    if devenv.is_file() {
        return Some(devenv);
    }
    None
}

/// Whether the project's own manifest already declares an environment —
/// either the typed `module env.*` surface or the Phase-1 `pkg.*` directive
/// surface. U16's foreign-flake auto-detection only fires when this is
/// false; a malformed typed surface still counts as "has env" (its author
/// clearly meant to declare one, so this never masks that error by silently
/// falling through to a foreign flake instead).
pub(super) fn project_declares_env(dir: &Path) -> bool {
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(dir)) else {
        return false;
    };
    if ModuleEval::is_module_surface(&src) {
        return ModuleEval::evaluate_env(&src, dir)
            .map(|p| {
                !p.package_refs.is_empty()
                    || p.prompt.is_some()
                    || !p.dev_services.is_empty()
                    || !p.secrets.is_empty()
            })
            .unwrap_or(true);
    }
    let ef = EnvFile::parse(&src);
    !ef.packages.is_empty() || ef.default_source.is_some() || !ef.named.is_empty()
}

/// U16: enter a foreign flake's default devShell directly through `nix
/// develop` — the ratified stopgap (jetpack never parses/composes a foreign
/// flake's devShell itself; `jetpack bridge flake` is the best-effort
/// translator for users who want to adopt it as `env.*` instead). Gated on
/// the same trust store as a declared env, keyed on the flake's content
/// (`Trust::gate_flake`) since arbitrary flake.nix text is untrusted input
/// the moment jetpack shells out to it.
fn enter_foreign_flake(
    theme: &Theme,
    project_dir: &Path,
    flake_path: &Path,
    parsed: &Parsed,
) -> i32 {
    if !Provider::nix_on_path() {
        theme.error_coded(
            "E1256",
            "this project's foreign flake needs `nix`, which isn't on PATH",
            "`jet env`'s foreign-flake fallback (U16) shells out to `nix develop`, the ratified \
             stopgap; without `nix` there's no way to enter that shell.",
            "install Nix from the official installer, or declare packages in env.* instead.",
        );
        return 2;
    }
    if let Err(code) = Trust::gate_flake(
        theme,
        &Trust::store_path(),
        project_dir,
        flake_path,
        parsed.flags.trust,
    ) {
        return code;
    }
    theme.status(&format!(
        "entering foreign flake shell: {}",
        theme.bold(&flake_path.display().to_string())
    ));
    let flake_dir = flake_path.parent().unwrap_or(project_dir);
    let mut args = vec![flake_dir.display().to_string()];
    if parsed.flags.pure {
        args.push(Syntax::ENV_FLAG_PURE.to_string());
    }
    // A foreign flake's devShell is a real Nix shell underneath, but it must
    // never be indistinguishable from a bare `nix develop` — brand the
    // interactive case the same way the native path does (Shell::enter),
    // so `jet env` always looks like `jet env`. A one-off `-- cmd` is
    // non-interactive and needs no prompt.
    let branded = if parsed.command.as_ref().is_none_or(|c| c.is_empty()) {
        let b = Shell::branded_shell(ShellKind::detect(), Syntax::JETPACK_PROMPT_LABEL);
        args.push("--command".to_string());
        args.extend(b.command_tail.clone());
        Some(b)
    } else {
        args.push("--command".to_string());
        args.extend(parsed.command.clone().unwrap_or_default());
        None
    };
    let mut cmd = std::process::Command::new("nix");
    cmd.arg("develop").args(&args);
    if theme.color {
        cmd.env_remove("NO_COLOR");
    } else {
        cmd.env("NO_COLOR", "");
    }
    if let Some(b) = &branded {
        for (k, v) in &b.env_vars {
            cmd.env(k, v);
        }
        // Interactive: the same threshold rule the native path draws, so a
        // foreign flake's shell still unmistakably reads as `jet env`.
        let file = flake_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        theme.rule(&[
            Syntax::JETPACK_PROMPT_LABEL,
            &format!("foreign {file} shell"),
            "exit to leave",
        ]);
    }
    let result = cmd.status();
    if let Some(b) = &branded {
        b.cleanup();
        theme.rule(&[
            &format!("left {}", Syntax::JETPACK_PROMPT_LABEL),
            "your machine is unchanged",
        ]);
    }
    match result {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            theme.error(
                "couldn't run `nix develop`",
                &e.to_string(),
                "check that `nix` is installed and on PATH.",
            );
            1
        }
    }
}

/// `jetpack dev` — U19 project-level dev (distinct from the already-shipped
/// `jet dev <file.jet>` file-watch interpreter loop, D-DEV4, which this never
/// touches). Realizes the project's declared env — today `load_project_plan`
/// already merges every `env.*` contribution into one plan, which is
/// `env(base + env.dev)` for the common case of a project that only declares
/// `module env.dev { … }` — gates on trust, waits for services (U12 is
/// unimplemented; see `wait_for_services_ready`), then runs the project's
/// `fn dev()` or falls back to `fn run()` by re-invoking `jet dev <entry>`
/// inside the composed env. Running Jet source is the compiler's job, never
/// jetpack's (D-JPK-DISPATCH1) — this shells out to the sibling `jet` binary
/// exactly the way `-- cmd` already shells out to an arbitrary command.
pub(super) fn cmd_dev(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
    }

    let project_dir = std::env::current_dir().unwrap_or_default();
    let entry = find_project_entry(&project_dir);
    if !has_dev_or_run_entry(&entry) {
        theme.error_coded(
            "E1254",
            "this project has no `jet dev` entry",
            &format!(
                "`jet dev` runs the entry file's top-level `fn dev()` if it defines one, else \
                 `fn run()` (U19); `{}` defines neither",
                entry.display()
            ),
            "add `fn dev() { … }` (a custom dev command) or `fn run() { … }` (the default) to the entry file.",
        );
        return 2;
    }

    let mut plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }

    if let Err(code) = Trust::gate(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        parsed.flags.trust,
    ) {
        return code;
    }

    if let Err(code) = validate_declared_secrets(theme, &project_dir, &plan.secrets) {
        return code;
    }

    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };

    if let Err(code) = wait_for_services_ready(theme, &project_dir, &env, &plan.dev_services) {
        return code;
    }

    theme.status(&format!(
        "running {}",
        theme.bold(&entry.display().to_string())
    ));
    let mut cmd = vec![
        find_jet_binary(),
        Syntax::DEV_SUBCOMMAND.to_string(),
        entry.to_string_lossy().into_owned(),
    ];
    // Any leftover positional token (e.g. `--watch=off`) is a flag `jet dev
    // <file>` itself understands — bare `jetpack dev` takes no file argument
    // of its own, so everything here is pass-through.
    cmd.extend(parsed.positional.iter().cloned());
    Shell::run_command(&env, &cmd)
}
