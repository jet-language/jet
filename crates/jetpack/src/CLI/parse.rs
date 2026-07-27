use super::add_remove_push_image::{cmd_add, cmd_image, cmd_push, cmd_remove};
use super::bridge_os_studio::{cmd_bridge, cmd_os, cmd_studio, cmd_user};
use super::cmd_doctor;
use super::package_hangar_vendor::{cmd_audit, cmd_clean, cmd_hangar, cmd_list, cmd_vendor};
use super::run_enter_dev::{cmd_dev, cmd_enter, cmd_run};
use super::services_secrets_config::{cmd_config, cmd_secrets, cmd_service_probe, cmd_services};
use super::tool::cmd_tool;
use super::browser::cmd_browser;
use super::trust_env_build::{cmd_build, cmd_test, cmd_trust};
use super::update_search_info::{
    cmd_explain, cmd_info, cmd_logs, cmd_outdated, cmd_override, cmd_search, cmd_update,
};
use crate::Output::Theme;
use crate::Store;
use crate::Syntax;
use jet_foundation::Terminal::ColorChoice;
use std::io::IsTerminal;
use std::path::PathBuf;

/// Parsed global flags shared by every command.
#[derive(Clone)]
pub(super) struct Flags {
    pub(super) color: ColorChoice,
    pub(super) fixtures: Option<PathBuf>,
    pub(super) offline: bool,
    pub(super) online: bool,
    /// U19: one-shot bypass of the env/dev trust gate (`--trust`). Never
    /// persists a grant — unlike accepting the interactive prompt.
    pub(super) trust: bool,
    /// U16: ad-hoc nixpkgs packages from `-p <pkg>...`, added to the shell
    /// without being declared in any manifest. Repeatable across multiple
    /// `-p` groups. On `build`/`test`/`run`, the same flag means workspace
    /// member names (D-JPK-SELECTOR1=C) — see `workspace_members`.
    pub(super) packages: Vec<String>,
    /// D-JPK-SELECTOR1=C: exact workspace member names from `-p` on
    /// build/test/run (cargo-style, repeatable).
    pub(super) workspace_members: Vec<String>,
    /// D-JPK-SELECTOR1=C: `--affected`.
    pub(super) affected: bool,
    /// D-JPK-SELECTOR1=C: `--affected-since <ref>`.
    pub(super) affected_since: Option<String>,
    /// U16: `--flake` forces foreign-flake/devenv detection even when the
    /// project's own manifest already declares `env.*` modules.
    pub(super) flake: bool,
    /// U16: `--pure` — isolate the shell from the host environment. Threaded
    /// straight through to the underlying `nix` invocation for the
    /// foreign-flake fallback; jetpack's own composed shells are already
    /// PATH-only, so this is a no-op there today.
    pub(super) pure: bool,
    /// D-JPK-IMAGE1: `jet image <name> --push <ref>` — the registry ref to
    /// push to. Always honestly gated (E1268): pushing needs TLS support that
    /// doesn't exist yet, so this is only ever read to report that gate.
    pub(super) push: Option<String>,
    /// D-JPK-OSGEN1=C: optional generation name for `jet os switch`.
    pub(super) os_name: Option<String>,
    /// D-JPK-OSDISK1=C: optional manual disk/device path for `jet os init|image`.
    pub(super) os_manual: Option<String>,
    /// D-JOS-VMCOMMAND1=A: optional VM proof disk image path.
    pub(super) os_disk: Option<String>,
    /// D-JOS-STUDIO-HOST1=A: local Studio projection service address.
    pub(super) studio_serve: Option<String>,
    /// D-JOS-STUDIO-HOST1=A: selected jetos host for Studio.
    pub(super) studio_host: Option<String>,
    /// U20: `jetpack add <ref> --adapt` drafts an adapter declaration instead
    /// of editing `env.jet` with a plain package ref.
    pub(super) adapt: bool,
    /// Emit machine-readable output for diagnostics that have structured
    /// payloads (currently U23 no-Nix package holes).
    pub(super) json: bool,
    /// U27: open a shell in preserved failed build scratch.
    pub(super) shell_on_fail: bool,
    /// D-JPK-GRANTCMD1=A: `jet trust grant <selector> --scope repo|user`.
    pub(super) trust_scope: Option<String>,
    /// D-FE-CLI1: bypass mutation confirmation gates (`-y` / `--yes`).
    pub(super) assume_yes: bool,
    /// D-JPK-TOOLRUN1: `jetpack tool install <ref> --as <name>` bin rename.
    pub(super) as_name: Option<String>,
    /// D-BROWSER-AUTO1=A (#1187): `jetpack browser lock --binary <path>`.
    pub(super) browser_binary: Option<String>,
    /// D-BROWSER-AUTO1=A (#1187): optional `--version` override for lock/provision.
    pub(super) browser_version: Option<String>,
    /// D-BROWSER-AUTO1=A (#1187): optional BiDi `--protocol` pin.
    pub(super) browser_protocol: Option<String>,
}

