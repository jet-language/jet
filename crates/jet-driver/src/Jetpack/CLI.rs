//! Jetpack Phase 1 command dispatch (D-JPK2/9).
//!
//! `jetpack run/build/list/clean/add/remove`. Independent from the `jet`
//! binary (D-JPK1). All user-facing output flows through `Output::Theme`.

use super::Bridge;
use super::BuildDebug;
use super::Components;
use super::Discovery;
use super::Image;
use super::ManifestTOML;
use super::Output::{self, Theme};
use super::Overlay;
use super::Provider::{self, ProviderError};
use super::RefSpec::{self, ProviderKind};
use super::RuntimePolicy;
use super::Secrets;
use super::Services;
use super::Shell::{self, Env, ShellKind};
use super::Store::{self, Roots};
use super::Trust;
use super::{
    EnvFile, ModuleEval, RefSpec::RefError, SemanticLock, WorkspaceFile, WorkspaceLock, JSON,
};
use crate::{Lock, Syntax};
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
    /// D-JPK-IMAGE1: `jet image <name> --push <ref>` — the registry ref to
    /// push to. Always honestly gated (E1268): pushing needs TLS support that
    /// doesn't exist yet, so this is only ever read to report that gate.
    push: Option<String>,
    /// D-JPK-OSGEN1=C: optional generation name for `jet os switch`.
    os_name: Option<String>,
    /// D-JPK-OSDISK1=C: optional manual disk/device path for `jet os init|image`.
    os_manual: Option<String>,
    /// D-JOS-VMCOMMAND1=A: optional VM proof disk image path.
    os_disk: Option<String>,
    /// D-JOS-STUDIO-HOST1=A: local Studio projection service address.
    studio_serve: Option<String>,
    /// D-JOS-STUDIO-HOST1=A: selected jetos host for Studio.
    studio_host: Option<String>,
    /// U20: `jetpack add <ref> --adapt` drafts an adapter declaration instead
    /// of editing `env.jet` with a plain package ref.
    adapt: bool,
    /// Emit machine-readable output for diagnostics that have structured
    /// payloads (currently U23 no-Nix package holes).
    json: bool,
    /// U27: open a shell in preserved failed build scratch.
    shell_on_fail: bool,
    /// D-JPK-GRANTCMD1=A: `jet trust grant <selector> --scope repo|user`.
    trust_scope: Option<String>,
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
        push: None,
        adapt: false,
        json: false,
        shell_on_fail: false,
        trust_scope: None,
        os_name: None,
        os_manual: None,
        os_disk: None,
        studio_serve: None,
        studio_host: None,
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
            "--color=never" => flags.no_color = true,
            "--color=auto" | "--color=always" => {}
            "--offline" => flags.offline = true,
            a if a == Syntax::TRUST_BYPASS_FLAG => flags.trust = true,
            a if a == Syntax::ENV_FLAG_FLAKE => flags.flake = true,
            a if a == Syntax::ENV_FLAG_PURE => flags.pure = true,
            "--adapt" => flags.adapt = true,
            "--json" => flags.json = true,
            a if a == Syntax::BUILD_FLAG_SHELL_ON_FAIL => flags.shell_on_fail = true,
            a if a == Syntax::TRUST_FLAG_SCOPE => {
                if let Some(scope) = args.get(i + 1).filter(|s| !s.starts_with('-')) {
                    i += 1;
                    flags.trust_scope = Some(scope.clone());
                } else {
                    flags.trust_scope = Some(String::new());
                }
            }
            a if a == Syntax::IMAGE_FLAG_PUSH => {
                i += 1;
                if let Some(r) = args.get(i) {
                    flags.push = Some(r.clone());
                }
            }
            a if a == Syntax::OS_FLAG_NAME => {
                i += 1;
                if let Some(name) = args.get(i) {
                    flags.os_name = Some(name.clone());
                }
            }
            a if a == Syntax::OS_FLAG_MANUAL_DISK => {
                i += 1;
                if let Some(path) = args.get(i) {
                    flags.os_manual = Some(path.clone());
                }
            }
            a if a == Syntax::OS_FLAG_DISK => {
                i += 1;
                if let Some(path) = args.get(i) {
                    flags.os_disk = Some(path.clone());
                }
            }
            a if a == Syntax::STUDIO_FLAG_SERVE => {
                i += 1;
                if let Some(addr) = args.get(i) {
                    flags.studio_serve = Some(addr.clone());
                }
            }
            a if a == Syntax::STUDIO_FLAG_HOST => {
                i += 1;
                if let Some(host) = args.get(i) {
                    flags.studio_host = Some(host.clone());
                }
            }
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
        v if v == Syntax::TRUST_SUBCOMMAND => cmd_trust(&theme, &parsed),
        "build" => cmd_build(&theme, &parsed),
        "list" => cmd_list(&theme),
        "hangar" => cmd_hangar(&theme, &parsed),
        "vendor" => cmd_vendor(&theme, &parsed),
        "audit" => cmd_audit(&theme),
        "clean" => cmd_clean(&theme),
        "add" => cmd_add(&theme, &parsed),
        "remove" => cmd_remove(&theme, &parsed),
        "update" => cmd_update(&theme, &parsed),
        "outdated" => cmd_outdated(&theme, &parsed),
        "search" => cmd_search(&theme, &parsed),
        "info" => cmd_info(&theme, &parsed),
        "explain" => cmd_explain(&theme, &parsed),
        "logs" => cmd_logs(&theme, &parsed),
        "override" => cmd_override(&theme, &parsed),
        "push" => cmd_push(&theme, &parsed),
        v if v == Syntax::IMAGE_SUBCOMMAND => cmd_image(&theme, &parsed),
        v if v == Syntax::BRIDGE_SUBCOMMAND => cmd_bridge(&theme, &parsed),
        v if v == Syntax::OS_SUBCOMMAND => cmd_os(&theme, &parsed),
        v if v == Syntax::STUDIO_SUBCOMMAND => cmd_studio(&theme, &parsed),
        v if v == Syntax::SERVICES_SUBCOMMAND => cmd_services(&theme, &parsed),
        v if v == Syntax::SECRETS_SUBCOMMAND => cmd_secrets(&theme, &parsed),
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
    match realize_ref_outcome(theme, roots, flags, table, spec, name_w) {
        RefOutcome::Realized(entry, state) => Some((entry, state)),
        RefOutcome::NeedsNix(need) => {
            report_nix_bridge_required(theme, flags, &[need], &[]);
            None
        }
        RefOutcome::Failed => None,
    }
}

enum RefOutcome {
    Realized(Store::StoreEntry, Provider::SourceState),
    NeedsNix(Provider::NixBridgeNeed),
    Failed,
}

fn realize_ref_outcome(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
) -> RefOutcome {
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
    if let Some(entry) = Store::find_by_reference(roots, &spec.raw) {
        drop(spinner);
        theme.ok(&format!("{} {}", theme.bold(&entry.name), "cached"));
        theme.detail(&theme.gray(&entry.out));
        return RefOutcome::Realized(entry, Provider::SourceState::Cached);
    }
    if flags.offline
        && Provider::uses_nix_provider(spec, table, flags.offline, &store_dir)
        && fixtures_for(flags).is_none()
    {
        drop(spinner);
        report_provider_error(
            theme,
            &ProviderError::Offline(format!(
                "`{}` is not in the hangar and --offline forbids fetching provider output",
                spec.raw
            )),
        );
        return RefOutcome::Failed;
    }
    if !package_fixture_available(flags, spec) && !Provider::nix_on_path() {
        if let Some(need) = Provider::needs_nix_bridge(spec, table, flags.offline, &store_dir) {
            return RefOutcome::NeedsNix(need);
        }
    }
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
                report_provider_error(
                    theme,
                    &ProviderError::Offline(format!(
                        "`{}` is not in the hangar and --offline forbids fetching provider output",
                        spec.raw
                    )),
                );
                return RefOutcome::Failed;
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
                    return RefOutcome::Failed;
                }
            }
            .map_or(RefOutcome::Failed, |(entry, state)| {
                RefOutcome::Realized(entry, state)
            })
        }
        Err(e) => {
            if matches!(e, ProviderError::NixMissing) {
                if let Some(need) =
                    Provider::needs_nix_bridge(spec, table, flags.offline, &store_dir)
                {
                    return RefOutcome::NeedsNix(need);
                }
            }
            report_provider_error(theme, &e);
            RefOutcome::Failed
        }
    }
}

fn package_fixture_available(flags: &Flags, spec: &RefSpec::RefSpec) -> bool {
    if flags.offline {
        fixtures_for(flags)
            .map(|dir| dir.join(Provider::fixture_name(spec)).is_file())
            .unwrap_or(false)
    } else {
        flags
            .fixtures
            .as_ref()
            .map(|dir| dir.join(Provider::fixture_name(spec)).is_file())
            .unwrap_or(false)
    }
}

fn report_nix_bridge_required(
    theme: &Theme,
    flags: &Flags,
    holes: &[Provider::NixBridgeNeed],
    realized_refs: &[String],
) {
    if flags.json {
        let holes_json = holes
            .iter()
            .map(|h| super::JSON::quote(&h.reference))
            .collect::<Vec<_>>()
            .join(", ");
        let realized_json = realized_refs
            .iter()
            .map(|r| super::JSON::quote(r))
            .collect::<Vec<_>>()
            .join(", ");
        println!("{{\"code\":\"E1272\",\"realized\":[{realized_json}],\"holes\":[{holes_json}]}}");
    }
    let count = holes.len();
    let subject = if count == 1 {
        "package needs"
    } else {
        "packages need"
    };
    let refs = holes
        .iter()
        .map(|h| format!("`{}`", h.reference))
        .collect::<Vec<_>>()
        .join(", ");
    let fix_ref = holes
        .first()
        .map(|h| h.reference.as_str())
        .unwrap_or("<ref>");
    theme.error_coded(
        "E1272",
        &format!("{count} {subject} the Nix bridge, and Nix is not installed"),
        &format!("{refs} currently realize through the Nix compatibility provider on this machine."),
        &format!(
            "install Nix (https://nixos.org/download), or replace the package with a native source/adapter; `jetpack add {fix_ref} --adapt` drafts one."
        ),
    );
}

