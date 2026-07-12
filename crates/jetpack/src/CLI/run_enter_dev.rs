use super::package_hangar_vendor::auto_clean_after_success;
use super::parse::Parsed;
use super::realize::{apply_locked_channels, classify_or_report, load_project_plan, RunPlan};
use super::services_secrets_config::{
    find_jet_binary, find_project_entry, has_dev_or_run_entry, validate_declared_secrets,
    wait_for_services_ready,
};
use super::trust_env_build::compose_env;
use super::workspace_sources::cwd_table;
use crate::EnvFile;
use crate::ModuleEval;
use crate::Output::Theme;
use crate::Provider;
use crate::RefSpec;
use crate::Shell::{self, Env, ShellKind};
use crate::Store;
use crate::Syntax;
use crate::Trust;
use std::path::{Path, PathBuf};

/// `jetpack run [<ref>] [-- cmd…]`
pub(super) fn cmd_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
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
            Err(_) => return 2,
        },
        None => match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
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
                Syntax::REF_SOURCE_NIXPKGS,
                Syntax::REF_SEPARATOR,
                name
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
