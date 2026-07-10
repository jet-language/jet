struct NixosImportArgs {
    source: PathBuf,
    host: Option<String>,
    users: Vec<String>,
    write: bool,
    out: Option<PathBuf>,
    facts_only: bool,
}

struct NixosImportPlan {
    source: PathBuf,
    mode: &'static str,
    host: String,
    target: String,
    nixpkgs_ref: String,
    /// Additional named sources beyond nixpkgs (label, `github@…` ref) —
    /// e.g. the `nix-cachyos-kernel` pin a `.CachyOS` kernel needs, or a
    /// flake input recovered by package-provenance import.
    extra_sources: Vec<(String, String)>,
    packages: Vec<String>,
    /// Packages recovered from non-nixpkgs flake inputs: `(source_label, pkgs)`.
    sourced_packages: Vec<(String, Vec<String>)>,
    omitted_packages: Vec<String>,
    services: Vec<String>,
    options: Vec<(String, String)>,
    users: Vec<NixosImportUser>,
    modules: Vec<String>,
    home_modules: Vec<String>,
    omissions: Vec<String>,
}

struct NixosImportUser {
    name: String,
    home: Option<String>,
    groups: Vec<String>,
    packages: Vec<String>,
    /// User packages recovered from non-nixpkgs flake inputs.
    sourced_packages: Vec<(String, Vec<String>)>,
    omitted_packages: Vec<String>,
    home_manager: bool,
}

fn cmd_import(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(mut import_args) = parse_nixos_import_args(theme, args) else {
        return 2;
    };
    // The global flag parser consumes `--host <name>` (shared with the Studio
    // surface) before subcommand args are sliced, so accept it from there.
    if import_args.host.is_none() {
        import_args.host = flags.host.clone();
    }
    let import_args = import_args;
    let plan = match load_nixos_import_plan(&import_args) {
        Ok(plan) => plan,
        Err(e) => {
            theme.error_coded(
                "E1289",
                "jetos could not import the NixOS configuration",
                &e,
                "pass a flake/root with `jetos-import-facts.json`, or rerun with `--facts-only` for an audited scan draft.",
            );
            return 2;
        }
    };
    let config = render_nixos_import_config(&plan);
    let audit = render_nixos_import_audit(&plan);
    if import_args.write {
        match write_nixos_import_output(&import_args, flags, &config, &audit) {
            Ok((config_path, audit_path)) => {
                theme.ok(&format!("wrote imported jetos config {}", config_path.display()));
                theme.detail(&format!("wrote import audit {}", audit_path.display()));
                0
            }
            Err(e) => {
                theme.error_coded(
                    "E1289",
                    "jetos could not write the imported configuration",
                    &e,
                    "choose a fresh `--out` path, or pass `-y` to replace an existing generated config.",
                );
                2
            }
        }
    } else {
        println!("{config}");
        if plan.mode == "facts-only-scan" {
            theme.status("printed audited facts-only jetos import draft");
        } else {
            theme.status("printed semantic jetos import draft");
        }
        0
    }
}