fn realize_adapter(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    plan: &ModuleEval::AdapterPlan,
) -> Option<(Store::StoreEntry, Provider::SourceState)> {
    theme.status(&format!("adapting {} …", theme.bold(&plan.name)));
    let store_dir = roots.hangar_dir();
    let ctx = Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: flags.offline,
    };
    match Provider::realize_adapter(plan, &ctx) {
        Ok(r) => {
            theme.ok(&format!(
                "{} {}",
                theme.bold(&r.name),
                r.source_state.label()
            ));
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
                        "could not record the adapted package",
                        &format!("writing to the Jetpack store failed: {e}"),
                        "check permissions on the store root, or set JETPACK_ROOT.",
                    );
                    None
                }
            }
        }
        Err(e) => {
            report_provider_error(theme, &e);
            if flags.shell_on_fail {
                shell_on_failed_build(theme, roots, &plan.name);
            }
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
        ProviderError::Adapter(reason) => theme.error_coded(
            "E1270",
            "adapter package could not be realized",
            reason,
            "check the `Pkg.adapt(...)` source and recipe.",
        ),
        ProviderError::BuildDebug(reason) => theme.error_coded(
            "E1273",
            "package build failed at a logged step",
            reason,
            "run `jet logs <pkg>` for full output, or rerun with `--shell-on-fail`.",
        ),
        ProviderError::Channel(reason) => theme.error_coded(
            "E1271",
            "source channel could not be resolved",
            reason,
            &format!(
                "run `jetpack update` with network or fixture metadata, then commit `{}`.",
                Syntax::UNIFIED_LOCK_FILE
            ),
        ),
        ProviderError::Offline(reason) => theme.error_coded(
            "E1276",
            "--offline forbids network access",
            reason,
            "drop `--offline` for this command, or realize/fetch the needed object before going offline.",
        ),
    }
}

