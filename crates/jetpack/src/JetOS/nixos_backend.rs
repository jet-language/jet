// Migration-only NixOS comparison backend. D-JOS-MIGRATIONVERB1=A allows this
// realizer only through `jet os migrate compare-nixos <host> --out <dir>`.
// It stages generated Nix and QEMU artifacts privately, proves the guest is
// honestly labeled NixOS, then publishes the image and proof bundle.
//
// D-JOS-NIXBACKEND1=C (card #363): every `SystemPlan` option/service/package
// that this backend cannot map to a NixOS setting is collected and reported
// in ONE diagnostic (E1291) before `nix` ever runs — no silent omissions,
// mirroring the import-direction discipline in D-JOS-NIXIMPORT1=C.

use super::options_rendering::{
    clean_bool_json, clean_symbol, clean_value, collect_names, is_option_priority_metadata,
    parse_list_items, resolved_option_value, service_extra,
};
use super::types::OsFlags;
use super::vm_proof::{file_sha256, require_real_vm_tools};
use jet_env_model::ModuleEval::SystemPlan;
use crate::Output::Theme;
use crate::RefSpec;
use crate::JSON;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NIXOS_STATE_VERSION: &str = "26.05";
const MIGRATION_TIMEOUT_MS: u64 = 1_200_000;
const MIGRATION_GUEST_PROOF_MARKER: &str = "NIXOS_COMPARISON_PROOF:";

/// One realized NixOS mapping of a `SystemPlan`, ready to render.
#[derive(Debug)]
struct NixosMapping {
    nixpkgs_owner: String,
    nixpkgs_repo: String,
    nixpkgs_rev: String,
    body_lines: Vec<String>,
    system_packages: Vec<String>,
    /// `boot.kernel: .CachyOS` — the declared `nix-cachyos-kernel` flake
    /// source `(owner, repo, rev-or-ref)` whose overlay provides the kernel.
    kernel_input: Option<(String, String, String)>,
    /// Which desktop shell process the guest proof must find alive.
    desktop_shell_process: String,
}

fn locked_inputs(mapping: &NixosMapping) -> Vec<String> {
    let mut inputs = vec![format!(
        "github:{}/{}/{}",
        mapping.nixpkgs_owner, mapping.nixpkgs_repo, mapping.nixpkgs_rev
    )];
    if let Some((owner, repo, rev)) = &mapping.kernel_input {
        inputs.push(format!("github:{owner}/{repo}/{rev}"));
    }
    inputs
}

enum OfflineInputError {
    Missing(String),
    Tool(String),
}

fn require_offline_inputs(
    dir: &Path,
    mapping: &NixosMapping,
) -> Result<(), OfflineInputError> {
    let result = Command::new("nix")
        .args(["flake", "metadata", "--offline", "--json", "path:."])
        .current_dir(dir)
        .output()
        .map_err(|error| {
            OfflineInputError::Tool(format!(
                "checking the composed comparison inputs with `nix flake metadata --offline` failed: {error}"
            ))
        })?;
    if result.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&result.stderr);
    if let Some(input) = locked_inputs(mapping)
        .into_iter()
        .find(|input| stderr.contains(input))
    {
        return Err(OfflineInputError::Missing(input));
    }
    Err(OfflineInputError::Tool(format!(
        "`nix flake metadata --offline path:.` exited {}; private resolver output was suppressed",
        result.status
    )))
}

