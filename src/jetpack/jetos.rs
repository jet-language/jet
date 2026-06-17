//! The jetos tier: `jetpack os <verb> [<config-path>]@<host>` (U15/U16).
//!
//! Whole-machine management. **Not** a separate `jetos` binary and **not** under
//! the `jet` tool (U15) — it is a subcommand group of `jetpack`. Verbs mirror
//! `nixos-rebuild`: `build` (realize the system into a generation) and `switch`
//! (build + activate: flip the `current` pointer and the boot `default`).
//!
//! A `config.jet` is the master config (`system.<name>:` / `image.<name>:`
//! contributions). It is loaded through the SAME typed-module path as `env.jet`
//! (`modeval::evaluate_env`), so the `System` field-checking + capture from gap #5
//! is reused verbatim. The `@host` selector (U16) picks which captured `System` to
//! realize; the optional path prefix names the config file (default
//! `~/.jet/config.jet`).
//!
//! Activation model (internal mechanics, not user-facing syntax — see brief):
//! each build assembles a content-addressed **generation directory** under the
//! managed store `<root>/systems/<host>-<fp>/` recording a `manifest.json`
//! (target, realized packages, services, options) — services/options are
//! *recorded as intent*, never started (there is no daemon yet, D-OS2..D-OS6 are
//! still open). `switch` additionally flips two symlinks: `current` (the active
//! generation) and `default` (the boot default).

use std::path::{Path, PathBuf};

use super::modeval::{self, SystemPlan};
use super::output::{self, Theme};
use super::provider;
use super::refspec::{self, SourceTable};
use super::store::{self, Roots};
use crate::diag::Diagnostic;
use crate::syntax;

/// Parsed global flags the os path honors (mirrors the package-command flags).
pub struct OsFlags {
    pub fixtures: Option<PathBuf>,
    pub offline: bool,
}

/// A `jetpack os` target `[<config-path>]@<host>` (U16), split on the LAST `@`
/// so a path may itself contain `@`.
struct OsTarget {
    /// `None` when no path prefix was given → default `~/.jet/config.jet`.
    config_path: Option<PathBuf>,
    host: String,
}

/// Dispatch `jetpack os <verb> ...`. `verb` is the token after `os`; `rest` are
/// the positional args (the `[<path>]@<host>` target) already stripped of flags.
pub fn main(theme: &Theme, verb: Option<&str>, target: Option<&str>, flags: &OsFlags) -> i32 {
    let Some(verb) = verb else {
        theme.error(
            "`jetpack os` needs a verb",
            &format!("the jetos verbs are: {}.", syntax::OS_VERBS.join(", ")),
            "try `jetpack os switch @<host>` or `jetpack os build @<host>`.",
        );
        return 2;
    };
    let activate = match verb {
        v if v == syntax::OS_VERB_SWITCH => true,
        v if v == syntax::OS_VERB_BUILD => false,
        other => {
            theme.error(
                &format!("`{other}` is not a `jetpack os` verb"),
                &format!("the jetos verbs are: {}.", syntax::OS_VERBS.join(", ")),
                "use `switch` (build + activate) or `build` (build only).",
            );
            return 2;
        }
    };

    let Some(raw) = target else {
        return os_missing_host(theme, "");
    };
    let target = match parse_target(raw) {
        Ok(t) => t,
        Err(()) => return os_missing_host(theme, raw),
    };

    let config_path = resolve_config_path(target.config_path.as_deref());
    let src = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => {
            render(theme, &config_path, "", &config_file_missing(&config_path));
            return 2;
        }
    };
    let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let plan = match modeval::evaluate_env(&src, base_dir) {
        Ok(p) => p,
        Err(d) => {
            render(theme, &config_path, &src, &d);
            return 2;
        }
    };

    let Some(system) = plan.systems.iter().find(|s| s.name == target.host) else {
        let names: Vec<String> = plan.systems.iter().map(|s| s.name.clone()).collect();
        render(theme, &config_path, &src, &unknown_host(&target.host, &names));
        return 2;
    };

    let roots = store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "dev mode: using {} (no write access to {})",
            roots.root.display(),
            "/etc/jet"
        )));
    }
    // A `config.jet` referring to a relative `path@./jet-pkgs` source means
    // "next to me". The core provider resolves relative `path:` upstreams against
    // the process cwd, so anchor it to the config's directory before realizing —
    // the same mental model as running `jetpack run` from a project folder.
    // (JETPACK_ROOT / the managed store are absolute, so this never moves them.)
    if let Ok(abs) = std::fs::canonicalize(base_dir) {
        let _ = std::env::set_current_dir(&abs);
    }
    realize_system(theme, &roots, flags, &plan.table, system, activate)
}