/// Result of separating flags, positional args, and a trailing `-- cmd`.
#[derive(Clone)]
pub(super) struct Parsed {
    pub(super) flags: Flags,
    pub(super) positional: Vec<String>,
    /// Everything after a `--`, if present.
    pub(super) command: Option<Vec<String>>,
}

// Verb-agnostic wrapper kept for usage_tests while the D-JPK-SELECTOR1
// dispatch migration to `parse_args_for` is in flight; only test code calls
// it today.
#[allow(dead_code)]
pub(super) fn parse_args(args: &[String]) -> Parsed {
    parse_args_for("", args)
}

/// Verb-aware parse so `-p` means ad-hoc nixpkgs on `enter` and workspace
/// members on `build`/`test`/`run` (D-JPK-SELECTOR1=C).
pub(super) fn parse_args_for(verb: &str, args: &[String]) -> Parsed {
    let workspace_select = matches!(verb, "build" | "test" | "run");
    let mut flags = Flags {
        color: ColorChoice::Auto,
        fixtures: None,
        offline: false,
        online: false,
        trust: false,
        packages: Vec::new(),
        workspace_members: Vec::new(),
        affected: false,
        affected_since: None,
        flake: false,
        pure: false,
        push: None,
        adapt: false,
        json: false,
        shell_on_fail: false,
        trust_scope: None,
        assume_yes: false,
        as_name: None,
        browser_binary: None,
        browser_version: None,
        browser_protocol: None,
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
            "--no-color" => flags.color = ColorChoice::Never,
            a if a.starts_with("--color=") => {
                flags.color = ColorChoice::parse(a.trim_start_matches("--color="));
            }
            "--offline" => flags.offline = true,
            "--online" => flags.online = true,
            a if a == Syntax::CLI_FLAG_YES_SHORT || a == Syntax::CLI_FLAG_YES_LONG => {
                flags.assume_yes = true;
            }
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
            a if a == Syntax::WS_FLAG_AFFECTED => flags.affected = true,
            a if a == Syntax::WS_FLAG_AFFECTED_SINCE => {
                i += 1;
                if let Some(r) = args.get(i) {
                    flags.affected_since = Some(r.clone());
                } else {
                    flags.affected_since = Some(String::new());
                }
            }
            a if a.starts_with(&format!("{}=", Syntax::WS_FLAG_AFFECTED_SINCE)) => {
                if let Some(v) = a.split_once('=').map(|(_, v)| v.to_string()) {
                    flags.affected_since = Some(v);
                }
            }
            a if a == Syntax::ENV_FLAG_PACKAGE => {
                // U16 enter: `-p <pkg>...` greedily consumes bare tokens.
                // D-JPK-SELECTOR1 build/test/run: each `-p` takes exactly one
                // member name (cargo-style, repeatable).
                i += 1;
                if workspace_select {
                    if let Some(next) = args.get(i).filter(|s| *s != "--" && !s.starts_with('-')) {
                        flags.workspace_members.push(next.clone());
                        i += 1;
                    }
                    continue;
                }
                while let Some(next) = args.get(i) {
                    if next == "--" || next.starts_with('-') {
                        break;
                    }
                    flags.packages.push(next.clone());
                    i += 1;
                }
                continue;
            }
            a if a == Syntax::TOOL_FLAG_AS => {
                i += 1;
                if let Some(name) = args.get(i) {
                    flags.as_name = Some(name.clone());
                }
            }
            a if a == Syntax::BROWSER_FLAG_BINARY => {
                i += 1;
                if let Some(path) = args.get(i) {
                    flags.browser_binary = Some(path.clone());
                }
            }
            a if a == Syntax::BROWSER_FLAG_VERSION => {
                i += 1;
                if let Some(version) = args.get(i) {
                    flags.browser_version = Some(version.clone());
                }
            }
            a if a == Syntax::BROWSER_FLAG_PROTOCOL => {
                i += 1;
                if let Some(protocol) = args.get(i) {
                    flags.browser_protocol = Some(protocol.clone());
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
        let theme = Theme::resolve_choice(ColorChoice::Auto);
        eprintln!("{}", super::usage_tests::usage_with_color(theme.color));
        return 2;
    };
    let parsed = parse_args_for(verb, rest);
    let color = if parsed.flags.json {
        ColorChoice::Never
    } else {
        parsed.flags.color
    };
    if let Some(diag) = crate::MemberSelect::reject_filter_dsl(rest) {
        let theme = Theme::resolve_choice(color);
        theme.error_coded(&diag.code, &diag.what, &diag.why, &diag.fix);
        return 2;
    }
    let theme = Theme::resolve_choice(color);
    // Doctor must observe state without repairing or migrating it.
    if verb != "doctor" {
        if let Err(error) = Store::migrate_nix_gc_roots(&Store::resolve()) {
            Store::report_integrity(
                &theme,
                &Store::IntegrityFailure {
                    package: "Nix compatibility closure".to_string(),
                    version: "legacy".to_string(),
                    expected: "durable GC root".to_string(),
                    actual: error.to_string(),
                    reason: "Nix GC-root migration".to_string(),
                    disposition: "Jetpack stopped before any package path could be consumed."
                        .to_string(),
                    fix: "Restore access to `nix-store`, then rerun this command before using the package."
                        .to_string(),
                },
            );
            return 2;
        }
    }

    match verb.as_str() {
        "doctor" => cmd_doctor(&theme, &parsed),
        "run" => cmd_run(&theme, &parsed),
        "enter" => cmd_enter(&theme, &parsed),
        v if v == Syntax::DEV_SUBCOMMAND => cmd_dev(&theme, &parsed),
        v if v == Syntax::CONFIG_SUBCOMMAND => cmd_config(&theme, &parsed),
        v if v == Syntax::TRUST_SUBCOMMAND => cmd_trust(&theme, &parsed),
        "build" => cmd_build(&theme, &parsed),
        "test" => cmd_test(&theme, &parsed),
        "list" => cmd_list(&theme),
        "hangar" => cmd_hangar(&theme, &parsed),
        "vendor" => cmd_vendor(&theme, &parsed),
        "audit" => cmd_audit(&theme),
        "clean" => cmd_clean(&theme, &parsed),
        "add" => cmd_add(&theme, &parsed),
        "remove" => cmd_remove(&theme, &parsed),
        "update" => cmd_update(&theme, &parsed),
        "outdated" => cmd_outdated(&theme, &parsed),
        "search" => cmd_search(&theme, &parsed),
        "info" => cmd_info(&theme, &parsed),
        "explain" => cmd_explain(&theme, &parsed),
        "logs" => cmd_logs(&theme, &parsed),
        "__service-probe" => cmd_service_probe(&theme, &parsed),
        "override" => cmd_override(&theme, &parsed),
        "push" => cmd_push(&theme, &parsed),
        v if v == Syntax::IMAGE_SUBCOMMAND => cmd_image(&theme, &parsed),
        v if v == Syntax::BRIDGE_SUBCOMMAND => cmd_bridge(&theme, &parsed),
        v if v == Syntax::OS_SUBCOMMAND => cmd_os(&theme, &parsed),
        v if v == Syntax::STUDIO_SUBCOMMAND => cmd_studio(&theme, &parsed),
        v if v == Syntax::USER_SUBCOMMAND => cmd_user(&theme, &parsed),
        v if v == Syntax::SERVICES_SUBCOMMAND => cmd_services(&theme, &parsed),
        v if v == Syntax::SECRETS_SUBCOMMAND => cmd_secrets(&theme, &parsed),
        v if v == Syntax::TOOL_SUBCOMMAND => cmd_tool(&theme, &parsed),
        v if v == Syntax::BROWSER_SUBCOMMAND => cmd_browser(&theme, &parsed),
        "help" | "--help" | "-h" => {
            let theme = Theme::resolve_for(color, std::io::stdout().is_terminal());
            println!("{}", super::usage_tests::usage_with_color(theme.color));
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