/// The refs to realize, the table that resolves their named sources, and the
/// prompt label for the resulting shell.
struct RunPlan {
    refs: Vec<RefSpec::RefSpec>,
    adapters: Vec<ModuleEval::AdapterPlan>,
    table: RefSpec::SourceTable,
    label: String,
    /// U12: dev-supervised `services:` entries the typed env surface
    /// declared, empty for the Phase-1 directive surface (which predates
    /// U12). `jetpack services <verb>` and `jet dev`'s health gate are the
    /// only readers.
    dev_services: Vec<ModuleEval::DevServicePlan>,
    /// U13: every declared `secrets: ["name", …]` entry from the typed env
    /// surface. `jet env`/`jet dev` trust-gate on this and validate the names
    /// exist before entering the environment.
    secrets: Vec<String>,
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
        adapters: Vec::new(),
        table,
        label: ef.prompt_label(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
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
    // U12: a dev service with no explicit `init:` that matches the built-in
    // catalog implicitly depends on that catalog's package (e.g. `redis: {
    // enable: true }` needs `redis-server` on PATH) — fold its ref in
    // alongside the author's own `packages:` so it realizes the same way.
    let mut package_refs = plan.package_refs;
    for svc in &plan.dev_services {
        if svc.enable && svc.init.is_none() {
            if let Some(pkg_ref) = Services::catalog_pkg_ref(&svc.name) {
                if !package_refs.iter().any(|r| r == pkg_ref) {
                    package_refs.push(pkg_ref.to_string());
                }
            }
        }
    }
    let refs = classify_all(theme, package_refs.iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        refs,
        adapters: plan.adapters,
        table,
        label: plan
            .prompt
            .unwrap_or_else(|| Syntax::JETPACK_PROMPT_LABEL.to_string()),
        dev_services: plan.dev_services,
        secrets: plan.secrets,
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

struct ChannelSource {
    name: String,
    base: String,
    channel: RefSpec::ChannelRef,
}

fn channel_sources(table: &RefSpec::SourceTable) -> Vec<ChannelSource> {
    table
        .declarations()
        .into_iter()
        .filter_map(|(name, upstream, _)| {
            let (base, channel) = RefSpec::split_channel_ref(&upstream);
            Some(ChannelSource {
                name,
                base: base.to_string(),
                channel: channel?,
            })
        })
        .collect()
}

/// D-JPK-CHANNEL1=A: realize-class commands use only exact lock entries.
/// Update-class commands are the only place a channel may move.
fn apply_locked_channels(
    theme: &Theme,
    project_dir: &Path,
    table: &mut RefSpec::SourceTable,
) -> Result<(), i32> {
    for source in channel_sources(table) {
        let Some(lock) = Lock::locked_source_channel(project_dir, &source.name) else {
            report_unlocked_channel(theme, &source.name, source.channel.as_str());
            return Err(2);
        };
        if lock.channel != source.channel.as_str() {
            report_unlocked_channel(theme, &source.name, source.channel.as_str());
            return Err(2);
        }
        table.set_upstream(&source.name, lock.exact);
    }
    Ok(())
}

fn report_unlocked_channel(theme: &Theme, name: &str, channel: &str) {
    theme.error_coded(
        "E1271",
        &format!("source `{name}` tracks `{channel}` but is not locked"),
        &format!(
            "channel refs resolve only during `jetpack update`; build/run/env read the exact source recorded in `{}`.",
            Syntax::UNIFIED_LOCK_FILE
        ),
        &format!(
            "run `jetpack update {name}` and commit `{}`.",
            Syntax::UNIFIED_LOCK_FILE
        ),
    );
}

fn resolve_source_channel(source: &ChannelSource, flags: &Flags) -> Result<String, ProviderError> {
    if let Some(exact) = resolve_channel_from_fixture(source, flags) {
        return Ok(exact);
    }
    if flags.offline || ci_mode() {
        return Err(ProviderError::Channel(format!(
            "source `{}` tracks `{}` but no exact lock entry exists",
            source.name,
            source.channel.as_str()
        )));
    }
    resolve_channel_with_git(source)
}

fn resolve_channel_from_fixture(source: &ChannelSource, flags: &Flags) -> Option<String> {
    let dir = fixtures_for(flags)?;
    let raw = std::fs::read_to_string(dir.join("channels.txt")).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 3 && cols[0] == source.base && cols[1] == source.channel.as_str() {
            return Some(exact_upstream(&source.base, cols[2]));
        }
    }
    None
}

fn exact_upstream(base: &str, exact: &str) -> String {
    if exact.contains(Syntax::REF_SEPARATOR) {
        exact.to_string()
    } else {
        format!("{base}#{exact}")
    }
}

fn ci_mode() -> bool {
    std::env::var_os("CI").is_some_and(|v| !v.is_empty())
}

fn resolve_channel_with_git(source: &ChannelSource) -> Result<String, ProviderError> {
    let Some(rest) = source.base.strip_prefix("github:") else {
        return Err(ProviderError::Channel(format!(
            "source `{}` uses `{}`; only GitHub source channels can be resolved without fixture metadata today",
            source.name, source.base
        )));
    };
    let mut parts = rest.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() {
        return Err(ProviderError::Channel(format!(
            "`{}` is not a GitHub repo source",
            source.base
        )));
    }
    let url = format!("https://github.com/{owner}/{repo}.git");
    match &source.channel {
        RefSpec::ChannelRef::Main => {
            let out = git_ls_remote(&url, "refs/heads/main")?;
            let rev = out
                .split_whitespace()
                .next()
                .ok_or_else(|| ProviderError::Channel("no `main` head found".to_string()))?;
            Ok(format!("{}#{}", source.base, rev))
        }
        RefSpec::ChannelRef::Latest => {
            let tags = git_ls_remote(&url, "refs/tags/*")?;
            let tag = newest_tag(tags.lines(), None).ok_or_else(|| {
                ProviderError::Channel(format!("no release tags found for `{}`", source.base))
            })?;
            Ok(format!("{}#{}", source.base, tag))
        }
        RefSpec::ChannelRef::SemverMask(mask) => {
            let tags = git_ls_remote(&url, "refs/tags/*")?;
            let tag = newest_tag(tags.lines(), Some(mask)).ok_or_else(|| {
                ProviderError::Channel(format!("no tags match `{mask}` for `{}`", source.base))
            })?;
            Ok(format!("{}#{}", source.base, tag))
        }
    }
}

fn git_ls_remote(url: &str, pattern: &str) -> Result<String, ProviderError> {
    if std::env::var_os("JETPACK_DENY_NETWORK").is_some_and(|v| !v.is_empty()) {
        return Err(ProviderError::Offline(
            "network disabled by JETPACK_DENY_NETWORK while refreshing source channels".to_string(),
        ));
    }
    let out = std::process::Command::new("git")
        .args(["ls-remote", "--refs", url, pattern])
        .output()
        .map_err(|e| ProviderError::Channel(format!("could not run `git ls-remote`: {e}")))?;
    if !out.status.success() {
        let reason = String::from_utf8_lossy(&out.stderr)
            .trim()
            .lines()
            .last()
            .unwrap_or("git ls-remote failed")
            .to_string();
        return Err(ProviderError::Channel(reason));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn offline_refusal(theme: &Theme, command: &str) -> i32 {
    report_provider_error(
        theme,
        &ProviderError::Offline(format!(
            "`jetpack {command}` is a network-class command and cannot run under --offline"
        )),
    );
    2
}

fn newest_tag<'a>(lines: impl Iterator<Item = &'a str>, mask: Option<&str>) -> Option<String> {
    let major = mask
        .and_then(|m| m.strip_prefix('v'))
        .and_then(|m| m.strip_suffix(".x"))
        .and_then(|m| m.parse::<u64>().ok());
    let mut best: Option<(Vec<u64>, String)> = None;
    for line in lines {
        let Some(tag) = line.split("refs/tags/").nth(1) else {
            continue;
        };
        let tag = tag.trim();
        let Some(nums) = semver_nums(tag) else {
            continue;
        };
        if major.is_some_and(|m| nums.first().copied() != Some(m)) {
            continue;
        }
        let wins = best
            .as_ref()
            .is_none_or(|(old, _)| nums.as_slice() > old.as_slice());
        if wins {
            best = Some((nums, tag.to_string()));
        }
    }
    best.map(|(_, tag)| tag)
}

fn semver_nums(tag: &str) -> Option<Vec<u64>> {
    let rest = tag.strip_prefix('v').unwrap_or(tag);
    let mut out = Vec::new();
    for part in rest.split('.') {
        let digits = part
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() {
            return None;
        }
        out.push(digits.parse().ok()?);
    }
    Some(out)
}

/// `jetpack run [<ref>] [-- cmd…]`
fn cmd_run(theme: &Theme, parsed: &Parsed) -> i32 {
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
fn cmd_enter(theme: &Theme, parsed: &Parsed) -> i32 {
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

/// U13: before `jet env`/`jet dev` enters a trusted project environment, every
/// declared `secrets: ["name", …]` entry must exist in `.jet/secrets.age`.
/// Values stay inside `Secrets::get` and are dropped immediately; this is a
/// presence check, not env-var injection.
fn validate_declared_secrets(
    theme: &Theme,
    project_dir: &Path,
    names: &[String],
) -> Result<(), i32> {
    for name in names {
        match Secrets::get(project_dir, name) {
            Ok(Some(_)) => {}
            Ok(None) => {
                theme.error_coded(
                    "E1263",
                    &format!("no secret named `{name}`"),
                    "this environment declares that secret, but the encrypted store doesn't have an entry with this name.",
                    &format!("set it first with `jetpack secrets set {name} <value>`, or check the spelling."),
                );
                return Err(2);
            }
            Err(msg) => {
                theme.error(&format!("couldn't read `{name}`"), &msg, "");
                return Err(2);
            }
        }
    }
    Ok(())
}

/// U12: bring up every enabled dev `services:` entry and block until each is
/// healthy (or E1261 on timeout) before `jetpack dev` runs the project's
/// `fn dev()`/`fn run()`. Takes the composed `Env` so a catalog binary (e.g.
/// `redis-server`, realized alongside the project's own packages) resolves
/// on PATH the same way the project's own command does.
fn wait_for_services_ready(
    theme: &Theme,
    project_dir: &Path,
    env: &Env,
    services: &[ModuleEval::DevServicePlan],
) -> Result<(), i32> {
    for svc in services {
        if !svc.enable {
            continue;
        }
        theme.detail(&format!(
            "waiting for service `{}` to become healthy…",
            svc.name
        ));
        bring_up_one(theme, project_dir, env, svc).map_err(|_| 2)?;
    }
    Ok(())
}

/// Bring up one enabled service and block until it's healthy: E1262 for an
/// unrecognized field, a plain error if it can't be started at all, E1261 on
/// a readiness timeout. Shared by `wait_for_services_ready` (the `jet dev`
/// health gate) and `cmd_services`'s `up` verb so the two never drift.
fn bring_up_one(
    theme: &Theme,
    project_dir: &Path,
    env: &Env,
    svc: &ModuleEval::DevServicePlan,
) -> Result<(), ()> {
    if let Some(field) = Services::unknown_field(svc) {
        theme.error_coded(
            "E1262",
            &format!("service `{}` has a field jetpack doesn't recognize: `{field}`", svc.name),
            "a dev-supervised `Service` stays open at parse time, but jetpack's dev-runtime tier is the only consumer of its fields — an unrecognized key is almost always a typo.",
            "rename it to one of `enable`, `ports`, `init`, `shutdown`, `data_dir`, `ready`, or remove it.",
        );
        return Err(());
    }
    if let Err(msg) = Services::up_one(project_dir, env, svc) {
        theme.error(&format!("couldn't start service `{}`", svc.name), &msg, "");
        return Err(());
    }
    if Services::wait_healthy(project_dir, svc, service_health_timeout()) {
        Ok(())
    } else {
        theme.error_coded(
            "E1261",
            &format!("service `{}` never became healthy", svc.name),
            "jetpack waited for its readiness contract (`ready:`, else a TCP probe on its first `ports:` entry, else a bare process-alive check) and it never passed.",
            &format!("check `jetpack services logs {}` for what it printed, and confirm its `init`/`ready` commands are correct.", svc.name),
        );
        Err(())
    }
}

/// U12: the readiness-poll ceiling `wait_for_services_ready`/`cmd_services`'s
/// `up` verb allow a service before reporting E1261. Not yet user-configurable
/// (no ratified field for it) — 15s comfortably covers a local dev dependency
/// (redis, a small HTTP mock, …) starting cold. `JETPACK_SERVICE_HEALTH_TIMEOUT_MS`
/// overrides it for tests that need to exercise the E1261 timeout path quickly
/// (mirrors `JETPACK_ROOT`/`JETPACK_FIXTURES`'s test-only env-var escape hatch).
fn service_health_timeout() -> std::time::Duration {
    std::env::var("JETPACK_SERVICE_HEALTH_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(15))
}

/// `jetpack services up|down|health|logs [<name>]` (U12). With no `<name>`,
/// every declared dev service is targeted; `logs` requires exactly one name.
fn cmd_services(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(verb) = parsed.positional.first().cloned() else {
        theme.error(
            "`jetpack services` needs a verb",
            &format!("known verbs: {}.", Syntax::SERVICES_VERBS.join(", ")),
            "try `jetpack services up`.",
        );
        return 2;
    };
    let name = parsed.positional.get(1).cloned();

    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
    let targets: Vec<&ModuleEval::DevServicePlan> = plan
        .dev_services
        .iter()
        .filter(|s| name.as_deref().is_none_or(|n| n == s.name))
        .collect();
    if targets.is_empty() {
        let headline = match &name {
            Some(n) => format!("no dev service named `{n}`"),
            None => "this project declares no dev `services:`".to_string(),
        };
        theme.error(
            &headline,
            "declare one under an `env.<name> { services: { … } }` role-module (U12).",
            "",
        );
        return 2;
    }

    match verb.as_str() {
        v if v == Syntax::SERVICES_VERB_LOGS => {
            let Some(n) = &name else {
                theme.error(
                    "`jetpack services logs` needs a service name",
                    "logs are per service.",
                    "try `jetpack services logs <name>`.",
                );
                return 2;
            };
            print!("{}", Services::logs(&project_dir, n));
            0
        }
        v if v == Syntax::SERVICES_VERB_UP => {
            let roots = Store::resolve();
            if roots.dev_mode {
                theme.detail(&theme.gray("user-owned hangar active"));
            }
            let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                Ok(env) => env,
                Err(code) => return code,
            };
            for svc in &targets {
                if !svc.enable {
                    theme.detail(&format!("service `{}` is disabled, skipping", svc.name));
                    continue;
                }
                if bring_up_one(theme, &project_dir, &env, svc).is_err() {
                    return 2;
                }
                theme.ok(&format!("service `{}` is up", svc.name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_DOWN => {
            for svc in &targets {
                Services::down_one(&project_dir, svc);
                theme.ok(&format!("service `{}` is down", svc.name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_HEALTH => {
            let mut all_healthy = true;
            for svc in &targets {
                let (label, healthy) = match Services::health_one(&project_dir, svc) {
                    Services::Health::Disabled => ("disabled", true),
                    Services::Health::NotRunning => ("not running", false),
                    Services::Health::Unhealthy => ("unhealthy", false),
                    Services::Health::Healthy => ("healthy", true),
                };
                all_healthy &= healthy;
                if healthy {
                    theme.ok(&format!("service `{}`: {label}", svc.name));
                } else {
                    theme.error(&format!("service `{}`: {label}", svc.name), "", "");
                }
            }
            if all_healthy {
                0
            } else {
                1
            }
        }
        other => {
            theme.error(
                &format!("`{other}` is not a `jetpack services` verb"),
                &format!("known verbs: {}.", Syntax::SERVICES_VERBS.join(", ")),
                "try `jetpack services up`.",
            );
            2
        }
    }
}

/// `jetpack secrets keygen|set|get|recipients` (U13, D-JPK-SECRETCRYPTO1).
fn cmd_secrets(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(verb) = parsed.positional.first().cloned() else {
        theme.error(
            "`jetpack secrets` needs a verb",
            &format!("known verbs: {}.", Syntax::SECRETS_VERBS.join(", ")),
            "try `jetpack secrets keygen`.",
        );
        return 2;
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
    match verb.as_str() {
        v if v == Syntax::SECRETS_VERB_KEYGEN => {
            let force = parsed
                .positional
                .iter()
                .any(|a| a == Syntax::SECRETS_FLAG_FORCE);
            match Secrets::keygen(force) {
                Ok((path, recipient)) => {
                    theme.ok(&format!("wrote identity to `{}`", path.display()));
                    theme.detail(&format!("recipient: {recipient}"));
                    theme.detail("add it with `jetpack secrets recipients add <recipient>`");
                    0
                }
                Err(msg) => {
                    theme.error("couldn't generate a secrets identity", &msg, "");
                    2
                }
            }
        }
        v if v == Syntax::SECRETS_VERB_RECIPIENTS => {
            let Some(sub) = parsed.positional.get(1).cloned() else {
                theme.error(
                    "`jetpack secrets recipients` needs a verb",
                    &format!(
                        "known verbs: {}.",
                        Syntax::SECRETS_RECIPIENTS_VERBS.join(", ")
                    ),
                    "try `jetpack secrets recipients list`.",
                );
                return 2;
            };
            match sub.as_str() {
                v if v == Syntax::SECRETS_RECIPIENTS_VERB_ADD => {
                    let Some(recipient) = parsed.positional.get(2) else {
                        theme.error(
                            "`jetpack secrets recipients add` needs a recipient",
                            "an `age1...` public key.",
                            "try `jetpack secrets recipients add age1...`.",
                        );
                        return 2;
                    };
                    if Secrets::add_recipient(&project_dir, recipient) {
                        theme.ok(&format!("added recipient `{recipient}`"));
                    } else {
                        theme.detail(&format!("recipient `{recipient}` already present"));
                    }
                    0
                }
                v if v == Syntax::SECRETS_RECIPIENTS_VERB_LIST => {
                    for r in Secrets::list_recipients(&project_dir) {
                        println!("{r}");
                    }
                    0
                }
                other => {
                    theme.error(
                        &format!("`{other}` is not a `jetpack secrets recipients` verb"),
                        &format!(
                            "known verbs: {}.",
                            Syntax::SECRETS_RECIPIENTS_VERBS.join(", ")
                        ),
                        "try `jetpack secrets recipients list`.",
                    );
                    2
                }
            }
        }
        v if v == Syntax::SECRETS_VERB_SET => {
            let (Some(name), Some(value)) = (parsed.positional.get(1), parsed.positional.get(2))
            else {
                theme.error(
                    "`jetpack secrets set` needs a name and a value",
                    "",
                    "try `jetpack secrets set db_password hunter2`.",
                );
                return 2;
            };
            match Secrets::set(&project_dir, name, value) {
                Ok(()) => {
                    theme.ok(&format!("set `{name}`"));
                    0
                }
                Err(msg) => {
                    theme.error(&format!("couldn't set `{name}`"), &msg, "");
                    2
                }
            }
        }
        v if v == Syntax::SECRETS_VERB_GET => {
            let Some(name) = parsed.positional.get(1) else {
                theme.error(
                    "`jetpack secrets get` needs a name",
                    "",
                    "try `jetpack secrets get db_password`.",
                );
                return 2;
            };
            match Secrets::get(&project_dir, name) {
                Ok(Some(value)) => {
                    println!("{value}");
                    0
                }
                Ok(None) => {
                    theme.error_coded(
                        "E1263",
                        &format!("no secret named `{name}`"),
                        "the encrypted store doesn't have an entry with this name.",
                        &format!("set it first with `jetpack secrets set {name} <value>`, or check the spelling."),
                    );
                    2
                }
                Err(msg) => {
                    theme.error(&format!("couldn't read `{name}`"), &msg, "");
                    2
                }
            }
        }
        other => {
            theme.error(
                &format!("`{other}` is not a `jetpack secrets` verb"),
                &format!("known verbs: {}.", Syntax::SECRETS_VERBS.join(", ")),
                "try `jetpack secrets keygen`.",
            );
            2
        }
    }
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
    prog.items
        .iter()
        .any(|i| matches!(i, crate::AST::Item::Func(f) if f.name == "dev" || f.name == "run"))
}

/// `jetpack config trust add/list/remove` (U19) — durable glob/prefix patterns
/// that pre-authorize matching projects with no per-hash prompt at all.
fn cmd_config(theme: &Theme, parsed: &Parsed) -> i32 {
    let group = parsed.positional.first().map(String::as_str);
    if group != Some(Syntax::CONFIG_VERB_TRUST) && group != Some(Syntax::CONFIG_VERB_SANDBOX) {
        theme.error(
            &format!("`jetpack config {}` isn't a command", group.unwrap_or("")),
            "jetpack config manages the env/dev trust store and sandbox fallback policy.",
            "try `jetpack config trust list` or `jetpack config sandbox status`.",
        );
        return 2;
    }
    let store = Trust::store_path();
    match parsed.positional.get(1).map(String::as_str) {
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_SANDBOX)
                && v == Syntax::CONFIG_SANDBOX_VERB_REQUIRE =>
        {
            match RuntimePolicy::write_sandbox_policy(RuntimePolicy::SandboxPolicy::Require) {
                Ok(path) => theme.status(&format!("sandbox fallback refused: {}", path.display())),
                Err(e) => {
                    theme.error(
                        "couldn't write sandbox policy",
                        &format!("{e}"),
                        "check permissions on your ~/.jet directory.",
                    );
                    return 1;
                }
            }
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_SANDBOX)
                && v == Syntax::CONFIG_SANDBOX_VERB_ALLOW =>
        {
            match RuntimePolicy::write_sandbox_policy(RuntimePolicy::SandboxPolicy::AllowFallback) {
                Ok(path) => theme.status(&format!("sandbox fallback allowed: {}", path.display())),
                Err(e) => {
                    theme.error(
                        "couldn't write sandbox policy",
                        &format!("{e}"),
                        "check permissions on your ~/.jet directory.",
                    );
                    return 1;
                }
            }
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_SANDBOX)
                && v == Syntax::CONFIG_SANDBOX_VERB_STATUS =>
        {
            let status = RuntimePolicy::detect_sandbox();
            let policy = RuntimePolicy::read_sandbox_policy();
            let policy_label = match policy {
                RuntimePolicy::SandboxPolicy::AllowFallback => "allow",
                RuntimePolicy::SandboxPolicy::Require => "require",
            };
            theme.status(&format!(
                "sandbox: {:?} via {} (policy: {policy_label})",
                status.level, status.mechanism
            ));
            theme.detail(&status.reason);
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_TRUST) && v == Syntax::CONFIG_TRUST_VERB_ADD =>
        {
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
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_TRUST) && v == Syntax::CONFIG_TRUST_VERB_LIST =>
        {
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
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_TRUST)
                && v == Syntax::CONFIG_TRUST_VERB_REMOVE =>
        {
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
        _ if group == Some(Syntax::CONFIG_VERB_TRUST) => {
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
        _ if group == Some(Syntax::CONFIG_VERB_SANDBOX) => {
            theme.error(
                "`jetpack config sandbox` needs a verb",
                &format!(
                    "the sandbox verbs are: {}.",
                    Syntax::CONFIG_SANDBOX_VERBS.join(", ")
                ),
                "try `require`, `allow`, or `status`.",
            );
            2
        }
        _ => {
            theme.error(
                &format!("`jetpack config {}` isn't a command", group.unwrap_or("")),
                "jetpack config manages trust and sandbox fallback policy.",
                "try `jetpack config trust list` or `jetpack config sandbox status`.",
            );
            2
        }
    }
}

/// D-JPK-GRANTCMD1=A: `jet trust grant/list/explain/revoke`. Jetpack owns the
/// store; top-level `jet trust` dispatches here.
fn cmd_trust(theme: &Theme, parsed: &Parsed) -> i32 {
    let store = Trust::store_path();
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::TRUST_VERB_GRANT => {
            let Some(selector) = parsed.positional.get(1) else {
                theme.error(
                    "`jet trust grant` needs a grant selector",
                    "a grant selector names one package, build, env, service, image, fleet, or jetos authority.",
                    "try `jet trust grant service:postgres --scope repo`.",
                );
                return 2;
            };
            let scope = parsed
                .flags
                .trust_scope
                .as_deref()
                .unwrap_or(Syntax::TRUST_SCOPE_USER);
            let grant = match Trust::parse_grant_selector(selector, scope) {
                Ok(g) => g,
                Err(e) => {
                    theme.error(
                        "couldn't parse trust grant",
                        &e,
                        "use `--scope user` or `--scope repo`.",
                    );
                    return 2;
                }
            };
            let added = Trust::add_grant(&store, &grant);
            theme.status(&if added {
                format!(
                    "trusted {} `{}` ({})",
                    grant.authority, grant.subject, grant.scope
                )
            } else {
                format!(
                    "already trusted {} `{}` ({})",
                    grant.authority, grant.subject, grant.scope
                )
            });
            0
        }
        Some(v) if v == Syntax::TRUST_VERB_LIST => {
            let records = Trust::list_records(&store);
            if parsed.flags.json {
                println!("{}", Trust::records_json(&records));
            } else if records.is_empty() {
                theme.status("no trust grants yet.");
            } else {
                for record in records {
                    print_trust_record(theme, &record);
                }
            }
            0
        }
        Some(v) if v == Syntax::TRUST_VERB_EXPLAIN => {
            let records = Trust::list_records(&store);
            let selected = parsed.positional.get(1).map(String::as_str);
            let matches: Vec<_> = records
                .into_iter()
                .filter(|record| selected.is_none_or(|s| trust_record_matches(record, s)))
                .collect();
            if parsed.flags.json {
                println!("{}", Trust::records_json(&matches));
            } else if matches.is_empty() {
                theme.status("no matching trust grants.");
            } else {
                for record in matches {
                    print_trust_record(theme, &record);
                    match &record {
                        Trust::TrustRecord::Grant(grant) => theme.detail(&format!(
                            "exact authority: {} subject `{}`; revoke with `jet trust revoke {}`",
                            grant.authority,
                            grant.subject,
                            grant.key()
                        )),
                        Trust::TrustRecord::Hash { hash } => theme.detail(&format!(
                            "exact env/build hash grant; revoke with `jet trust revoke hash:{hash}`"
                        )),
                        Trust::TrustRecord::Pattern { pattern } => theme.detail(&format!(
                            "path pattern grant; revoke with `jet trust revoke pattern:{pattern}`"
                        )),
                        Trust::TrustRecord::Raw { line } => theme.detail(&format!(
                            "legacy/raw grant; revoke with `jet trust revoke {line}`"
                        )),
                    }
                }
            }
            0
        }
        Some(v) if v == Syntax::TRUST_VERB_REVOKE => {
            let Some(selector) = parsed.positional.get(1) else {
                theme.error(
                    "`jet trust revoke` needs a grant selector",
                    "revocation is exact: pass the subject, grant key, hash, pattern, or raw line shown by `jet trust list`.",
                    "try `jet trust revoke service:postgres.service`.",
                );
                return 2;
            };
            let removed = Trust::revoke(&store, selector);
            theme.status(&if removed {
                format!("revoked: {selector}")
            } else {
                format!("not found: {selector}")
            });
            0
        }
        _ => {
            theme.error(
                "`jet trust` needs a verb",
                &format!("the trust verbs are: {}.", Syntax::TRUST_VERBS.join(", ")),
                "try `list`, `explain`, `grant`, or `revoke`.",
            );
            2
        }
    }
}

fn print_trust_record(theme: &Theme, record: &Trust::TrustRecord) {
    match record {
        Trust::TrustRecord::Hash { hash } => theme.detail(&format!("hash     {hash}")),
        Trust::TrustRecord::Pattern { pattern } => theme.detail(&format!("pattern  {pattern}")),
        Trust::TrustRecord::Grant(grant) => theme.detail(&format!(
            "{:<7} {}  scope:{}",
            grant.authority, grant.subject, grant.scope
        )),
        Trust::TrustRecord::Raw { line } => theme.detail(&format!("raw      {line}")),
    }
}

fn trust_record_matches(record: &Trust::TrustRecord, selector: &str) -> bool {
    match record {
        Trust::TrustRecord::Hash { hash } => selector == hash || selector == format!("hash:{hash}"),
        Trust::TrustRecord::Pattern { pattern } => {
            selector == pattern || selector == format!("pattern:{pattern}")
        }
        Trust::TrustRecord::Grant(grant) => {
            selector == grant.subject
                || selector == grant.key()
                || selector == format!("{}:{}", grant.scope, grant.key())
        }
        Trust::TrustRecord::Raw { line } => selector == line,
    }
}

/// Realize every ref in `plan` and compose the shell env (PATH dirs + prompt
/// label). Returns an exit code after reporting if any ref fails to realize.
fn compose_env(theme: &Theme, roots: &Roots, flags: &Flags, plan: &RunPlan) -> Result<Env, i32> {
    RuntimePolicy::enforce_sandbox_policy(theme, flags.json)?;
    let mut bin_dirs = Vec::new();
    let mut realized_refs = Vec::new();
    let mut holes = Vec::new();
    let mut failed = false;
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
        match realize_ref_outcome(theme, roots, flags, &plan.table, spec, name_w) {
            RefOutcome::Realized(entry, state) => {
                match state {
                    Provider::SourceState::Built => built += 1,
                    Provider::SourceState::Cached => cached += 1,
                    Provider::SourceState::Substituted => substituted += 1,
                }
                // A `library` package realizes with an empty `bin` (U10) — it
                // stages source for import and contributes nothing to PATH.
                if !entry.bin.is_empty() {
                    bin_dirs.push(entry.bin);
                }
                realized_refs.push(entry.reference);
            }
            RefOutcome::NeedsNix(need) => holes.push(need),
            RefOutcome::Failed => failed = true,
        }
    }
    for adapter in &plan.adapters {
        match realize_adapter(theme, roots, flags, adapter) {
            Some((entry, _state)) => {
                if !entry.bin.is_empty() {
                    bin_dirs.push(entry.bin);
                }
                realized_refs.push(entry.reference);
            }
            None => failed = true,
        }
    }
    if !holes.is_empty() {
        report_nix_bridge_required(theme, flags, &holes, &realized_refs);
        return Err(2);
    }
    if failed {
        return Err(1);
    }
    if plan.refs.len() > 1 {
        theme.status(&format!(
            "env ready — {}",
            state_summary(built, cached, substituted)
        ));
    }
    Ok(Env {
        bin_dirs,
        refs: realized_refs,
        label: plan.label.clone(),
    })
}

/// The ledger's name-column width for a set of refs (min 8 so a single short
/// name doesn't collapse the table).
fn name_column_width(refs: &[RefSpec::RefSpec]) -> usize {
    refs.iter()
        .map(|r| r.package.len())
        .max()
        .unwrap_or(0)
        .max(8)
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
    if let Err(code) = RuntimePolicy::enforce_sandbox_policy(theme, parsed.flags.json) {
        return code;
    }

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

    let mut plan = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(s) => RunPlan {
                refs: vec![s],
                adapters: Vec::new(),
                table: cwd_table(),
                label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
                dev_services: Vec::new(),
                secrets: Vec::new(),
            },
            Err(_) => return 2,
        },
        None => match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };
    if let Err(code) = apply_locked_channels(theme, &dir, &mut plan.table) {
        return code;
    }

    let mut ok = true;
    let (mut built, mut cached, mut substituted) = (0usize, 0usize, 0usize);
    let mut realized_refs = Vec::new();
    let mut holes = Vec::new();
    let name_w = name_column_width(&plan.refs);
    for spec in &plan.refs {
        match realize_ref_outcome(theme, &roots, &parsed.flags, &plan.table, spec, name_w) {
            RefOutcome::Realized(entry, state) => {
                realized_refs.push(entry.reference);
                match state {
                    Provider::SourceState::Built => built += 1,
                    Provider::SourceState::Cached => cached += 1,
                    Provider::SourceState::Substituted => substituted += 1,
                }
            }
            RefOutcome::NeedsNix(need) => holes.push(need),
            RefOutcome::Failed => ok = false,
        }
    }
    for adapter in &plan.adapters {
        match realize_adapter(theme, &roots, &parsed.flags, adapter) {
            Some((entry, state)) => {
                realized_refs.push(entry.reference);
                match state {
                    Provider::SourceState::Built => built += 1,
                    Provider::SourceState::Cached => cached += 1,
                    Provider::SourceState::Substituted => substituted += 1,
                }
            }
            None => ok = false,
        }
    }
    if !holes.is_empty() {
        report_nix_bridge_required(theme, &parsed.flags, &holes, &realized_refs);
        return 2;
    }
    if ok {
        // T4: per-run source-state summary (mirrors the D-JPK-CACHE1 example).
        theme.status(&format!(
            "built {} package(s): {} built, {} cached, {} substituted",
            plan.refs.len() + plan.adapters.len(),
            built,
            cached,
            substituted
        ));
        auto_clean_after_success(theme, &roots);
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
    let name_w = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let ver_w = entries
        .iter()
        .map(|e| {
            if e.version.is_empty() {
                1
            } else {
                e.version.len()
            }
        })
        .max()
        .unwrap_or(1);
    for e in entries {
        let v = if e.version.is_empty() {
            "—"
        } else {
            &e.version
        };
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
        theme.detail(&format!(
            "  output-hash: {}",
            theme.gray(&e.envelope.output_hash)
        ));
        theme.detail(&format!(
            "  platform:    {}",
            theme.gray(&e.envelope.platform)
        ));
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

/// `jetpack clean` — collect stale hangar objects and optimize owned bytes.
fn cmd_clean(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    match Store::clean(&roots) {
        Ok(report) => {
            theme.ok(&format!(
                "cleaned hangar: removed {} stale object(s), freed {}, swept {} scratch item(s), optimized {} file(s)",
                report.removed_objects,
                human_bytes(report.removed_bytes + report.swept_tmp_bytes),
                report.swept_tmp,
                report.optimized_files
            ));
            if report.optimized_bytes > 0 {
                theme.detail(&format!(
                    "optimized duplicate Jet-owned files: saved {}",
                    human_bytes(report.optimized_bytes)
                ));
            }
            0
        }
        Err(e) => {
            theme.error(
                "could not clean the hangar",
                &format!("{e}"),
                "check permissions on the hangar root.",
            );
            1
        }
    }
}

fn auto_clean_after_success(theme: &Theme, roots: &Roots) {
    match Store::maybe_auto_clean(roots) {
        Ok(Some(report)) if !report.is_empty() => theme.detail(&format!(
            "auto-cleaned hangar: removed {} stale object(s), swept {} scratch item(s), optimized {} file(s)",
            report.removed_objects, report.swept_tmp, report.optimized_files
        )),
        Ok(_) => {}
        Err(e) => theme.detail(&theme.gray(&format!("auto-clean skipped: {e}"))),
    }
}

/// `jetpack update [<source>]` — resolve channel source refs and move only
/// their lock entries. Does not realize packages.
fn cmd_update(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.flags.offline {
        return offline_refusal(theme, "update");
    }
    let project_dir = std::env::current_dir().unwrap_or_default();
    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let only = parsed.positional.first().map(String::as_str);
    let sources = channel_sources(&plan.table);
    let selected: Vec<_> = sources
        .into_iter()
        .filter(|s| only.is_none_or(|name| name == s.name))
        .collect();
    if selected.is_empty() {
        match only {
            Some(name) => theme.error(
                &format!("no channel source named `{name}`"),
                "only sources declared with `#latest`, `#main`, or `#vN.x` can be updated.",
                "run `jetpack outdated` to see channel sources.",
            ),
            None => theme.status("no channel sources to update."),
        }
        return if only.is_some() { 2 } else { 0 };
    }

    let mut ok = true;
    for source in &selected {
        match resolve_source_channel(source, &parsed.flags) {
            Ok(exact) => {
                Lock::record_source_channel(
                    &project_dir,
                    Lock::LockedSourceChannel {
                        name: source.name.clone(),
                        channel: source.channel.as_str().to_string(),
                        exact: exact.clone(),
                    },
                );
                theme.status(&format!(
                    "{} {} → {}",
                    theme.bold(&source.name),
                    theme.gray(source.channel.as_str()),
                    exact
                ));
            }
            Err(e) => {
                report_provider_error(theme, &e);
                ok = false;
            }
        }
    }
    if ok {
        0
    } else {
        2
    }
}

/// `jetpack outdated` — read-only channel freshness report. It may query
/// metadata, but never writes `.jet/lock`.
fn cmd_outdated(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.flags.offline {
        return offline_refusal(theme, "outdated");
    }
    let project_dir = std::env::current_dir().unwrap_or_default();
    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let sources = channel_sources(&plan.table);
    if sources.is_empty() {
        theme.status("no channel sources.");
        return 0;
    }
    let mut any = false;
    let mut ok = true;
    for source in &sources {
        let locked = Lock::locked_source_channel(&project_dir, &source.name);
        let Some(locked) = locked else {
            theme.detail(&format!(
                "{}  {}  unlocked (run `jetpack update {}`)",
                theme.bold(&source.name),
                theme.gray(source.channel.as_str()),
                source.name
            ));
            any = true;
            continue;
        };
        match resolve_source_channel(source, &parsed.flags) {
            Ok(latest) if latest != locked.exact => {
                any = true;
                theme.detail(&format!(
                    "{}  {}  {} → {}",
                    theme.bold(&source.name),
                    theme.gray(source.channel.as_str()),
                    locked.exact,
                    latest
                ));
            }
            Ok(_) => {}
            Err(e) => {
                report_provider_error(theme, &e);
                ok = false;
            }
        }
    }
    if ok && !any {
        theme.status("all channel sources are current.");
    }
    if ok {
        0
    } else {
        2
    }
}

/// `jetpack search <query>` — local/offline package discovery (U26).
fn cmd_search(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        theme.error(
            "search needs a query",
            "`jetpack search` reads the local discovery index; it never fetches.",
            "write `jetpack search postgres`.",
        );
        return 2;
    };
    let index = match discovery_index(theme, parsed) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let records = index.search(query);
    if parsed.flags.json {
        println!("{}", Discovery::search_json(&records));
        return 0;
    }
    if records.is_empty() {
        println!("no packages found for `{query}`");
        if let Some(nearest) = index.nearest(query) {
            println!("nearest: {nearest}");
        }
        return 1;
    }
    for record in records {
        println!(
            "{:<24} {:<10} {}",
            record.display_ref(),
            empty_dash(&record.version),
            record.platforms.join(", ")
        );
    }
    0
}

/// `jetpack info <ref>` — local/offline package metadata (U26).
fn cmd_info(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        theme.error(
            "info needs a package ref",
            "`jetpack info` reads the local discovery index; it never fetches.",
            "write `jetpack info default.ripgrep`.",
        );
        return 2;
    };
    let index = match discovery_index(theme, parsed) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let Some(record) = index.info(query) else {
        let fix = index
            .nearest(query)
            .map(|n| format!("try `jetpack info {n}`."))
            .unwrap_or_else(|| "run `jetpack search <name>` to see local matches.".to_string());
        theme.error(
            &format!("no local package info for `{query}`"),
            "`jetpack info` uses only the local discovery index.",
            &fix,
        );
        return 2;
    };
    if parsed.flags.json {
        println!("{}", Discovery::info_json(record));
        return 0;
    }
    println!("{}", record.display_ref());
    println!("  ref: {}", record.reference);
    println!("  version: {}", empty_dash(&record.version));
    println!("  platforms: {}", record.platforms.join(", "));
    println!("  source: {}", record.source);
    println!("  provenance: {}", record.provenance);
    if !record.options.is_empty() {
        println!("  service options:");
        for opt in &record.options {
            println!(
                "    {:<10} default: {:<24} {}",
                opt.name,
                empty_dash(&opt.default),
                opt.docs
            );
        }
    }
    0
}

fn cmd_explain(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        theme.error(
            "explain needs a package ref",
            "`jet explain <CODE>` is handled by the main compiler; `jetpack explain` explains package refs.",
            "write `jet explain weirdctl` after a failed build.",
        );
        return 2;
    };
    if query.starts_with("package-overlay:") {
        return cmd_explain_overlay(theme, query);
    }
    let roots = Store::resolve();
    let package = query
        .split_once(':')
        .map(|(_, p)| p)
        .or_else(|| query.split_once('.').map(|(_, p)| p))
        .unwrap_or(query);
    match BuildDebug::latest(&roots.hangar_dir(), package) {
        Ok(Some(attempt)) => {
            print!("{}", BuildDebug::explain(&attempt));
            0
        }
        Ok(None) => {
            theme.error_coded(
                "E1274",
                &format!("no build log for `{query}`"),
                "`jet explain <ref>` can explain refs after Jetpack has recorded a build attempt.",
                &format!("run `jet build {query}` first, or use `jet explain E1234` for diagnostic-code help."),
            );
            2
        }
        Err(e) => {
            theme.error_coded(
                "E1274",
                "couldn't read build explanation",
                &e,
                "check the build log directory under the Jetpack hangar.",
            );
            2
        }
    }
}

fn cmd_explain_overlay(theme: &Theme, query: &str) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let Some(result) = WorkspaceFile::load(&dir) else {
        theme.error_coded(
            "E1274",
            &format!("no overlay policy for `{query}`"),
            "`package-overlay:*` explanations come from reviewed `workspace.jet` overlay policy.",
            "run `jetpack override draft <ref> --patch <file>` or add an `overlay` block to `workspace.jet`.",
        );
        return 2;
    };
    let plan = match result {
        Ok(plan) => plan,
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    Syntax::WORKSPACE_FILE,
                    "",
                    std::slice::from_ref(&d)
                )
            );
            return 2;
        }
    };
    let lock = SemanticLock::SemanticLockFile {
        records: Overlay::semantic_records(&plan.overlay_policy, "workspace", std::env::consts::OS),
    };
    let Some(fact) = SemanticLock::explain(&lock, query) else {
        theme.error_coded(
            "E1274",
            &format!("no overlay record for `{query}`"),
            "`workspace.jet` has overlay policy, but not that overlay/package key.",
            "query `package-overlay:<overlay>:<package>`.",
        );
        return 2;
    };
    println!("{}", fact.semantic_key);
    println!("  owners: {}", fact.owners.join(", "));
    println!("  provider: {}", empty_dash(&fact.provider));
    println!("  platform: {}", empty_dash(&fact.platform));
    println!("  exact: {}", empty_dash(&fact.exact_artifact));
    println!("  policy: {}", empty_dash(&fact.policy_fingerprint));
    println!("  update: {}", empty_dash(&fact.update_command));
    println!("  offline: {}", fact.offline_satisfied);
    0
}

fn cmd_override(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(action) = parsed.positional.first().map(String::as_str) else {
        theme.error(
            "override needs an action",
            "`jetpack override` only drafts reviewed source policy; it never records hidden override state.",
            "write `jetpack override draft <ref> --patch <file>`.",
        );
        return 2;
    };
    if action != "draft" {
        theme.error(
            &format!("unknown override action `{action}`"),
            "`draft` is the only supported override action.",
            "write `jetpack override draft <ref> --patch <file>`.",
        );
        return 2;
    }
    let Some(reference) = parsed.positional.get(1) else {
        theme.error(
            "override draft needs a package ref",
            "the ref names the package whose typed workspace policy should be drafted.",
            "write `jetpack override draft nixpkgs:foo --patch patches/foo.patch`.",
        );
        return 2;
    };
    let mut overlay = "local".to_string();
    let mut patch = None::<String>;
    let mut provider = None::<String>;
    let mut channel = None::<String>;
    let mut allow_unfree = false;
    let mut i = 2usize;
    while i < parsed.positional.len() {
        match parsed.positional[i].as_str() {
            "--overlay" => {
                i += 1;
                overlay = parsed.positional.get(i).cloned().unwrap_or_default();
            }
            "--patch" => {
                i += 1;
                patch = parsed.positional.get(i).cloned();
            }
            "--provider" => {
                i += 1;
                provider = parsed.positional.get(i).cloned();
            }
            "--channel" => {
                i += 1;
                channel = parsed.positional.get(i).cloned();
            }
            "--allow-unfree" => allow_unfree = true,
            other => {
                theme.error(
                    &format!("unknown override draft flag `{other}`"),
                    "override drafts accept `--overlay`, `--provider`, `--channel`, `--patch`, and `--allow-unfree`.",
                    "write `jetpack override draft nixpkgs:foo --patch patches/foo.patch`.",
                );
                return 2;
            }
        }
        i += 1;
    }
    if overlay.trim().is_empty() {
        theme.error(
            "override draft needs a non-empty overlay name",
            "`workspace.jet` stores overrides in named overlay sets.",
            "pass `--overlay local` or another source-reviewed name.",
        );
        return 2;
    }
    let package = reference
        .split_once(':')
        .map(|(_, p)| p)
        .unwrap_or(reference)
        .to_string();
    let workspace = std::env::current_dir().unwrap_or_default();
    let path = workspace.join(Syntax::WORKSPACE_FILE);
    let existing = std::fs::read_to_string(&path).ok();
    let next = Overlay::draft_overlay_source(
        existing.as_deref(),
        &overlay,
        &package,
        patch.as_deref(),
        provider.as_deref(),
        channel.as_deref(),
        allow_unfree,
    );
    if let Err(e) = std::fs::write(&path, next) {
        theme.error(
            "could not write workspace overlay policy",
            &format!("writing `{}` failed: {e}", path.display()),
            "check permissions and retry.",
        );
        return 2;
    }
    theme.ok(&format!("drafted overlay `{overlay}` for `{package}`"));
    theme.detail(&format!(
        "wrote reviewed source policy to {}",
        path.display()
    ));
    0
}

fn cmd_logs(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(package) = parsed.positional.first() else {
        theme.error(
            "logs needs a package name",
            "`jet logs` prints the latest recorded build attempt for one package.",
            "write `jet logs weirdctl`.",
        );
        return 2;
    };
    let roots = Store::resolve();
    if parsed.flags.json {
        match BuildDebug::latest_json(&roots.hangar_dir(), package) {
            Ok(Some(json)) => {
                print!("{json}");
                0
            }
            Ok(None) => missing_logs(theme, package),
            Err(e) => read_logs_error(theme, &e),
        }
    } else {
        match BuildDebug::latest(&roots.hangar_dir(), package) {
            Ok(Some(attempt)) => {
                print!("{}", BuildDebug::text_logs(&attempt));
                0
            }
            Ok(None) => missing_logs(theme, package),
            Err(e) => read_logs_error(theme, &e),
        }
    }
}

fn missing_logs(theme: &Theme, package: &str) -> i32 {
    theme.error_coded(
        "E1274",
        &format!("no build log for `{package}`"),
        "`jet logs` reads persisted Jetpack build attempts; no attempt is recorded for that package.",
        &format!("run `jet build {package}` first."),
    );
    2
}

fn read_logs_error(theme: &Theme, reason: &str) -> i32 {
    theme.error_coded(
        "E1274",
        "couldn't read build logs",
        reason,
        "check the build log directory under the Jetpack hangar.",
    );
    2
}

fn shell_on_failed_build(theme: &Theme, roots: &Roots, package: &str) {
    let Ok(Some(attempt)) = BuildDebug::latest(&roots.hangar_dir(), package) else {
        return;
    };
    if attempt.scratch_dir.is_empty() {
        return;
    }
    let shell = std::env::var("JETPACK_SHELL_ON_FAIL")
        .ok()
        .unwrap_or_else(|| {
            std::env::var("SHELL").unwrap_or_else(|_| {
                if cfg!(windows) {
                    "cmd".to_string()
                } else {
                    "sh".to_string()
                }
            })
        });
    theme.status(&format!(
        "build failed at step {} · shell in preserved build dir {}",
        attempt.failed_step, attempt.scratch_dir
    ));
    let mut cmd = std::process::Command::new(&shell);
    cmd.current_dir(&attempt.scratch_dir)
        .env("JETPACK_FAILED_SCRATCH", &attempt.scratch_dir)
        .env("JETPACK_FAILED_STEP", attempt.failed_step.to_string())
        .env("JETPACK_FAILED_PACKAGE", package);
    let _ = cmd.status();
}

fn discovery_index(theme: &Theme, parsed: &Parsed) -> Result<Discovery::Index, i32> {
    let project_dir = std::env::current_dir().unwrap_or_default();
    let mut index = match Discovery::load(&project_dir) {
        Ok(Some(index)) => index,
        Ok(None) => Discovery::Index::default(),
        Err(e) => {
            theme.error(
                "local discovery index is malformed",
                &e,
                "delete `.jet/discovery/index.jsonl` and rerun `jetpack search` from a project with env metadata.",
            );
            return Err(2);
        }
    };

    let roots = Store::resolve();
    let store_entries = Store::list(&roots);
    Discovery::merge_store_entries(&mut index, &store_entries);

    if EnvFile::path_in(&project_dir).exists() {
        let plan = load_project_plan(theme)?;
        let fixtures = fixtures_for(&parsed.flags);
        Discovery::merge_refs(&mut index, &plan.refs, fixtures.as_deref(), &store_entries);
        Discovery::merge_adapters(&mut index, &plan.adapters);
        if let Err(e) = Discovery::write(&project_dir, &index) {
            theme.error(
                "couldn't write local discovery index",
                &e,
                "check permissions on `.jet/discovery/`.",
            );
            return Err(2);
        }
    }

    if index.is_empty() {
        theme.error(
            "no local discovery index",
            "`jetpack search` and `jetpack info` never fetch package metadata.",
            "run from a project with env metadata, or realize packages once so hangar metadata exists.",
        );
        return Err(2);
    }
    Ok(index)
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
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
    if parsed.flags.offline {
        return offline_refusal(theme, "add");
    }
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
    if parsed.flags.adapt {
        return cmd_add_adapt(theme, raw);
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
            if let Ok(plan) = load_project_plan(theme) {
                for source in channel_sources(&plan.table) {
                    if let Ok(exact) = resolve_source_channel(&source, &parsed.flags) {
                        Lock::record_source_channel(
                            &dir,
                            Lock::LockedSourceChannel {
                                name: source.name.clone(),
                                channel: source.channel.as_str().to_string(),
                                exact,
                            },
                        );
                    }
                }
            }
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

fn cmd_add_adapt(theme: &Theme, raw: &str) -> i32 {
    let source = if raw.contains(Syntax::REF_PROVIDER_AT) {
        match RefSpec::classify_provider_ref(raw) {
            Ok(r) => r.raw,
            Err(e) => {
                Output::ref_error(theme, &e);
                return 2;
            }
        }
    } else {
        let table = cwd_table();
        let spec = match RefSpec::classify_in(raw, &table) {
            Ok(s) => s,
            Err(e) => {
                Output::ref_error(theme, &e);
                return 2;
            }
        };
        match spec.source {
            RefSpec::Source::Path => format!("path@{}", spec.package),
            RefSpec::Source::Github => format!("github@{}", spec.package),
            RefSpec::Source::Named(name) => match table.upstream(&name) {
                Some(upstream) if upstream.starts_with("path:") => {
                    format!("path@{}", upstream.trim_start_matches("path:"))
                }
                Some(upstream) if upstream.starts_with("github:") => {
                    format!("github@{}", upstream.trim_start_matches("github:"))
                }
                _ => {
                    theme.error_coded(
                        "E1270",
                        "adapter draft needs source bytes",
                        "that named source does not point at a path or GitHub source tree.",
                        "write `Pkg.adapt(...)` by hand with `source: path@...`.",
                    );
                    return 2;
                }
            },
            RefSpec::Source::Nixpkgs => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "`nixpkgs:<pkg>` names a package in an index, not an upstream source tree.",
                    "use the package's source URL with `source: github@owner/repo#rev` or `source: path@vendor/pkg`.",
                );
                return 2;
            }
        }
    };
    let name = source
        .split(['/', ':', '@', '#'])
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("tool")
        .trim_end_matches(".git");
    println!(
        "Pkg.adapt(\n    name: \"{name}\",\n    source: {source},\n    recipe: Recipe.copy(),\n)"
    );
    0
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
            &format!(
                "there is no {} declaring any `fleet.<name>`.",
                Syntax::ENV_FILE
            ),
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
/// D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): `jet image <name>`
/// builds the named `.Oci` `image.<name>` module contribution into a native
/// OCI layout (`Jetpack::Image`). `.Iso` images ride the jetos installer tier
/// (Phase D, owner-gated — untouched here); `--push` is honestly gated on TLS
/// (E1268), never a fake push.
fn cmd_image(theme: &Theme, parsed: &Parsed) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "no image here",
            &format!(
                "there is no {} declaring any `image.<name>`.",
                Syntax::ENV_FILE
            ),
            "declare `module image.<name> { kind: .Oci, from: packages.<name> }`, then `jet image <name>`.",
        );
        return 2;
    };
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

    let available: Vec<String> = plan.images.iter().map(|i| i.name.clone()).collect();
    let declared = || {
        if available.is_empty() {
            format!("no `image.<name>` is declared in {}.", Syntax::ENV_FILE)
        } else {
            format!("declared images: {}.", available.join(", "))
        }
    };
    let Some(name) = parsed.positional.first() else {
        theme.error(
            "build which image?",
            &declared(),
            "name an image: `jet image <name>`.",
        );
        return 2;
    };

    let Some(image) = plan.images.iter().find(|i| &i.name == name) else {
        theme.error(
            &format!("no image `{name}`"),
            &declared(),
            "declare `module image.<name> { … }`, or build an existing image.",
        );
        return 2;
    };

    if image.kind != ModuleEval::ImageKind::Oci {
        theme.error(
            &format!("`{name}` is a `.Iso` disk image, not a container"),
            "U14: `.Iso`/`.Qcow`/`.Raw` disk images ride the jetos installer tier, which is gated (Phase D, owner greenlight required) — `jet image` only builds `.Oci` containers today.",
            "build an `.Oci` image instead, or track the jetos realization tier for disk images.",
        );
        return 2;
    }

    if let Some(push_ref) = &parsed.flags.push {
        theme.error_coded(
            "E1268",
            &format!("`jet image {name}` can't push to `{push_ref}` yet"),
            "D-JPK-IMAGE1: pushing to a registry needs TLS support for the connection, which jetpack doesn't have yet — `jet image` never fakes a push.",
            &format!(
                "build without `--push` (`jet image {name}`) and push the OCI layout with another tool for now; `--push` will work once TLS lands."
            ),
        );
        return 2;
    }

    if let Some(base) = &image.base {
        theme.error(
            &format!("`base: oci(\"{base}\")` isn't realized yet"),
            "D-JPK-IMAGE1: layering onto a base image needs a native registry-pull client, which doesn't exist yet — `jet image` never silently builds from scratch instead of the requested base.",
            "drop `base:` to build a from-scratch image, or track the registry-pull client.",
        );
        return 2;
    }

    // D-JPK-IMAGE1: build from what `jet build` already realized. Jetpack has
    // no dependency on the compiler's own build machinery (the dependency
    // runs the other way — `jet` depends on `jet-driver`, not vice versa), so
    // this mirrors, rather than calls into, `jet build`'s `build/<name>`
    // output convention (`Source/CmdCompile.rs::bin_path`).
    let bin_path = dir.join("build").join(&image.from);
    let Ok(bin_data) = std::fs::read(&bin_path) else {
        theme.error(
            &format!("`{}` isn't built yet", image.from),
            &format!(
                "`jet image {name}` needs `{}` already built at `{}`.",
                image.from,
                bin_path.display()
            ),
            &format!("run `jet build` first, then `jet image {name}`."),
        );
        return 2;
    };

    let mut files = vec![Image::LayerFile {
        path: format!("usr/local/bin/{}", image.from),
        data: bin_data,
        mode: 0o755,
    }];
    for rel in &image.files {
        let Ok(data) = std::fs::read(dir.join(rel)) else {
            theme.error(
                &format!("`{rel}` (from `files:`) doesn't exist"),
                &format!("`image.{name}`'s `files:` names `{rel}`, relative to the project dir."),
                "fix the path, or remove it from `files:`.",
            );
            return 2;
        };
        files.push(Image::LayerFile {
            path: rel.trim_start_matches('/').to_string(),
            data,
            mode: 0o644,
        });
    }

    let spec = Image::BuildSpec {
        files,
        entrypoint: vec![format!("/usr/local/bin/{}", image.from)],
        env: image.env_vars.clone(),
        expose: image.expose.clone(),
    };
    let out_dir = dir.join(".jet").join("images").join(name);
    match Image::build(&spec, &out_dir, name) {
        Ok(built) => {
            theme.ok(&format!(
                "built image `{name}` -> {} ({})",
                out_dir.display(),
                built.manifest_digest
            ));
            0
        }
        Err(e) => {
            theme.error(
                &format!("couldn't build image `{name}`"),
                &e.to_string(),
                "check that the output directory is writable.",
            );
            2
        }
    }
}

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
    let args = parsed.positional.get(1..).unwrap_or(&[]);
    let flags = super::JetOS::OsFlags {
        fixtures: parsed.flags.fixtures.clone(),
        offline: parsed.flags.offline,
        name: parsed.flags.os_name.clone(),
        manual_disk: parsed.flags.os_manual.clone(),
        disk: parsed.flags.os_disk.clone(),
        json: parsed.flags.json,
    };
    super::JetOS::main(theme, verb, args, &flags)
}

/// `jetos studio` — launch the installed first-party Studio app, with a
/// browser/headless fallback over the same generated projection.
fn cmd_studio(theme: &Theme, parsed: &Parsed) -> i32 {
    let headless = parsed
        .positional
        .iter()
        .any(|arg| arg == Syntax::STUDIO_FLAG_HEADLESS);
    let root = std::env::var_os("JETOS_STUDIO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/current-system"));
    let app = root.join("studio/index.html");
    let meta = root.join("studio/app.json");
    let data = root.join("studio/data.json");
    if !app.is_file() || !meta.is_file() || !data.is_file() {
        theme.error(
            "jetos Studio app is not installed",
            &format!(
                "`{}` does not contain studio/index.html, studio/app.json, and studio/data.json.",
                root.display()
            ),
            "activate a jetos generation, or set JETOS_STUDIO_ROOT to a generation path.",
        );
        return 2;
    }
    if parsed.flags.json {
        println!(
            "{{\"root\":{},\"app\":{},\"metadata\":{},\"data\":{},\"host\":{}}}",
            JSON::quote(&root.display().to_string()),
            JSON::quote(&app.display().to_string()),
            JSON::quote(&meta.display().to_string()),
            JSON::quote(&data.display().to_string()),
            JSON::quote(studio_host(parsed).as_deref().unwrap_or(""))
        );
        return 0;
    }
    if let Some(addr) = parsed.flags.studio_serve.as_deref() {
        let context = studio_context(parsed);
        return serve_studio(theme, addr, &app, &meta, &data, context.as_ref());
    }
    println!("{}", app.display());
    if headless {
        theme.ok("jetos Studio app ready");
        return 0;
    }
    match std::process::Command::new("xdg-open").arg(&app).spawn() {
        Ok(_) => {
            theme.ok("opened jetos Studio");
            0
        }
        Err(_) => {
            theme.ok("jetos Studio browser fallback ready");
            theme.detail("open the printed path in a browser.");
            0
        }
    }
}

struct StudioContext {
    config: PathBuf,
    host: String,
}

fn studio_host(parsed: &Parsed) -> Option<String> {
    parsed.flags.studio_host.clone()
}

fn studio_context(parsed: &Parsed) -> Option<StudioContext> {
    let project = parsed
        .positional
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let config = if project.is_dir() {
        project.join(Syntax::CONFIG_FILE)
    } else {
        project
    };
    let host = studio_host(parsed).unwrap_or_else(|| "host".to_string());
    Some(StudioContext { config, host })
}

fn serve_studio(
    theme: &Theme,
    addr: &str,
    app: &Path,
    meta: &Path,
    data: &Path,
    context: Option<&StudioContext>,
) -> i32 {
    let listener = match std::net::TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            theme.error(
                "jetos Studio could not bind the local service",
                &format!("binding `{addr}` failed: {e}"),
                "choose a free loopback address, for example `--serve 127.0.0.1:7417`.",
            );
            return 2;
        }
    };
    let local = listener
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| addr.to_string());
    println!("http://{local}/studio/");
    theme.ok("jetos Studio service listening");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let _ = handle_studio_request(&mut stream, app, meta, data, context);
            }
            Err(e) => {
                theme.error(
                    "jetos Studio service connection failed",
                    &format!("accepting a local connection failed: {e}"),
                    "restart `jetos studio --serve`.",
                );
                return 2;
            }
        }
    }
    0
}

