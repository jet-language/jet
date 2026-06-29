//! Jetpack Phase 1 command dispatch (D-JPK2/9).
//!
//! `jetpack run/build/list/clean/add/remove`. Independent from the `jet`
//! binary (D-JPK1). All user-facing output flows through `Output::Theme`.

use super::ManifestTOML;
use super::Output::{self, Theme};
use super::Provider::{self, ProviderError};
use super::RefSpec::{self, ProviderKind};
use super::Shell::{self, Env, ShellKind};
use super::Store::{self, Roots};
use super::{EnvFile, ModuleEval, RefSpec::RefError, WorkspaceFile, WorkspaceLock};
use crate::Syntax;
use std::path::{Path, PathBuf};

/// Parsed global flags shared by every command.
struct Flags {
    no_color: bool,
    fixtures: Option<PathBuf>,
    offline: bool,
}

/// Result of separating flags, positional args, and a trailing `-- cmd`.
struct Parsed {
    flags: Flags,
    positional: Vec<String>,
    /// Everything after a `--`, if present.
    command: Option<Vec<String>>,
}

fn parse_args(args: &[String]) -> Parsed {
    let mut flags = Flags {
        no_color: false,
        fixtures: None,
        offline: false,
    };
    let mut positional = Vec::new();
    let mut command = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            command = Some(args[i + 1..].to_vec());
            break;
        }
        match a.as_str() {
            "--no-color" => flags.no_color = true,
            "--offline" => flags.offline = true,
            "--fixtures" => {
                i += 1;
                if let Some(dir) = args.get(i) {
                    flags.fixtures = Some(PathBuf::from(dir));
                }
            }
            _ => positional.push(a.clone()),
        }
        i += 1;
    }
    Parsed {
        flags,
        positional,
        command,
    }
}

/// Entry point. Returns a process exit code.
pub fn main(args: Vec<String>) -> i32 {
    let Some((verb, rest)) = args.split_first() else {
        eprintln!("{}", usage());
        return 2;
    };
    let parsed = parse_args(rest);
    let theme = Theme::resolve(parsed.flags.no_color);

    match verb.as_str() {
        "run" => cmd_run(&theme, &parsed),
        "enter" => cmd_enter(&theme, &parsed),
        "build" => cmd_build(&theme, &parsed),
        "list" => cmd_list(&theme),
        "clean" => cmd_clean(&theme),
        "add" => cmd_add(&theme, &parsed),
        "remove" => cmd_remove(&theme, &parsed),
        v if v == Syntax::OS_SUBCOMMAND => cmd_os(&theme, &parsed),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            0
        }
        other => {
            theme.error(
                &format!("`{other}` is not a jetpack command"),
                &format!(
                    "Phase 1 commands are: {}.",
                    Syntax::JETPACK_VERBS.join(", ")
                ),
                "run `jetpack help` to see them.",
            );
            2
        }
    }
}

/// Resolve the fixtures dir (explicit flag, env, or none). `--offline` only
/// requires fixtures for Nix-backed refs; core refs use the source cache.
fn fixtures_for(flags: &Flags) -> Option<PathBuf> {
    Provider::fixtures_from_env(flags.fixtures.clone())
}

/// Load and evaluate `workspace.jet` from `dir`, emit workspace entries into
/// `.jet/lock`, and return the `WorkspacePlan`. Returns `None` when the file is absent. Prints
/// the diagnostic to stderr and returns `Err(2)` if the file exists but fails
/// to evaluate (D-WORKSPACE1=B clean break: workspace.jet is the sole index).
pub fn load_workspace(dir: &Path) -> Option<Result<WorkspaceFile::WorkspacePlan, i32>> {
    let result = WorkspaceFile::load(dir)?;
    match result {
        Ok(plan) => {
            // Best-effort: write the generated lock for external tools.
            WorkspaceLock::write(dir, &plan);
            Some(Ok(plan))
        }
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    Syntax::WORKSPACE_FILE,
                    "",
                    std::slice::from_ref(&d)
                )
            );
            Some(Err(2))
        }
    }
}