fn nix_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn nix_string_list(values: &[String]) -> String {
    if values.is_empty() {
        "[ ]".to_string()
    } else {
        format!(
            "[ {} ]",
            values
                .iter()
                .map(|v| nix_string(v))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn nix_int_list(values: &[i64]) -> String {
    if values.is_empty() {
        "[ ]".to_string()
    } else {
        format!(
            "[ {} ]",
            values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn parse_int_list(value: &str) -> Vec<i64> {
    parse_list_items(value)
        .iter()
        .filter_map(|s| s.parse::<i64>().ok())
        .collect()
}

/// `github:owner/nixpkgs/rev` (the colon/flake upstream form `SourceTable`
/// stores, see `ModuleEval::Source::build_source_table`) -> `(owner, repo,
/// rev)`, accepting only a `nixpkgs` repo (case-insensitive).
fn nixpkgs_owner_repo_rev(upstream: &str) -> Option<(String, String, String)> {
    let rest = upstream.strip_prefix("github:")?;
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    let rev = parts.next()?;
    if repo.eq_ignore_ascii_case("nixpkgs") {
        Some((owner.to_string(), repo.to_string(), rev.to_string()))
    } else {
        None
    }
}

/// The system's nixpkgs pin: the first declared source whose upstream is a
/// `NixOS/nixpkgs/<rev-or-ref>@github` ref (D-JPK-REF1 `sources:` block).
fn nixpkgs_pin(table: &RefSpec::SourceTable) -> Option<(String, String, String)> {
    table
        .declarations()
        .into_iter()
        .find_map(|(_, upstream, _)| nixpkgs_owner_repo_rev(&upstream))
}

fn is_nixpkgs_source(source: &RefSpec::Source, table: &RefSpec::SourceTable) -> bool {
    match source {
        RefSpec::Source::Nixpkgs => true,
        RefSpec::Source::Named(name) => table
            .upstream(name)
            .and_then(nixpkgs_owner_repo_rev)
            .is_some(),
        _ => false,
    }
}

/// Every option key this backend understands. `users.<name>.{normal,home,
/// groups,initialPassword,shell}` is matched dynamically (any user name).
fn is_known_option_key(key: &str) -> bool {
    if is_option_priority_metadata(key) {
        return true;
    }
    if let Some(rest) = key.strip_prefix("users.") {
        if let Some((_, field)) = rest.split_once('.') {
            return matches!(
                field,
                "normal" | "home" | "groups" | "initialPassword" | "shell"
            );
        }
        return false;
    }
    if let Some(rest) = key.strip_prefix("user.") {
        if let Some((_, field)) = rest.split_once('.') {
            return matches!(field, "packages" | "homeManager");
        }
        return false;
    }
    if let Some(rest) = key.strip_prefix("apps.program.") {
        // apps.program.<module>.<field>
        return rest.split('.').count() >= 2;
    }
    if let Some(rest) = key.strip_prefix("groups.") {
        return rest.split_once('.').map(|(_, f)| f == "members").unwrap_or(false);
    }
    if key.strip_prefix("performance.sysctl.").is_some() {
        return true;
    }
    matches!(
        key,
        "network.hostName"
            | "network.networkmanager.enable"
            | "network.firewall.allowedTcpPorts"
            | "network.firewall.allowedUdpPorts"
            | "network.dns"
            | "filesystem.timeZone"
            | "services.localization.locale"
            | "services.localization.keyboardLayout"
            | "boot.loader"
            | "boot.loader.efi.canTouchVariables"
            | "boot.kernel"
            | "boot.kernel.profile"
            | "boot.kernel.params"
            | "init.defaultTarget"
            | "performance.zram.memoryPercent"
            | "services.desktop.profile"
            | "services.desktop.plasma.enable"
            | "services.displayManager"
            | "services.desktop.autoLogin.user"
            | "services.audio.pipewire.enable"
            | "services.audio.rtkit.enable"
            | "services.virtualization.libvirtd.enable"
            | "services.virtualization.docker.enable"
            | "services.gaming.steam.enable"
            | "services.gaming.gamemode.enable"
            | "services.smartcard.pcscd.enable"
            | "hardware.bluetooth.enable"
            | "apps.flatpak.enable"
    )
}

fn shell_package_attr(value: &str) -> String {
    clean_value(value)
        .rsplit('.')
        .next()
        .unwrap_or(value)
        .to_string()
}

/// Package names out of a rendered ref list: `[nixpkgs.[a, b-c], nixpkgs.d]`
/// -> `["a", "b-c", "d"]`. Group brackets and source qualifiers are noise
/// here — the backend resolves every name against the pinned nixpkgs.
fn parse_package_ref_names(value: &str) -> Vec<String> {
    value
        .split(|c: char| c == '[' || c == ']' || c == ',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.rsplit('.')
                .next()
                .unwrap_or(part)
                .trim()
                .to_string()
        })
        .filter(|name| !name.is_empty() && name != "nixpkgs")
        .collect()
}

const MISSING_NIXPKGS_PIN: &str =
    "no declared source resolves to `NixOS/nixpkgs/<rev-or-ref>@github` (the real tier needs exactly one nixpkgs pin)";

/// Map a `SystemPlan` to a NixOS `configuration.nix` body + package list, or
/// collect every declaration this backend cannot map (D-JOS-NIXBACKEND1=C —
/// NO SILENT OMISSIONS, checked in full before returning so one diagnostic
/// lists every problem at once).
fn map_system_to_nixos(
    system: &SystemPlan,
    table: &RefSpec::SourceTable,
) -> Result<NixosMapping, Vec<String>> {
    let mut unmapped = Vec::new();

    for option in &system.options {
        if !is_known_option_key(&option.key) {
            unmapped.push(format!("option `{}`", option.key));
        }
    }

    let pin = nixpkgs_pin(table);
    if pin.is_none() {
        unmapped.push(MISSING_NIXPKGS_PIN.to_string());
    }

    let mut system_packages = Vec::new();
    for pkg in &system.packages {
        let raw = if pkg.source.is_empty() {
            pkg.name.clone()
        } else {
            format!("{}@{}", pkg.name, pkg.source)
        };
        let is_nixpkgs = match RefSpec::classify_in(&raw, table) {
            Ok(spec) => is_nixpkgs_source(&spec.source, table),
            Err(_) => false,
        };
        if is_nixpkgs {
            system_packages.push(pkg.name.clone());
        } else {
            let label = if pkg.source.is_empty() {
                pkg.name.clone()
            } else {
                format!("{}.{}", pkg.source, pkg.name)
            };
            unmapped.push(format!("package `{label}` (non-nixpkgs source in real tier)"));
        }
    }

    let mut body_lines = Vec::new();
    if let Some(v) = resolved_option_value(system, "network.hostName") {
        body_lines.push(format!("  networking.hostName = {};", nix_string(&v)));
    }
    if let Some(v) = resolved_option_value(system, "network.networkmanager.enable") {
        body_lines.push(format!(
            "  networking.networkmanager.enable = {};",
            clean_bool_json(&v)
        ));
    }
    if let Some(v) = resolved_option_value(system, "network.firewall.allowedTcpPorts") {
        body_lines.push(format!(
            "  networking.firewall.allowedTCPPorts = {};",
            nix_int_list(&parse_int_list(&v))
        ));
    }
    if let Some(v) = resolved_option_value(system, "network.dns") {
        body_lines.push(format!(
            "  networking.nameservers = {};",
            nix_string_list(&parse_list_items(&v))
        ));
    }
    if let Some(v) = resolved_option_value(system, "filesystem.timeZone") {
        body_lines.push(format!("  time.timeZone = {};", nix_string(&v)));
    }
    if let Some(v) = resolved_option_value(system, "services.localization.locale") {
        body_lines.push(format!("  i18n.defaultLocale = {};", nix_string(&v)));
    }
    if let Some(v) = resolved_option_value(system, "services.localization.keyboardLayout") {
        body_lines.push(format!("  services.xserver.xkb.layout = {};", nix_string(&v)));
    }
    if let Some(v) = resolved_option_value(system, "boot.loader") {
        match clean_symbol(&v).as_str() {
            "SystemdBoot" => body_lines.push("  boot.loader.systemd-boot.enable = true;".to_string()),
            "Limine" => body_lines.push("  boot.loader.limine.enable = true;".to_string()),
            other => unmapped.push(format!(
                "option `boot.loader` value `.{other}` has no NixOS mapping (supported: .SystemdBoot, .Limine)"
            )),
        }
    }
    if let Some(v) = resolved_option_value(system, "boot.loader.efi.canTouchVariables") {
        body_lines.push(format!(
            "  boot.loader.efi.canTouchEfiVariables = {};",
            clean_bool_json(&v)
        ));
    }
    {
        // Serial console always rides along so the host can watch boot and
        // the guest proof marker on ttyS0; tty0 keeps the graphical console.
        let mut params = resolved_option_value(system, "boot.kernel.params")
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        params.push("console=ttyS0,115200".to_string());
        params.push("console=tty0".to_string());
        body_lines.push(format!("  boot.kernelParams = {};", nix_string_list(&params)));
    }

    // Kernel: `.CachyOS` rides on a declared `nix-cachyos-kernel` flake
    // source whose overlay provides `pkgs.cachyosKernels.*`.
    let mut kernel_input = None;
    if let Some(v) = resolved_option_value(system, "boot.kernel") {
        match clean_symbol(&v).as_str() {
            "CachyOS" => {
                let declared = table.declarations().into_iter().find_map(|(_, upstream, _)| {
                    let rest = upstream.strip_prefix("github:")?;
                    let mut parts = rest.splitn(3, '/');
                    let owner = parts.next()?.to_string();
                    let repo = parts.next()?.to_string();
                    let rev = parts.next().unwrap_or("release").to_string();
                    repo.eq_ignore_ascii_case("nix-cachyos-kernel")
                        .then_some((owner, repo, rev))
                });
                match declared {
                    Some(input) => {
                        let attr = match resolved_option_value(system, "boot.kernel.profile")
                            .map(|p| clean_symbol(&p))
                            .as_deref()
                        {
                            Some("CachyOSLts") => "linuxPackages-cachyos-lts",
                            _ => "linuxPackages-cachyos-latest",
                        };
                        body_lines.push(format!(
                            "  boot.kernelPackages = pkgs.cachyosKernels.{attr};"
                        ));
                        kernel_input = Some(input);
                    }
                    None => unmapped.push(
                        "option `boot.kernel` `.CachyOS` needs a declared `<owner>/nix-cachyos-kernel/<rev>@github` source for the kernel overlay"
                            .to_string(),
                    ),
                }
            }
            other => unmapped.push(format!(
                "option `boot.kernel` value `.{other}` has no NixOS mapping (supported: .CachyOS)"
            )),
        }
    }
    if let Some(v) = resolved_option_value(system, "network.firewall.allowedUdpPorts") {
        body_lines.push(format!(
            "  networking.firewall.allowedUDPPorts = {};",
            nix_int_list(&parse_int_list(&v))
        ));
    }
    let sysctl_entries: Vec<(String, String)> = system
        .options
        .iter()
        .filter_map(|option| {
            option
                .key
                .strip_prefix("performance.sysctl.")
                .map(|rest| (rest.to_string(), option.value.clone()))
        })
        .collect();
    if !sysctl_entries.is_empty() {
        body_lines.push("  boot.kernel.sysctl = {".to_string());
        for (key, value) in sysctl_entries {
            let cleaned = clean_value(&value);
            let rendered = if cleaned == "true" || cleaned == "false" {
                cleaned.clone()
            } else if cleaned.parse::<f64>().is_ok() {
                cleaned.clone()
            } else {
                nix_string(&cleaned)
            };
            // mkForce: a declared sysctl is user intent and must win over any
            // NixOS module that also touches the key.
            body_lines.push(format!("    \"{key}\" = lib.mkForce {rendered};"));
        }
        body_lines.push("  };".to_string());
    }
    if let Some(v) = resolved_option_value(system, "performance.zram.memoryPercent") {
        body_lines.push("  zramSwap = {".to_string());
        body_lines.push("    enable = true;".to_string());
        body_lines.push(format!("    memoryPercent = {};", clean_value(&v)));
        body_lines.push("  };".to_string());
    }
    for (key, nixos) in [
        ("services.virtualization.libvirtd.enable", "virtualisation.libvirtd.enable"),
        ("services.virtualization.docker.enable", "virtualisation.docker.enable"),
        ("services.gaming.steam.enable", "programs.steam.enable"),
        ("services.gaming.gamemode.enable", "programs.gamemode.enable"),
        ("services.smartcard.pcscd.enable", "services.pcscd.enable"),
        ("hardware.bluetooth.enable", "hardware.bluetooth.enable"),
        ("apps.flatpak.enable", "services.flatpak.enable"),
    ] {
        if let Some(v) = resolved_option_value(system, key) {
            body_lines.push(format!("  {nixos} = {};", clean_bool_json(&v)));
        }
    }
    for group in collect_names(system, "groups") {
        if let Some(v) = resolved_option_value(system, &format!("groups.{group}.members")) {
            let members: Vec<String> = parse_list_items(&v)
                .iter()
                .map(|m| m.strip_prefix("users.").unwrap_or(m).to_string())
                .collect();
            body_lines.push(format!(
                "  users.groups.{group}.members = {};",
                nix_string_list(&members)
            ));
        }
    }

    let desktop_default = resolved_option_value(system, "services.desktop.profile")
        .map(|v| clean_symbol(&v) == "Default")
        .unwrap_or(false);
    if let Some(v) = resolved_option_value(system, "services.desktop.profile") {
        if clean_symbol(&v) != "Default" {
            unmapped.push(format!(
                "option `services.desktop.profile` value `.{}` has no NixOS mapping (supported: .Default)",
                clean_symbol(&v)
            ));
        }
    }
    let plasma_enabled = resolved_option_value(system, "services.desktop.plasma.enable")
        .map(|v| clean_bool_json(&v) == "true")
        .unwrap_or(false);
    let display_manager = resolved_option_value(system, "services.displayManager")
        .map(|v| clean_value(&v))
        .or_else(|| desktop_default.then(|| "gdm".to_string()))
        .or_else(|| plasma_enabled.then(|| "sddm".to_string()));
    if desktop_default {
        body_lines.push("  services.desktopManager.gnome.enable = true;".to_string());
    }
    if plasma_enabled {
        body_lines.push("  services.desktopManager.plasma6.enable = true;".to_string());
    }
    match display_manager.as_deref() {
        Some("gdm") => body_lines.push("  services.displayManager.gdm.enable = true;".to_string()),
        Some("sddm") => body_lines.push("  services.displayManager.sddm.enable = true;".to_string()),
        Some(other) => unmapped.push(format!(
            "option `services.displayManager` value `{other}` has no NixOS mapping (supported: gdm, sddm)"
        )),
        None => {}
    }
    if let Some(user) = resolved_option_value(system, "services.desktop.autoLogin.user") {
        body_lines.push("  services.displayManager.autoLogin = {".to_string());
        body_lines.push("    enable = true;".to_string());
        body_lines.push(format!("    user = {};", nix_string(&user)));
        body_lines.push("  };".to_string());
        body_lines.push("  systemd.services.\"getty@tty1\".enable = false;".to_string());
        body_lines.push("  systemd.services.\"autovt@tty1\".enable = false;".to_string());
    }
    if let Some(v) = resolved_option_value(system, "services.audio.pipewire.enable") {
        body_lines.push("  services.pipewire = {".to_string());
        body_lines.push(format!("    enable = {};", clean_bool_json(&v)));
        body_lines.push(format!("    alsa.enable = {};", clean_bool_json(&v)));
        body_lines.push(format!("    pulse.enable = {};", clean_bool_json(&v)));
        body_lines.push("  };".to_string());
    }
    if let Some(v) = resolved_option_value(system, "services.audio.rtkit.enable") {
        body_lines.push(format!("  security.rtkit.enable = {};", clean_bool_json(&v)));
    }

    let mut fish_enabled = false;
    for name in collect_names(system, "users") {
        let mut user_lines = Vec::new();
        if let Some(v) = resolved_option_value(system, &format!("users.{name}.normal")) {
            user_lines.push(format!("    isNormalUser = {};", clean_bool_json(&v)));
        }
        if let Some(v) = resolved_option_value(system, &format!("users.{name}.home")) {
            user_lines.push(format!("    home = {};", nix_string(&v)));
        }
        if let Some(v) = resolved_option_value(system, &format!("users.{name}.groups")) {
            user_lines.push(format!(
                "    extraGroups = {};",
                nix_string_list(&parse_list_items(&v))
            ));
        }
        if let Some(v) = resolved_option_value(system, &format!("users.{name}.initialPassword")) {
            user_lines.push(format!("    initialPassword = {};", nix_string(&v)));
        }
        if let Some(v) = resolved_option_value(system, &format!("users.{name}.shell")) {
            let attr = shell_package_attr(&v);
            user_lines.push(format!("    shell = pkgs.{attr};"));
            if attr == "fish" {
                fish_enabled = true;
            }
        }
        // `user.<name>.packages` (per-user scope) — bracket-group refs like
        // `[nixpkgs.[a, b]]` or single `source.name` refs.
        if let Some(v) = resolved_option_value(system, &format!("user.{name}.packages")) {
            let attrs = parse_package_ref_names(&v);
            if !attrs.is_empty() {
                user_lines.push(format!(
                    "    packages = map jetosPkg [ {} ];",
                    attrs
                        .iter()
                        .map(|p| nix_string(p))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        }
        body_lines.push(format!("  users.users.{name} = {{"));
        body_lines.extend(user_lines);
        body_lines.push("  };".to_string());
    }
    for option in &system.options {
        let Some(rest) = option.key.strip_prefix("apps.program.") else {
            continue;
        };
        let Some((module, field)) = rest.split_once('.') else {
            continue;
        };
        if field != "enable" || clean_bool_json(&option.value) != "true" {
            continue;
        }
        match module {
            "git" => body_lines.push("  programs.git.enable = true;".to_string()),
            "fish" => {
                fish_enabled = true;
            }
            "starship" | "helix" | "ghostty" | "vscode" | "yazi" | "btop" | "bat" | "eza"
            | "fzf" | "zoxide" | "ripgrep" | "tealdeer" | "fastfetch" | "cursor" | "discord"
            | "spicetify" | "browser" | "ssh" => {
                if !system_packages.iter().any(|p| p == module) {
                    system_packages.push(module.to_string());
                }
            }
            other => unmapped.push(format!(
                "option `apps.program.{other}.enable` has no NixOS mapping yet"
            )),
        }
    }
    if fish_enabled {
        body_lines.push("  programs.fish.enable = true;".to_string());
    }

    for service in &system.services {
        match service.name.as_str() {
            "openssh" => {
                body_lines.push(format!(
                    "  services.openssh.enable = {};",
                    if service.enable { "true" } else { "false" }
                ));
                if let Some(ports) = service_extra(service, &["ports"]) {
                    let ports = parse_int_list(&ports);
                    if !ports.is_empty() {
                        body_lines.push(format!("  services.openssh.ports = {};", nix_int_list(&ports)));
                    }
                }
            }
            "tailscale" => {
                body_lines.push(format!(
                    "  services.tailscale.enable = {};",
                    if service.enable { "true" } else { "false" }
                ));
            }
            other => {
                if let Some(exec) = service_extra(service, &["exec"]) {
                    let attr = nix_string(other);
                    body_lines.push(format!("  systemd.services.{attr} = {{"));
                    body_lines.push(format!(
                        "    enable = {};",
                        if service.enable { "true" } else { "false" }
                    ));
                    body_lines.push(format!("    serviceConfig.ExecStart = {};", nix_string(&exec)));
                    body_lines.push("  };".to_string());
                    if let Some(timer) = service_extra(service, &["timer"]) {
                        body_lines.push(format!("  systemd.timers.{attr} = {{"));
                        body_lines.push("    wantedBy = [ \"timers.target\" ];".to_string());
                        body_lines.push(format!("    timerConfig.OnCalendar = {};", nix_string(&timer)));
                        body_lines.push("  };".to_string());
                    }
                } else {
                    unmapped.push(format!(
                        "service `{}` (unknown service with no `exec:` field)",
                        service.name
                    ));
                }
            }
        }
    }

    if !unmapped.is_empty() {
        unmapped.sort();
        unmapped.dedup();
        return Err(unmapped);
    }
    // A missing pin entered `unmapped` above, so a successful aggregate check
    // proves `Some`; keep the exact existing error if that ordering ever changes.
    let Some((nixpkgs_owner, nixpkgs_repo, nixpkgs_rev)) = pin else {
        return Err(vec![MISSING_NIXPKGS_PIN.to_string()]);
    };
    Ok(NixosMapping {
        nixpkgs_owner,
        nixpkgs_repo,
        nixpkgs_rev,
        body_lines,
        system_packages,
        kernel_input,
        desktop_shell_process: if plasma_enabled {
            "plasmashell".to_string()
        } else {
            "gnome-shell".to_string()
        },
    })
}

const FLAKE_TEMPLATE: &str = r#"{
  description = "NixOS comparison for @@HOST@@ (generated by `jet os migrate compare-nixos`; do not hand-edit)";

  inputs.nixpkgs.url = "github:@@OWNER@@/@@REPO@@/@@REV@@";
@@EXTRA_INPUTS@@
  outputs = { self, nixpkgs, ... }@inputs:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = nixpkgs.lib;
    in {
      nixosConfigurations.@@HOST@@ = lib.nixosSystem {
        inherit system;
        modules = [ ./configuration.nix@@EXTRA_MODULES@@ ];
      };

      packages.${system} = {
        disk = import (nixpkgs + "/nixos/lib/make-disk-image.nix") {
          inherit lib pkgs;
          config = self.nixosConfigurations.@@HOST@@.config;
          format = "qcow2";
          diskSize = "auto";
          additionalSpace = "12G";
          partitionTableType = "efi";
        };
        firmware = pkgs.OVMF.fd;
      };
    };
}
"#;

fn render_flake_nix(host: &str, mapping: &NixosMapping) -> String {
    let (extra_inputs, extra_modules) = match &mapping.kernel_input {
        Some((owner, repo, rev)) => (
            format!(
                "  inputs.cachyos.url = \"github:{owner}/{repo}/{rev}\";\n  inputs.cachyos.inputs.nixpkgs.follows = \"nixpkgs\";\n"
            ),
            " { nixpkgs.overlays = [ inputs.cachyos.overlays.default ]; }".to_string(),
        ),
        None => (String::new(), String::new()),
    };
    FLAKE_TEMPLATE
        .replace("@@HOST@@", host)
        .replace("@@OWNER@@", &mapping.nixpkgs_owner)
        .replace("@@REPO@@", &mapping.nixpkgs_repo)
        .replace("@@REV@@", &mapping.nixpkgs_rev)
        .replace("@@EXTRA_INPUTS@@", &extra_inputs)
        .replace("@@EXTRA_MODULES@@", &extra_modules)
}

const CONFIGURATION_HEAD: &str = r#"{ config, pkgs, lib, ... }:
let
  # jetos package resolver: names come from imported package sets whose
  # attribute may live under a package group (KDE apps live in
  # `kdePackages.*`). A miss is a hard eval error, never a silent drop.
  jetosPkg = name:
    let
      # `gnome-shell-extension-user-themes` (pname) lives at
      # `gnomeExtensions.user-themes`; `dejavu-fonts` at `dejavu_fonts`.
      extension = lib.removePrefix "gnome-shell-extension-" name;
      underscored = builtins.replaceStrings ["-"] ["_"] name;
      # Removed-alias attrs exist but throw on access — tryEval skips them
      # so lookup falls through to the real location (e.g. kdePackages).
      tryAttr = attrs: n:
        if attrs ? ${n} then
          let r = builtins.tryEval attrs.${n}; in
          if r.success then [ r.value ] else [ ]
        else [ ];
      found = (tryAttr pkgs name) ++ (tryAttr pkgs underscored)
        ++ (tryAttr (pkgs.kdePackages or { }) name)
        ++ (tryAttr (pkgs.gnomeExtensions or { }) extension);
    in
    if builtins.length found > 0 then builtins.head found
    else throw "jetos: package `${name}` is not in nixpkgs, kdePackages, or gnomeExtensions at the pinned revision";
in
{
  system.stateVersion = "@@STATEVERSION@@";
  # Imported package sets routinely include unfree software (steam, discord,
  # editors); nixpkgs' license gate is a nixpkgs-ism, not a jetos policy.
  nixpkgs.config.allowUnfree = true;

  boot.loader.timeout = 1;
  boot.growPartition = true;
  boot.initrd.availableKernelModules = [
    "virtio_pci" "virtio_blk" "virtio_scsi" "virtio_net"
    "ahci" "xhci_pci" "nvme" "sd_mod" "sr_mod"
  ];

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
    autoResize = true;
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/ESP";
    fsType = "vfat";
  };

  environment.variables.NIXOS_COMPARISON = "1";
  programs.bash.promptInit = lib.mkAfter ''
    PS1='NixOS comparison $ '
    export PS1
  '';
  environment.etc.issue.text = "NixOS comparison guest\n";
  environment.etc.motd.text = "NixOS comparison guest for A/B migration checks\n";
"#;

const CONFIGURATION_PROOF_SERVICE: &str = r#"
  # NixOS comparison proof: emit one fact only after identity and desktop checks.
  systemd.services.nixos-comparison-proof = {
    description = "NixOS comparison guest proof emitter";
    wantedBy = [ "multi-user.target" ];
    after = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.bash pkgs.util-linux pkgs.systemd pkgs.procps pkgs.jq pkgs.gawk pkgs.gnused pkgs.coreutils ];
    script = ''
      deadline=$((SECONDS + 300))
      while [ "$SECONDS" -lt "$deadline" ]; do
        dm=$(systemctl is-active display-manager.service || true)
        # NixOS wraps gnome-shell (comm = ".gnome-shell-wr…"), so match the
        # full command line rather than the truncated process name.
        shell_pid=$(pgrep -u @@USER@@ -f @@DESKTOP_SHELL_PROCESS@@ | head -n1 || true)
        session=$(loginctl list-sessions --no-legend | awk '$3=="@@USER@@" {print $1; exit}')
        stype=""
        if [ -n "$session" ]; then
          stype=$(loginctl show-session "$session" -p Type --value || true)
        fi
        echo "NIXOS_COMPARISON_DEBUG dm=$dm shell=$shell_pid session=$session stype=$stype" > /dev/ttyS0 || true
        if [ "$dm" = "active" ] && [ -n "$shell_pid" ] && [ "$stype" = "wayland" ]; then
          os_name=$(. /etc/os-release; printf '%s' "$NAME")
          prompt=$(runuser -u @@USER@@ -- bash --noprofile --rcfile /etc/bashrc -i -c 'printf "%s" "$PS1"' </dev/null 2>/dev/null)
          banner=$(head -n1 /etc/issue)
          jq -cn \
            --arg host "$(cat /proc/sys/kernel/hostname)" \
            --arg kernel "$(uname -r)" \
            --arg dm "$dm" \
            --arg session_type "$stype" \
            --arg shell_pid "$shell_pid" \
            --arg user "@@USER@@" \
            --arg os_release "$os_name" \
            --arg prompt "$prompt" \
            --arg banner "$banner" \
            '{host:$host,kernel:$kernel,display_manager:$dm,session_type:$session_type,shell_pid:$shell_pid,user:$user,desktop:"@@DESKTOP_NAME@@",os_release:$os_release,prompt:$prompt,banner:$banner,proof:"live-desktop"}' \
            | sed 's/^/NIXOS_COMPARISON_PROOF:/' > /dev/ttyS0
          exit 0
        fi
        sleep 2
      done
      echo 'NIXOS_COMPARISON_PROOF:{"proof":"failed","reason":"desktop-not-live-within-300s"}' > /dev/ttyS0
      exit 1
    '';
  };
}
"#;

fn render_configuration_nix(system: &SystemPlan, mapping: &NixosMapping) -> String {
    let mut out = CONFIGURATION_HEAD.replace("@@STATEVERSION@@", NIXOS_STATE_VERSION);
    out.push('\n');
    for line in &mapping.body_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format!(
        "  environment.systemPackages = map jetosPkg [ {} ];\n",
        mapping
            .system_packages
            .iter()
            .map(|p| nix_string(p))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    let proof_user = resolved_option_value(system, "services.desktop.autoLogin.user")
        .unwrap_or_else(|| {
            collect_names(system, "users")
                .into_iter()
                .next()
                .unwrap_or_else(|| "root".to_string())
        });
    let desktop_name = if mapping.desktop_shell_process == "plasmashell" {
        "plasma"
    } else {
        "gnome"
    };
    out.push_str(
        &CONFIGURATION_PROOF_SERVICE
            .replace("@@USER@@", &proof_user)
            .replace(
                "@@DESKTOP_SHELL_PROCESS@@",
                &mapping.desktop_shell_process,
            )
            .replace("@@DESKTOP_NAME@@", desktop_name)
            .replace("@@HOST@@", &system.name),
    );
    out
}

/// Write generated NixOS comparison inputs into the private staging directory.
/// backend dir. Deterministic and offline — no `nix`/`qemu` invocation here.
fn write_nixos_backend(
    dir: &Path,
    system: &SystemPlan,
    mapping: &NixosMapping,
) -> std::io::Result<()> {
    verify_private_stage(dir)?;
    fs::write(dir.join("flake.nix"), render_flake_nix(&system.name, mapping))?;
    fs::write(
        dir.join("configuration.nix"),
        render_configuration_nix(system, mapping),
    )?;
    let facts = format!(
        "{{\"kind\":\"nixos.migration.input-facts\",\"host\":{},\"nixpkgs\":{},\"packages\":[{}]}}\n",
        JSON::quote(&system.name),
        JSON::quote(&format!(
            "github:{}/{}/{}",
            mapping.nixpkgs_owner, mapping.nixpkgs_repo, mapping.nixpkgs_rev
        )),
        mapping
            .system_packages
            .iter()
            .map(|p| JSON::quote(p))
            .collect::<Vec<_>>()
            .join(",")
    );
    fs::write(dir.join("nixos-input-facts.json"), facts)
}

fn migration_timeout() -> Duration {
    std::env::var("JETOS_VM_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(MIGRATION_TIMEOUT_MS))
}

fn real_qemu_prove_argv(
    disk: &str,
    ovmf_code: &Path,
    ovmf_vars: &Path,
    log_path: &Path,
    sock_path: &Path,
) -> Vec<String> {
    vec![
        "qemu-system-x86_64".to_string(),
        "-enable-kvm".to_string(),
        "-cpu".to_string(),
        "host".to_string(),
        "-m".to_string(),
        "4096".to_string(),
        "-smp".to_string(),
        "4".to_string(),
        "-drive".to_string(),
        format!("if=pflash,format=raw,readonly=on,file={}", ovmf_code.display()),
        "-drive".to_string(),
        format!("if=pflash,format=raw,file={}", ovmf_vars.display()),
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=virtio"),
        "-device".to_string(),
        "virtio-gpu-pci".to_string(),
        "-display".to_string(),
        "none".to_string(),
        "-serial".to_string(),
        format!("file:{}", log_path.display()),
        "-qmp".to_string(),
        format!("unix:{},server,nowait", sock_path.display()),
    ]
}

/// Deterministic migration plan. No command outside the explicit migration
/// verb calls this backend.
fn write_migration_plan(
    dir: &Path,
    system: &SystemPlan,
    disk: &str,
    offline: bool,
) -> std::io::Result<PathBuf> {
    let log_path = dir.join("nixos-comparison.serial.log");
    let sock_path = dir.join("nixos-comparison.qmp.sock");
    let ovmf_code = dir.join("firmware/FV/OVMF_CODE.fd");
    let ovmf_vars = dir.join("OVMF_VARS.fd");
    let build_commands: [(&str, Vec<String>); 2] = [
        ("nix-build-disk", nix_build_argv("disk", offline)),
        (
            "nix-build-firmware",
            nix_build_argv("firmware", offline),
        ),
    ];
    let mut commands_json = build_commands
        .iter()
        .map(|(phase, argv)| {
            format!(
                "{{\"phase\":{},\"cwd\":{},\"argv\":[{}]}}",
                JSON::quote(phase),
                JSON::quote(&dir.display().to_string()),
                argv.iter().map(|a| JSON::quote(a)).collect::<Vec<_>>().join(",")
            )
        })
        .collect::<Vec<_>>();
    let boot_argv = real_qemu_prove_argv(disk, &ovmf_code, &ovmf_vars, &log_path, &sock_path);
    commands_json.push(format!(
        "{{\"phase\":{},\"cwd\":null,\"argv\":[{}]}}",
        JSON::quote("boot-real-guest"),
        boot_argv.iter().map(|a| JSON::quote(a)).collect::<Vec<_>>().join(",")
    ));
    let text = format!(
        "{{\"kind\":\"nixos.migration.plan\",\"host\":{},\"disk\":{},\"commands\":[{}]}}\n",
        JSON::quote(&system.name),
        JSON::quote(disk),
        commands_json.join(",")
    );
    let path = dir.join("nixos-comparison-plan.json");
    fs::write(&path, text)?;
    Ok(path)
}

pub(super) fn cmd_migrate_compare_nixos(
    theme: &Theme,
    table: &RefSpec::SourceTable,
    system: &SystemPlan,
    out: &Path,
    flags: &OsFlags,
) -> i32 {
    if out.exists() {
        theme.error(
            "NixOS comparison output already exists",
            &format!("`{}` must not exist before publication.", out.display()),
            "choose a new `--out <dir>` path.",
        );
        return 2;
    }
    let mapping = match map_system_to_nixos(system, table) {
        Ok(mapping) => mapping,
        Err(unmapped) => {
            theme.error_coded(
                "E1291",
                "NixOS comparison could not map every system declaration",
                &format!(
                    "The migration-only NixOS backend cannot drop declarations; unmapped: {}.",
                    unmapped.join("; ")
                ),
                "map or remove each listed declaration, then run the explicit migration command again.",
            );
            return 2;
        }
    };
    if let Err(error) = require_real_vm_tools() {
        theme.error_coded(
            "E1290",
            "NixOS comparison needs real migration tools",
            &error,
            "put real `nix` and QEMU tools on PATH, then run the migration command again.",
        );
        return 2;
    }
    let parent = out.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if let Err(error) = fs::create_dir_all(parent) {
        theme.error(
            "could not prepare the NixOS comparison output",
            &format!("creating `{}` failed: {error}.", parent.display()),
            "choose a writable `--out <dir>` path.",
        );
        return 2;
    }
    let stage = parent.join(format!(
        ".nixos-comparison-{}-{}",
        system.name,
        std::process::id()
    ));
    if let Err(error) = create_private_stage(&stage) {
        theme.error(
            "could not protect the NixOS comparison staging path",
            &format!("creating private `{}` failed: {error}.", stage.display()),
            "choose a private writable output parent and remove any stale staging path.",
        );
        return 2;
    }
    let disk = stage.join(format!("nixos-{}.qcow2", system.name));
    if let Err(error) = write_nixos_backend(&stage, system, &mapping) {
        let error = finish_private_stage(
            &stage,
            Err(format!("writing private NixOS inputs failed: {error}")),
        )
        .unwrap_err();
        return report_migration_failure(theme, system, &error);
    }
    if flags.offline {
        match require_offline_inputs(&stage, &mapping) {
            Ok(()) => {}
            Err(error) => {
                if let Err(cleanup) = finish_private_stage(&stage, Ok(())) {
                    return report_migration_failure(theme, system, &cleanup);
                }
                match error {
                    OfflineInputError::Missing(input) => theme.error_coded(
                        "E1276",
                        &format!("migration comparison unavailable offline: {input}"),
                        "The locked Nix input is not present in the local Nix store.",
                        "run the migration once online to fetch the locked input, then retry with `--offline`.",
                    ),
                    OfflineInputError::Tool(error) => theme.error_coded(
                        "E1290",
                        "NixOS comparison needs real migration tools",
                        &error,
                        "put real `nix` and QEMU tools on PATH, then run the migration command again.",
                    ),
                }
                return 2;
            }
        }
    }
    let result = (|| -> Result<(), String> {
        let plan = write_migration_plan(
            &stage,
            system,
            &disk.display().to_string(),
            flags.offline,
        )
            .map_err(|error| format!("writing migration plan failed: {error}"))?;
        let run = run_migration_build_and_boot(&stage, &disk, flags.offline)?;
        publish_migration_bundle(out, system, &mapping, &disk, &plan, &run)
    })();
    let result = finish_private_stage(&stage, result);
    match result {
        Ok(()) => {
            let host = out.join("nixos").join(&system.name);
            let image = host.join("system.qcow2");
            let proof = host.join("proof.json");
            let receipt = host.join("receipt.json");
            if flags.json {
                println!(
                    "{{\"kind\":\"nixos.migration.comparison\",\"state\":\"proved\",\"host\":{},\"image\":{},\"proof\":{},\"receipt\":{}}}",
                    JSON::quote(&system.name),
                    JSON::quote(&image.display().to_string()),
                    JSON::quote(&proof.display().to_string()),
                    JSON::quote(&receipt.display().to_string()),
                );
            } else {
                println!("built NixOS comparison: {}", image.display());
                println!("boot proof: {}", proof.display());
                println!("receipt: {}", receipt.display());
            }
            0
        }
        Err(error) => {
            report_migration_failure(theme, system, &error)
        }
    }
}

fn report_migration_failure(theme: &Theme, system: &SystemPlan, error: &str) -> i32 {
    theme.error_coded(
        "E1285",
        "NixOS comparison guest proof has not run",
        &format!("the NixOS build and boot for `{}` failed: {error}.", system.name),
        "fix the build or guest failure, then run the explicit migration command again.",
    );
    2
}

fn create_private_stage(stage: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    // Non-recursive mkdir is atomic and fails if any file, directory, or
    // symlink already occupies the leaf.
    builder.create(stage)?;

    verify_private_stage(stage)
}

fn verify_private_stage(stage: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(stage)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "private staging path is not a no-follow directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o700 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("private staging mode is {mode:o}, expected 700"),
            ));
        }
    }
    Ok(())
}

fn finish_private_stage(stage: &Path, result: Result<(), String>) -> Result<(), String> {
    finish_private_stage_with(stage, result, |path| fs::remove_dir_all(path))
}

fn finish_private_stage_with(
    stage: &Path,
    result: Result<(), String>,
    cleanup: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let cleanup = match cleanup(stage) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "removing private staging directory `{}` failed: {error}",
            stage.display()
        )),
    };
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(root), Ok(())) => Err(root),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(root), Err(cleanup)) => Err(format!("{root}; cleanup also failed: {cleanup}")),
    }
}