fn handle_studio_request(
    stream: &mut std::net::TcpStream,
    app: &Path,
    meta: &Path,
    data: &Path,
    context: Option<&StudioContext>,
) -> std::io::Result<()> {
    use std::io::Write;
    let request_bytes = read_http_request(stream)?;
    let request = String::from_utf8_lossy(&request_bytes);
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("GET");
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    if method == "POST" && path == "/studio/transaction" {
        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
        let (status, body) = handle_studio_transaction(body, context);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        return stream.write_all(body.as_bytes());
    }
    if method == "POST" && path == "/studio/run" {
        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
        let (status, body) = handle_studio_run(body, context);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        return stream.write_all(body.as_bytes());
    }
    let (status, content_type, body) = match path {
        "/" | "/studio" | "/studio/" | "/studio/index.html" => {
            ("200 OK", "text/html; charset=utf-8", fs_read_for_http(app))
        }
        "/studio/app.json" => ("200 OK", "application/json", fs_read_for_http(meta)),
        "/studio/data.json" => ("200 OK", "application/json", fs_read_for_http(data)),
        "/studio/source" => match context {
            Some(context) => (
                "200 OK",
                "text/plain; charset=utf-8",
                fs_read_for_http(&context.config),
            ),
            None => (
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"missing Studio project context\n".to_vec(),
            ),
        },
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found\n".to_vec(),
        ),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(&body)
}