/// Parse `[<config-path>]@<host>` (U16). `<host>` is everything after the LAST
/// `@`; the optional path is the prefix before it. An empty path → the default
/// location. A missing `@` or an empty host is an error.
fn parse_target(raw: &str) -> Result<OsTarget, ()> {
    let raw = raw.trim();
    let (path, host) = raw.rsplit_once(syntax::OS_HOST_SELECTOR).ok_or(())?;
    if host.is_empty() {
        return Err(());
    }
    let config_path = if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    };
    Ok(OsTarget {
        config_path,
        host: host.to_string(),
    })
}

/// Resolve the config file: an explicit path as written, else the default
/// `~/.jet/config.jet` (U16).
fn resolve_config_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(syntax::CONFIG_DEFAULT_DIR).join(syntax::CONFIG_FILE)
}

/// Realize a selected `System` into a generation; `activate` also flips the
/// `current`/`default` pointers (the `switch` verb).
fn realize_system(
    theme: &Theme,
    roots: &Roots,
    flags: &OsFlags,
    table: &SourceTable,
    system: &SystemPlan,
    activate: bool,
) -> i32 {
    theme.status(&format!(
        "building system {} ({})",
        theme.bold(&system.name),
        theme.gray(&system.target)
    ));

    // Realize every package through the existing provider boundary into the
    // shared hangar, exactly like the dev-shell path (codegen/realize stays dumb,
    // I3). Each Pkg → a `<source>:<package>` ref classified against the config's
    // source table.
    let mut realized: Vec<store::StoreEntry> = Vec::new();
    for pkg in &system.packages {
        let raw = modeval::pkg_ref(pkg);
        let spec = match refspec::classify_in(&raw, table) {
            Ok(s) => s,
            Err(e) => {
                output::ref_error(theme, &e);
                return 2;
            }
        };
        match realize_one(theme, roots, flags, table, &spec) {
            Some(entry) => realized.push(entry),
            None => return 1,
        }
    }

    // Assemble the generation: a content-addressed directory recording the
    // realized machine. Services/options are recorded as *intent* (U12/U13) so
    // they're never silently dropped — there is no daemon to start them yet.
    let manifest = build_manifest(system, &realized);
    let gen_dir = match write_generation(roots, &system.name, &manifest) {
        Ok(dir) => dir,
        Err(e) => {
            theme.error(
                "could not write the system generation",
                &format!("{e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            return 1;
        }
    };
    theme.ok(&format!(
        "generation ready: {}",
        theme.bold(&gen_dir.file_name().unwrap_or_default().to_string_lossy())
    ));
    theme.detail(&theme.gray(&gen_dir.to_string_lossy()));
    report_intent(theme, system);

    if activate {
        match activate_generation(roots, &gen_dir) {
            Ok(()) => {
                theme.ok(&format!(
                    "activated {} (current + boot default)",
                    theme.bold(&system.name)
                ));
            }
            Err(e) => {
                theme.error(
                    "could not activate the generation",
                    &format!("{e}"),
                    "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                );
                return 1;
            }
        }
    } else {
        theme.status(&format!(
            "built system {} ({} package(s)); run `jetpack os switch` to activate.",
            system.name,
            realized.len()
        ));
    }
    0
}

/// Realize one ref through the provider, honoring offline/fixtures, and record it
/// in the hangar. Mirrors `cli::realize_ref` but scoped to the os path.
fn realize_one(
    theme: &Theme,
    roots: &Roots,
    flags: &OsFlags,
    table: &SourceTable,
    spec: &refspec::RefSpec,
) -> Option<store::StoreEntry> {
    theme.status(&format!("resolving {} …", theme.bold(&spec.raw)));
    let store_dir = roots.hangar_dir();
    let fixtures = if flags.offline
        && provider::uses_nix_provider(spec, table, flags.offline, &store_dir)
    {
        let fx = provider::fixtures_from_env(flags.fixtures.clone());
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
            match store::record(roots, &r.name, &r.version, &r.reference, &r.out, &r.bin) {
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
            super::cli::report_provider_error(theme, &e);
            None
        }
    }
}

/// Surface the recorded services/options so the user sees the machine intent.
fn report_intent(theme: &Theme, system: &SystemPlan) {
    for svc in &system.services {
        let state = if svc.enable { "enabled" } else { "disabled" };
        theme.detail(&theme.gray(&format!("service {} → {state}", svc.name)));
    }
    for opt in &system.options {
        theme.detail(&theme.gray(&format!("option {} = {}", opt.key, opt.value)));
    }
}

/// The canonical-ish JSON manifest a generation records. Hand-built (I6, no serde)
/// with the project's `json` helpers, deterministic field order.
fn build_manifest(system: &SystemPlan, realized: &[store::StoreEntry]) -> String {
    use super::json::quote;
    let pkgs = realized
        .iter()
        .map(|e| {
            format!(
                "{{\"name\":{},\"ref\":{},\"out\":{}}}",
                quote(&e.name),
                quote(&e.reference),
                quote(&e.out)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let services = system
        .services
        .iter()
        .map(|s| {
            let enable = if s.enable { "true" } else { "false" };
            format!("{{\"name\":{},\"enable\":{enable}}}", quote(&s.name))
        })
        .collect::<Vec<_>>()
        .join(",");
    let options = system
        .options
        .iter()
        .map(|o| format!("{{\"key\":{},\"value\":{}}}", quote(&o.key), quote(&o.value)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"system\":{},\"target\":{},\"packages\":[{pkgs}],\"services\":[{services}],\"options\":[{options}]}}",
        quote(&system.name),
        quote(&system.target),
    )
}

/// The managed system store dir, `<root>/systems`.
fn systems_dir(roots: &Roots) -> PathBuf {
    roots.root.join("systems")
}

/// Write a content-addressed generation directory and its manifest. The dir name
/// is `<host>-<fp>` where `<fp>` fingerprints the manifest, so an identical
/// machine reuses its generation and a change gets a fresh one.
fn write_generation(roots: &Roots, host: &str, manifest: &str) -> std::io::Result<PathBuf> {
    let fp = crate::sha256::sha256_hex(manifest.as_bytes());
    let dir = systems_dir(roots).join(format!("{host}-{}", &fp[..12]));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("manifest.json"), manifest)?;
    Ok(dir)
}

/// Activate a generation (the `switch` verb): point `current` at it (the active
/// system) and `default` at it (the boot default). Atomic-ish: write a temp
/// symlink then rename over the old pointer, so a crash never leaves a dangling
/// pointer. Internal mechanic only — no user-facing syntax.
fn activate_generation(roots: &Roots, gen_dir: &Path) -> std::io::Result<()> {
    point(&systems_dir(roots).join("current"), gen_dir)?;
    point(&systems_dir(roots).join("default"), gen_dir)?;
    Ok(())
}

/// Repoint `link` at `target`, replacing any existing pointer atomically.
fn point(link: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = link.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    symlink(target, &tmp)?;
    std::fs::rename(&tmp, link)
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    // Non-Unix fallback: record the target path in a plain file. Activation is a
    // Unix-first feature; this keeps the build portable.
    std::fs::write(link, target.to_string_lossy().as_bytes())
}

fn render(theme: &Theme, path: &Path, src: &str, d: &Diagnostic) {
    let _ = theme;
    eprint!(
        "{}",
        crate::diag::render_all(&path.to_string_lossy(), src, std::slice::from_ref(d))
    );
}

fn os_missing_host(theme: &Theme, raw: &str) -> i32 {
    render(theme, Path::new(syntax::CONFIG_FILE), "", &missing_host(raw));
    2
}

/// E0979: a `jetpack os` target with no `@host` selector (U16).
fn missing_host(raw: &str) -> Diagnostic {
    let got = if raw.is_empty() {
        "no target was given".to_string()
    } else {
        format!("`{raw}` has no `@host`")
    };
    Diagnostic::error(
        "E0979",
        format!("`jetpack os` needs a `@host` to apply — {got}"),
        "U16: `jetpack os <verb>` takes `[<config-path>]@<host>` — the `@host` segment selects which `System` in the config to apply, and it is required".to_string(),
        "write `jetpack os switch @<host>` (default config) or `jetpack os switch ./config.jet@<host>`".to_string(),
        None,
    )
}

/// E0980: the `@host` selector names a `System` no config contribution defines (U16).
fn unknown_host(host: &str, known: &[String]) -> Diagnostic {
    let hint = if known.is_empty() {
        "this config defines no `system.<name>:` contribution".to_string()
    } else {
        format!("available systems: {}", known.join(", "))
    };
    Diagnostic::error(
        "E0980",
        format!("no system `{host}` in this config"),
        "U16: the `@host` selector picks which `System` to apply; it must name a `system.<name>:` contribution the config defines".to_string(),
        format!("define `system.{host}: {{ … }}`, or select an existing one ({hint})"),
        None,
    )
}

/// E0981: the config file named by the path prefix (or the default location)
/// doesn't exist (U16).
fn config_file_missing(path: &Path) -> Diagnostic {
    Diagnostic::error(
        "E0981",
        format!("no config file at `{}`", path.display()),
        format!(
            "U16: `jetpack os <verb>` loads `[<config-path>]@<host>`; with no path prefix it defaults to `~/.jet/{}`",
            syntax::CONFIG_FILE
        ),
        "create the config file, or pass an explicit path before the `@`, e.g. `jetpack os switch ./config.jet@<host>`".to_string(),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_and_host() {
        let t = parse_target("./jet-test@halcyon").unwrap();
        assert_eq!(t.config_path, Some(PathBuf::from("./jet-test")));
        assert_eq!(t.host, "halcyon");
    }

    #[test]
    fn parses_bare_host_with_default_path() {
        let t = parse_target("@halcyon").unwrap();
        assert_eq!(t.config_path, None);
        assert_eq!(t.host, "halcyon");
    }

    #[test]
    fn splits_on_last_at() {
        // A path may contain `@`; only the final `@` separates the host.
        let t = parse_target("user@host/cfg.jet@box").unwrap();
        assert_eq!(t.config_path, Some(PathBuf::from("user@host/cfg.jet")));
        assert_eq!(t.host, "box");
    }

    #[test]
    fn rejects_missing_at() {
        assert!(parse_target("./config.jet").is_err());
    }

    #[test]
    fn rejects_empty_host() {
        assert!(parse_target("./config.jet@").is_err());
    }

    #[test]
    fn default_path_is_home_dot_jet_config() {
        let p = resolve_config_path(None);
        assert!(p.ends_with(".jet/config.jet"), "{}", p.display());
    }

    // ── pinned diagnostics (I4): exact what/why/fix rendering ──────────────

    #[test]
    fn missing_host_renders_pinned() {
        let d = missing_host("./config.jet");
        assert_eq!(d.code, "E0979");
        let r = crate::diag::render_all("config.jet", "", std::slice::from_ref(&d));
        assert_eq!(
            r,
            "Error [E0979]: `jetpack os` needs a `@host` to apply — `./config.jet` has no `@host`\n Why: U16: `jetpack os <verb>` takes `[<config-path>]@<host>` — the `@host` segment selects which `System` in the config to apply, and it is required\n Fix: write `jetpack os switch @<host>` (default config) or `jetpack os switch ./config.jet@<host>`\n"
        );
    }

    #[test]
    fn unknown_host_lists_known_systems() {
        let d = unknown_host("nope", &["halcyon".to_string(), "box".to_string()]);
        assert_eq!(d.code, "E0980");
        let r = crate::diag::render_all("config.jet", "", std::slice::from_ref(&d));
        assert_eq!(
            r,
            "Error [E0980]: no system `nope` in this config\n Why: U16: the `@host` selector picks which `System` to apply; it must name a `system.<name>:` contribution the config defines\n Fix: define `system.nope: { … }`, or select an existing one (available systems: halcyon, box)\n"
        );
    }

    #[test]
    fn missing_config_renders_pinned() {
        let d = config_file_missing(Path::new("/nope/config.jet"));
        assert_eq!(d.code, "E0981");
        let r = crate::diag::render_all("config.jet", "", std::slice::from_ref(&d));
        assert_eq!(
            r,
            "Error [E0981]: no config file at `/nope/config.jet`\n Why: U16: `jetpack os <verb>` loads `[<config-path>]@<host>`; with no path prefix it defaults to `~/.jet/config.jet`\n Fix: create the config file, or pass an explicit path before the `@`, e.g. `jetpack os switch ./config.jet@<host>`\n"
        );
    }
}
