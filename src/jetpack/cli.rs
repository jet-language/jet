//! Jetpack Phase 1 command dispatch (D-JPK2/9).
//!
//! `jetpack run/build/list/clean/add/remove`. Independent from the `jet`
//! binary (D-JPK1). All user-facing output flows through `output::Theme`.

use super::output::{self, Theme};
use super::provider::{self, ProviderError};
use super::refspec::{self, RefSpec};
use super::shell::{self, Env, ShellKind};
use super::store::{self, Roots};
use super::{envfile, modeval, refspec::RefError};
use crate::syntax;
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
        "build" => cmd_build(&theme, &parsed),
        "list" => cmd_list(&theme),
        "clean" => cmd_clean(&theme),
        "add" => cmd_add(&theme, &parsed),
        "remove" => cmd_remove(&theme, &parsed),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            0
        }
        other => {
            theme.error(
                &format!("`{other}` is not a jetpack command"),
                &format!(
                    "Phase 1 commands are: {}.",
                    syntax::JETPACK_VERBS.join(", ")
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
    provider::fixtures_from_env(flags.fixtures.clone())
}

/// The named-source table declared by the current project's env file (empty
/// when there is none). Used so explicit CLI refs are project-aware.
fn cwd_table() -> refspec::SourceTable {
    envfile::load(&std::env::current_dir().unwrap_or_default())
        .map(|ef| ef.source_table())
        .unwrap_or_else(refspec::SourceTable::empty)
}

/// Classify an explicit CLI ref, accepting any named source declared in the
/// current project's env file so `jetpack run stable:ripgrep` works there.
/// Prints the diagnostic on failure.
fn classify_or_report(theme: &Theme, raw: &str) -> Result<RefSpec, RefError> {
    refspec::classify_in(raw, &cwd_table()).map_err(|e| {
        output::ref_error(theme, &e);
        e
    })
}

/// Realize one ref, recording it in the store and printing progress. `table`
/// resolves named sources (D-JPK17); it is empty for direct CLI refs.
fn realize_ref(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &refspec::SourceTable,
    spec: &RefSpec,
) -> Option<store::StoreEntry> {
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
    let fixtures = if flags.offline
        && provider::uses_nix_provider(spec, table, flags.offline, &store_dir)
    {
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
    let ctx = provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
    };
    match provider::realize(spec, table, &ctx) {
        Ok(r) => {
            theme.ok(&format!("{} ready", theme.bold(&r.name)));
            theme.detail(&theme.gray(&r.out));
            match store::record(roots, &r.name, &r.reference, &r.out, &r.bin) {
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

fn report_provider_error(theme: &Theme, err: &ProviderError) {
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
    refs: Vec<RefSpec>,
    table: refspec::SourceTable,
    label: String,
}

/// Build a plan from the project `env.jet` (the no-explicit-ref path). `Err`
/// carries the exit code to return.
fn load_project_plan(theme: &Theme) -> Result<RunPlan, i32> {
    let dir = std::env::current_dir().unwrap_or_default();
    let Ok(src) = std::fs::read_to_string(envfile::path_in(&dir)) else {
        theme.error(
            "nothing to do",
            &format!(
                "no ref was given and there is no {} here.",
                syntax::ENV_FILE
            ),
            "try `jetpack run nixpkgs:fastfetch`, or `jetpack add <ref>` first.",
        );
        return Err(2);
    };

    // Two author surfaces share one file. The typed `module { … }` surface
    // (U3/U6/U8) is evaluated through `modeval`; the Phase-1 `pkg.*` directive
    // surface stays the fallback until the typed example fully replaces it.
    if modeval::is_module_surface(&src) {
        return typed_plan(theme, &src, &dir);
    }

    let ef = envfile::parse(&src);
    let table = ef.source_table();
    let refs = classify_all(theme, ef.refs().iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        refs,
        table,
        label: ef.prompt_label(),
    })
}

/// Evaluate the typed `module { … }` env surface (U3/U6/U8) into a plan. Source
/// refs merge across modules and `Pkg` sugar resolves to `<source>:<package>`
/// refs; the merged `prompt` becomes the shell label.
fn typed_plan(theme: &Theme, src: &str, dir: &Path) -> Result<RunPlan, i32> {
    let plan = modeval::evaluate_env(src, dir).map_err(|d| {
        eprint!(
            "{}",
            crate::diag::render_all(syntax::ENV_FILE, src, std::slice::from_ref(&d))
        );
        2
    })?;
    let refs = classify_all(theme, plan.package_refs.iter().map(String::as_str), &plan.table)?;
    Ok(RunPlan {
        refs,
        table: plan.table,
        label: plan
            .prompt
            .unwrap_or_else(|| syntax::JETPACK_PROMPT_LABEL.to_string()),
    })
}

/// Classify a sequence of `<source>:<package>` refs against `table`, printing
/// the first failure and returning exit code 2.
fn classify_all<'a>(
    theme: &Theme,
    raws: impl Iterator<Item = &'a str>,
    table: &refspec::SourceTable,
) -> Result<Vec<RefSpec>, i32> {
    let mut refs = Vec::new();
    for raw in raws {
        match refspec::classify_in(raw, table) {
            Ok(s) => refs.push(s),
            Err(e) => {
                output::ref_error(theme, &e);
                return Err(2);
            }
        }
    }
    Ok(refs)
}

/// `jetpack run [<ref>] [-- cmd…]`
fn cmd_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = store::resolve();
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
                label: syntax::JETPACK_PROMPT_LABEL.to_string(),
            },
            Err(_) => return 2,
        },
        None => match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };

    let mut bin_dirs = Vec::new();
    let mut realized_refs = Vec::new();
    for spec in &plan.refs {
        let Some(entry) = realize_ref(theme, &roots, &parsed.flags, &plan.table, spec) else {
            return 1;
        };
        // A `library` package realizes with an empty `bin` (U10) — it stages
        // source for import and contributes nothing to PATH.
        if !entry.bin.is_empty() {
            bin_dirs.push(entry.bin);
        }
        realized_refs.push(entry.reference);
    }
    let label = plan.label;

    let env = Env {
        bin_dirs,
        refs: realized_refs,
        label,
    };

    match &parsed.command {
        Some(cmd) if !cmd.is_empty() => shell::run_command(&env, cmd),
        _ => shell::enter(theme, &env, ShellKind::detect()),
    }
}

