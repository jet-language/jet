fn cmd_bridge(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::BRIDGE_VERB_FLAKE => {
            if !Provider::nix_on_path() {
                theme.error_coded(
                    "E1256",
                    "`jet bridge flake` needs `nix`, which isn't on PATH",
                    "translating a flake.nix's devShell shells out to `nix eval` (U16); without \
                     `nix` there's nothing to read the devShell from.",
                    "install Nix from the official installer, or write env.* by hand.",
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
    if verb == Some(Syntax::USER_SUBCOMMAND) {
        let user_verb = args.first().map(String::as_str);
        let user_args = args.get(1..).unwrap_or(&[]);
        return super::JetOS::user_main(theme, user_verb, user_args, &flags);
    }
    if verb == Some(Syntax::STUDIO_SUBCOMMAND) {
        let nested = Parsed {
            flags: parsed.flags.clone(),
            positional: args.to_vec(),
            command: parsed.command.clone(),
        };
        return cmd_studio(theme, &nested);
    }
    super::JetOS::main(theme, verb, args, &flags)
}

/// `jetos user <plan|build|switch|rollback|prove> <name>` — standalone user
/// generations over the same profile engine used by `jet os switch`.
fn cmd_user(theme: &Theme, parsed: &Parsed) -> i32 {
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
    super::JetOS::user_main(theme, verb, args, &flags)
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
