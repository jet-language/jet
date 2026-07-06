//! Jetpack Phase 1 command dispatch (D-JPK2/9).
//!
//! `jetpack run/build/list/clean/add/remove`. Independent from the `jet`
//! binary (D-JPK1). All user-facing output flows through `Output::Theme`.

use super::Bridge;
use super::Components;
use super::ManifestTOML;
use super::Output::{self, Theme};
use super::Provider::{self, ProviderError};
use super::RefSpec::{self, ProviderKind};
use super::Shell::{self, Env, ShellKind};
use super::Store::{self, Roots};
use super::Trust;
use super::{EnvFile, ModuleEval, RefSpec::RefError, WorkspaceFile, WorkspaceLock};
use crate::Syntax;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Parsed global flags shared by every command.
struct Flags {
    no_color: bool,
    fixtures: Option<PathBuf>,
    offline: bool,
    /// U19: one-shot bypass of the env/dev trust gate (`--trust`). Never
    /// persists a grant — unlike accepting the interactive prompt.
    trust: bool,
    /// U16: ad-hoc nixpkgs packages from `-p <pkg>...`, added to the shell
    /// without being declared in any manifest. Repeatable across multiple
    /// `-p` groups.
    packages: Vec<String>,
    /// U16: `--flake` forces foreign-flake/devenv detection even when the
    /// project's own manifest already declares `env.*` modules.
    flake: bool,
    /// U16: `--pure` — isolate the shell from the host environment. Threaded
    /// straight through to the underlying `nix` invocation for the
    /// foreign-flake fallback; jetpack's own composed shells are already
    /// PATH-only, so this is a no-op there today.
    pure: bool,
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
        trust: false,
        packages: Vec::new(),
        flake: false,
        pure: false,
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
            a if a == Syntax::TRUST_BYPASS_FLAG => flags.trust = true,
            a if a == Syntax::ENV_FLAG_FLAKE => flags.flake = true,
            a if a == Syntax::ENV_FLAG_PURE => flags.pure = true,
            "--fixtures" => {
                i += 1;
                if let Some(dir) = args.get(i) {
                    flags.fixtures = Some(PathBuf::from(dir));
                }
            }
            a if a == Syntax::ENV_FLAG_PACKAGE => {
                // U16: `-p <pkg>...` greedily consumes bare tokens until the
                // next flag/`--`/end, so `-p nodejs ripgrep -- cmd` and
                // `-p nodejs -p ripgrep -- cmd` both work.
                i += 1;
                while let Some(next) = args.get(i) {
                    if next == "--" || next.starts_with('-') {
                        break;
                    }
                    flags.packages.push(next.clone());
                    i += 1;
                }
                continue;
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
        v if v == Syntax::DEV_SUBCOMMAND => cmd_dev(&theme, &parsed),
        v if v == Syntax::CONFIG_SUBCOMMAND => cmd_config(&theme, &parsed),
        "build" => cmd_build(&theme, &parsed),
        "list" => cmd_list(&theme),
        "hangar" => cmd_hangar(&theme, &parsed),
        "vendor" => cmd_vendor(&theme, &parsed),
        "audit" => cmd_audit(&theme),
        "clean" => cmd_clean(&theme),
        "add" => cmd_add(&theme, &parsed),
        "remove" => cmd_remove(&theme, &parsed),
        "push" => cmd_push(&theme, &parsed),
        v if v == Syntax::BRIDGE_SUBCOMMAND => cmd_bridge(&theme, &parsed),
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

/// The workspace member index for the current directory (Slice B). Evaluated
/// from `workspace.jet` when present (discovery-by-declaration), else read from
/// the `.jet/lock` mirror, else empty. Lets bare (`logging`) and path-form
/// (`packages/logging`) refs resolve against workspace members.
fn cwd_workspace_index() -> RefSpec::WorkspaceIndex {
    let dir = std::env::current_dir().unwrap_or_default();
    let plan = match WorkspaceFile::load(&dir) {
        Some(Ok(plan)) => Some(plan),
        // A malformed `workspace.jet` is surfaced by project-scoped commands;
        // for ref classification we fall back to the lock mirror.
        Some(Err(_)) => WorkspaceLock::load(&dir),
        None => WorkspaceLock::load(&dir),
    };
    match plan {
        Some(plan) => RefSpec::WorkspaceIndex::from_members(
            plan.members.into_iter().map(|m| (m.name, m.path)),
        ),
        None => RefSpec::WorkspaceIndex::empty(),
    }
}

/// Classify an explicit CLI ref, accepting any named source declared in the
/// current project's env file so `jetpack run stable:ripgrep` works there, and
/// any workspace member so `jetpack run logging` / `jetpack run packages/logging`
/// resolve in a monorepo (Slice B, D-MONOREF1=A). Prints the diagnostic on failure.
fn classify_or_report(theme: &Theme, raw: &str) -> Result<RefSpec::RefSpec, RefError> {
    RefSpec::classify_with_workspace(raw, &cwd_table(), &cwd_workspace_index()).map_err(|e| {
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
    name_w: usize,
) -> Option<(Store::StoreEntry, Provider::SourceState)> {
    // A TTY gets a live spinner that the final ledger row replaces; without
    // one (piped output, NO_COLOR) the plain status line stands instead.
    let spinner = if theme.color {
        Some(theme.spinner(&format!("resolving {} …", spec.raw)))
    } else {
        theme.status(&format!("resolving {} …", theme.bold(&spec.raw)));
        None
    };
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
                drop(spinner);
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
    let started = std::time::Instant::now();
    let result = Provider::realize(spec, table, &ctx);
    drop(spinner);
    match result {
        Ok(r) => {
            // T4 (D-JPK-CACHE1): one ledger row per package — how it was
            // satisfied, and how long a from-source build took.
            let elapsed = started.elapsed();
            let state = if r.source_state == Provider::SourceState::Built && elapsed.as_secs() >= 1
            {
                format!("built {}", Output::human_duration(elapsed))
            } else {
                r.source_state.label().to_string()
            };
            // Nix-provided packages often carry no version of their own; the
            // store path's `<hash>-<name>-<version>` basename usually does.
            let version = if r.version.is_empty() {
                version_from_out(&r.name, &r.out).unwrap_or_default()
            } else {
                r.version.clone()
            };
            theme.row(&r.name, name_w, &version, &state);
            theme.detail(&theme.gray(&r.out));
            let state = r.source_state;
            match Store::record(
                roots,
                &r.name,
                &r.version,
                &r.reference,
                &r.out,
                &r.bin,
                &r.rlib,
                &r.envelope,
            ) {
                Ok(entry) => Some((entry, state)),
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

/// Best-effort version out of a store-path basename like
/// `<hash>-ripgrep-14.1.0`: everything after `<name>-`, if it starts with a
/// digit (so `my-tool-src` never masquerades as a version).
fn version_from_out(name: &str, out: &str) -> Option<String> {
    let base = out.rsplit('/').next()?;
    let idx = base.find(&format!("-{name}-"))?;
    let version = &base[idx + name.len() + 2..];
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        Some(version.to_string())
    } else {
        None
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
        // E1232: sparse subtree fetch + full-clone fallback both failed.
        ProviderError::MonorepoFetch(reason) => theme.error_coded(
            "E1232",
            "couldn't fetch that monorepo source",
            reason,
            "check the source URL and revision, and that the network/provider is reachable.",
        ),
        // E1233: an in-repo dependency is outside the source's workspace index.
        ProviderError::MemberOutsideWorkspace(reason) => theme.error_coded(
            "E1233",
            "an in-repo dependency is outside the workspace",
            reason,
            "add the dependency to the source repo's `workspace.jet` `members:`, or depend on it as an external `source:package` ref.",
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
///
/// U16 additions: `-p <pkg>...` folds ad-hoc nixpkgs packages into the plan
/// (same trust gate, same realize path, as a manifest ref); `--flake` forces
/// (and the absence of any declared `env.*` module otherwise triggers) a
/// foreign `flake.nix`/`devenv.nix` fallback that shells straight to `nix
/// develop` instead of composing jetpack's own env.
fn cmd_enter(theme: &Theme, parsed: &Parsed) -> i32 {
    let project_dir = std::env::current_dir().unwrap_or_default();

    // U16: a project's own `env.*` always wins; the foreign-flake fallback
    // only kicks in when it declares none, or when `--flake` forces it. An
    // explicit `-p` request is an active signal of intent and must never be
    // silently discarded by the passive auto-detect fallback — only
    // `--flake` (an equally explicit signal) can still force the foreign
    // shell over ad-hoc packages.
    let foreign = foreign_flake_path(&project_dir);
    let auto_detect_wants_foreign =
        foreign.is_some() && !project_declares_env(&project_dir) && parsed.flags.packages.is_empty();
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
            "dev mode: using {} (no write access to {})",
            roots.root.display(),
            "/etc/jet"
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
            table: RefSpec::SourceTable::empty(),
            label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
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

    // U19: `jet env` never runs a project function (the invariant this card
    // confirms), but it DOES realize the project's own declared packages —
    // first entry to a repo whose env is trust-sensitive gates on it.
    if let Err(code) = Trust::gate(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        parsed.flags.trust,
    ) {
        return code;
    }

    let Some(env) = compose_env(theme, &roots, &parsed.flags, &plan) else {
        return 1;
    };

    match &parsed.command {
        Some(cmd) if !cmd.is_empty() => Shell::run_command(&env, cmd),
        _ => Shell::enter(theme, &env, ShellKind::detect()),
    }
}

/// The foreign flake/devenv file in `dir`, if either exists. `flake.nix` wins
/// when both are present (Nix's own name for the concept jetpack is bridging
/// from; `devenv.nix` is devenv's flake-backed variant of the same file).
fn foreign_flake_path(dir: &Path) -> Option<PathBuf> {
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
fn project_declares_env(dir: &Path) -> bool {
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(dir)) else {
        return false;
    };
    if ModuleEval::is_module_surface(&src) {
        return ModuleEval::evaluate_env(&src, dir)
            .map(|p| !p.package_refs.is_empty() || p.prompt.is_some())
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
fn enter_foreign_flake(theme: &Theme, project_dir: &Path, flake_path: &Path, parsed: &Parsed) -> i32 {
    if !Provider::nix_on_path() {
        theme.error_coded(
            "E1256",
            "this project's foreign flake needs `nix`, which isn't on PATH",
            "`jet env`'s foreign-flake fallback (U16) shells out to `nix develop`, the ratified \
             stopgap; without `nix` there's no way to enter that shell.",
            "install Nix (https://nixos.org/download), or declare packages in env.* instead.",
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
fn cmd_dev(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "dev mode: using {} (no write access to {})",
            roots.root.display(),
            "/etc/jet"
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

    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };

    if let Err(code) = Trust::gate(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        parsed.flags.trust,
    ) {
        return code;
    }

    let Some(env) = compose_env(theme, &roots, &parsed.flags, &plan) else {
        return 1;
    };

    if let Err(code) = wait_for_services_ready(&env) {
        return code;
    }

    theme.status(&format!("running {}", theme.bold(&entry.display().to_string())));
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

/// U12 (supervised services) is unimplemented — this is a deliberate no-op,
/// not a fake "waiting for services…" message. It takes the composed `Env` so
/// the day U12 ships, this gains a real health-check loop against the
/// project's `services:` and gates on it right here — the one call site
/// `jetpack dev` already runs before starting the project — with no change
/// needed at the call site itself.
fn wait_for_services_ready(_env: &Env) -> Result<(), i32> {
    Ok(())
}

/// The sibling `jet` binary next to the running `jetpack` process, falling
/// back to a bare PATH lookup. Mirrors `Source/EngineDispatch.rs`'s
/// same-directory-then-PATH search in the other direction; jetpack never
/// links the compiler in-process (D-JPK-DISPATCH1), so this is the one place
/// it hands off to it.
fn find_jet_binary() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let name = if cfg!(windows) { "jet.exe" } else { "jet" };
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    Syntax::BINARY_NAME.to_string()
}

/// The project's entry file for the bare (no-file) `jetpack dev`: `.jet/main.jet`
/// if present, else `main.jet` — the same convention `jet run`/`jet build` use
/// for a bare project (`Source/main.rs`'s `find_project_entry`). Duplicated by
/// hand rather than shared: jetpack and jet are separate binaries by design
/// (D-JPK-DISPATCH1), so deleting either still leaves the other's own commands
/// working.
fn find_project_entry(project_dir: &Path) -> PathBuf {
    let dot_jet = project_dir
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(format!("main.{}", Syntax::FILE_EXT));
    if dot_jet.is_file() {
        return dot_jet;
    }
    project_dir.join(format!("main.{}", Syntax::FILE_EXT))
}

/// Whether `file` defines a top-level `fn dev()` or `fn run()` (U19's
/// dev-with-fallback rule, E1254 otherwise). A parse failure just means "no"
/// here — the real diagnostics surface a moment later when the compiler
/// actually loads the file.
fn has_dev_or_run_entry(file: &Path) -> bool {
    let Ok(src) = std::fs::read_to_string(file) else {
        return false;
    };
    let (toks, diags) = crate::Lexer::lex(&src);
    if !diags.is_empty() {
        return false;
    }
    let Ok(prog) = crate::Parser::parse(&toks) else {
        return false;
    };
    prog.items.iter().any(|i| {
        matches!(i, crate::AST::Item::Func(f) if f.name == "dev" || f.name == "run")
    })
}

/// `jetpack config trust add/list/remove` (U19) — durable glob/prefix patterns
/// that pre-authorize matching projects with no per-hash prompt at all.
fn cmd_config(theme: &Theme, parsed: &Parsed) -> i32 {
    let group = parsed.positional.first().map(String::as_str);
    if group != Some(Syntax::CONFIG_VERB_TRUST) {
        theme.error(
            &format!("`jetpack config {}` isn't a command", group.unwrap_or("")),
            "today `jetpack config` only manages the env/dev trust store.",
            "try `jetpack config trust add <pattern>`, `list`, or `remove <pattern>`.",
        );
        return 2;
    }
    let store = Trust::store_path();
    match parsed.positional.get(1).map(String::as_str) {
        Some(v) if v == Syntax::CONFIG_TRUST_VERB_ADD => {
            let Some(pattern) = parsed.positional.get(2) else {
                theme.error(
                    "`jetpack config trust add` needs a pattern",
                    "a pattern pre-authorizes matching projects by path prefix/glob.",
                    "try `jetpack config trust add ~/work/*`.",
                );
                return 2;
            };
            let added = Trust::add_pattern(&store, pattern);
            theme.status(&if added {
                format!("trusted: {pattern}")
            } else {
                format!("already trusted: {pattern}")
            });
            0
        }
        Some(v) if v == Syntax::CONFIG_TRUST_VERB_LIST => {
            let entries = Trust::list_entries(&store);
            if entries.is_empty() {
                theme.status("no trust entries yet.");
            } else {
                for e in entries {
                    theme.detail(&e);
                }
            }
            0
        }
        Some(v) if v == Syntax::CONFIG_TRUST_VERB_REMOVE => {
            let Some(pattern) = parsed.positional.get(2) else {
                theme.error(
                    "`jetpack config trust remove` needs a pattern",
                    "removes a previously trusted path pattern.",
                    "try `jetpack config trust remove ~/work/*`.",
                );
                return 2;
            };
            let removed = Trust::remove_pattern(&store, pattern);
            theme.status(&if removed {
                format!("removed: {pattern}")
            } else {
                format!("not found: {pattern}")
            });
            0
        }
        _ => {
            theme.error(
                "`jetpack config trust` needs a verb",
                &format!(
                    "the trust verbs are: {}.",
                    Syntax::CONFIG_TRUST_VERBS.join(", ")
                ),
                "try `add <pattern>`, `list`, or `remove <pattern>`.",
            );
            2
        }
    }
}

/// Realize every ref in `plan` and compose the shell env (PATH dirs + prompt
/// label). Returns `None` after reporting if any ref fails to realize.
fn compose_env(theme: &Theme, roots: &Roots, flags: &Flags, plan: &RunPlan) -> Option<Env> {
    let mut bin_dirs = Vec::new();
    let mut realized_refs = Vec::new();
    let name_w = name_column_width(&plan.refs);
    if plan.refs.len() > 1 {
        theme.status(&format!(
            "composing {} — {} packages",
            theme.bold(&plan.label),
            plan.refs.len()
        ));
    }
    let (mut built, mut cached, mut substituted) = (0usize, 0usize, 0usize);
    for spec in &plan.refs {
        let (entry, state) = realize_ref(theme, roots, flags, &plan.table, spec, name_w)?;
        match state {
            Provider::SourceState::Built => built += 1,
            Provider::SourceState::Cached => cached += 1,
            Provider::SourceState::Substituted => substituted += 1,
        }
        // A `library` package realizes with an empty `bin` (U10) — it stages
        // source for import and contributes nothing to PATH.
        if !entry.bin.is_empty() {
            bin_dirs.push(entry.bin);
        }
        realized_refs.push(entry.reference);
    }
    if plan.refs.len() > 1 {
        theme.status(&format!(
            "env ready — {}",
            state_summary(built, cached, substituted)
        ));
    }
    Some(Env {
        bin_dirs,
        refs: realized_refs,
        label: plan.label.clone(),
    })
}

/// The ledger's name-column width for a set of refs (min 8 so a single short
/// name doesn't collapse the table).
fn name_column_width(refs: &[RefSpec::RefSpec]) -> usize {
    refs.iter().map(|r| r.package.len()).max().unwrap_or(0).max(8)
}

/// `2 built, 1 cached` — non-zero source-state counts, joined.
fn state_summary(built: usize, cached: usize, substituted: usize) -> String {
    let mut parts = Vec::new();
    if built > 0 {
        parts.push(format!("{built} built"));
    }
    if cached > 0 {
        parts.push(format!("{cached} cached"));
    }
    if substituted > 0 {
        parts.push(format!("{substituted} substituted"));
    }
    parts.join(", ")
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
                        if realize_ref(
                            theme,
                            &roots,
                            &parsed.flags,
                            &table,
                            &spec,
                            member.name.len().max(8),
                        )
                        .is_none()
                        {
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
                    // (workspace members: state is printed per-package by realize_ref)
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
    let (mut built, mut cached, mut substituted) = (0usize, 0usize, 0usize);
    let name_w = name_column_width(&refs);
    for spec in &refs {
        match realize_ref(theme, &roots, &parsed.flags, &table, spec, name_w) {
            Some((_entry, state)) => match state {
                Provider::SourceState::Built => built += 1,
                Provider::SourceState::Cached => cached += 1,
                Provider::SourceState::Substituted => substituted += 1,
            },
            None => ok = false,
        }
    }
    if ok {
        // T4: per-run source-state summary (mirrors the D-JPK-CACHE1 example).
        theme.status(&format!(
            "built {} package(s): {} built, {} cached, {} substituted",
            refs.len(),
            built,
            cached,
            substituted
        ));
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
    let name_w = entries.iter().map(|e| e.name.len()).max().unwrap_or(0).max(8);
    let ver_w = entries
        .iter()
        .map(|e| if e.version.is_empty() { 1 } else { e.version.len() })
        .max()
        .unwrap_or(1);
    for e in entries {
        let v = if e.version.is_empty() { "—" } else { &e.version };
        theme.detail(&format!(
            "{}  {}  {}",
            theme.bold(&format!("{:<name_w$}", e.name)),
            format!("{v:<ver_w$}"),
            theme.gray(&e.reference)
        ));
    }
    0
}

/// `jetpack hangar du` — honest per-object disk usage (U22 / D-JPK-GC1).
/// Source-built objects are counted like any other, so `du` never hides them.
fn cmd_hangar(theme: &Theme, parsed: &Parsed) -> i32 {
    let sub = parsed.positional.first().map(String::as_str);
    match sub {
        Some("du") | None => {
            let roots = Store::resolve();
            let entries = Store::du(&roots);
            if entries.is_empty() {
                theme.status("hangar is empty.");
                return 0;
            }
            let mut total = 0u64;
            let mut built = 0usize;
            for e in &entries {
                total += e.bytes;
                if e.source_built {
                    built += 1;
                }
                let tag = if e.source_built { " (built)" } else { "" };
                theme.detail(&format!(
                    "{:>10}  {}{}",
                    human_bytes(e.bytes),
                    theme.bold(&e.id),
                    theme.gray(tag)
                ));
            }
            theme.status(&format!(
                "{} object(s), {} built from source, {} total",
                entries.len(),
                built,
                human_bytes(total)
            ));
            0
        }
        Some(other) => {
            theme.error(
                &format!("`hangar {other}` is not a hangar command"),
                "the hangar subcommand is `du` (honest disk usage).",
                "run `jetpack hangar du`.",
            );
            2
        }
    }
}

/// Render a byte count as a short human string (B/K/M/G).
fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// `jetpack vendor [<dir>]` — write vendored + hash-pinned sources for every
/// source-built hangar object (D-BFS1 / T4). Each object's realized tree is
/// copied into `<dir>/<name>/` and a `<dir>/<name>.sha256` records the A4 output
/// hash, so a later build is reproducible offline from pinned sources.
fn cmd_vendor(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let cwd = std::env::current_dir().unwrap_or_default();
    let vendor_dir = match parsed.positional.first() {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                p
            } else {
                cwd.join(p)
            }
        }
        None => cwd.join("vendor"),
    };
    let built: Vec<_> = Store::list(&roots)
        .into_iter()
        .filter(|e| e.envelope.provenance.contains("core-"))
        .collect();
    if built.is_empty() {
        theme.status("nothing to vendor: no source-built packages in the hangar.");
        return 0;
    }
    if std::fs::create_dir_all(&vendor_dir).is_err() {
        theme.error(
            "could not create the vendor directory",
            &vendor_dir.display().to_string(),
            "check write permissions here.",
        );
        return 1;
    }
    let mut count = 0;
    for e in &built {
        let dest = vendor_dir.join(&e.name);
        let _ = std::fs::remove_dir_all(&dest);
        if copy_dir(std::path::Path::new(&e.out), &dest).is_err() {
            theme.error(
                "could not vendor a package",
                &format!("copying {} failed", e.out),
                "check disk space and permissions.",
            );
            return 1;
        }
        // Hash-pin: the A4 output hash is the reproducibility anchor.
        let pin = vendor_dir.join(format!("{}.sha256", e.name));
        let _ = std::fs::write(&pin, format!("{}\n", e.envelope.output_hash));
        theme.detail(&format!(
            "vendored {} ({})",
            theme.bold(&e.name),
            theme.gray(&e.envelope.output_hash)
        ));
        count += 1;
    }
    theme.status(&format!(
        "vendored {count} source-built package(s) with pinned hashes."
    ));
    0
}

/// `jetpack audit` — read the build provenance of every realized object
/// (D-BUILDSCOPE1 audit contract, T4): source ref + recipe id, output hash,
/// platform, and locked source hash. **Executes nothing** — a pure read of the
/// hangar records, so it is safe to run against untrusted builds.
fn cmd_audit(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    let entries = Store::list(&roots);
    if entries.is_empty() {
        theme.status("audit: hangar is empty, nothing to read.");
        return 0;
    }
    theme.status(&format!(
        "audit: {} realized object(s) (read-only, no build ran):",
        entries.len()
    ));
    for e in &entries {
        theme.detail(&format!("{}", theme.bold(&e.id)));
        theme.detail(&format!(
            "  provenance: {}",
            if e.envelope.provenance.is_empty() {
                "<none recorded>"
            } else {
                &e.envelope.provenance
            }
        ));
        theme.detail(&format!("  output-hash: {}", theme.gray(&e.envelope.output_hash)));
        theme.detail(&format!("  platform:    {}", theme.gray(&e.envelope.platform)));
    }
    0
}

/// Recursively copy a directory tree (std-only, preserves Unix modes).
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&from)?.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
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

/// `jetpack add <ref>` — edit the project env file. `jetpack add <Component>`
/// (an exact, case-sensitive match against the starter component catalog —
/// Button/Label/Input/Container) is a distinct behavior checked first: it
/// copies real `.jet` source into `./components/` instead of touching the env
/// file (Tower c134 Phase 4, the ownable component kit). The two never
/// collide because Jetpack source names are always lowercase
/// (`nixpkgs`/`github`/`path`/user-declared names), so an exact-case
/// `Button`-style name can only ever mean a component.
fn cmd_add(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "add what?",
            "`jetpack add` needs a ref or a starter component to add.",
            "try `jetpack add nixpkgs:ripgrep` or `jetpack add Button`.",
        );
        return 2;
    };
    if let Some(component) = Components::find(raw) {
        return cmd_add_component(theme, component);
    }
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

/// Copy a starter component's source into `./components/<Name>.jet`.
fn cmd_add_component(theme: &Theme, component: &Components::StarterComponent) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    match Components::add_component(&dir, component) {
        Ok(dest) => {
            theme.ok(&format!(
                "added {} to {}",
                theme.bold(component.name),
                Components::COMPONENTS_DIR
            ));
            theme.detail(&theme.gray(&format!("wrote {}", dest.display())));
            theme.detail("it's yours now — edit it freely.");
            0
        }
        Err(Components::ComponentError::AlreadyExists(path)) => {
            theme.error(
                &format!("{} already exists", path.display()),
                "it may already be customized — `jetpack add` never overwrites a component you own.",
                "edit it directly, or remove it first if you want a fresh copy.",
            );
            1
        }
        Err(e) => {
            theme.error(
                "could not add that component",
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

/// `jetpack push <fleet>` (U15) — deploy a fleet's hosts. Parses and
/// cross-checks the fleet now (each host references a known `System`, E1242);
/// the ssh/closure rollout is gated on single-host jetos realization (Phase D),
/// so a valid fleet gets an honest E1243 gated notice rather than a fake deploy.
fn cmd_push(theme: &Theme, parsed: &Parsed) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "no fleet here",
            &format!("there is no {} declaring any `fleet.<name>`.", Syntax::ENV_FILE),
            "declare `module fleet.<name> { hosts: { … } }`, then `jet push <name>`.",
        );
        return 2;
    };
    // evaluate_env parses, discovers imports, and cross-checks every fleet host
    // against the known systems (E1242) — a bad host fails here.
    let plan = match ModuleEval::evaluate_env(&src, &dir) {
        Ok(p) => p,
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(Syntax::ENV_FILE, &src, std::slice::from_ref(&d))
            );
            return 2;
        }
    };

    let available: Vec<String> = plan.fleets.iter().map(|f| f.name.clone()).collect();
    let Some(name) = parsed.positional.first() else {
        theme.error(
            "push which fleet?",
            &if available.is_empty() {
                format!("no `fleet.<name>` is declared in {}.", Syntax::ENV_FILE)
            } else {
                format!("declared fleets: {}.", available.join(", "))
            },
            "name a fleet: `jet push <fleet>`.",
        );
        return 2;
    };

    let Some(fleet) = plan.fleets.iter().find(|f| &f.name == name) else {
        theme.error(
            &format!("no fleet `{name}`"),
            &if available.is_empty() {
                format!("no `fleet.<name>` is declared in {}.", Syntax::ENV_FILE)
            } else {
                format!("declared fleets: {}.", available.join(", "))
            },
            "declare `module fleet.<name> { hosts: { … } }`, or push an existing fleet.",
        );
        return 2;
    };

    // The fleet is valid and fully captured. Deployment is gated (Phase D).
    let host_list = fleet
        .hosts
        .iter()
        .map(|h| format!("{} → system.{}", h.name, h.system))
        .collect::<Vec<_>>()
        .join(", ");
    theme.error(
        &format!("[E1243] fleet `{name}` is validated, but `jet push` is not available yet"),
        &format!(
            "the fleet's {} host(s) ({host_list}) parse and cross-check clean, but rolling a fleet out over ssh needs single-host jetos realization, which is gated (Phase D, owner greenlight required).",
            fleet.hosts.len()
        ),
        "track the jetos realization tier; until it lands, `jet push` captures and validates fleets without deploying them.",
    );
    2
}

/// `jetpack bridge flake` (U16, card c9jetpackgates) — best-effort translator
/// from a foreign `flake.nix`'s devShell into jetpack's own `env.*` module
/// form. Never edits the project's `env.jet`; the shim prints to stdout for
/// the user to review and merge (I8 — one canonical env surface).
fn cmd_bridge(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::BRIDGE_VERB_FLAKE => {
            if !Provider::nix_on_path() {
                theme.error_coded(
                    "E1256",
                    "`jet bridge flake` needs `nix`, which isn't on PATH",
                    "translating a flake.nix's devShell shells out to `nix eval` (U16); without \
                     `nix` there's nothing to read the devShell from.",
                    "install Nix (https://nixos.org/download), or write env.* by hand.",
                );
                return 2;
            }
            let dir = std::env::current_dir().unwrap_or_default();
            Bridge::cmd_flake(theme, &dir, fixtures_for(&parsed.flags).as_deref())
        }
        Some(other) => {
            theme.error(
                &format!("`jetpack bridge {other}` is not a bridge command"),
                "today `jetpack bridge` only translates `flake` (a flake.nix devShell).",
                "run `jetpack bridge flake`.",
            );
            2
        }
        None => {
            theme.error(
                "bridge what?",
                "`jetpack bridge` needs a verb.",
                "run `jetpack bridge flake`.",
            );
            2
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
    let pack = Syntax::ENV_FILE;
    // Bold section headers on a TTY only; the text is identical when piped.
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let h = |s: &str| {
        if color {
            format!("\x1b[1m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    };
    format!(
        "\
{title}

{envs}
  {bin} enter                          enter the project shell described by ./{pack}
  {bin} enter -- cmd                   run a command in the project shell, then exit
  {bin} enter -p <pkg>...              add ad-hoc nixpkgs packages, undeclared
  {bin} enter --flake                  force a foreign flake.nix/devenv.nix shell
  {bin} run   <source>:<package>       enter a temporary shell with that package
  {bin} run   <source>:<package> -- cmd run a command in that environment, then exit
  {bin} run                            enter the shell described by ./{pack}
  {bin} dev                            realize the env, then run the project's fn dev()

{manifest}
  {bin} add    <source>:<package>      add a package to ./{pack}
  {bin} add    <Component>             copy a starter component into ./components
  {bin} remove <source>:<package>      remove a package from ./{pack}
  {bin} bridge flake                   print an env.* shim translated from ./flake.nix

{store}
  {bin} build [<source>:<package>]     realize a package/environment, don't enter
  {bin} list                           show realized packages
  {bin} hangar du                      honest per-object hangar disk usage
  {bin} vendor [<dir>]                 write vendored + hash-pinned sources
  {bin} audit                          read build provenance (runs nothing)
  {bin} clean                          drop unused store records

{machines}
  {bin} os switch [<config>]@<host>    build + activate a machine from a config.jet
  {bin} os build  [<config>]@<host>    build a machine generation, don't activate
  {bin} push <fleet>                   validate a fleet's hosts (deploy is gated)

{trust}
  {bin} config trust add <pattern>     pre-authorize matching project paths
  {bin} config trust list              show trusted hashes and patterns
  {bin} config trust remove <pattern>  drop a trusted pattern

{refs}
  nixpkgs:fastfetch                    a package from nixpkgs
  github:owner/repo                    a Jet pack repo (or a flake fallback)
  path:./my-env                        a local pack/flake directory

{components}
  Button, Label, Input, Container      starter kit — ownable, editable .jet source

{flags}
  --no-color                           disable colored output (also: NO_COLOR)
  --offline                            resolve from fixtures only, never network
  --fixtures <dir>                     read provider output from captured fixtures
  --trust                              skip the trust prompt for this one run
  -p <pkg>...                          (enter) ad-hoc nixpkgs packages, not declared anywhere
  --flake                              (enter) force the foreign flake.nix/devenv.nix fallback
  --pure                               (enter) isolate the shell from the host environment
",
        title = h(&format!("{bin} — Jet's package manager (Phase 1)")),
        envs = h("environments:"),
        manifest = h("manifest:"),
        store = h("store:"),
        machines = h("machines:"),
        trust = h("trust:"),
        refs = h("refs:"),
        components = h("components:"),
        flags = h("flags:"),
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

    // ── U16: -p / --flake / --pure ──

    #[test]
    fn dash_p_collects_packages_until_dash_dash() {
        let args: Vec<String> = ["-p", "nodejs", "ripgrep", "--", "some-command"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.flags.packages, vec!["nodejs", "ripgrep"]);
        assert_eq!(p.command, Some(vec!["some-command".to_string()]));
        assert!(p.positional.is_empty());
    }

    #[test]
    fn dash_p_stops_at_next_flag() {
        let args: Vec<String> = ["-p", "nodejs", "--no-color"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.flags.packages, vec!["nodejs"]);
        assert!(p.flags.no_color);
    }

    #[test]
    fn repeated_dash_p_groups_accumulate() {
        let args: Vec<String> = ["-p", "nodejs", "-p", "ripgrep", "fd"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.flags.packages, vec!["nodejs", "ripgrep", "fd"]);
    }

    #[test]
    fn parses_flake_and_pure_flags() {
        let args: Vec<String> = ["--flake", "--pure"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert!(p.flags.flake);
        assert!(p.flags.pure);
    }

    // ── U16: foreign-flake detection ordering ──

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jetpack_cli_u16_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn foreign_flake_not_detected_without_a_flake_file() {
        let dir = scratch("no_flake");
        assert_eq!(foreign_flake_path(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn foreign_flake_prefers_flake_nix_over_devenv() {
        let dir = scratch("both");
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        std::fs::write(dir.join("devenv.nix"), "{ }").unwrap();
        assert_eq!(foreign_flake_path(&dir), Some(dir.join("flake.nix")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_declares_env_false_with_no_env_file() {
        let dir = scratch("no_env");
        assert!(!project_declares_env(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_declares_env_true_for_typed_module_with_packages() {
        let dir = scratch("typed_env");
        std::fs::write(
            dir.join(Syntax::ENV_FILE),
            "module env.dev { packages: [ripgrep] }\n",
        )
        .unwrap();
        assert!(project_declares_env(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_declares_env_true_for_phase1_directive_surface() {
        let dir = scratch("phase1_env");
        std::fs::write(
            dir.join(Syntax::ENV_FILE),
            "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.packages([\"ripgrep\"])];\n}\n",
        )
        .unwrap();
        assert!(project_declares_env(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn foreign_flake_detection_ordering_only_when_no_env_declared() {
        // The core U16 ordering rule: a project that already declares env.*
        // is never silently swapped for a foreign flake, even if one exists —
        // only `--flake` can force that.
        let dir = scratch("ordering");
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        std::fs::write(
            dir.join(Syntax::ENV_FILE),
            "module env.dev { packages: [ripgrep] }\n",
        )
        .unwrap();
        let has_foreign = foreign_flake_path(&dir).is_some();
        let declares_env = project_declares_env(&dir);
        assert!(has_foreign);
        assert!(declares_env);
        // Auto-detection condition from `cmd_enter`: foreign.is_some() &&
        // !project_declares_env(..) — false here, so the project's own env
        // wins unless `--flake` is passed explicitly.
        assert!(!(has_foreign && !declares_env));
        std::fs::remove_dir_all(&dir).ok();
    }
}