fn fs_read_for_http(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|_| b"missing\n".to_vec())
}

fn read_http_request(stream: &mut std::net::TcpStream) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if http_request_complete(&buf) || buf.len() > 64 * 1024 {
            break;
        }
    }
    Ok(buf)
}

fn http_request_complete(buf: &[u8]) -> bool {
    let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4) else {
        return false;
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_len = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    buf.len().saturating_sub(header_end) >= content_len
}

fn handle_studio_transaction(
    body: &str,
    context: Option<&StudioContext>,
) -> (&'static str, String) {
    let Some(context) = context else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing Studio project context\"}".to_string(),
        );
    };
    let op = json_string_field(body, "op").unwrap_or_default();
    if op != "set-option" {
        return (
            "400 Bad Request",
            "{\"error\":\"unsupported Studio transaction\"}".to_string(),
        );
    }
    let Some(key) = json_string_field(body, "key") else {
        return ("400 Bad Request", "{\"error\":\"missing key\"}".to_string());
    };
    let Some(value) = json_string_field(body, "value") else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing value\"}".to_string(),
        );
    };
    let write = json_bool_field(body, "write");
    let source = match std::fs::read_to_string(&context.config) {
        Ok(source) => source,
        Err(e) => {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{},\"path\":{}}}",
                    JSON::quote(&format!("reading config failed: {e}")),
                    JSON::quote(&context.config.display().to_string())
                ),
            )
        }
    };
    let (next, changed) = match apply_option_transaction(&source, &key, &value) {
        Ok(result) => result,
        Err(e) => {
            return (
                "400 Bad Request",
                format!("{{\"error\":{}}}", JSON::quote(&e)),
            )
        }
    };
    if write && changed {
        if let Err(e) = std::fs::write(&context.config, &next) {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{},\"path\":{}}}",
                    JSON::quote(&format!("writing config failed: {e}")),
                    JSON::quote(&context.config.display().to_string())
                ),
            );
        }
    }
    let diff = source_diff(&context.config, &source, &next);
    (
        "200 OK",
        format!(
            "{{\"host\":{},\"path\":{},\"op\":\"set-option\",\"key\":{},\"value\":{},\"write\":{},\"changed\":{},\"diff\":{}}}",
            JSON::quote(&context.host),
            JSON::quote(&context.config.display().to_string()),
            JSON::quote(&key),
            JSON::quote(&value),
            if write { "true" } else { "false" },
            if changed { "true" } else { "false" },
            JSON::quote(&diff)
        ),
    )
}