fn parse_nixos_import_args(theme: &Theme, args: &[String]) -> Option<NixosImportArgs> {
    let mut source = None;
    let mut host = None;
    let mut users = Vec::new();
    let mut out = None;
    let mut write = false;
    let mut facts_only = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            a if a == Syntax::OS_IMPORT_FLAG_HOST => {
                i += 1;
                let Some(value) = args.get(i).filter(|s| !s.starts_with('-')) else {
                    theme.error_coded(
                        "E1289",
                        "jetos import needs a host after `--host`",
                        "D-JOS-NIXIMPORT1 imports one concrete NixOS host into one `system.<host>` tree.",
                        "run `jet os import <flake-or-dir> --host laptop`.",
                    );
                    return None;
                };
                host = Some(value.clone());
            }
            a if a == Syntax::OS_IMPORT_FLAG_USER => {
                i += 1;
                let Some(value) = args.get(i).filter(|s| !s.starts_with('-')) else {
                    theme.error_coded(
                        "E1289",
                        "jetos import needs a user after `--user`",
                        "D-JOS-NIXIMPORT1 imports selected Home Manager users into `user.<name>` options.",
                        "run `jet os import <flake-or-dir> --host laptop --user nate`.",
                    );
                    return None;
                };
                users.push(value.clone());
            }
            a if a == Syntax::OS_IMPORT_FLAG_OUT => {
                i += 1;
                let Some(value) = args.get(i).filter(|s| !s.starts_with('-')) else {
                    theme.error_coded(
                        "E1289",
                        "jetos import needs a path after `--out`",
                        "`--out` names the generated config file or output directory.",
                        "run `jet os import <flake-or-dir> --host laptop --write --out ./jetos`.",
                    );
                    return None;
                };
                out = Some(PathBuf::from(value));
            }
            a if a == Syntax::OS_IMPORT_FLAG_WRITE => write = true,
            a if a == Syntax::OS_IMPORT_FLAG_FACTS_ONLY => facts_only = true,
            a if a.starts_with('-') => {
                theme.error_coded(
                    "E1289",
                    &format!("`{a}` is not a jetos import flag"),
                    "D-JOS-NIXIMPORT1 keeps import flags explicit: --host, --user, --write, --out, --facts-only.",
                    "run `jet os import <flake-or-dir> --host laptop`.",
                );
                return None;
            }
            value => {
                if source.is_some() {
                    theme.error_coded(
                        "E1289",
                        "jetos import takes one flake or directory",
                        &format!("extra input `{value}` would make the import source ambiguous."),
                        "run `jet os import <flake-or-dir> --host laptop`.",
                    );
                    return None;
                }
                source = Some(PathBuf::from(value));
            }
        }
        i += 1;
    }
    let Some(source) = source else {
        theme.error_coded(
            "E1289",
            "jetos import needs a flake or directory",
            "D-JOS-NIXIMPORT1 imports from a concrete NixOS/flake-parts/Home Manager root.",
            "run `jet os import <flake-or-dir> --host laptop`.",
        );
        return None;
    };
    Some(NixosImportArgs {
        source,
        host,
        users,
        write,
        out,
        facts_only,
    })
}

fn load_nixos_import_plan(args: &NixosImportArgs) -> Result<NixosImportPlan, String> {
    if !args.source.exists() {
        return Err(format!("import source `{}` does not exist.", args.source.display()));
    }
    let facts_path = nixos_import_facts_path(&args.source);
    if !args.facts_only {
        if let Some(path) = facts_path {
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("reading `{}` failed: {e}", path.display()))?;
            return import_plan_from_json(args, &path, &text);
        }
        // Live semantic tier (D-JOS-NIXIMPORT1=C): evaluate the flake's real
        // option values. A non-flake source falls through to the audited
        // scan draft; a broken flake is a hard error, never a silent scan.
        if let Some(plan) = live_import_plan(args)? {
            return Ok(plan);
        }
    }
    import_plan_from_scan(args)
}

fn nixos_import_facts_path(source: &Path) -> Option<PathBuf> {
    if source.is_file() {
        let name = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "jetos-import-facts.json" || name.ends_with(".json") {
            return Some(source.to_path_buf());
        }
        return None;
    }
    let path = source.join("jetos-import-facts.json");
    path.is_file().then_some(path)
}

fn import_plan_from_json(
    args: &NixosImportArgs,
    facts_path: &Path,
    text: &str,
) -> Result<NixosImportPlan, String> {
    let json = JSON::parse(text).map_err(|e| format!("parsing `{}` failed: {e}", facts_path.display()))?;
    let root = json.as_object().map_err(|e| format!("import facts root: {e}"))?;
    let host = args
        .host
        .clone()
        .or_else(|| import_json_string(root, "host"))
        .ok_or_else(|| "import facts need `host`, or pass `--host <name>`.".to_string())?;
    let target = import_json_string(root, "target").unwrap_or_else(|| "linux.x64".to_string());
    let nixpkgs_ref =
        import_json_string(root, "nixpkgs").unwrap_or_else(|| "nixpkgs@nixpkgs-unstable".to_string());
    let (packages, omitted_packages) = import_package_list(root, "packages");
    let services = import_json_string_array(root, "services");
    let mut options = import_json_option_object(root, "options");
    if !options.iter().any(|(key, _)| key == "network.hostName") {
        options.push(("network.hostName".to_string(), import_render_string(&host)));
    }
    let modules = import_json_string_array(root, "flakePartsModules");
    let home_modules = import_json_string_array(root, "homeManagerModules");
    let mut omissions = import_json_string_array(root, "omissions");
    let mut users = import_json_users(root, &args.users)?;
    for user in &mut users {
        if user.home_manager {
            options.push((
                format!("user.{}.homeManager", user.name),
                "true".to_string(),
            ));
        }
    }
    omissions.extend(
        omitted_packages
            .iter()
            .map(|p| format!("package `{p}` uses a Nix attr path JetOS cannot encode yet")),
    );
    for user in &users {
        omissions.extend(user.omitted_packages.iter().map(|p| {
            format!(
                "Home Manager package `{p}` for user `{}` uses a Nix attr path JetOS cannot encode yet",
                user.name
            )
        }));
    }
    Ok(NixosImportPlan {
        source: args.source.clone(),
        mode: "semantic-facts",
        host,
        target,
        nixpkgs_ref,
        extra_sources: Vec::new(),
        packages,
        sourced_packages: Vec::new(),
        omitted_packages,
        services,
        options,
        users,
        modules,
        home_modules,
        omissions,
    })
}