/// `jetpack build [<ref>]` — realize without entering a shell.
fn cmd_build(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = store::resolve();
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
    let roots = store::resolve();
    let entries = store::list(&roots);
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
    let roots = store::resolve();
    match store::clean(&roots) {
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
    let table = envfile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(refspec::SourceTable::empty);
    let spec = match refspec::classify_in(raw, &table) {
        Ok(s) => s,
        Err(e) => {
            output::ref_error(theme, &e);
            return 2;
        }
    };
    match envfile::add(&dir, &spec) {
        Ok(ef) => {
            theme.ok(&format!(
                "added {} to {}",
                theme.bold(&spec.package),
                syntax::ENV_FILE
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
    let table = envfile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(refspec::SourceTable::empty);
    let spec = match refspec::classify_in(raw, &table) {
        Ok(s) => s,
        Err(e) => {
            output::ref_error(theme, &e);
            return 2;
        }
    };
    match envfile::remove(&dir, &spec) {
        Ok((_ef, true)) => {
            theme.ok(&format!(
                "removed {} from {}",
                theme.bold(&spec.package),
                syntax::ENV_FILE
            ));
            0
        }
        Ok((_ef, false)) => {
            theme.status(&format!(
                "{} was not in {}.",
                spec.package,
                syntax::ENV_FILE
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

fn usage() -> String {
    let bin = syntax::JETPACK_BINARY_NAME;
    format!(
        "\
{bin} — Jet's package manager (Phase 1)

usage:
  {bin} run   <source>:<package>        enter a temporary shell with that package
  {bin} run   <source>:<package> -- cmd run a command in that environment, then exit
  {bin} run                            enter the shell described by ./{pack}
  {bin} build [<source>:<package>]     realize a package/environment, don't enter
  {bin} list                           show realized packages
  {bin} clean                          drop unused store records
  {bin} add    <source>:<package>      add a package to ./{pack}
  {bin} remove <source>:<package>      remove a package from ./{pack}

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
        pack = syntax::ENV_FILE,
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