struct MigrationRun {
    report: String,
    screenshot: PathBuf,
    argv: Vec<String>,
}

fn run_migration_build_and_boot(
    dir: &Path,
    disk: &Path,
    offline: bool,
) -> Result<MigrationRun, String> {
    let disk_out = nix_build(dir, "disk", offline)?;
    let firmware_out = nix_build(dir, "firmware", offline)?;
    let built_qcow2 = find_qcow2(&disk_out)?;
    let disk_path = disk;
    if let Some(parent) = disk_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("creating `{}` failed: {e}", parent.display()))?;
        }
    }
    // A prior run's copy inherits the store's read-only mode — drop it first
    // or the fresh copy is denied write access.
    let _ = fs::remove_file(disk_path);
    fs::copy(&built_qcow2, disk_path).map_err(|e| {
        format!(
            "copying `{}` to `{}` failed: {e}",
            built_qcow2.display(),
            disk_path.display()
        )
    })?;
    make_writable(disk_path)?;
    let ovmf_code = firmware_out.join("FV/OVMF_CODE.fd");
    let ovmf_vars_src = firmware_out.join("FV/OVMF_VARS.fd");
    let ovmf_vars = dir.join("OVMF_VARS.fd");
    fs::copy(&ovmf_vars_src, &ovmf_vars)
        .map_err(|e| format!("copying `{}` failed: {e}", ovmf_vars_src.display()))?;
    make_writable(&ovmf_vars)?;
    let log_path = dir.join("nixos-comparison.serial.log");
    let _ = fs::remove_file(&log_path);
    // AF_UNIX socket paths are capped at ~107 bytes; the backend dir easily
    // exceeds that, so the QMP socket lives in the system temp dir.
    let sock_path =
        std::env::temp_dir().join(format!("nixos-comparison-qmp-{}.sock", std::process::id()));
    let _ = fs::remove_file(&sock_path);
    let screenshot_path = dir.join("nixos-comparison-boot.png");
    let stderr_path = dir.join("nixos-comparison.qemu.stderr");
    let stderr_file = fs::File::create(&stderr_path)
        .map_err(|e| format!("creating `{}` failed: {e}", stderr_path.display()))?;
    let disk_abs = fs::canonicalize(disk_path)
        .map_err(|e| format!("resolving `{}` failed: {e}", disk_path.display()))?
        .display()
        .to_string();
    let argv = real_qemu_prove_argv(&disk_abs, &ovmf_code, &ovmf_vars, &log_path, &sock_path);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| format!("starting real QEMU proof failed: {e}"))?;
    let report = poll_for_guest_proof(&mut child, &log_path, migration_timeout());
    let qmp_result = qmp_screendump_and_powerdown(&sock_path, &screenshot_path);
    let _ = wait_child_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_file(&sock_path);
    let report = report?;
    require_nixos_guest_fact(&report)?;
    qmp_result?;
    Ok(MigrationRun {
        report,
        screenshot: screenshot_path,
        argv,
    })
}

