// Real-guest tier backend (card #363): `jet os vm prove <host> --disk <path>
// --real` realizes the system through a hidden NixOS backend instead of the
// plumbing installer-ISO harness. The user never writes Nix — `SystemPlan`
// (U11-U13) is mapped to a generated `flake.nix` + `configuration.nix` under
// the Jetpack root, `nix build` produces a bootable qcow2, and QEMU boots it
// to capture a live-desktop guest proof. Mirrors the Jet -> generated-Rust ->
// hidden-rustc pattern (I2): nix output never reaches the user directly.
//
// D-JOS-NIXBACKEND1=C (card #363): every `SystemPlan` option/service/package
// that this backend cannot map to a NixOS setting is collected and reported
// in ONE diagnostic (E1291) before `nix` ever runs — no silent omissions,
// mirroring the import-direction discipline in D-JOS-NIXIMPORT1=C.

const NIXOS_STATE_VERSION: &str = "26.05";
const REAL_TIER_TIMEOUT_MS: u64 = 1_200_000;

/// One realized NixOS mapping of a `SystemPlan`, ready to render.
#[derive(Debug)]
struct NixosMapping {
    nixpkgs_owner: String,
    nixpkgs_repo: String,
    nixpkgs_rev: String,
    body_lines: Vec<String>,
    system_packages: Vec<String>,
}

fn nixos_backend_dir(host: &str) -> PathBuf {
    systems_dir().join("backend").join(host)
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
/// `github@NixOS/nixpkgs/<rev-or-ref>` ref (U6 `sources:` block).
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
    matches!(
        key,
        "network.hostName"
            | "network.networkmanager.enable"
            | "network.firewall.allowedTcpPorts"
            | "network.dns"
            | "filesystem.timeZone"
            | "services.localization.locale"
            | "services.localization.keyboardLayout"
            | "boot.loader"
            | "boot.loader.efi.canTouchVariables"
            | "boot.kernel.params"
            | "init.defaultTarget"
            | "services.desktop.profile"
            | "services.displayManager"
            | "services.desktop.autoLogin.user"
            | "services.audio.pipewire.enable"
            | "services.audio.rtkit.enable"
    )
}