fn import_plan_from_scan(args: &NixosImportArgs) -> Result<NixosImportPlan, String> {
    let flake = if args.source.is_dir() {
        args.source.join("flake.nix")
    } else {
        args.source.clone()
    };
    let text = fs::read_to_string(&flake).unwrap_or_default();
    let host = args
        .host
        .clone()
        .or_else(|| scan_first_nixos_host(&text))
        .unwrap_or_else(|| "host".to_string());
    let mut modules = Vec::new();
    let mut home_modules = Vec::new();
    if text.contains("flake-parts") {
        modules.push("flake-parts detected; exact module graph needs semantic facts".to_string());
    }
    if text.contains("home-manager") || text.contains("homeManager") {
        home_modules.push("Home Manager detected; exact user graph needs semantic facts".to_string());
    }
    let mut omissions = vec![
        "no jetos-import-facts.json was present; this is an audited scan draft, not a complete conversion".to_string(),
        "Nix module merges, option priorities, overlays, specialisations, secrets, and Home Manager activation facts were not semantically evaluated".to_string(),
    ];
    if !flake.is_file() {
        omissions.push(format!("no flake.nix was found at `{}`", flake.display()));
    }
    let users = args
        .users
        .iter()
        .map(|name| NixosImportUser {
            name: name.clone(),
            home: Some(format!("/home/{name}")),
            groups: Vec::new(),
            packages: Vec::new(),
            sourced_packages: Vec::new(),
            omitted_packages: Vec::new(),
            home_manager: text.contains(name) && (text.contains("home-manager") || text.contains("homeManager")),
        })
        .collect::<Vec<_>>();
    Ok(NixosImportPlan {
        source: args.source.clone(),
        mode: "facts-only-scan",
        host,
        target: "linux.x64".to_string(),
        nixpkgs_ref: "nixpkgs@nixpkgs-unstable".to_string(),
        extra_sources: Vec::new(),
        packages: Vec::new(),
        sourced_packages: Vec::new(),
        omitted_packages: Vec::new(),
        services: Vec::new(),
        options: vec![("network.hostName".to_string(), import_render_string("host"))],
        users,
        modules,
        home_modules,
        omissions,
    })
}

fn import_json_string(
    root: &std::collections::BTreeMap<String, JSON::Json>,
    key: &str,
) -> Option<String> {
    root.get(key)
        .and_then(|value| value.as_str().ok())
        .map(str::to_string)
}

fn import_json_string_array(
    root: &std::collections::BTreeMap<String, JSON::Json>,
    key: &str,
) -> Vec<String> {
    root.get(key)
        .and_then(|value| value.as_array().ok())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().ok().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn import_package_list(
    root: &std::collections::BTreeMap<String, JSON::Json>,
    key: &str,
) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut omitted = Vec::new();
    for package in import_json_string_array(root, key) {
        if import_is_package_name(&package) {
            kept.push(package);
        } else {
            omitted.push(package);
        }
    }
    (kept, omitted)
}

/// Package list entries accept interior hyphens (`xdg-utils`, `btrfs-progs`)
/// — the ratified `source.[a, b]` grammar parses them — but stay conservative
/// elsewhere: no leading/trailing dash, no dots or version suffixes, and no
/// hyphen segment that lexes as a Jet keyword (`codex-…-use-…` would parse
/// as the `use` keyword mid-list).
fn import_is_package_name(value: &str) -> bool {
    let mut chars = value.chars();
    let head_ok = matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic());
    head_ok
        && !value.ends_with('-')
        && chars.all(|c| c == '_' || c == '-' || c.is_ascii_alphanumeric())
        && value
            .split('-')
            .all(|segment| !Syntax::JET_KEYWORD_LIST.contains(&segment))
}