fn handle_studio_run(body: &str, context: Option<&StudioContext>) -> (&'static str, String) {
    let Some(context) = context else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing Studio project context\"}".to_string(),
        );
    };
    let Some(action) = json_string_field(body, "action") else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing action\"}".to_string(),
        );
    };
    if !["check", "plan", "build", "proof", "generations"].contains(&action.as_str()) {
        return (
            "400 Bad Request",
            "{\"error\":\"unsupported Studio run action\"}".to_string(),
        );
    }
    let Some(jet) = sibling_binary("jet") else {
        return (
            "500 Internal Server Error",
            "{\"error\":\"could not find sibling jet binary\"}".to_string(),
        );
    };
    let cwd = context
        .config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut cmd = std::process::Command::new(jet);
    cmd.arg("os")
        .arg(&action)
        .arg(&context.host)
        .arg("--no-color");
    if action == "plan" || action == "proof" {
        cmd.arg("--json");
    }
    if action == "build" {
        cmd.arg("--name").arg("zz-studio-candidate");
    }
    let output = match cmd.current_dir(&cwd).output() {
        Ok(output) => output,
        Err(e) => {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{}}}",
                    JSON::quote(&format!("running jet failed: {e}"))
                ),
            )
        }
    };
    (
        "200 OK",
        format!(
            "{{\"host\":{},\"action\":{},\"status\":{},\"success\":{},\"stdout\":{},\"stderr\":{}}}",
            JSON::quote(&context.host),
            JSON::quote(&action),
            output.status.code().unwrap_or(1),
            if output.status.success() { "true" } else { "false" },
            JSON::quote(&String::from_utf8_lossy(&output.stdout)),
            JSON::quote(&String::from_utf8_lossy(&output.stderr))
        ),
    )
}