fn shell_package_attr(value: &str) -> String {
    clean_value(value)
        .rsplit('.')
        .next()
        .unwrap_or(value)
        .to_string()
}

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
        unmapped.push(
            "no declared source resolves to `github@NixOS/nixpkgs/<rev-or-ref>` (the real tier needs exactly one nixpkgs pin)"
                .to_string(),
        );
    }

    let mut system_packages = Vec::new();
    for pkg in &system.packages {
        let raw = if pkg.source.is_empty() {
            pkg.name.clone()
        } else {
            format!("{}:{}", pkg.source, pkg.name)
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
    let display_manager = resolved_option_value(system, "services.displayManager")
        .map(|v| clean_value(&v))
        .or_else(|| desktop_default.then(|| "gdm".to_string()));
    if desktop_default {
        body_lines.push("  services.desktopManager.gnome.enable = true;".to_string());
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
        body_lines.push(format!("  users.users.{name} = {{"));
        body_lines.extend(user_lines);
        body_lines.push("  };".to_string());
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
    let (nixpkgs_owner, nixpkgs_repo, nixpkgs_rev) = pin.expect("checked above");
    Ok(NixosMapping {
        nixpkgs_owner,
        nixpkgs_repo,
        nixpkgs_rev,
        body_lines,
        system_packages,
    })
}

const FLAKE_TEMPLATE: &str = r#"{
  description = "jetos backend for @@HOST@@ (generated by `jet os vm prove --real`; do not hand-edit)";

  inputs.nixpkgs.url = "github:@@OWNER@@/@@REPO@@/@@REV@@";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = nixpkgs.lib;
    in {
      nixosConfigurations.@@HOST@@ = lib.nixosSystem {
        inherit system;
        modules = [ ./configuration.nix ];
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
    FLAKE_TEMPLATE
        .replace("@@HOST@@", host)
        .replace("@@OWNER@@", &mapping.nixpkgs_owner)
        .replace("@@REPO@@", &mapping.nixpkgs_repo)
        .replace("@@REV@@", &mapping.nixpkgs_rev)
}

const CONFIGURATION_HEAD: &str = r#"{ config, pkgs, lib, ... }:
{
  system.stateVersion = "@@STATEVERSION@@";
  system.nixos.distroName = "jetos";
  system.nixos.distroId = "jetos";

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
"#;

const CONFIGURATION_PROOF_SERVICE: &str = r#"
  # jetos guest proof: emits one JSON line on the serial console once the
  # graphical session is genuinely live. Consumed by `jet os vm prove --real`.
  systemd.services.jetos-proof = {
    description = "jetos real-guest proof emitter";
    wantedBy = [ "multi-user.target" ];
    after = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.systemd pkgs.procps pkgs.jq pkgs.gawk pkgs.gnused pkgs.coreutils ];
    script = ''
      deadline=$((SECONDS + 300))
      while [ "$SECONDS" -lt "$deadline" ]; do
        dm=$(systemctl is-active display-manager.service || true)
        shell_pid=$(pgrep -u @@USER@@ -x gnome-shell || true)
        session=$(loginctl list-sessions --no-legend | awk '$3=="@@USER@@" {print $1; exit}')
        stype=""
        if [ -n "$session" ]; then
          stype=$(loginctl show-session "$session" -p Type --value || true)
        fi
        if [ "$dm" = "active" ] && [ -n "$shell_pid" ] && [ "$stype" = "wayland" ]; then
          jq -cn \
            --arg host "$(cat /proc/sys/kernel/hostname)" \
            --arg kernel "$(uname -r)" \
            --arg dm "$dm" \
            --arg session_type "$stype" \
            --arg shell_pid "$shell_pid" \
            --arg user "@@USER@@" \
            '{host:$host,kernel:$kernel,display_manager:$dm,session_type:$session_type,gnome_shell_pid:$shell_pid,user:$user,desktop:"gnome",proof:"live-desktop"}' \
            | sed 's/^/JETOS_GUEST_PROOF:/' > /dev/ttyS0
          exit 0
        fi
        sleep 2
      done
      echo 'JETOS_GUEST_PROOF:{"proof":"failed","reason":"desktop-not-live-within-300s"}' > /dev/ttyS0
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
        "  environment.systemPackages = with pkgs; [ {} ];\n",
        mapping.system_packages.join(" ")
    ));
    let proof_user = resolved_option_value(system, "services.desktop.autoLogin.user")
        .unwrap_or_else(|| {
            collect_names(system, "users")
                .into_iter()
                .next()
                .unwrap_or_else(|| "root".to_string())
        });
    out.push_str(&CONFIGURATION_PROOF_SERVICE.replace("@@USER@@", &proof_user));
    out
}

/// Write `flake.nix` + `configuration.nix` + `jetos-facts.json` into the
/// backend dir. Deterministic and offline — no `nix`/`qemu` invocation here.
fn write_nixos_backend(
    dir: &Path,
    system: &SystemPlan,
    mapping: &NixosMapping,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join("flake.nix"), render_flake_nix(&system.name, mapping))?;
    fs::write(
        dir.join("configuration.nix"),
        render_configuration_nix(system, mapping),
    )?;
    let facts = format!(
        "{{\"kind\":\"jetos.nixos-backend.facts\",\"host\":{},\"nixpkgs\":{},\"packages\":[{}]}}\n",
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
    fs::write(dir.join("jetos-facts.json"), facts)
}

fn real_tier_timeout() -> Duration {
    std::env::var("JETOS_VM_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(REAL_TIER_TIMEOUT_MS))
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

fn qemu_interactive_real_run_command(disk: &str) -> VmCommand {
    VmCommand {
        phase: "run-real-installed-disk",
        argv: vec![
            "qemu-system-x86_64".to_string(),
            "-enable-kvm".to_string(),
            "-cpu".to_string(),
            "host".to_string(),
            "-m".to_string(),
            "4096".to_string(),
            "-smp".to_string(),
            "4".to_string(),
            "-drive".to_string(),
            format!("file={disk},format=qcow2,if=virtio"),
            "-device".to_string(),
            "virtio-gpu-pci".to_string(),
            "-display".to_string(),
            "gtk,gl=off".to_string(),
        ],
    }
}

/// The deterministic plan (nix build argv + QEMU boot argv) for a real-tier
/// proof run, snapshot-tested without ever invoking `nix`/`qemu` (the paths
/// below firmware/OVMF are display-only until the real build runs).
fn write_real_tier_plan(
    dir: &Path,
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
) -> std::io::Result<PathBuf> {
    let log_path = dir.join(format!("{}-real-boot.serial.log", gen.name));
    let sock_path = dir.join(format!("{}-real-boot.qmp.sock", gen.name));
    let ovmf_code = dir.join("firmware/FV/OVMF_CODE.fd");
    let ovmf_vars = dir.join("OVMF_VARS.fd");
    let build_commands: [(&str, Vec<String>); 2] = [
        (
            "nix-build-disk",
            vec![
                "nix".to_string(),
                "build".to_string(),
                "path:.#disk".to_string(),
                "--no-link".to_string(),
                "--print-out-paths".to_string(),
            ],
        ),
        (
            "nix-build-firmware",
            vec![
                "nix".to_string(),
                "build".to_string(),
                "path:.#firmware".to_string(),
                "--no-link".to_string(),
                "--print-out-paths".to_string(),
            ],
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
        "{{\"kind\":\"jetos.vm.real-plan\",\"host\":{},\"generation\":{},\"disk\":{},\"backend_dir\":{},\"commands\":[{}]}}\n",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&dir.display().to_string()),
        commands_json.join(",")
    );
    let path = dir.join("vm-real-plan.json");
    fs::write(&path, text)?;
    Ok(path)
}

fn real_tier_proof_marker_path(disk: &str) -> PathBuf {
    PathBuf::from(format!("{disk}.jetos-real-proof.json"))
}

/// `jet os vm prove <host> --disk <path> --real`'s driver, invoked from
/// `cmd_vm` after `build_generation` succeeds (E1279/E1290 already gated the
/// caller). Never exercised end-to-end by the test suite (no test may invoke
/// real `nix build`/`qemu`) — codegen + planning above this point is what
/// tests snapshot.
fn cmd_vm_prove_real(
    theme: &Theme,
    gen: &Generation,
    table: &RefSpec::SourceTable,
    system: &SystemPlan,
    disk: &str,
    flags: &OsFlags,
) -> i32 {
    let mapping = match map_system_to_nixos(system, table) {
        Ok(mapping) => mapping,
        Err(unmapped) => {
            theme.error_coded(
                "E1291",
                "jetos real tier could not map every system declaration to NixOS",
                &format!(
                    "D-JOS-NIXBACKEND1=C forbids silently dropping a declaration when generating the hidden NixOS backend (mirrors D-JOS-NIXIMPORT1=C's no-silent-omissions rule for the import direction); unmapped: {}.",
                    unmapped.join("; ")
                ),
                "rename or drop the unmapped keys/packages/services, or map them to the nearest supported real-tier option listed in docs/spec/diagnostics.md (E1291).",
            );
            return 2;
        }
    };
    let dir = nixos_backend_dir(&system.name);
    if let Err(e) = write_nixos_backend(&dir, system, &mapping) {
        theme.error(
            "could not write the jetos NixOS backend",
            &format!("writing `{}` failed: {e}.", dir.display()),
            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
        );
        return 2;
    }
    let plan_path = match write_real_tier_plan(&dir, gen, system, disk) {
        Ok(path) => path,
        Err(e) => {
            theme.error(
                "could not write the jetos real-tier VM plan",
                &format!("writing plan under `{}` failed: {e}.", dir.display()),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            return 2;
        }
    };
    if flags.offline {
        theme.error_coded(
            "E1276",
            "jetos real tier needs network",
            "the hidden NixOS backend runs `nix build` against the pinned nixpkgs input (D-JOS-NIXBACKEND1=C); `--offline` forbids that fetch/build unless every input is already cached.",
            "drop `--offline` once the pinned nixpkgs input and packages are cached locally, or run `jet os vm prove` without `--real --offline` together.",
        );
        return 2;
    }
    match run_real_tier_build_and_boot(&dir, gen, system, disk, &mapping, &plan_path) {
        Ok(()) => {
            theme.ok(&format!(
                "proved jetos real-guest VM {} generation {}",
                theme.bold(&system.name),
                theme.bold(&gen.name)
            ));
            0
        }
        Err(e) => {
            theme.error_coded(
                "E1285",
                "jetos VM guest proof has not run",
                &format!("the real-tier build/boot for `{}` did not produce a passing guest proof: {e}.", system.name),
                "inspect the backend build/QEMU logs under the backend dir, fix the failure, then rerun `jet os vm prove <host> --disk <disk> --real`.",
            );
            2
        }
    }
}

fn run_real_tier_build_and_boot(
    dir: &Path,
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    mapping: &NixosMapping,
    plan_path: &Path,
) -> Result<(), String> {
    let disk_out = nix_build(dir, "disk")?;
    let firmware_out = nix_build(dir, "firmware")?;
    let built_qcow2 = find_qcow2(&disk_out)?;
    let disk_path = Path::new(disk);
    if let Some(parent) = disk_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("creating `{}` failed: {e}", parent.display()))?;
        }
    }
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
    let log_path = dir.join(format!("{}-real-boot.serial.log", gen.name));
    let _ = fs::remove_file(&log_path);
    // AF_UNIX socket paths are capped at ~107 bytes; the backend dir easily
    // exceeds that, so the QMP socket lives in the system temp dir.
    let sock_path = std::env::temp_dir().join(format!("jetos-qmp-{}.sock", std::process::id()));
    let _ = fs::remove_file(&sock_path);
    let screenshot_path = dir.join(format!("{}-real-boot.png", gen.name));
    let stderr_path = dir.join(format!("{}-real-boot.qemu.stderr", gen.name));
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
    let report = poll_for_guest_proof(&mut child, &stderr_path, &log_path, real_tier_timeout());
    let qmp_result = qmp_screendump_and_powerdown(&sock_path, &screenshot_path);
    let _ = wait_child_with_timeout(&mut child, Duration::from_secs(30));
    let _ = fs::remove_file(&sock_path);
    let report = report?;
    require_report_live_desktop(&report)?;
    qmp_result?;
    write_real_tier_proof(
        dir,
        gen,
        system,
        disk,
        mapping,
        &report,
        &screenshot_path,
        &argv,
        plan_path,
    )
}

fn nix_build(dir: &Path, target: &str) -> Result<PathBuf, String> {
    // `path:` keeps the generated flake usable when the backend dir sits
    // inside a user git repo (a bare `.#` ref would demand git-tracked files).
    let out = Command::new("nix")
        .args([
            "build",
            &format!("path:.#{target}"),
            "--no-link",
            "--print-out-paths",
        ])
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
        let stderr = String::from_utf8_lossy(&out.stderr);
        let line = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("");
        return Err(format!(
            "`nix build .#{target}` failed; see `{}`; {line}",
            log.display()
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
    stderr_path: &Path,
    log_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let start = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(log_path) {
            if let Some(report) = extract_guest_proof_report(&text) {
                return Ok(report);
            }
        }
        // A dead QEMU can never produce the marker — fail fast with its
        // stderr instead of burning the whole timeout.
        if let Ok(Some(status)) = child.try_wait() {
            let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
            let line = stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("no stderr output");
            let excerpt: String = line.chars().take(240).collect();
            return Err(format!(
                "QEMU exited ({status}) before the guest proof marker; {excerpt}; full stderr at `{}`",
                stderr_path.display()
            ));
        }
        if start.elapsed() > timeout {
            return Err(format!(
                "no `{VM_GUEST_PROOF_MARKER}` line appeared in `{}` within {}ms",
                log_path.display(),
                timeout.as_millis()
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn require_report_live_desktop(report: &str) -> Result<(), String> {
    if report.contains("\"proof\":\"live-desktop\"") {
        Ok(())
    } else {
        Err(format!("guest report did not claim live-desktop: {report}"))
    }
}

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

fn write_real_tier_proof(
    dir: &Path,
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    mapping: &NixosMapping,
    report: &str,
    screenshot: &Path,
    argv: &[String],
    plan_path: &Path,
) -> Result<(), String> {
    let disk_sha = file_sha256(Path::new(disk))?;
    let text = format!(
        "{{\"kind\":\"jetos.vm.real-proof\",\"proof_tier\":\"real-guest\",\"host\":{},\"generation\":{},\"disk\":{},\"disk_sha256\":{},\"backend_dir\":{},\"nixpkgs\":{},\"qemu_argv\":[{}],\"screenshot\":{},\"plan\":{},\"serial_report\":{}}}\n",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&disk_sha),
        JSON::quote(&dir.display().to_string()),
        JSON::quote(&format!(
            "github:{}/{}/{}",
            mapping.nixpkgs_owner, mapping.nixpkgs_repo, mapping.nixpkgs_rev
        )),
        argv.iter().map(|a| JSON::quote(a)).collect::<Vec<_>>().join(","),
        JSON::quote(&screenshot.display().to_string()),
        JSON::quote(&plan_path.display().to_string()),
        JSON::quote(report),
    );
    let proof_dir = systems_dir().join("vm-proofs");
    fs::create_dir_all(&proof_dir)
        .map_err(|e| format!("creating `{}` failed: {e}", proof_dir.display()))?;
    let proof_path = proof_dir.join(format!("{}-{}-real-vm-proof.json", system.name, gen.name));
    fs::write(&proof_path, &text)
        .map_err(|e| format!("writing `{}` failed: {e}", proof_path.display()))?;
    let marker = real_tier_proof_marker_path(disk);
    fs::write(&marker, &text).map_err(|e| format!("writing `{}` failed: {e}", marker.display()))?;
    Ok(())
}

/// Real-tier `jet os vm run`: an interactive boot of a disk that carries a
/// passing real-tier proof marker (written beside the disk by
/// `write_real_tier_proof`). Plumbing-tier disks are unaffected — `cmd_vm_run`
/// only takes this path when the marker file exists.
fn cmd_vm_run_real(theme: &Theme, system: &SystemPlan, disk: &str, marker: &Path) -> i32 {
    let text = match fs::read_to_string(marker) {
        Ok(text) => text,
        Err(e) => {
            theme.error(
                "could not read the jetos real VM proof",
                &format!("reading `{}` failed: {e}.", marker.display()),
                "rerun `jet os vm prove <host> --disk <disk> --real`.",
            );
            return 2;
        }
    };
    if require_json_field(&text, "host", &system.name).is_err()
        || require_json_field(&text, "disk", disk).is_err()
    {
        theme.error_coded(
            "E1287",
            "jetos VM run needs a proved installed disk",
            "the real-tier proof marker beside this disk does not match this host/disk.",
            "run `jet os vm prove <host> --disk <disk> --real` first, then rerun `jet os vm run`.",
        );
        return 2;
    }
    let missing = missing_vm_tools()
        .into_iter()
        .filter(|tool| tool == "qemu-system-x86_64")
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        theme.error_coded(
            "E1279",
            "jetos VM proof tools are missing",
            &format!(
                "D-JOS-VMDEPS1=A requires pinned VM/media tools before a VM can run; missing: {}.",
                missing.join(", ")
            ),
            "realize or expose qemu-system-x86_64, then rerun `jet os vm run`.",
        );
        return 2;
    }
    let command = qemu_interactive_real_run_command(disk);
    theme.ok(&format!(
        "booting jetos real-guest VM {}",
        theme.bold(&system.name)
    ));
    match run_interactive_vm_command(&command) {
        Ok(code) => code,
        Err(e) => {
            theme.error(
                "could not run the jetos VM",
                &format!("starting interactive QEMU failed: {e}"),
                "check the real VM proof artifacts and rerun `jet os vm prove --real` if the disk changed.",
            );
            2
        }
    }
}

// Unit-tested directly: the real-guest CLI path is gated by `require_real_vm_tools`
// (E1290), which byte-scans every VM tool on PATH and rejects any file whose
// bytes contain "fake" — several genuine dev-shell binaries (e.g. `zstd`,
// `mkfs.vfat`) happen to contain that 4-byte sequence somewhere in their
// compiled data, so no environment observed so far can drive this path
// through the CLI. `map_system_to_nixos`/rendering/planning are pure and
// deterministic, so they are tested here instead of via `tests/jetpack.rs`.
#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(configuration.contains("environment.systemPackages = with pkgs; [ firefox btop ];"));
        assert!(configuration.contains("systemd.services.jetos-proof = {"));
        assert!(configuration.contains("pgrep -u nate -x gnome-shell"));
        assert!(configuration.contains("JETOS_GUEST_PROOF:"));
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
    fn real_tier_plan_snapshots_planned_argv() {
        let dir_root = std::env::temp_dir().join(format!(
            "jetos-nixbackend-test-plan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir_root);
        fs::create_dir_all(&dir_root).unwrap();
        let gen = Generation {
            name: "gen-1".to_string(),
            host: "halcyon-gnome".to_string(),
            path: dir_root.join("generation"),
            created_at: 0,
        };
        let system = full_system();
        let path = write_real_tier_plan(&dir_root, &gen, &system, "halcyon.qcow2").unwrap();
        let text = fs::read_to_string(&path).unwrap();
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
    fn interactive_real_run_uses_gtk_and_kvm() {
        let command = qemu_interactive_real_run_command("halcyon.qcow2");
        assert_eq!(command.phase, "run-real-installed-disk");
        assert!(command.argv.contains(&"-enable-kvm".to_string()));
        assert!(command.argv.contains(&"gtk,gl=off".to_string()));
        assert!(!command.argv.iter().any(|a| a == "-serial"));
        assert!(command
            .argv
            .iter()
            .any(|a| a == "file=halcyon.qcow2,format=qcow2,if=virtio"));
    }
}
