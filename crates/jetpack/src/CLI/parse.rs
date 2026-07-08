/// Parsed global flags shared by every command.
#[derive(Clone)]
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
#[derive(Clone)]
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
        v if v == Syntax::USER_SUBCOMMAND => cmd_user(&theme, &parsed),
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