fn sibling_binary(name: &str) -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let dir = current.parent()?;
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    #[cfg(unix)]
    {
        let debug = dir.parent()?.join(name);
        if debug.is_file() {
            return Some(debug);
        }
    }
    Some(PathBuf::from(name))
}

fn apply_option_transaction(
    source: &str,
    key: &str,
    value: &str,
) -> Result<(String, bool), String> {
    let mut lines = source.lines().map(str::to_string).collect::<Vec<_>>();
    let mut in_options = false;
    let mut insert_at = None;
    for (idx, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("options:") && trimmed.contains('[') {
            in_options = true;
            continue;
        }
        if in_options && trimmed.starts_with(']') {
            insert_at = Some(idx);
            break;
        }
        if in_options && trimmed.starts_with(&format!("{key}:")) {
            let indent_len = line.len() - trimmed.len();
            let indent = line[..indent_len].to_string();
            let comma = if trimmed.trim_end().ends_with(',') {
                ","
            } else {
                ""
            };
            let next = format!("{indent}{key}: {value}{comma}");
            let changed = *line != next;
            *line = next;
            let mut output = lines.join("\n");
            if source.ends_with('\n') {
                output.push('\n');
            }
            return Ok((output, changed));
        }
    }
    let Some(idx) = insert_at else {
        return Err("Studio could not find an options block in config.jet".to_string());
    };
    lines.insert(idx, format!("            {key}: {value},"));
    let mut output = lines.join("\n");
    if source.ends_with('\n') {
        output.push('\n');
    }
    Ok((output, true))
}