/// Load `[sources]` from `jetpack.toml` in `dir`, print any parse errors, and
/// return the resulting `SourceTable`. Returns an empty table when the file is
/// absent (not an error). Prints E1214/E1215 to stderr and returns `Err(2)` if
/// the file exists but has errors; the non-error entries are still returned so
/// the caller can decide whether to hard-exit or soft-degrade.
fn load_toml_sources(dir: &Path) -> Result<RefSpec::SourceTable, (RefSpec::SourceTable, i32)> {
    let Some((manifest, errors)) = ManifestTOML::load(dir) else {
        return Ok(RefSpec::SourceTable::empty());
    };

    // Convert `provider@target` entries in `[sources]` to SourceTable decls.
    // We use `Infer` as the provider kind so U9 inference runs at realize time,
    // matching the typed `module { … }` surface behaviour.
    let decls = manifest.sources.into_iter().filter_map(|(name, raw_ref)| {
        match RefSpec::classify_provider_ref(&raw_ref) {
            Ok(pr) => {
                let upstream = format!("{}:{}", pr.provider.label(), pr.target);
                Some((name, upstream, ProviderKind::Infer))
            }
            Err(_) => None, // malformed ref: skip silently (E1214 covers the line)
        }
    });
    let table = RefSpec::SourceTable::from_decls(decls);

    if errors.is_empty() {
        Ok(table)
    } else {
        let rendered = ManifestTOML::render_errors(Syntax::JETPACK_TOML, &errors);
        eprint!("{}", rendered);
        Err((table, 2))
    }
}

/// The named-source table declared by the current project's env file (empty
/// when there is none). Used so explicit CLI refs are project-aware.
/// Also merges any `[sources]` declared in `jetpack.toml` (additive — env.jet
/// inline declarations win on conflict).
fn cwd_table() -> RefSpec::SourceTable {
    let dir = std::env::current_dir().unwrap_or_default();
    let mut table = EnvFile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(RefSpec::SourceTable::empty);
    // Merge jetpack.toml [sources] as defaults (non-overriding).
    // Ignore parse errors here — cwd_table is used for explicit CLI refs;
    // load_project_plan handles the hard-exit case for project-scoped commands.
    let toml_table = match load_toml_sources(&dir) {
        Ok(t) | Err((t, _)) => t,
    };
    table.merge_defaults(toml_table);
    table
}

/// Classify an explicit CLI ref, accepting any named source declared in the
/// current project's env file so `jetpack run stable:ripgrep` works there.
/// Prints the diagnostic on failure.
fn classify_or_report(theme: &Theme, raw: &str) -> Result<RefSpec::RefSpec, RefError> {
    RefSpec::classify_in(raw, &cwd_table()).map_err(|e| {
        Output::ref_error(theme, &e);
        e
    })
}

/// Realize one ref, recording it in the store and printing progress. `table`
/// resolves named sources (D-JPK17); it is empty for direct CLI refs.
fn realize_ref(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
) -> Option<Store::StoreEntry> {
    theme.status(&format!("resolving {} …", theme.bold(&spec.raw)));
    // The provider writes store/source-cache records under the hangar (U2). The
    // store dir also seeds the U9 remote probe's source-cache lookup, so it is
    // resolved before the fixtures decision below.
    let store_dir = roots.hangar_dir();
    // Fixtures are a testing/offline mechanism only. They never override real
    // resolution: a stray `JETPACK_FIXTURES` in the environment must not
    // silently force fixture mode for an ordinary online run. The provider check
    // resolves an inferred `github@…` source's kind (U9) so a `core` source is
    // not mistakenly asked for nix fixtures.
    let fixtures =
        if flags.offline && Provider::uses_nix_provider(spec, table, flags.offline, &store_dir) {
            let fx = fixtures_for(flags);
            if fx.is_none() {
                theme.error(
                    "offline mode needs fixtures",
                    "`--offline` was set but no fixtures directory was given.",
                    "pass `--fixtures <dir>` or set JETPACK_FIXTURES.",
                );
                return None;
            }
            fx
        } else {
            // `--fixtures <dir>` without `--offline` is still honored (explicit
            // opt-in); the bare env var alone is not.
            flags.fixtures.clone()
        };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
    };
    match Provider::realize(spec, table, &ctx) {
        Ok(r) => {
            theme.ok(&format!("{} ready", theme.bold(&r.name)));
            theme.detail(&theme.gray(&r.out));
            match Store::record(
                roots,
                &r.name,
                &r.version,
                &r.reference,
                &r.out,
                &r.bin,
                &r.rlib,
            ) {
                Ok(entry) => Some(entry),
                Err(e) => {
                    theme.error(
                        "could not record the package",
                        &format!("writing to the Jetpack store failed: {e}"),
                        "check permissions on the store root, or set JETPACK_ROOT.",
                    );
                    None
                }
            }
        }
        Err(e) => {
            report_provider_error(theme, &e);
            None
        }
    }
}