fn nix_build_argv(target: &str, offline: bool) -> Vec<String> {
    let mut argv = vec![
        "nix".to_string(),
        "build".to_string(),
        format!("path:.#{target}"),
        "--no-link".to_string(),
        "--print-out-paths".to_string(),
    ];
    if offline {
        argv.push("--offline".to_string());
    }
    argv
}

fn nix_build(dir: &Path, target: &str, offline: bool) -> Result<PathBuf, String> {
    // `path:` keeps the generated flake usable when the backend dir sits
    // inside a user git repo (a bare `.#` ref would demand git-tracked files).
    let argv = nix_build_argv(target, offline);
    let out = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("starting `nix build path:.#{target}` failed: {e}"))?;
    let log = dir.join(format!("nix-build-{target}.log"));
    let _ = fs::write(
        &log,
        format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    );
    if !out.status.success() {
        return Err(format!(
            "`nix build .#{target}` exited {}; private build output was suppressed",
            out.status
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let path = stdout
        .lines()
        .next()
        .ok_or_else(|| format!("`nix build .#{target}` produced no output path"))?
        .trim();
    Ok(PathBuf::from(path))
}

fn find_qcow2(disk_out: &Path) -> Result<PathBuf, String> {
    if disk_out.extension().and_then(|e| e.to_str()) == Some("qcow2") {
        return Ok(disk_out.to_path_buf());
    }
    fn walk(dir: &Path, found: &mut Option<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if found.is_some() {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().and_then(|e| e.to_str()) == Some("qcow2") {
                *found = Some(path);
            }
        }
    }
    let mut found = None;
    walk(disk_out, &mut found);
    found.ok_or_else(|| format!("no `.qcow2` file found under `{}`", disk_out.display()))
}

fn make_writable(path: &Path) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("reading `{}` failed: {e}", path.display()))?;
    let mut perms = meta.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o644);
    }
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(path, perms).map_err(|e| format!("chmod `{}` failed: {e}", path.display()))
}