fn import_json_option_object(
    root: &std::collections::BTreeMap<String, JSON::Json>,
    key: &str,
) -> Vec<(String, String)> {
    let Some(JSON::Json::Object(map)) = root.get(key) else {
        return Vec::new();
    };
    map.iter()
        .map(|(key, value)| (key.clone(), import_render_json_value(value)))
        .collect()
}

fn import_json_users(
    root: &std::collections::BTreeMap<String, JSON::Json>,
    selected: &[String],
) -> Result<Vec<NixosImportUser>, String> {
    let Some(users_json) = root.get("users") else {
        return Ok(Vec::new());
    };
    let mut users = Vec::new();
    for user_json in users_json.as_array().map_err(|e| format!("users: {e}"))? {
        let user = user_json.as_object().map_err(|e| format!("user entry: {e}"))?;
        let name = import_json_string(user, "name")
            .ok_or_else(|| "each imported user needs a `name`.".to_string())?;
        if !selected.is_empty() && !selected.iter().any(|wanted| wanted == &name) {
            continue;
        }
        let (packages, omitted_packages) = import_package_list(user, "packages");
        users.push(NixosImportUser {
            name,
            home: import_json_string(user, "home"),
            groups: import_json_string_array(user, "groups"),
            packages,
            sourced_packages: Vec::new(),
            omitted_packages,
            home_manager: matches!(user.get("homeManager"), Some(JSON::Json::Bool(true))),
        });
    }
    Ok(users)
}

fn scan_first_nixos_host(text: &str) -> Option<String> {
    for marker in ["nixosConfigurations.", "nixosConfigurations = {"] {
        if let Some(pos) = text.find(marker) {
            let rest = &text[pos + marker.len()..];
            let candidate = rest
                .trim_start()
                .trim_start_matches('{')
                .trim_start()
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .next()
                .unwrap_or("");
            if !candidate.is_empty() && candidate != "=" {
                return Some(candidate.replace('-', "_"));
            }
        }
    }
    None
}

fn render_nixos_import_config(plan: &NixosImportPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `jet os import` from {}\n",
        plan.source.display()
    ));
    out.push_str("// Review jetos-import-audit.json before switching this host.\n");
    out.push_str(&format!("module {} {{\n", import_ident_or_host(&plan.host)));
    out.push_str("    sources: {\n");
    out.push_str(&format!("        nixpkgs: {},\n", plan.nixpkgs_ref));
    for (name, source) in &plan.extra_sources {
        out.push_str(&format!("        {}: {},\n", name, source));
    }
    out.push_str("    }\n");
    out.push_str(&format!(
        "    system.{}: {{\n",
        import_ident_or_host(&plan.host)
    ));
    out.push_str(&format!("        target: {},\n", plan.target));
    out.push_str("        packages: ");
    out.push_str(&render_import_package_groups(
        &plan.packages,
        &plan.sourced_packages,
    ));
    out.push_str(",\n");
    out.push_str("        services: {\n");
    for service in &plan.services {
        if import_is_ident(service) {
            out.push_str(&format!(
                "            {service}: {{ enable: true, exec: \"/usr/bin/env true\" }},\n"
            ));
        }
    }
    out.push_str("        },\n");
    out.push_str("        options: [\n");
    for (key, value) in &plan.options {
        out.push_str(&format!("            {key}: {value},\n"));
    }
    for user in &plan.users {
        out.push_str(&format!("            users.{}.normal: true,\n", user.name));
        if let Some(home) = &user.home {
            out.push_str(&format!(
                "            users.{}.home: {},\n",
                user.name,
                import_render_string(home)
            ));
        }
        for group in &user.groups {
            if import_is_ident(group) {
                out.push_str(&format!(
                    "            groups.{group}.members: [users.{}],\n",
                    user.name
                ));
            }
        }
        if !user.packages.is_empty() || !user.sourced_packages.is_empty() {
            out.push_str(&format!(
                "            user.{}.packages: {},\n",
                user.name,
                render_import_package_groups(&user.packages, &user.sourced_packages)
            ));
        }
    }
    out.push_str("        ],\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn render_import_package_groups(
    nixpkgs: &[String],
    sourced: &[(String, Vec<String>)],
) -> String {
    let mut groups = Vec::new();
    if !nixpkgs.is_empty() {
        groups.push(format!("nixpkgs.[{}]", nixpkgs.join(", ")));
    }
    for (source, packages) in sourced {
        if packages.is_empty() {
            continue;
        }
        groups.push(format!("{source}.[{}]", packages.join(", ")));
    }
    if groups.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", groups.join(", "))
    }
}