pub(super) fn report_provider_error(theme: &Theme, err: &ProviderError) {
    match err {
        ProviderError::NixMissing => theme.error(
            "couldn't run `nix`",
            "This package comes from the Nix provider, but `nix` isn't on your PATH.",
            "install Nix (https://nixos.org/download), or use a native Jetpack source.",
        ),
        ProviderError::BuildFailed(reason) => theme.error(
            "the provider failed to build that package",
            reason,
            "check the package name, e.g. `nixpkgs:fastfetch`.",
        ),
        ProviderError::BadOutput(reason) => theme.error(
            "couldn't understand the provider's output",
            reason,
            "this is likely a Jetpack bug — please report it.",
        ),
        ProviderError::FixtureMissing(path) => theme.error(
            "no offline fixture for that ref",
            &format!("expected a fixture at {}", path.display()),
            "drop a captured `nix build --json` file there, or run online.",
        ),
        ProviderError::Unsupported(reason) => theme.error(
            "that source can't be realized yet",
            reason,
            "for now use a `nixpkgs:`/`github:` ref while the native builder lands.",
        ),
        ProviderError::CoreBuild(reason) => theme.error(
            "couldn't build that Jet package",
            reason,
            "check the package name and that its source repo has an env.jet.",
        ),
    }
}

/// The refs to realize, the table that resolves their named sources, and the
/// prompt label for the resulting shell.
struct RunPlan {
    refs: Vec<RefSpec::RefSpec>,
    table: RefSpec::SourceTable,
    label: String,
}

/// Build a plan from the project `env.jet` (the no-explicit-ref path). `Err`
/// carries the exit code to return.
fn load_project_plan(theme: &Theme) -> Result<RunPlan, i32> {
    let dir = std::env::current_dir().unwrap_or_default();

    // Load jetpack.toml [sources] first so they are available as defaults.
    // If the file exists but is malformed, surface the diagnostics and bail out.
    let toml_table = match load_toml_sources(&dir) {
        Ok(t) => t,
        Err((_, code)) => return Err(code),
    };

    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "nothing to do",
            &format!(
                "no ref was given and there is no {} here.",
                Syntax::ENV_FILE
            ),
            "try `jetpack run nixpkgs:fastfetch`, or `jetpack add <ref>` first.",
        );
        return Err(2);
    };

    // Two author surfaces share one file. The typed `module { … }` surface
    // (U3/U6/U8) is evaluated through `modeval`; the Phase-1 `pkg.*` directive
    // surface stays the fallback until the typed example fully replaces it.
    if ModuleEval::is_module_surface(&src) {
        return typed_plan_with_defaults(theme, &src, &dir, toml_table);
    }

    let ef = EnvFile::parse(&src);
    let mut table = ef.source_table();
    // Fold jetpack.toml sources as defaults (env.jet inline declarations win).
    table.merge_defaults(toml_table);
    let refs = classify_all(theme, ef.refs().iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        refs,
        table,
        label: ef.prompt_label(),
    })
}

