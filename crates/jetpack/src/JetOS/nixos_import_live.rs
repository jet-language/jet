// Live semantic tier for `jet os import` (D-JOS-NIXIMPORT1=C): when the
// source is a flake root with no `jetos-import-facts.json`, evaluate the
// host's NixOS configuration through `nix eval --json … --apply` and build
// the import plan from real option values. Facts a jetos option cannot
// express become explicit omissions — never silent drops.

/// The `--apply` extractor. Runs over the evaluated NixOS `config` with
/// builtins only (`lib` is not in scope under `--apply`); every optional
/// path is `or`-guarded so hosts without a subsystem still evaluate.
const NIXOS_LIVE_EXTRACTOR: &str = r#"c: {
  host = c.networking.hostName;
  stateVersion = c.system.stateVersion;
  tz = c.time.timeZone;
  locale = c.i18n.defaultLocale;
  keyboard = c.services.xserver.xkb.layout or "us";
  desktopGnome = c.services.desktopManager.gnome.enable or false;
  desktopPlasma = c.services.desktopManager.plasma6.enable or false;
  dmGdm = c.services.displayManager.gdm.enable or false;
  dmSddm = c.services.displayManager.sddm.enable or false;
  loaderLimine = c.boot.loader.limine.enable or false;
  loaderSystemdBoot = c.boot.loader.systemd-boot.enable or false;
  efiTouch = c.boot.loader.efi.canTouchEfiVariables or false;
  kernelName = c.boot.kernelPackages.kernel.pname or "?";
  kernelParams = c.boot.kernelParams or [];
  sysctl = c.boot.kernel.sysctl or {};
  firewallTcp = c.networking.firewall.allowedTCPPorts or [];
  firewallUdp = c.networking.firewall.allowedUDPPorts or [];
  nameservers = c.networking.nameservers or [];
  networkmanager = c.networking.networkmanager.enable or false;
  zramEnable = c.zramSwap.enable or false;
  zramPercent = c.zramSwap.memoryPercent or 50;
  svcOpenssh = c.services.openssh.enable or false;
  svcPipewire = c.services.pipewire.enable or false;
  svcRtkit = c.security.rtkit.enable or false;
  svcTailscale = c.services.tailscale.enable or false;
  svcLibvirtd = c.virtualisation.libvirtd.enable or false;
  svcDocker = c.virtualisation.docker.enable or false;
  svcFlatpak = c.services.flatpak.enable or false;
  svcSteam = c.programs.steam.enable or false;
  svcGamemode = c.programs.gamemode.enable or false;
  svcPcscd = c.services.pcscd.enable or false;
  svcBluetooth = c.hardware.bluetooth.enable or false;
  stylix = c.stylix.enable or false;
  packages = map (p: p.pname or p.name or "?") c.environment.systemPackages;
  users = map (n: {
    name = n;
    home = c.users.users.${n}.home or "/home/${n}";
    groups = c.users.users.${n}.extraGroups or [];
    shell = c.users.users.${n}.shell.pname or "";
  }) (builtins.filter (n: (c.users.users.${n}.isNormalUser or false))
       (builtins.attrNames c.users.users));
  hm = map (n: {
    name = n;
    packages = map (p: p.pname or p.name or "?")
      (c.home-manager.users.${n}.home.packages or []);
    programs = builtins.filter
      (p: (c.home-manager.users.${n}.programs.${p}.enable or false))
      ["git" "fish" "starship" "helix" "ghostty" "vscode"];
  }) (builtins.attrNames (c.home-manager.users or {}));
}"#;

/// Attempt the live semantic tier. `Ok(None)` means the source is not a
/// flake root, so the caller falls back to the audited scan draft. Eval
/// failures are hard errors (surfaced as E1289) — a broken flake must not
/// silently degrade into a scan.
fn live_import_plan(args: &NixosImportArgs) -> Result<Option<NixosImportPlan>, String> {
    if !args.source.is_dir() || !args.source.join("flake.nix").is_file() {
        return Ok(None);
    }
    let flake_text = fs::read_to_string(args.source.join("flake.nix")).unwrap_or_default();
    let Some(host) = args.host.clone().or_else(|| scan_first_nixos_host(&flake_text)) else {
        return Err(
            "the flake declares no discoverable nixosConfigurations host; pass `--host <name>`."
                .to_string(),
        );
    };
    let facts = run_live_extractor(&args.source, &host)?;
    let root = facts
        .as_object()
        .map_err(|e| format!("live eval result: {e}"))?;
    Ok(Some(plan_from_live_facts(args, &host, root)?))
}