fn wait_child_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<()> {
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn poll_for_guest_proof(
    child: &mut std::process::Child,
    log_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let start = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(log_path) {
            if let Some(report) = text.lines().find_map(|line| {
                line.split_once(MIGRATION_GUEST_PROOF_MARKER)
                    .map(|(_, rest)| rest.trim().to_string())
            }) {
                return Ok(report);
            }
        }
        // A dead QEMU can never produce the marker. Never project its
        // untrusted stderr into a public diagnostic.
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "QEMU exited ({status}) before the guest proof marker; private VM output was suppressed"
            ));
        }
        if start.elapsed() > timeout {
            return Err(format!(
                "no `{MIGRATION_GUEST_PROOF_MARKER}` line appeared within {}ms; private VM output was suppressed",
                timeout.as_millis(),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[derive(Debug)]
struct ObservedGuestIdentity {
    os_release: String,
    prompt: String,
    banner: String,
}

fn require_nixos_guest_fact(report: &str) -> Result<ObservedGuestIdentity, String> {
    let fact =
        JSON::parse(report).map_err(|_| "invalid guest fact JSON; guest payload was suppressed")?;
    let field = |name| -> Result<String, String> {
        fact.get(name)
            .and_then(JSON::Json::as_str)
            .map(str::to_string)
            .map_err(|_| {
                "guest fact omitted a required identity field; guest payload was suppressed"
                    .to_string()
            })
    };
    let proof = field("proof")?;
    let os_release = field("os_release")?;
    let prompt = field("prompt")?;
    let banner = field("banner")?;
    let honest = proof == "live-desktop"
        && os_release == "NixOS"
        && prompt.starts_with("NixOS ")
        && banner == "NixOS comparison guest"
        && !report.to_ascii_lowercase().contains("jetos");
    if !honest {
        return Err(
            "guest fact did not prove honest NixOS os-release, observed prompt, banner, and live desktop; guest payload was suppressed"
                .to_string(),
        );
    }
    Ok(ObservedGuestIdentity {
        os_release,
        prompt,
        banner,
    })
}

#[cfg(not(unix))]
fn qmp_screendump_and_powerdown(sock_path: &Path, _screenshot: &Path) -> Result<(), String> {
    Err(format!(
        "QMP control socket `{}` needs Unix domain sockets, unavailable on this platform",
        sock_path.display()
    ))
}

#[cfg(unix)]
fn qmp_screendump_and_powerdown(sock_path: &Path, screenshot: &Path) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let stream = UnixStream::connect(sock_path)
        .map_err(|e| format!("connecting QMP socket `{}` failed: {e}", sock_path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("setting QMP read timeout failed: {e}"))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| format!("cloning QMP socket failed: {e}"))?,
    );
    let mut writer = stream;
    let mut greeting = String::new();
    reader
        .read_line(&mut greeting)
        .map_err(|e| format!("reading QMP greeting failed: {e}"))?;
    writer
        .write_all(b"{\"execute\":\"qmp_capabilities\"}\n")
        .map_err(|e| format!("QMP capabilities negotiation failed: {e}"))?;
    let mut capabilities_response = String::new();
    reader
        .read_line(&mut capabilities_response)
        .map_err(|e| format!("reading QMP capabilities response failed: {e}"))?;
    let screendump = format!(
        "{{\"execute\":\"screendump\",\"arguments\":{{\"filename\":{}}}}}\n",
        JSON::quote(&screenshot.display().to_string())
    );
    writer
        .write_all(screendump.as_bytes())
        .map_err(|e| format!("QMP screendump command failed: {e}"))?;
    let mut screendump_response = String::new();
    reader
        .read_line(&mut screendump_response)
        .map_err(|e| format!("reading QMP screendump response failed: {e}"))?;
    writer
        .write_all(b"{\"execute\":\"system_powerdown\"}\n")
        .map_err(|e| format!("QMP system_powerdown command failed: {e}"))?;
    Ok(())
}

fn publish_migration_bundle(
    out: &Path,
    system: &SystemPlan,
    mapping: &NixosMapping,
    disk: &Path,
    plan_path: &Path,
    run: &MigrationRun,
) -> Result<(), String> {
    let observed = require_nixos_guest_fact(&run.report)?;
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let publish = parent.join(format!(
        ".nixos-comparison-publish-{}-{}",
        system.name,
        std::process::id()
    ));
    create_private_stage(&publish)
        .map_err(|error| format!("creating `{}` failed: {error}", publish.display()))?;
    let result = (|| -> Result<(), String> {
        let nixos = publish.join("nixos");
        create_private_stage(&nixos)
            .map_err(|error| format!("creating `{}` failed: {error}", nixos.display()))?;
        let host = nixos.join(&system.name);
        create_private_stage(&host)
            .map_err(|error| format!("creating `{}` failed: {error}", host.display()))?;
        let image = host.join("system.qcow2");
        let screenshot = host.join("boot.png");
        let plan = host.join("build-boot-plan.json");
        fs::copy(disk, &image)
            .map_err(|error| format!("publishing system image failed: {error}"))?;
        make_writable(&image)?;
        fs::copy(&run.screenshot, &screenshot)
            .map_err(|error| format!("publishing boot screenshot failed: {error}"))?;
        fs::copy(plan_path, &plan)
            .map_err(|error| format!("publishing build/boot plan failed: {error}"))?;
        let disk_sha = file_sha256(&image)?;
        let guest_fact = format!(
            "{{\"kind\":\"nixos.migration.vmtest.guest-fact\",\"host\":{},\"os_release\":{},\"prompt\":{},\"banner\":{},\"report\":{}}}\n",
            JSON::quote(&system.name),
            JSON::quote(&observed.os_release),
            JSON::quote(&observed.prompt),
            JSON::quote(&observed.banner),
            run.report.trim()
        );
        fs::write(host.join("guest-fact.json"), &guest_fact)
            .map_err(|error| format!("writing guest fact failed: {error}"))?;
        let proof = format!(
            "{{\"kind\":\"nixos.migration.boot-proof\",\"state\":\"passed\",\"host\":{},\"image\":\"system.qcow2\",\"disk_sha256\":{},\"screenshot\":\"boot.png\",\"guest_fact\":\"guest-fact.json\",\"qemu_argv\":[{}]}}\n",
            JSON::quote(&system.name),
            JSON::quote(&disk_sha),
            run.argv
                .iter()
                .map(|arg| JSON::quote(arg))
                .collect::<Vec<_>>()
                .join(",")
        );
        fs::write(host.join("proof.json"), proof)
            .map_err(|error| format!("writing boot proof failed: {error}"))?;
        let receipt = format!(
            "{{\"kind\":\"nixos.migration.receipt\",\"state\":\"proved\",\"command\":\"jet os migrate compare-nixos\",\"host\":{},\"nixpkgs\":{},\"image\":\"system.qcow2\",\"boot_proof\":\"proof.json\",\"guest_fact\":\"guest-fact.json\"}}\n",
            JSON::quote(&system.name),
            JSON::quote(&format!(
                "github:{}/{}/{}",
                mapping.nixpkgs_owner, mapping.nixpkgs_repo, mapping.nixpkgs_rev
            ))
        );
        fs::write(host.join("receipt.json"), receipt)
            .map_err(|error| format!("writing migration receipt failed: {error}"))?;
        fs::rename(&publish, out)
            .map_err(|error| format!("publishing `{}` failed: {error}", out.display()))
    })();
    finish_private_stage(&publish, result)
}

// Unit-tested directly: the real-guest CLI path is gated by `require_real_vm_tools`
// (E1290), which byte-scans every VM tool on PATH and rejects any file whose
// bytes contain "fake" — several genuine dev-shell binaries (e.g. `zstd`,
// `mkfs.vfat`) happen to contain that 4-byte sequence somewhere in their
// compiled data, so no environment observed so far can drive this path
// through the CLI. `map_system_to_nixos`/rendering/planning are pure and
// deterministic, so they are tested here instead of via `tests/jetpack_jetos.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use jet_env_model::ModuleEval::{self, ServicePlan};

    fn table_with_nixpkgs() -> RefSpec::SourceTable {
        RefSpec::SourceTable::from_decls([(
            "default".to_string(),
            "github:NixOS/nixpkgs/fef9403a3e4d31b0a23f0bacebbec52c248fbb51".to_string(),
            RefSpec::ProviderKind::Nix,
        )])
    }

    fn opt(key: &str, value: &str) -> ModuleEval::OptionPlan {
        ModuleEval::OptionPlan {
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    fn full_system() -> SystemPlan {
        SystemPlan {
            name: "halcyon-gnome".to_string(),
            target: "linux.x64".to_string(),
            packages: vec![
                crate::Merge::Pkg::new("default", "firefox"),
                crate::Merge::Pkg::new("default", "btop"),
            ],
            services: vec![
                ServicePlan {
                    name: "openssh".to_string(),
                    enable: true,
                    extra: vec![("ports".to_string(), "[22]".to_string())],
                },
                ServicePlan {
                    name: "backup".to_string(),
                    enable: true,
                    extra: vec![
                        ("exec".to_string(), "/run/current-system/sw/bin/hello".to_string()),
                        ("timer".to_string(), "daily".to_string()),
                    ],
                },
            ],
            options: vec![
                opt("network.hostName", "halcyon-gnome"),
                opt("network.networkmanager.enable", "true"),
                opt("network.firewall.allowedTcpPorts", "[22, 443]"),
                opt("network.dns", "[\"1.1.1.1\", \"8.8.8.8\"]"),
                opt("filesystem.timeZone", "\"America/New_York\""),
                opt("services.localization.locale", "\"en_US.UTF-8\""),
                opt("services.localization.keyboardLayout", "\"us\""),
                opt("users.nate.normal", "true"),
                opt("users.nate.home", "\"/home/nate\""),
                opt("users.nate.groups", "[\"wheel\", \"networkmanager\"]"),
                opt("users.nate.initialPassword", "\"jetos\""),
                opt("users.nate.shell", "default.fish"),
                opt("boot.loader", ".SystemdBoot"),
                opt("boot.loader.efi.canTouchVariables", "false"),
                opt("boot.kernel.params", "[\"quiet\"]"),
                opt("init.defaultTarget", "\"graphical.target\""),
                opt("services.desktop.profile", ".Default"),
                opt("services.displayManager", "\"gdm\""),
                opt("services.desktop.autoLogin.user", "\"nate\""),
                opt("services.audio.pipewire.enable", "true"),
                opt("services.audio.rtkit.enable", "true"),
            ],
        }
    }

    #[test]
    fn maps_full_system_to_nixos() {
        let table = table_with_nixpkgs();
        let system = full_system();
        let mapping = map_system_to_nixos(&system, &table).expect("fully mapped system");
        assert_eq!(mapping.nixpkgs_owner, "NixOS");
        assert_eq!(mapping.nixpkgs_repo, "nixpkgs");
        assert_eq!(
            mapping.nixpkgs_rev,
            "fef9403a3e4d31b0a23f0bacebbec52c248fbb51"
        );
        assert_eq!(mapping.system_packages, vec!["firefox".to_string(), "btop".to_string()]);
        assert!(mapping
            .body_lines
            .contains(&"  networking.hostName = \"halcyon-gnome\";".to_string()));
        assert!(mapping
            .body_lines
            .contains(&"  boot.loader.systemd-boot.enable = true;".to_string()));
        assert!(mapping
            .body_lines
            .contains(&"  services.desktopManager.gnome.enable = true;".to_string()));
        assert!(mapping
            .body_lines
            .contains(&"  services.displayManager.gdm.enable = true;".to_string()));
        assert!(mapping.body_lines.iter().any(|l| l.contains("shell = pkgs.fish;")));
        assert!(mapping
            .body_lines
            .contains(&"  programs.fish.enable = true;".to_string()));
        assert!(mapping
            .body_lines
            .iter()
            .any(|l| l.contains("services.openssh.enable = true;")));
        assert!(mapping
            .body_lines
            .iter()
            .any(|l| l.contains("systemd.services.\"backup\"")));
        assert!(mapping
            .body_lines
            .iter()
            .any(|l| l.contains("systemd.timers.\"backup\"")));
    }

    #[test]
    fn flake_and_configuration_render_expected_nix() {
        let table = table_with_nixpkgs();
        let system = full_system();
        let mapping = map_system_to_nixos(&system, &table).unwrap();
        let flake = render_flake_nix(&system.name, &mapping);
        assert!(flake.contains(
            "inputs.nixpkgs.url = \"github:NixOS/nixpkgs/fef9403a3e4d31b0a23f0bacebbec52c248fbb51\";"
        ));
        assert!(flake.contains("nixosConfigurations.halcyon-gnome = lib.nixosSystem {"));
        assert!(flake.contains("config = self.nixosConfigurations.halcyon-gnome.config;"));
        assert!(flake.contains("firmware = pkgs.OVMF.fd;"));

        let configuration = render_configuration_nix(&system, &mapping);
        assert!(configuration.contains("system.stateVersion = \"26.05\";"));
        assert!(!configuration.contains("system.nixos.distroName"));
        assert!(!configuration.contains("system.nixos.distroId"));
        assert!(configuration.contains("environment.systemPackages = map jetosPkg [ \"firefox\" \"btop\" ];"));
        assert!(configuration.contains("programs.bash.promptInit = lib.mkAfter"));
        assert!(configuration.contains("PS1='NixOS comparison $ '"));
        assert!(configuration.contains("environment.etc.issue.text = \"NixOS comparison guest"));
        assert!(configuration.contains("systemd.services.nixos-comparison-proof = {"));
        assert!(configuration.contains("path = [ pkgs.bash pkgs.util-linux pkgs.systemd"));
        assert!(configuration.contains(
            "prompt=$(runuser -u nate -- bash --noprofile --rcfile /etc/bashrc -i -c 'printf \"%s\" \"$PS1\"'"
        ));
        assert!(configuration.contains("pgrep -u nate -f gnome-shell"));
        assert!(configuration.contains("NIXOS_COMPARISON_PROOF:"));
    }

    #[test]
    fn unmapped_option_key_is_rejected() {
        let table = table_with_nixpkgs();
        let mut system = full_system();
        system.options.push(opt("apps.workload.bogusFeature", "true"));
        let err = map_system_to_nixos(&system, &table).unwrap_err();
        assert!(
            err.iter().any(|m| m.contains("option `apps.workload.bogusFeature`")),
            "{err:?}"
        );
    }

    #[test]
    fn non_nixpkgs_package_is_rejected() {
        let table = table_with_nixpkgs();
        let mut system = full_system();
        system.packages.push(crate::Merge::Pkg::new("mine", "hello"));
        let err = map_system_to_nixos(&system, &table).unwrap_err();
        assert!(err.iter().any(|m| m.contains("package `mine.hello`")), "{err:?}");
    }

    #[test]
    fn unknown_service_without_exec_is_rejected() {
        let table = table_with_nixpkgs();
        let mut system = full_system();
        system.services.push(ServicePlan {
            name: "mystery".to_string(),
            enable: true,
            extra: vec![],
        });
        let err = map_system_to_nixos(&system, &table).unwrap_err();
        assert!(err.iter().any(|m| m.contains("service `mystery`")), "{err:?}");
    }

    #[test]
    fn missing_nixpkgs_pin_is_rejected() {
        let table = RefSpec::SourceTable::from_decls(Vec::<(String, String, RefSpec::ProviderKind)>::new());
        let system = SystemPlan {
            packages: vec![],
            services: vec![],
            options: vec![],
            ..full_system()
        };
        let err = map_system_to_nixos(&system, &table).unwrap_err();
        assert!(
            err.iter().any(|m| m.contains("needs exactly one nixpkgs pin")),
            "{err:?}"
        );
    }

    #[test]
    fn nixos_migration_plan_snapshots_build_and_boot_argv() {
        let dir_root = std::env::temp_dir().join(format!(
            "nixos-migration-test-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir_root);
        fs::create_dir_all(&dir_root).unwrap();
        let system = full_system();
        let path = write_migration_plan(&dir_root, &system, "halcyon.qcow2", false).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"kind\":\"nixos.migration.plan\""));
        assert!(text.contains("\"phase\":\"nix-build-disk\""));
        assert!(text.contains("\"path:.#disk\""));
        assert!(text.contains("\"phase\":\"nix-build-firmware\""));
        assert!(text.contains("\"path:.#firmware\""));
        assert!(text.contains("\"phase\":\"boot-real-guest\""));
        assert!(text.contains("\"-enable-kvm\""));
        assert!(text.contains("\"virtio-gpu-pci\""));
        assert!(text.contains("\"-qmp\""));
        assert!(text.contains("file=halcyon.qcow2,format=qcow2,if=virtio"));
        let _ = fs::remove_dir_all(&dir_root);
    }

    #[test]
    fn migration_bundle_preserves_observed_guest_identity() {
        let root = std::env::temp_dir().join(format!(
            "nixos-migration-guest-fact-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let disk = root.join("staged.qcow2");
        let screenshot = root.join("staged.png");
        let plan = root.join("plan.json");
        fs::write(&disk, b"qcow2").unwrap();
        fs::write(&screenshot, b"png").unwrap();
        fs::write(&plan, b"{}").unwrap();
        let system = full_system();
        let mapping = map_system_to_nixos(&system, &table_with_nixpkgs()).unwrap();
        let report = r#"{"host":"halcyon-gnome","os_release":"NixOS","prompt":"NixOS observed-profile","banner":"NixOS comparison guest","proof":"live-desktop"}"#;
        let run = MigrationRun {
            report: report.to_string(),
            screenshot,
            argv: vec!["qemu-system-x86_64".to_string()],
        };
        let out = root.join("published");
        publish_migration_bundle(&out, &system, &mapping, &disk, &plan, &run).unwrap();
        let host = out.join("nixos/halcyon-gnome");
        let fact = fs::read_to_string(host.join("guest-fact.json")).unwrap();
        assert!(fact.contains("\"kind\":\"nixos.migration.vmtest.guest-fact\""));
        assert!(fact.contains("\"os_release\":\"NixOS\""));
        assert!(fact.contains("\"prompt\":\"NixOS observed-profile\""));
        assert!(!fact.contains("\"prompt\":\"NixOS halcyon-gnome\""));
        assert!(fact.contains("\"banner\":\"NixOS comparison guest\""));
        assert!(!fact.to_ascii_lowercase().contains("jetos"));
        assert!(host.join("system.qcow2").is_file());
        assert!(host.join("proof.json").is_file());
        assert!(host.join("receipt.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_guest_report_is_not_projected_into_diagnostics() {
        let report = "{not-json AWS_SECRET_ACCESS_KEY=secret Authorization: Bearer token";
        let error = require_nixos_guest_fact(&report).unwrap_err();
        assert_eq!(
            error,
            "invalid guest fact JSON; guest payload was suppressed"
        );
        assert!(!error.contains("secret"));
        assert!(!error.contains("token"));
    }

    #[test]
    fn cleanup_failure_preserves_the_root_cause() {
        let stage = Path::new("/private-stage");
        let error = finish_private_stage_with(
            stage,
            Err("root build failure".to_string()),
            |_| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied")),
        )
        .unwrap_err();
        assert!(error.contains("root build failure"));
        assert!(error.contains("cleanup also failed"));
        assert!(error.contains("/private-stage"));
        assert!(error.contains("denied"));
    }
}