/// Evaluate the typed `module { … }` env surface (U3/U6/U8) into a plan,
/// optionally seeding `jetpack.toml` [sources] as defaults. Source refs merge
/// across modules and `Pkg` sugar resolves to `<source>:<package>` refs; the
/// merged `prompt` becomes the shell label.
fn typed_plan_with_defaults(
    theme: &Theme,
    src: &str,
    dir: &Path,
    toml_defaults: RefSpec::SourceTable,
) -> Result<RunPlan, i32> {
    let plan = ModuleEval::evaluate_env(src, dir).map_err(|d| {
        eprint!(
            "{}",
            crate::Diagnostics::render_all(Syntax::ENV_FILE, src, std::slice::from_ref(&d))
        );
        2
    })?;
    let mut table = plan.table;
    table.merge_defaults(toml_defaults);
    let refs = classify_all(theme, plan.package_refs.iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        refs,
        table,
        label: plan
            .prompt
            .unwrap_or_else(|| Syntax::JETPACK_PROMPT_LABEL.to_string()),
    })
}

/// Classify a sequence of `<source>:<package>` refs against `table`, printing
/// the first failure and returning exit code 2.
fn classify_all<'a>(
    theme: &Theme,
    raws: impl Iterator<Item = &'a str>,
    table: &RefSpec::SourceTable,
) -> Result<Vec<RefSpec::RefSpec>, i32> {
    let mut refs = Vec::new();
    for raw in raws {
        match RefSpec::classify_in(raw, table) {
            Ok(s) => refs.push(s),
            Err(e) => {
                Output::ref_error(theme, &e);
                return Err(2);
            }
        }
    }
    Ok(refs)
}

/// `jetpack run [<ref>] [-- cmd…]`
fn cmd_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "dev mode: using {} (no write access to {})",
            roots.root.display(),
            "/etc/jet"
        )));
    }

    // Collect the refs to realize plus the source table that resolves any
    // named sources: an explicit CLI ref (built-ins only), or the project pack.
    let plan = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(spec) => RunPlan {
                refs: vec![spec],
                table: cwd_table(),
                label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
            },
            Err(_) => return 2,
        },
        None => match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };

    let Some(env) = compose_env(theme, &roots, &parsed.flags, &plan) else {
        return 1;
    };

    match &parsed.command {
        Some(cmd) if !cmd.is_empty() => Shell::run_command(&env, cmd),
        _ => Shell::enter(theme, &env, ShellKind::detect()),
    }
}

/// `jetpack enter [-- cmd]` — realize the project environment and drop into its
/// shell (Scale-2; U §8). Unlike `run`, `enter` is project-scoped: it never
/// takes an explicit ref, it always composes the env declared by the project
/// `env.jet`. The `-- cmd` form runs a one-off command in that env, then exits.
fn cmd_enter(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "dev mode: using {} (no write access to {})",
            roots.root.display(),
            "/etc/jet"
        )));
    }

    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let Some(env) = compose_env(theme, &roots, &parsed.flags, &plan) else {
        return 1;
    };

    match &parsed.command {
        Some(cmd) if !cmd.is_empty() => Shell::run_command(&env, cmd),
        _ => Shell::enter(theme, &env, ShellKind::detect()),
    }
}

/// Realize every ref in `plan` and compose the shell env (PATH dirs + prompt
/// label). Returns `None` after reporting if any ref fails to realize.
fn compose_env(theme: &Theme, roots: &Roots, flags: &Flags, plan: &RunPlan) -> Option<Env> {
    let mut bin_dirs = Vec::new();
    let mut realized_refs = Vec::new();
    for spec in &plan.refs {
        let entry = realize_ref(theme, roots, flags, &plan.table, spec)?;
        // A `library` package realizes with an empty `bin` (U10) — it stages
        // source for import and contributes nothing to PATH.
        if !entry.bin.is_empty() {
            bin_dirs.push(entry.bin);
        }
        realized_refs.push(entry.reference);
    }
    Some(Env {
        bin_dirs,
        refs: realized_refs,
        label: plan.label.clone(),
    })
}