fn run_live_extractor(source: &Path, host: &str) -> Result<JSON::Json, String> {
    let attr = format!(
        "{}#nixosConfigurations.{}.config",
        source.display(),
        host
    );
    let output = Command::new("nix")
        .args(["eval", "--json", &attr, "--apply", NIXOS_LIVE_EXTRACTOR])
        .output()
        .map_err(|e| format!("running `nix eval` failed: {e}; is `nix` on PATH?"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let decisive = stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("nix eval failed with no diagnostic output");
        let excerpt: String = decisive.chars().take(240).collect();
        return Err(format!(
            "evaluating `nixosConfigurations.{host}` failed: {excerpt}"
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    JSON::parse(&text).map_err(|e| format!("parsing `nix eval` output failed: {e}"))
}

fn live_str(root: &std::collections::BTreeMap<String, JSON::Json>, key: &str) -> String {
    import_json_string(root, key).unwrap_or_default()
}

fn live_bool(root: &std::collections::BTreeMap<String, JSON::Json>, key: &str) -> bool {
    matches!(root.get(key), Some(JSON::Json::Bool(true)))
}

fn live_num(root: &std::collections::BTreeMap<String, JSON::Json>, key: &str) -> Option<f64> {
    match root.get(key) {
        Some(JSON::Json::Num(n)) => Some(*n),
        _ => None,
    }
}

fn live_num_list(root: &std::collections::BTreeMap<String, JSON::Json>, key: &str) -> Vec<i64> {
    root.get(key)
        .and_then(|v| v.as_array().ok())
        .map(|values| {
            values
                .iter()
                .filter_map(|v| match v {
                    JSON::Json::Num(n) => Some(*n as i64),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn dedup_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// Which of `names` the real-tier backend cannot resolve against the pinned
/// nixpkgs (same fallback chain as the generated `jetosPkg` resolver:
/// direct, underscored, kdePackages, gnomeExtensions). Packages from other
/// flake inputs land here — they become explicit audit omissions instead of
/// entries that break the build.
fn probe_unresolvable_packages(
    nixpkgs_ref: &str,
    names: &[String],
) -> Result<Vec<String>, String> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let flake_ref = nixpkgs_ref
        .strip_prefix("github@")
        .map(|rest| format!("github:{rest}"))
        .ok_or_else(|| format!("nixpkgs ref `{nixpkgs_ref}` is not a github pin"))?;
    let name_list = names
        .iter()
        .map(|n| JSON::quote(n))
        .collect::<Vec<_>>()
        .join(" ");
    // Probe against the same pkgs the generated configuration will see:
    // allowUnfree on, so unfree packages (steam, discord, …) don't read as
    // "missing" when their license gate throws under tryEval.
    let expr = format!(
        r#"let
  flake = builtins.getFlake "{flake_ref}";
  pkgs = import flake {{ system = "x86_64-linux"; config.allowUnfree = true; }};
  resolves = name: let
    underscored = builtins.replaceStrings ["-"] ["_"] name;
    extension = if builtins.substring 0 22 name == "gnome-shell-extension-"
      then builtins.substring 22 (builtins.stringLength name) name else name;
    ok = attrs: n: (attrs ? ${{n}}) && (builtins.tryEval attrs.${{n}}).success;
  in (ok pkgs name) || (ok pkgs underscored)
    || (ok (pkgs.kdePackages or {{}}) name)
    || (ok (pkgs.gnomeExtensions or {{}}) extension);
in builtins.filter (n: !(resolves n)) [ {name_list} ]"#
    );
    let output = Command::new("nix")
        .args(["eval", "--json", "--expr", &expr])
        .output()
        .map_err(|e| format!("running the package resolvability probe failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let line = stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("no diagnostic output");
        let excerpt: String = line.chars().take(240).collect();
        return Err(format!("package resolvability probe failed: {excerpt}"));
    }
    let json = JSON::parse(&String::from_utf8_lossy(&output.stdout))
        .map_err(|e| format!("parsing probe output failed: {e}"))?;
    Ok(json
        .as_array()
        .map_err(|e| format!("probe output: {e}"))?
        .iter()
        .filter_map(|v| v.as_str().ok().map(str::to_string))
        .collect())
}

/// Home Manager's own plumbing derivations surface in `home.packages` but are
/// implementation details, not configuration intent — importing them would
/// only make the generated config unbuildable.
fn is_hm_plumbing_package(name: &str) -> bool {
    name == "?"
        || name.starts_with("dummy-")
        || name.starts_with("hm-session-vars")
        || name.ends_with(".desktop")
        || name.ends_with("-manpage")
}

fn render_live_number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn render_live_int_list(values: &[i64]) -> String {
    let parts = values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{parts}]")
}

/// The nixpkgs pin from the source's `flake.lock`, in the `github@owner/repo/rev`
/// source spelling the generated `config.jet` needs for the real tier. A
/// tarball-type lock (channels.nixos.org) still carries the github rev, so it
/// pins to `NixOS/nixpkgs` at that rev.
fn live_nixpkgs_ref(source: &Path) -> Option<String> {
    let text = fs::read_to_string(source.join("flake.lock")).ok()?;
    let lock = JSON::parse(&text).ok()?;
    let locked = lock.get("nodes").ok()?.get("nixpkgs").ok()?.get("locked").ok()?;
    let locked = locked.as_object().ok()?;
    let rev = import_json_string(locked, "rev")?;
    let owner = import_json_string(locked, "owner").unwrap_or_else(|| "NixOS".to_string());
    let repo = import_json_string(locked, "repo").unwrap_or_else(|| "nixpkgs".to_string());
    Some(format!("github@{owner}/{repo}/{rev}"))
}

/// A locked github input's `github@owner/repo/rev` ref from the source's
/// `flake.lock`, looked up by node name.
fn live_locked_github_ref(source: &Path, node: &str) -> Option<String> {
    let text = fs::read_to_string(source.join("flake.lock")).ok()?;
    let lock = JSON::parse(&text).ok()?;
    let locked = lock.get("nodes").ok()?.get(node).ok()?.get("locked").ok()?;
    let locked = locked.as_object().ok()?;
    let owner = import_json_string(locked, "owner")?;
    let repo = import_json_string(locked, "repo")?;
    let rev = import_json_string(locked, "rev")?;
    Some(format!("github@{owner}/{repo}/{rev}"))
}

/// Locked github flake inputs other than `nixpkgs` / `root`, as
/// `(jet_source_label, github@owner/repo/rev)`. Labels are sanitized to Jet
/// idents so they can appear in `sources:` and `label.[pkg, …]` groups.
fn live_external_github_inputs(source: &Path) -> Vec<(String, String)> {
    let Ok(text) = fs::read_to_string(source.join("flake.lock")) else {
        return Vec::new();
    };
    let Ok(lock) = JSON::parse(&text) else {
        return Vec::new();
    };
    let Ok(nodes_json) = lock.get("nodes") else {
        return Vec::new();
    };
    let Ok(nodes) = nodes_json.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen_labels = BTreeSet::new();
    for (node, value) in nodes {
        if node == "root" || node == "nixpkgs" {
            continue;
        }
        let Ok(obj) = value.as_object() else {
            continue;
        };
        let Some(locked) = obj.get("locked").and_then(|v| v.as_object().ok()) else {
            continue;
        };
        let Some(owner) = import_json_string(locked, "owner") else {
            continue;
        };
        let Some(repo) = import_json_string(locked, "repo") else {
            continue;
        };
        let Some(rev) = import_json_string(locked, "rev") else {
            continue;
        };
        let label = sanitize_source_label(node);
        if !seen_labels.insert(label.clone()) {
            continue;
        }
        out.push((label, format!("github@{owner}/{repo}/{rev}")));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn sanitize_source_label(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if ch == '-' {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("src_{out}")
    } else {
        out
    }
}

/// Probe each non-nixpkgs flake input for packages the nixpkgs probe rejected.
/// Returns `(recovered_by_source, still_missing)`.
fn recover_package_provenance(
    source: &Path,
    missing: &[String],
) -> Result<(Vec<(String, Vec<String>)>, Vec<String>), String> {
    if missing.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let inputs = live_external_github_inputs(source);
    if inputs.is_empty() {
        return Ok((Vec::new(), missing.to_vec()));
    }
    let mut remaining: BTreeSet<String> = missing.iter().cloned().collect();
    let mut recovered: Vec<(String, Vec<String>)> = Vec::new();
    for (label, github_ref) in inputs {
        if remaining.is_empty() {
            break;
        }
        let candidates: Vec<String> = remaining.iter().cloned().collect();
        // Reuse the nixpkgs probe against this input's package set.
        match probe_unresolvable_packages(&github_ref, &candidates) {
            Ok(still_bad) => {
                let still_bad: BTreeSet<String> = still_bad.into_iter().collect();
                let found: Vec<String> = candidates
                    .into_iter()
                    .filter(|n| !still_bad.contains(n))
                    .collect();
                if !found.is_empty() {
                    for name in &found {
                        remaining.remove(name);
                    }
                    recovered.push((label, found));
                }
            }
            Err(_) => {
                // A single input probe failure is not fatal — keep trying others.
                continue;
            }
        }
    }
    let still: Vec<String> = remaining.into_iter().collect();
    Ok((recovered, still))
}

fn merge_sourced_packages(
    existing: &mut Vec<(String, Vec<String>)>,
    incoming: Vec<(String, Vec<String>)>,
) {
    for (label, pkgs) in incoming {
        if let Some((_, slot)) = existing.iter_mut().find(|(l, _)| l == &label) {
            for p in pkgs {
                if !slot.contains(&p) {
                    slot.push(p);
                }
            }
        } else {
            existing.push((label, pkgs));
        }
    }
}

fn plan_from_live_facts(
    args: &NixosImportArgs,
    host: &str,
    root: &std::collections::BTreeMap<String, JSON::Json>,
) -> Result<NixosImportPlan, String> {
    let mut options: Vec<(String, String)> = Vec::new();
    let mut services: Vec<String> = Vec::new();
    let mut omissions: Vec<String> = Vec::new();

    let host_fact = import_json_string(root, "host").unwrap_or_else(|| host.to_string());
    options.push(("network.hostName".to_string(), import_render_string(&host_fact)));
    if live_bool(root, "networkmanager") {
        options.push(("network.networkmanager.enable".to_string(), "true".to_string()));
    }
    let tcp = live_num_list(root, "firewallTcp");
    if !tcp.is_empty() {
        options.push((
            "network.firewall.allowedTcpPorts".to_string(),
            render_live_int_list(&tcp),
        ));
    }
    let udp = live_num_list(root, "firewallUdp");
    if !udp.is_empty() {
        options.push((
            "network.firewall.allowedUdpPorts".to_string(),
            render_live_int_list(&udp),
        ));
    }
    let dns = import_json_string_array(root, "nameservers");
    if !dns.is_empty() {
        options.push(("network.dns".to_string(), import_json_array(&dns)));
    }
    for (fact, option) in [
        ("tz", "filesystem.timeZone"),
        ("locale", "services.localization.locale"),
        ("keyboard", "services.localization.keyboardLayout"),
    ] {
        let value = live_str(root, fact);
        if !value.is_empty() {
            options.push((option.to_string(), import_render_string(&value)));
        }
    }

    if live_bool(root, "loaderLimine") {
        options.push(("boot.loader".to_string(), ".Limine".to_string()));
    } else if live_bool(root, "loaderSystemdBoot") {
        options.push(("boot.loader".to_string(), ".SystemdBoot".to_string()));
    }
    options.push((
        "boot.loader.efi.canTouchVariables".to_string(),
        live_bool(root, "efiTouch").to_string(),
    ));
    let params = import_json_string_array(root, "kernelParams");
    if !params.is_empty() {
        options.push(("boot.kernel.params".to_string(), import_json_array(&params)));
    }
    let kernel = live_str(root, "kernelName");
    let mut extra_sources: Vec<(String, String)> = Vec::new();
    if kernel.to_ascii_lowercase().contains("cachyos") {
        options.push(("boot.kernel".to_string(), ".CachyOS".to_string()));
        match live_locked_github_ref(&args.source, "nix-cachyos-kernel") {
            Some(pin) => extra_sources.push(("cachyos".to_string(), pin)),
            None => omissions.push(
                "boot.kernel .CachyOS: the source flake.lock has no `nix-cachyos-kernel` pin; \
                 declare a `github@<owner>/nix-cachyos-kernel/<rev>` source before the real tier can build"
                    .to_string(),
            ),
        }
    }
    if live_bool(root, "stylix") {
        omissions.push(
            "stylix theming is enabled upstream and has no jetos realization yet (theme.* options are declarative only)"
                .to_string(),
        );
    }
    if let Some(JSON::Json::Object(sysctl)) = root.get("sysctl") {
        for (key, value) in sysctl {
            // Values embedding store paths are NixOS machinery echoes (e.g.
            // `kernel.poweroff_cmd` from shutdown.nix), not configuration
            // intent — the target system regenerates them itself, and
            // re-declaring them collides with the module that owns them.
            if matches!(value, JSON::Json::Str(s) if s.contains("/nix/store/")) {
                continue;
            }
            let rendered = match value {
                JSON::Json::Num(n) => render_live_number(*n),
                JSON::Json::Bool(b) => b.to_string(),
                JSON::Json::Str(s) => import_render_string(s),
                other => {
                    omissions.push(format!(
                        "boot.kernel.sysctl.{key} has a shape jetos cannot encode yet: {other:?}"
                    ));
                    continue;
                }
            };
            options.push((format!("performance.sysctl.{key}"), rendered));
        }
    }
    if live_bool(root, "zramEnable") {
        let percent = live_num(root, "zramPercent").unwrap_or(50.0);
        options.push((
            "performance.zram.memoryPercent".to_string(),
            render_live_number(percent),
        ));
    }

    let gnome = live_bool(root, "desktopGnome") || live_bool(root, "dmGdm");
    let plasma = live_bool(root, "desktopPlasma") || live_bool(root, "dmSddm");
    if gnome {
        options.push(("services.desktop.profile".to_string(), ".Default".to_string()));
        options.push(("services.displayManager".to_string(), "\"gdm\"".to_string()));
    } else if plasma {
        options.push(("services.desktop.plasma.enable".to_string(), "true".to_string()));
        options.push(("services.displayManager".to_string(), "\"sddm\"".to_string()));
    }
    if live_bool(root, "svcPipewire") {
        options.push(("services.audio.pipewire.enable".to_string(), "true".to_string()));
    }
    if live_bool(root, "svcRtkit") {
        options.push(("services.audio.rtkit.enable".to_string(), "true".to_string()));
    }
    if live_bool(root, "svcLibvirtd") {
        options.push((
            "services.virtualization.libvirtd.enable".to_string(),
            "true".to_string(),
        ));
    }
    if live_bool(root, "svcSteam") {
        options.push(("services.gaming.steam.enable".to_string(), "true".to_string()));
    }
    if live_bool(root, "svcGamemode") {
        options.push(("services.gaming.gamemode.enable".to_string(), "true".to_string()));
    }
    if live_bool(root, "svcPcscd") {
        options.push(("services.smartcard.pcscd.enable".to_string(), "true".to_string()));
    }
    for (fact, service) in [("svcOpenssh", "openssh"), ("svcTailscale", "tailscale")] {
        if live_bool(root, fact) {
            services.push(service.to_string());
        }
    }
    for (fact, nixos_path) in [
        ("svcDocker", "virtualisation.docker.enable"),
        ("svcFlatpak", "services.flatpak.enable"),
        ("svcBluetooth", "hardware.bluetooth.enable"),
    ] {
        if live_bool(root, fact) {
            omissions.push(format!("{nixos_path} has no jetos option yet"));
        }
    }

    let (packages, omitted_packages) = {
        let (kept, omitted) = import_package_list(root, "packages");
        (dedup_preserving_order(kept), dedup_preserving_order(omitted))
    };

    let mut hm_by_user: std::collections::BTreeMap<String, (Vec<String>, Vec<String>, Vec<String>)> =
        std::collections::BTreeMap::new();
    if let Some(hm_json) = root.get("hm") {
        for entry in hm_json.as_array().map_err(|e| format!("hm: {e}"))? {
            let entry = entry.as_object().map_err(|e| format!("hm entry: {e}"))?;
            let Some(name) = import_json_string(entry, "name") else {
                continue;
            };
            let (kept, omitted) = import_package_list(entry, "packages");
            let programs = import_json_string_array(entry, "programs");
            hm_by_user.insert(name, (kept, omitted, programs));
        }
    }

    let mut users = Vec::new();
    if let Some(users_json) = root.get("users") {
        for user_json in users_json.as_array().map_err(|e| format!("users: {e}"))? {
            let user = user_json.as_object().map_err(|e| format!("user entry: {e}"))?;
            let Some(name) = import_json_string(user, "name") else {
                continue;
            };
            if !args.users.is_empty() && !args.users.iter().any(|wanted| wanted == &name) {
                continue;
            }
            let shell = live_str(user, "shell");
            if !shell.is_empty() && import_is_ident(&shell) {
                options.push((format!("users.{name}.shell"), format!("nixpkgs.{shell}")));
            }
            let has_home_manager = hm_by_user.contains_key(&name);
            let (hm_packages, hm_omitted, hm_programs) =
                hm_by_user.remove(&name).unwrap_or_default();
            let hm_packages = dedup_preserving_order(hm_packages)
                .into_iter()
                .filter(|p| !is_hm_plumbing_package(p))
                .collect::<Vec<_>>();
            let hm_omitted = dedup_preserving_order(hm_omitted)
                .into_iter()
                .filter(|p| !is_hm_plumbing_package(p))
                .collect::<Vec<_>>();
            for program in &hm_programs {
                omissions.push(format!(
                    "Home Manager program `{program}` for user `{name}` needs manual conversion"
                ));
            }
            if has_home_manager {
                options.push((format!("user.{name}.homeManager"), "true".to_string()));
            }
            users.push(NixosImportUser {
                name: name.clone(),
                home: import_json_string(user, "home"),
                groups: import_json_string_array(user, "groups"),
                packages: hm_packages,
                sourced_packages: Vec::new(),
                omitted_packages: hm_omitted,
                home_manager: has_home_manager,
            });
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

    let nixpkgs_ref = live_nixpkgs_ref(&args.source)
        .unwrap_or_else(|| "nixpkgs@nixpkgs-unstable".to_string());

    // Snapshot ownership before the nixpkgs probe strips external packages.
    let system_before: BTreeSet<String> = packages.iter().cloned().collect();
    let user_before: Vec<(String, BTreeSet<String>)> = users
        .iter()
        .map(|u| (u.name.clone(), u.packages.iter().cloned().collect()))
        .collect();

    // Packages that only exist in the source's other flake inputs would break
    // the real-tier build against nixpkgs alone. Recover them via package-
    // provenance import (map each name to the providing flake input); anything
    // still unresolvable stays an explicit audit omission.
    let mut probe_names: Vec<String> = packages.clone();
    for user in &users {
        probe_names.extend(user.packages.iter().cloned());
    }
    probe_names = dedup_preserving_order(probe_names);
    let mut packages = packages;
    let mut users = users;
    let mut sourced_packages: Vec<(String, Vec<String>)> = Vec::new();
    match probe_unresolvable_packages(&nixpkgs_ref, &probe_names) {
        Ok(unresolvable) if !unresolvable.is_empty() => {
            let external: BTreeSet<String> = unresolvable.into_iter().collect();
            packages.retain(|p| !external.contains(p));
            for user in &mut users {
                user.packages.retain(|p| !external.contains(p));
            }
            let missing: Vec<String> = external.into_iter().collect();
            let input_pins = live_external_github_inputs(&args.source);
            match recover_package_provenance(&args.source, &missing) {
                Ok((recovered, still_missing)) => {
                    for (label, pkgs) in recovered {
                        if let Some((_, pin)) = input_pins.iter().find(|(l, _)| l == &label) {
                            if !extra_sources.iter().any(|(n, _)| n == &label) {
                                extra_sources.push((label.clone(), pin.clone()));
                            }
                        }
                        let mut system_pkgs = Vec::new();
                        for pkg in &pkgs {
                            if system_before.contains(pkg) {
                                system_pkgs.push(pkg.clone());
                            }
                            for (uname, upkgs) in &user_before {
                                if upkgs.contains(pkg) {
                                    if let Some(user) =
                                        users.iter_mut().find(|u| u.name == *uname)
                                    {
                                        merge_sourced_packages(
                                            &mut user.sourced_packages,
                                            vec![(label.clone(), vec![pkg.clone()])],
                                        );
                                    }
                                }
                            }
                        }
                        if !system_pkgs.is_empty() {
                            merge_sourced_packages(
                                &mut sourced_packages,
                                vec![(label, system_pkgs)],
                            );
                        }
                    }
                    for name in still_missing {
                        omissions.push(format!(
                            "package `{name}` comes from another flake input (not the pinned nixpkgs) \
                             and no locked github input resolves it"
                        ));
                    }
                }
                Err(e) => {
                    for name in missing {
                        omissions.push(format!(
                            "package `{name}` comes from another flake input (not the pinned nixpkgs); \
                             provenance recovery failed ({e})"
                        ));
                    }
                }
            }
        }
        Ok(_) => {}
        Err(e) => {
            omissions.push(format!(
                "package resolvability probe failed ({e}); non-nixpkgs packages may break the real-tier build"
            ));
        }
    }

    Ok(NixosImportPlan {
        source: args.source.clone(),
        mode: "semantic-eval",
        host: host.to_string(),
        target: "linux.x64".to_string(),
        nixpkgs_ref,
        extra_sources,
        packages,
        sourced_packages,
        omitted_packages,
        services,
        options,
        users,
        modules: Vec::new(),
        home_modules: Vec::new(),
        omissions,
    })
}
