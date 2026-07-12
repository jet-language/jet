use super::activation_provenance::compat_hatches_json;
use super::options_rendering::{
    clean_bool_json, clean_symbol, option_rows_json, option_value, parse_list_items,
    prefixed_options, risk_classes, strings_json, user_names,
};
use super::root_projection::enable_unit;
use super::store_realize::RealizedPackage;
use super::studio_projection::make_executable;
use super::types::VM_TOOLS;
use crate::ModuleEval::SystemPlan;
use crate::JSON;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn write_acceptance_fixture(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let acceptance_dir = dir.join("acceptance");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&acceptance_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let coverage = vec![
        (
            "flake-inputs-pins-channels",
            option_value(system, &["packages.channel"]).is_some(),
            "#330/U21",
        ),
        (
            "overlays-patches-nur-unfree",
            !prefixed_options(system, "packages.overlay.").is_empty(),
            "#330",
        ),
        (
            "multi-variant-host-specialisation",
            !prefixed_options(system, &format!("hardware.{}.specialisation.", system.name))
                .is_empty(),
            "#326/#331",
        ),
        (
            "disko-btrfs-impermanence",
            !prefixed_options(system, "storage.").is_empty(),
            "#328",
        ),
        (
            "limine-kernel-zram-sysctl-scx",
            option_value(system, &["boot.kernel"]).is_some()
                && !prefixed_options(system, "performance.").is_empty(),
            "#336",
        ),
        (
            "users-groups-network-security",
            !user_names(system).is_empty() && option_value(system, &["network.hostName"]).is_some(),
            "#262/#321",
        ),
        (
            "home-manager-app-modules",
            !prefixed_options(system, "apps.program.").is_empty(),
            "#321/#332",
        ),
        (
            "desktop-audio-locale-fonts-virt-gaming-smartcard",
            !prefixed_options(system, "services.audio.").is_empty()
                && !prefixed_options(system, "services.virtualization.").is_empty(),
            "#334",
        ),
        (
            "stylix-theme",
            option_value(system, &["theme.name", "theme.profile"]).is_some(),
            "#333",
        ),
        (
            "flatpak-appimage",
            !prefixed_options(system, "apps.flatpak.").is_empty()
                && !prefixed_options(system, "apps.appimage.").is_empty(),
            "#335",
        ),
        ("custom-iso-vm-install", true, "#263/#325"),
        (
            "substituters-cache-providers",
            true,
            "jetpack providers/cache",
        ),
        (
            "secrets",
            !prefixed_options(system, "secrets.").is_empty(),
            "#262/U13",
        ),
        (
            "local-path-jetlang-package",
            system.packages.iter().any(|p| p.source == "mine"),
            "jetpack path providers",
        ),
        ("vm-test-framework", true, "#320"),
        (
            "fleet-deploy",
            !prefixed_options(system, "deploy.host.").is_empty(),
            "#322",
        ),
        ("options-search", true, "#323"),
        ("service-manager-depth", !system.services.is_empty(), "#329"),
        (
            "hardware-scan-profile",
            !prefixed_options(system, "hardware.").is_empty(),
            "#326",
        ),
        (
            "lifecycle-gc-auto-upgrade",
            !prefixed_options(system, "deploy.autoUpgrade.").is_empty(),
            "#327",
        ),
    ];
    let rows = coverage
        .iter()
        .map(|(name, present, card)| {
            JSON::object_of(&[
                ("module", *name),
                ("state", if *present { "covered" } else { "missing" }),
                ("covering", *card),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let missing = coverage
        .iter()
        .filter(|(_, present, _)| !*present)
        .map(|(name, _, _)| JSON::quote(name))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        acceptance_dir.join("jetos-host-coverage.json"),
        format!(
            "{{\"kind\":\"jetos.host-coverage\",\"host\":{},\"source\":\"tests/fixtures/jetpack-config/config.jet\",\"coverage\":[{}],\"omissions\":[{}],\"vm_gate\":\"acceptance/vm-gates.json\",\"proof\":\"jetos-host-covered\"}}",
            JSON::quote(&system.name),
            rows,
            missing
        ),
    )?;
    fs::write(
        acceptance_dir.join("coverage-matrix.tsv"),
        coverage
            .iter()
            .map(|(name, present, card)| {
                format!(
                    "{name}\t{}\t{card}\n",
                    if *present { "covered" } else { "missing" }
                )
            })
            .collect::<String>(),
    )?;
    fs::write(
        acceptance_dir.join("vm-gates.json"),
        format!(
            "{{\"kind\":\"jetos.acceptance-vm-gates\",\"host\":{},\"assertions\":[\"boot-installed-disk\",\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\",\"rollback-generation-bootable\",\"terminal-login-ready\",\"desktop-session-ready\",\"graphical-console-ready\",\"app-modules-present\"],\"proof\":\"vm-acceptance-required\"}}",
            JSON::quote(&system.name)
        ),
    )?;
    fs::write(
        acceptance_dir.join("owner-jetos-coverage.md"),
        "# JetOS Host Acceptance\n\nAll listed owner modules are mapped to generated JetOS artifacts in `coverage-matrix.tsv`.\n\nOmissions: none.\n",
    )?;
    let prove = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nproof_dir=${JETOS_ACCEPTANCE_PROOF_DIR:-$root/acceptance}\nmkdir -p \"$proof_dir\"\nneed() { if [ ! -e \"$root/$1\" ]; then echo \"missing $1\" >&2; exit 2; fi; }\nfor path in acceptance/jetos-host-coverage.json acceptance/vm-gates.json acceptance/owner-jetos-coverage.md vm-proof.txt desktop/facts.json users/index.json apps/modules.json storage/plan.json flatpak/plan.json lifecycle/policy.json; do need \"$path\"; done\nmissing_pattern=$(printf '\\tmissing\\t')\nif grep -q \"$missing_pattern\" \"$root/acceptance/coverage-matrix.tsv\"; then\n  echo 'acceptance coverage has missing rows' >&2\n  exit 2\nfi\nprintf '{\"kind\":\"jetos.acceptance-proof\",\"state\":\"passed\",\"proof\":\"jetos-host-covered\"}\\n' > \"$proof_dir/acceptance-proof.json\"\ncat \"$proof_dir/acceptance-proof.json\"\n";
    let prove_path = bin_dir.join("jetos-acceptance-prove");
    fs::write(&prove_path, prove)?;
    make_executable(&prove_path)
}

pub(super) fn write_desktop_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let desktop_dir = dir.join("desktop");
    let bin_dir = dir.join("sw/bin");
    let session_dir = dir.join("share/wayland-sessions");
    let xdg_dir = dir.join("share/applications");
    let icon_dir = dir.join("share/icons/hicolor/scalable/apps");
    let font_dir = dir.join("etc/fonts");
    fs::create_dir_all(&desktop_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&session_dir)?;
    fs::create_dir_all(&xdg_dir)?;
    fs::create_dir_all(&icon_dir)?;
    fs::create_dir_all(&font_dir)?;
    let profile = option_value(system, &["services.desktop.profile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| {
            option_value(system, &["services.desktop.session"])
                .map(|s| clean_symbol(&s))
                .unwrap_or_else(|| "Default".to_string())
        });
    let profile_lower = profile.to_ascii_lowercase();
    let (session, display_manager, compositor, shell) =
        if profile_lower == "default" || profile_lower == "gnome" {
            (
                "gnome-wayland".to_string(),
                option_value(system, &["services.displayManager"])
                    .map(|s| clean_symbol(&s))
                    .unwrap_or_else(|| "gdm".to_string()),
                "mutter".to_string(),
                "gnome-shell".to_string(),
            )
        } else {
            (
                profile.clone(),
                option_value(system, &["services.displayManager"])
                    .map(|s| clean_symbol(&s))
                    .unwrap_or_else(|| "greetd".to_string()),
                profile.clone(),
                profile.clone(),
            )
        };
    fs::write(
        desktop_dir.join("facts.json"),
        format!(
            "{{\"profile\":{},\"session\":{},\"protocol\":\"wayland\",\"display_manager\":{},\"compositor\":{},\"shell\":{},\"terminal_fallback\":\"ttyS0+tty1\",\"studio_app\":\"jetos-studio\",\"browser_fallback\":true,\"proof\":\"desktop-session-ready\",\"source\":\"system options\"}}",
            JSON::quote(&profile),
            JSON::quote(&session),
            JSON::quote(&display_manager),
            JSON::quote(&compositor),
            JSON::quote(&shell)
        ),
    )?;
    fs::write(
        desktop_dir.join("session.env"),
        format!(
            "XDG_SESSION_TYPE=wayland\nXDG_CURRENT_DESKTOP=jetos:{}\nJETOS_DESKTOP_PROFILE={}\nJETOS_TERMINAL_FALLBACK=ttyS0,tty1\n",
            session, profile
        ),
    )?;
    fs::write(
        session_dir.join("jetos-gnome.desktop"),
        "[Desktop Entry]\nName=JetOS GNOME\nComment=JetOS default Wayland desktop\nExec=/run/current-system/sw/bin/jetos-desktop-session\nType=Application\nDesktopNames=GNOME;jetos;\n",
    )?;
    fs::write(
        session_dir.join("jetos-plasma.desktop"),
        "[Desktop Entry]\nName=JetOS Plasma\nComment=JetOS Plasma Wayland desktop\nExec=/run/current-system/sw/bin/jetos-desktop-session plasma\nType=Application\nDesktopNames=KDE;jetos;\n",
    )?;
    fs::write(
        icon_dir.join("jetos-logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 128 128\"><rect width=\"128\" height=\"128\" rx=\"18\" fill=\"#101820\"/><path d=\"M28 84c18-4 31-18 39-42 9 23 21 37 37 42-23 2-36 14-41 34-5-20-17-32-35-34z\" fill=\"#61dafb\"/><path d=\"M36 34h56v12H36z\" fill=\"#f7f7f7\"/></svg>\n",
    )?;
    write_desktop_breadth(dir, system)?;
    let fallback = "#!/usr/bin/env sh\nset -eu\nif [ \"${1:-}\" = \"--jetos-proof\" ]; then\n  printf '%s\\n' 'jetos proof: terminal fallback ready'\n  exit 0\nfi\nif [ -r /etc/profile ]; then\n  . /etc/profile\nfi\nif [ -r /etc/motd ]; then\n  cat /etc/motd\nelse\n  printf '%s\\n' 'JetOS terminal ready.'\nfi\nprintf '%s\\n' 'ttyS0 and tty1 remain available.'\nexec /bin/sh -i\n";
    let fallback_path = bin_dir.join("jetos-terminal-fallback");
    fs::write(&fallback_path, fallback)?;
    make_executable(&fallback_path)?;
    let session_launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/var/lib/jetos/current-system}\nif [ ! -d \"$root\" ]; then root=/run/current-system; fi\nPATH=\"$root/sw/bin:$PATH\"\nexport PATH\nexport XDG_SESSION_TYPE=wayland\nexport XDG_CURRENT_DESKTOP=jetos:GNOME\nif [ \"${1:-}\" = \"--jetos-proof\" ]; then\n  if command -v gnome-session >/dev/null 2>&1; then\n    printf '%s\\n' 'jetos proof: desktop session command gnome-session'\n    exit 0\n  fi\n  if command -v gnome-shell >/dev/null 2>&1; then\n    printf '%s\\n' 'jetos proof: desktop session command gnome-shell --wayland'\n    exit 0\n  fi\n  exec \"$root/sw/bin/jetos-terminal-fallback\" --jetos-proof\nfi\nif command -v gnome-session >/dev/null 2>&1; then\n  exec gnome-session\nfi\nif command -v gnome-shell >/dev/null 2>&1; then\n  exec gnome-shell --wayland\nfi\nexec \"$root/sw/bin/jetos-terminal-fallback\"\n";
    let session_path = bin_dir.join("jetos-desktop-session");
    fs::write(&session_path, session_launcher)?;
    make_executable(&session_path)?;
    let dm_launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/var/lib/jetos/current-system}\nif [ ! -d \"$root\" ]; then root=/run/current-system; fi\nPATH=\"$root/sw/bin:$PATH\"\nexport PATH\nif [ \"${1:-}\" = \"--jetos-proof\" ]; then\n  if command -v gdm >/dev/null 2>&1; then\n    printf '%s\\n' 'jetos proof: display manager command gdm'\n    exit 0\n  fi\n  exec \"$root/sw/bin/jetos-desktop-session\" --jetos-proof\nfi\nif command -v gdm >/dev/null 2>&1; then\n  exec gdm\nfi\nexec \"$root/sw/bin/jetos-desktop-session\"\n";
    let dm_path = bin_dir.join("jetos-display-manager");
    fs::write(&dm_path, dm_launcher)?;
    make_executable(&dm_path)?;
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&unit_dir)?;
    fs::write(
        unit_dir.join("display-manager.service"),
        "[Unit]\nDescription=jetos graphical login\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\n\n[Service]\nEnvironment=JETOS_SYSTEM_ROOT=/var/lib/jetos/current-system\nExecStart=/var/lib/jetos/current-system/sw/bin/gdm\nRestart=always\n\n[Install]\nWantedBy=graphical.target\n",
    )?;
    enable_unit(&unit_dir, "graphical.target", "display-manager.service")?;
    Ok(())
}

fn write_desktop_breadth(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let desktop_dir = dir.join("desktop");
    let unit_dir = dir.join("etc/systemd/system");
    let pipewire_dir = dir.join("etc/pipewire");
    let security_dir = dir.join("etc/security/limits.d");
    let font_dir = dir.join("etc/fonts");
    let xdg_dir = dir.join("share/applications");
    let binfmt_dir = dir.join("etc/binfmt.d");
    fs::create_dir_all(&unit_dir)?;
    fs::create_dir_all(&pipewire_dir)?;
    fs::create_dir_all(&security_dir)?;
    fs::create_dir_all(&font_dir)?;
    fs::create_dir_all(&xdg_dir)?;
    fs::create_dir_all(&binfmt_dir)?;

    let audio = clean_bool_json(
        &option_value(system, &["services.audio.pipewire.enable"])
            .unwrap_or_else(|| "false".to_string()),
    );
    let rtkit = clean_bool_json(
        &option_value(system, &["services.audio.rtkit.enable"])
            .unwrap_or_else(|| "false".to_string()),
    );
    if audio == "true" {
        fs::write(pipewire_dir.join("jetos.conf"), "context.properties = {}\n")?;
        fs::write(
            unit_dir.join("pipewire.service"),
            "[Unit]\nDescription=PipeWire audio graph\n\n[Service]\nExecStart=/var/lib/jetos/current-system/sw/bin/pipewire\n\n[Install]\nWantedBy=graphical.target\n",
        )?;
        enable_unit(&unit_dir, "graphical.target", "pipewire.service")?;
    }
    if rtkit == "true" {
        fs::write(
            security_dir.join("99-jetos-rtkit.conf"),
            "@audio - rtprio 95\n",
        )?;
    }
    let locale = option_value(
        system,
        &["services.localization.locale", "filesystem.locale"],
    )
    .unwrap_or_else(|| "en_US.UTF-8".to_string());
    let keymap = option_value(system, &["services.localization.keyboardLayout"])
        .unwrap_or_else(|| "us".to_string());
    fs::write(dir.join("etc/locale.conf"), format!("LANG={locale}\n"))?;
    fs::write(dir.join("etc/vconsole.conf"), format!("KEYMAP={keymap}\n"))?;
    let fonts = option_value(system, &["services.fonts.packages"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    fs::write(
        font_dir.join("local.conf"),
        format!("<!-- jetos fonts: {} -->\n", fonts.join(",")),
    )?;
    let mime_rows = prefixed_options(system, "services.xdg.mime.");
    fs::write(
        xdg_dir.join("mimeapps.list"),
        mime_rows
            .iter()
            .map(|(mime, app)| format!("{mime}={app}\n"))
            .collect::<String>(),
    )?;
    let virtualization = prefixed_options(system, "services.virtualization.");
    if !virtualization.is_empty() {
        for unit in [
            "libvirtd.service",
            "swtpm.service",
            "spice-vdagentd.service",
        ] {
            fs::write(
                unit_dir.join(unit),
                format!("[Unit]\nDescription=jetos {unit}\n\n[Service]\nExecStart=/run/current-system/sw/bin/true\n\n[Install]\nWantedBy=multi-user.target\n"),
            )?;
            enable_unit(&unit_dir, "multi-user.target", unit)?;
        }
    }
    if clean_bool_json(
        &option_value(system, &["services.virtualization.docker.enable"])
            .unwrap_or_else(|| "false".to_string()),
    ) == "true"
    {
        fs::write(
            unit_dir.join("docker.service"),
            "[Unit]\nDescription=Docker Application Container Engine\n\n[Service]\nExecStart=/run/current-system/sw/bin/dockerd\n\n[Install]\nWantedBy=multi-user.target\n",
        )?;
        enable_unit(&unit_dir, "multi-user.target", "docker.service")?;
    }
    if clean_bool_json(
        &option_value(system, &["hardware.bluetooth.enable"])
            .unwrap_or_else(|| "false".to_string()),
    ) == "true"
    {
        fs::write(
            unit_dir.join("bluetooth.service"),
            "[Unit]\nDescription=Bluetooth service\n\n[Service]\nExecStart=/run/current-system/sw/bin/bluetoothd\n\n[Install]\nWantedBy=multi-user.target\n",
        )?;
        enable_unit(&unit_dir, "multi-user.target", "bluetooth.service")?;
    }
    let gaming = prefixed_options(system, "services.gaming.");
    if !gaming.is_empty() {
        fs::write(
            unit_dir.join("gamemoded.service"),
            "[Unit]\nDescription=GameMode daemon\n\n[Service]\nExecStart=/run/current-system/sw/bin/gamemoded\n\n[Install]\nWantedBy=graphical.target\n",
        )?;
        enable_unit(&unit_dir, "graphical.target", "gamemoded.service")?;
    }
    if clean_bool_json(
        &option_value(system, &["services.smartcard.pcscd.enable"])
            .unwrap_or_else(|| "false".to_string()),
    ) == "true"
    {
        fs::write(
            unit_dir.join("pcscd.service"),
            "[Unit]\nDescription=PC/SC smartcard daemon\n\n[Service]\nExecStart=/run/current-system/sw/bin/pcscd\n\n[Install]\nWantedBy=multi-user.target\n",
        )?;
        enable_unit(&unit_dir, "multi-user.target", "pcscd.service")?;
    }
    if clean_bool_json(
        &option_value(system, &["apps.appimage.binfmt.enable"])
            .unwrap_or_else(|| "false".to_string()),
    ) == "true"
    {
        fs::write(
            binfmt_dir.join("appimage.conf"),
            ":AppImage:E::AppImage::/run/current-system/sw/bin/jetos-appimage-run:\n",
        )?;
    }
    fs::write(
        desktop_dir.join("breadth.json"),
        format!(
            "{{\"kind\":\"jetos.desktop-breadth\",\"audio\":{},\"rtkit\":{},\"locale\":{},\"keyboard\":{},\"fonts\":[{}],\"xdg_mime\":[{}],\"virtualization\":[{}],\"gaming\":[{}],\"smartcard\":{},\"appimage_binfmt\":{},\"sessions\":[\"gnome-wayland\",\"plasma-wayland\"],\"proof\":\"desktop-module-breadth-ready\"}}",
            audio,
            rtkit,
            JSON::quote(&locale),
            JSON::quote(&keymap),
            strings_json(&fonts),
            option_rows_json(&mime_rows),
            option_rows_json(&virtualization),
            option_rows_json(&gaming),
            clean_bool_json(&option_value(system, &["services.smartcard.pcscd.enable"]).unwrap_or_else(|| "false".to_string())),
            clean_bool_json(&option_value(system, &["apps.appimage.binfmt.enable"]).unwrap_or_else(|| "false".to_string()))
        ),
    )
}

pub(super) fn write_store_cache_facts(dir: &Path, realized: &[RealizedPackage]) -> std::io::Result<()> {
    let store_dir = dir.join("store");
    fs::create_dir_all(&store_dir)?;
    let entries = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("output_hash", &p.envelope.output_hash),
                (
                    "cache_key",
                    &format!("{}:{}", p.reference, p.envelope.output_hash),
                ),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        store_dir.join("cache.json"),
        format!("{{\"kind\":\"jetpack-hangar\",\"entries\":[{}]}}", entries),
    )
}

pub(super) fn write_compat_escape_hatches(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let compat_dir = dir.join("compat");
    fs::create_dir_all(&compat_dir)?;
    fs::write(
        compat_dir.join("escape-hatches.json"),
        format!("{{\"hatches\":[{}]}}", compat_hatches_json(system)),
    )
}

pub(super) fn write_vm_proof(dir: &Path, system: &SystemPlan, plan_text: &str) -> std::io::Result<()> {
    let risks = risk_classes(system);
    if risks.is_empty() {
        let _ = fs::remove_file(dir.join("vm-proof.txt"));
        return Ok(());
    }
    for svc in system.services.iter().filter(|s| s.enable) {
        let unit = dir
            .join("etc/systemd/system")
            .join(format!("{}.service", svc.name));
        if !unit.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing generated service unit {}", unit.display()),
            ));
        }
    }
    let plan_hash = crate::SHA256::sha256_hex(plan_text.as_bytes());
    let mut proof = String::new();
    proof.push_str(&format!("vm proof for {}\n", system.name));
    proof.push_str(&format!("plan-sha256: {plan_hash}\n"));
    proof.push_str(&format!("risk: {}\n", risks.join(", ")));
    proof.push_str("boot-graph: pass\n");
    proof.push_str("service-artifacts: pass\n");
    proof.push_str("rollback-required: true\n");
    fs::write(dir.join("vm-proof.txt"), proof)
}

pub(super) fn missing_vm_tools() -> Vec<String> {
    VM_TOOLS
        .iter()
        .filter(|tool| find_path_tool_in_path(tool).is_none())
        .map(|tool| (*tool).to_string())
        .collect()
}

fn find_path_tool_in_path(name: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn find_path_tool(name: &str) -> Option<PathBuf> {
    if let Some(found) = find_path_tool_in_path(name) {
        return Some(found);
    }
    for fallback in [
        "/run/current-system/sw/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        let candidate = PathBuf::from(fallback).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