/// `jetpack build [<ref>]` — realize without entering a shell.
fn cmd_build(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let dir = std::env::current_dir().unwrap_or_default();

    // D-WORKSPACE1=B: if workspace.jet is present, build all workspace members
    // via the first-party core provider (no Nix required).
    if dir.join(Syntax::WORKSPACE_FILE).exists() {
        if let Some(result) = load_workspace(&dir) {
            return match result {
                Err(code) => code,
                Ok(plan) => {
                    let mut ok = true;
                    for member in &plan.members {
                        let abs = if std::path::Path::new(&member.path).is_absolute() {
                            std::path::PathBuf::from(&member.path)
                        } else {
                            dir.join(&member.path)
                        };
                        theme.status(&format!("building workspace member: {}", member.name));
                        // Route the member through the core provider using its
                        // absolute local path as the upstream (source_repo handles
                        // "path:<abs>" → PathBuf directly, no Nix needed).
                        let table = RefSpec::SourceTable::from_decls([(
                            member.name.clone(),
                            format!("path:{}", abs.display()),
                            ProviderKind::Core,
                        )]);
                        let raw = format!("{}:{}", member.name, member.name);
                        let spec = match RefSpec::classify_in(&raw, &table) {
                            Ok(s) => s,
                            Err(e) => {
                                Output::ref_error(theme, &e);
                                ok = false;
                                continue;
                            }
                        };
                        if realize_ref(theme, &roots, &parsed.flags, &table, &spec).is_none() {
                            ok = false;
                        }
                    }
                    if ok {
                        theme.status(&format!(
                            "built {} workspace member(s).",
                            plan.members.len()
                        ));
                        0
                    } else {
                        1
                    }
                }
            };
        }
    }

    let (refs, table) = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(s) => (vec![s], cwd_table()),
            Err(_) => return 2,
        },
        None => match load_project_plan(theme) {
            Ok(plan) => (plan.refs, plan.table),
            Err(code) => return code,
        },
    };

    let mut ok = true;
    for spec in &refs {
        if realize_ref(theme, &roots, &parsed.flags, &table, spec).is_none() {
            ok = false;
        }
    }
    if ok {
        theme.status(&format!("built {} package(s).", refs.len()));
        0
    } else {
        1
    }
}

/// `jetpack list` — show realized store entries.
fn cmd_list(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    let entries = Store::list(&roots);
    if entries.is_empty() {
        theme.status("no realized packages yet.");
        return 0;
    }
    theme.status(&format!("{} realized package(s):", entries.len()));
    for e in entries {
        theme.detail(&format!(
            "{}  {}",
            theme.bold(&e.name),
            theme.gray(&e.reference)
        ));
    }
    0
}

/// `jetpack clean` — drop unused store records.
fn cmd_clean(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    match Store::clean(&roots) {
        Ok(n) => {
            theme.ok(&format!("removed {n} unused store record(s)"));
            0
        }
        Err(e) => {
            theme.error(
                "could not clean the store",
                &format!("{e}"),
                "check permissions on the store root.",
            );
            1
        }
    }
}

/// `jetpack add <ref>` — edit the project env file.
fn cmd_add(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "add what?",
            "`jetpack add` needs a ref to add.",
            "try `jetpack add nixpkgs:ripgrep`.",
        );
        return 2;
    };
    let dir = std::env::current_dir().unwrap_or_default();
    // Classify against the env's declared sources so `add unstable:fd` works
    // when `unstable` is already declared.
    let table = EnvFile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(RefSpec::SourceTable::empty);
    let spec = match RefSpec::classify_in(raw, &table) {
        Ok(s) => s,
        Err(e) => {
            Output::ref_error(theme, &e);
            return 2;
        }
    };
    match EnvFile::add(&dir, &spec) {
        Ok(ef) => {
            theme.ok(&format!(
                "added {} to {}",
                theme.bold(&spec.package),
                Syntax::ENV_FILE
            ));
            theme.detail(&theme.gray(&format!("now: {}", ef.packages.join(", "))));
            0
        }
        Err(e) => {
            theme.error(
                "could not edit the env file",
                &format!("{e}"),
                "check write permissions here.",
            );
            1
        }
    }
}