fn source_diff(path: &Path, before: &str, after: &str) -> String {
    if before == after {
        return format!("diff -- {}\n(no changes)\n", path.display());
    }
    let mut diff = format!("diff -- {}\n", path.display());
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let max = before_lines.len().max(after_lines.len());
    for idx in 0..max {
        let old = before_lines.get(idx).copied();
        let new = after_lines.get(idx).copied();
        if old == new {
            continue;
        }
        if let Some(old) = old {
            diff.push_str(&format!("-{old}\n"));
        }
        if let Some(new) = new {
            diff.push_str(&format!("+{new}\n"));
        }
    }
    diff
}

fn json_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = body.split_once(&needle)?.1;
    let rest = rest.split_once(':')?.1.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn json_bool_field(body: &str, key: &str) -> bool {
    let needle = format!("\"{key}\"");
    let Some(rest) = body.split_once(&needle).map(|(_, rest)| rest) else {
        return false;
    };
    let Some(rest) = rest.split_once(':').map(|(_, rest)| rest.trim_start()) else {
        return false;
    };
    rest.starts_with("true")
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
  {bin} clean                          collect stale hangar objects + optimize
  {bin} search <query>                 search the local offline package index
  {bin} info <source>.<package>         show local offline package metadata
  {bin} explain <ref>                  show resolution path and latest build status
  {bin} logs <pkg> --json              show persisted per-step build logs
  {bin} override draft <ref> --patch <file>
                                      draft reviewed workspace overlay policy

{machines}
  jet os check <host>                  validate ./config.jet system.<host>
  jet os plan <host> --json            print checked plan/proof input without building
  jet os proof <host> --json           print latest generation proof/provenance facts
  jet os build <host>                  build a named jetos generation
  jet os switch <host> [--name <name>] build + activate a named generation
  jet os generations [<host>]          list generations newest first
  jet os rollback <host> [<name>]      activate a previous generation
  jet os init <host> [--manual <path>] write starter ./config.jet
  jet os lift <host> [<root>]          draft ./config.jet from a host root
  jet os image <host> [--manual <path>] write jetos hybrid ISO media/proof
  jet os vm prove <host> --disk <path> boot installer, install, reboot, prove
  jet os vm test <vmtest> --disk <path> run declared VM scenario proof
  jetos studio [path] --host <host>    open installed jetos Studio app
  jetos studio [path] --serve 127.0.0.1:7417 serve browser/edit fallback
  {bin} push <fleet>                   validate a fleet's hosts (deploy is gated)
  {bin} services up   [<name>]         start dev services declared under env.*
  {bin} services down [<name>]         stop them
  {bin} services health [<name>]       one-shot readiness check
  {bin} services logs <name>           print a service's captured stdout/stderr
  {bin} image <name>                   build a declared `.Oci` image into a native OCI layout
  {bin} image <name> --push <ref>      (gated on TLS support, E1268 — not yet)

{trust}
  {bin} trust list                    show package/build/env/service/image/fleet/jetos grants
  {bin} trust explain [<grant>]        explain exact authority and revocation key
  {bin} trust grant <grant>            add a reviewed local grant
  {bin} trust revoke <grant>           drop a grant; next risky action asks again
  {bin} config trust add <pattern>     pre-authorize matching project paths
  {bin} config trust list              show trusted hashes and patterns
  {bin} config trust remove <pattern>  drop a trusted pattern
  {bin} config sandbox require         refuse unsandboxed build fallback
  {bin} config sandbox allow           allow fallback with L0205 warning

{refs}
  nixpkgs:fastfetch                    a package from nixpkgs
  github:owner/repo                    a Jet pack repo (or a flake fallback)
  path:./my-env                        a local pack/flake directory

{components}
  Button, Label, Input, Container      starter kit — ownable, editable .jet source

{flags}
  --no-color                           disable colored output (also: NO_COLOR)
  --offline                            resolve from fixtures only, never network
  --shell-on-fail                      after a failed build, open a shell in preserved scratch
  --fixtures <dir>                     read provider output from captured fixtures
  --trust                              skip the trust prompt for this one run
  --scope <user|repo>                  (trust grant) where the grant applies
  -p <pkg>...                          (enter) ad-hoc nixpkgs packages, not declared anywhere
  --flake                              (enter) force the foreign flake.nix/devenv.nix fallback
  --pure                               (enter) isolate the shell from the host environment
  --push <ref>                         (image) push after building — gated on TLS, E1268
  --name <name>                        (os switch) override generation name
  --manual <path>                      (os init/image) record manual disk path
  --disk <path>                        (os vm prove) target qcow2/raw disk image
  --headless                           (jetos studio) print app path without opening
  --serve <addr>                       (jetos studio) run local projection service
  --host <host>                        (jetos studio) select system host
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
        let fixtures = std::env::temp_dir().join("fx");
        let fixtures_arg = fixtures.to_string_lossy().to_string();
        let args: Vec<String> = ["--no-color", "--fixtures", &fixtures_arg, "nixpkgs:jq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert!(p.flags.no_color);
        assert_eq!(p.flags.fixtures, Some(fixtures));
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