fn render_nixos_import_audit(plan: &NixosImportPlan) -> String {
    let mut all_packages = plan.packages.clone();
    for (_, pkgs) in &plan.sourced_packages {
        all_packages.extend(pkgs.iter().cloned());
    }
    let sourced_labels: Vec<String> = plan
        .sourced_packages
        .iter()
        .map(|(label, _)| label.clone())
        .collect();
    format!(
        "{{\n  \"kind\":\"jetos.import.audit\",\n  \"mode\":{},\n  \"source\":{},\n  \"host\":{},\n  \"packages\":{},\n  \"sourced_package_inputs\":{},\n  \"omitted_packages\":{},\n  \"services\":{},\n  \"users\":{},\n  \"flake_parts_modules\":{},\n  \"home_manager_modules\":{},\n  \"omissions\":{}\n}}\n",
        JSON::quote(plan.mode),
        JSON::quote(&plan.source.display().to_string()),
        JSON::quote(&plan.host),
        import_json_array(&all_packages),
        import_json_array(&sourced_labels),
        import_json_array(&plan.omitted_packages),
        import_json_array(&plan.services),
        import_json_array(
            &plan
                .users
                .iter()
                .map(|user| user.name.clone())
                .collect::<Vec<_>>()
        ),
        import_json_array(&plan.modules),
        import_json_array(&plan.home_modules),
        import_json_array(&plan.omissions),
    )
}

fn write_nixos_import_output(
    args: &NixosImportArgs,
    flags: &OsFlags,
    config: &str,
    audit: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (config_path, audit_path) = if out.extension().and_then(|e| e.to_str()) == Some("jet") {
        let audit = out
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("jetos-import-audit.json");
        (out, audit)
    } else {
        (out.join(Syntax::CONFIG_FILE), out.join("jetos-import-audit.json"))
    };
    if config_path.exists() && !flags.assume_yes {
        return Err(format!(
            "`{}` already exists; jetos import will not overwrite it without `-y`.",
            config_path.display()
        ));
    }
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("creating `{}` failed: {e}", parent.display()))?;
    }
    if let Some(parent) = audit_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("creating `{}` failed: {e}", parent.display()))?;
    }
    fs::write(&config_path, config)
        .map_err(|e| format!("writing `{}` failed: {e}", config_path.display()))?;
    fs::write(&audit_path, audit)
        .map_err(|e| format!("writing `{}` failed: {e}", audit_path.display()))?;
    Ok((config_path, audit_path))
}

fn import_render_json_value(value: &JSON::Json) -> String {
    match value {
        JSON::Json::Null => "null".to_string(),
        JSON::Json::Bool(value) => value.to_string(),
        JSON::Json::Num(value) => {
            if value.fract() == 0.0 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            }
        }
        JSON::Json::Str(value) => import_render_string(value),
        JSON::Json::Array(values) => {
            let rendered = values
                .iter()
                .map(import_render_json_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{rendered}]")
        }
        JSON::Json::Object(_) => import_render_string(&import_render_json_for_audit(value)),
    }
}

fn import_render_json_for_audit(value: &JSON::Json) -> String {
    match value {
        JSON::Json::Null => "null".to_string(),
        JSON::Json::Bool(value) => value.to_string(),
        JSON::Json::Num(value) => value.to_string(),
        JSON::Json::Str(value) => JSON::quote(value),
        JSON::Json::Array(values) => {
            let parts = values
                .iter()
                .map(import_render_json_for_audit)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{parts}]")
        }
        JSON::Json::Object(map) => {
            let parts = map
                .iter()
                .map(|(key, value)| {
                    format!("{}:{}", JSON::quote(key), import_render_json_for_audit(value))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{parts}}}")
        }
    }
}

fn import_render_string(value: &str) -> String {
    JSON::quote(value)
}

fn import_json_array(values: &[String]) -> String {
    let parts = values
        .iter()
        .map(|value| JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{parts}]")
}

fn import_is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn import_ident_or_host(value: &str) -> String {
    let candidate = value.replace('-', "_");
    if import_is_ident(&candidate) {
        candidate
    } else {
        "host".to_string()
    }
}