/// `jetpack remove <ref>` — edit the project env file.
fn cmd_remove(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "remove what?",
            "`jetpack remove` needs a ref to remove.",
            "try `jetpack remove nixpkgs:ripgrep`.",
        );
        return 2;
    };
    let dir = std::env::current_dir().unwrap_or_default();
    let table = EnvFile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(RefSpec::SourceTable::empty);
    let spec = match RefSpec::classify_in(raw, &table) {
        Ok(s) => s,
        Err(e) => {
            Output::ref_error(theme, &e);
            return 2;
        }
    };
    match EnvFile::remove(&dir, &spec) {
        Ok((_ef, true)) => {
            theme.ok(&format!(
                "removed {} from {}",
                theme.bold(&spec.package),
                Syntax::ENV_FILE
            ));
            0
        }
        Ok((_ef, false)) => {
            theme.status(&format!(
                "{} was not in {}.",
                spec.package,
                Syntax::ENV_FILE
            ));
            0
        }
        Err(e) => {
            theme.error(
                "could not edit the env file",
                &format!("{e}"),
                "check write permissions here.",
            );
            1
        }
    }
}

/// `jetpack os <verb> [<config-path>]@<host>` (U15/U16) — the jetos tier: whole
/// machine management as a subcommand group, not a separate binary. `<verb>` is
/// the first positional (`switch`/`build`); the target is the second.
fn cmd_os(theme: &Theme, parsed: &Parsed) -> i32 {
    let verb = parsed.positional.first().map(String::as_str);
    let target = parsed.positional.get(1).map(String::as_str);
    let flags = super::JetOS::OsFlags {
        fixtures: parsed.flags.fixtures.clone(),
        offline: parsed.flags.offline,
    };
    super::JetOS::main(theme, verb, target, &flags)
}

fn usage() -> String {
    let bin = Syntax::JETPACK_BINARY_NAME;
    format!(
        "\
{bin} — Jet's package manager (Phase 1)

usage:
  {bin} run   <source>:<package>        enter a temporary shell with that package
  {bin} run   <source>:<package> -- cmd run a command in that environment, then exit
  {bin} run                            enter the shell described by ./{pack}
  {bin} enter                          enter the project shell described by ./{pack}
  {bin} enter -- cmd                   run a command in the project shell, then exit
  {bin} build [<source>:<package>]     realize a package/environment, don't enter
  {bin} list                           show realized packages
  {bin} clean                          drop unused store records
  {bin} add    <source>:<package>      add a package to ./{pack}
  {bin} remove <source>:<package>      remove a package from ./{pack}
  {bin} os switch [<config>]@<host>    build + activate a machine from a config.jet
  {bin} os build  [<config>]@<host>    build a machine generation, don't activate

refs:
  nixpkgs:fastfetch                    a package from nixpkgs
  github:owner/repo                    a Jet pack repo (or a flake fallback)
  path:./my-env                        a local pack/flake directory

flags:
  --no-color                           disable colored output (also: NO_COLOR)
  --offline                            resolve from fixtures only, never network
  --fixtures <dir>                     read provider output from captured fixtures
",
        bin = bin,
        pack = Syntax::ENV_FILE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_trailing_command() {
        let args: Vec<String> = ["nixpkgs:jq", "--", "jq", "--version"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.positional, vec!["nixpkgs:jq"]);
        assert_eq!(p.command, Some(vec!["jq".into(), "--version".into()]));
    }

    #[test]
    fn parses_flags() {
        let args: Vec<String> = ["--no-color", "--fixtures", "/tmp/fx", "nixpkgs:jq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert!(p.flags.no_color);
        assert_eq!(p.flags.fixtures, Some(PathBuf::from("/tmp/fx")));
        assert_eq!(p.positional, vec!["nixpkgs:jq"]);
    }
}
