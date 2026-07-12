use super::etc_boot_facts::{boot_artifact, kernel_package_json};
use super::identity::jetos_release_label;
use super::options_rendering::{
    collect_names, option_rows_json, option_value, parse_list_items, prefixed_options,
    safe_filename, safe_identifier, service_extra, shell_single_quote,
};
use super::root_projection::{copy_file_replace, enable_unit};
use super::store_realize::RealizedPackage;
use super::studio_projection::make_executable;
use super::types::SYSTEMD_INIT_PACKAGE;
use crate::ModuleEval::SystemPlan;
use crate::JSON;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn write_init_facts(
    dir: &Path,
    system: &SystemPlan,
    realized: &[RealizedPackage],
) -> std::io::Result<()> {
    let init_dir = dir.join("init");
    fs::create_dir_all(&init_dir)?;
    let default_target = option_value(system, &["init.defaultTarget"])
        .unwrap_or_else(|| "multi-user.target".to_string());
    let init_entry = realized
        .iter()
        .find(|entry| entry.name == SYSTEMD_INIT_PACKAGE);
    if let Some(entry) = init_entry {
        let sbin = dir.join("sbin");
        fs::create_dir_all(&sbin)?;
        let init_path = boot_artifact(entry, &["bin/systemd", "lib/systemd/systemd", "sbin/init"])
            .unwrap_or_else(|| {
                entry
                    .consumption_path(&entry.out)
                    .unwrap_or_else(|_| PathBuf::from(&entry.out))
                    .join("bin/systemd")
            });
        copy_file_replace(&init_path, &sbin.join("init"))?;
        write_systemd_unit_library(dir, entry, &default_target)?;
    }
    let init_package = init_entry
        .map(kernel_package_json)
        .unwrap_or_else(|| "null".to_string());
    fs::write(
        init_dir.join("systemd.json"),
        format!(
            "{{\"init\":\"systemd\",\"default_target\":{},\"unit_dir\":\"etc/systemd/system\",\"init_package\":{}}}",
            JSON::quote(&default_target),
            init_package
        ),
    )
}

fn write_systemd_unit_library(
    dir: &Path,
    entry: &RealizedPackage,
    default_target: &str,
) -> std::io::Result<()> {
    let unit_roots = [
        dir.join("etc/systemd/system"),
        dir.join("usr/lib/systemd/system"),
        dir.join("lib/systemd/system"),
        dir.join("systemd/lib/systemd/system"),
    ];
    for root in &unit_roots {
        write_minimal_systemd_units(root)?;
    }
    if let Some(systemd_bin) = boot_artifact(entry, &["lib/systemd/systemd", "bin/systemd"]) {
        copy_file_replace(&systemd_bin, &dir.join("systemd/lib/systemd/systemd"))?;
    }

    let etc_units = dir.join("etc/systemd/system");
    link_or_copy_unit(
        &etc_units.join(default_target),
        &etc_units.join("default.target"),
    )
}

fn write_minimal_systemd_units(unit_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(unit_dir)?;
    for (name, body) in [
        (
            "graphical.target",
            "[Unit]\nDescription=JetOS graphical interface\nRequires=multi-user.target\nWants=display-manager.service\nAfter=multi-user.target display-manager.service\nAllowIsolate=yes\n",
        ),
        (
            "multi-user.target",
            "[Unit]\nDescription=JetOS multi-user system\nRequires=basic.target\nAfter=basic.target\nAllowIsolate=yes\n",
        ),
        (
            "basic.target",
            "[Unit]\nDescription=JetOS basic system\nRequires=sysinit.target\nWants=sockets.target timers.target paths.target slices.target\nAfter=sysinit.target sockets.target paths.target slices.target\n",
        ),
        (
            "sysinit.target",
            "[Unit]\nDescription=JetOS system initialization\nWants=local-fs.target swap.target\nAfter=local-fs.target swap.target\n",
        ),
        (
            "local-fs.target",
            "[Unit]\nDescription=JetOS local filesystems\nDefaultDependencies=no\n",
        ),
        ("swap.target", "[Unit]\nDescription=JetOS swap\n"),
        ("sockets.target", "[Unit]\nDescription=JetOS sockets\n"),
        ("timers.target", "[Unit]\nDescription=JetOS timers\n"),
        ("paths.target", "[Unit]\nDescription=JetOS paths\n"),
        ("slices.target", "[Unit]\nDescription=JetOS slices\n"),
        ("getty.target", "[Unit]\nDescription=JetOS login prompts\n"),
        (
            "rescue.target",
            "[Unit]\nDescription=JetOS rescue mode\nRequires=sysinit.target\nAfter=sysinit.target\nAllowIsolate=yes\n",
        ),
        (
            "emergency.target",
            "[Unit]\nDescription=JetOS emergency mode\nAllowIsolate=yes\n",
        ),
    ] {
        fs::write(unit_dir.join(name), body)?;
    }
    Ok(())
}

#[cfg(unix)]
fn link_or_copy_unit(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    match std::os::unix::fs::symlink(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dst).map(|_| ()),
    }
}

#[cfg(not(unix))]
fn link_or_copy_unit(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::copy(src, dst).map(|_| ())
}

pub(super) fn write_secret_manifest(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let mut manifest = String::new();
    manifest.push_str("repo ciphertext + host key; activation decrypts into tmpfs only\n");
    for name in collect_names(system, "secrets") {
        let source = option_value(system, &[&format!("secrets.{name}.source")])
            .unwrap_or_else(|| format!("secrets/{name}.age"));
        manifest.push_str(&format!("{name}\t{source}\t/run/jetos-secrets/{name}\n"));
    }
    fs::write(dir.join("secrets.tmpfs.manifest"), manifest)
}

pub(super) fn write_network_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let network_dir = dir.join("etc/systemd/network");
    fs::create_dir_all(&network_dir)?;
    let interface =
        option_value(system, &["network.interface"]).unwrap_or_else(|| "eth0".to_string());
    let address = option_value(system, &["network.address"]);
    let gateway = option_value(system, &["network.gateway"]);
    let dns = option_value(system, &["network.dns"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let wireless_ssid = option_value(system, &["network.wireless.ssid"]);
    let firewall_ports = option_value(system, &["network.firewall.allowedTcpPorts"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let mut network = String::new();
    network.push_str("[Match]\n");
    network.push_str(&format!("Name={interface}\n\n[Network]\n"));
    match address {
        Some(a) => network.push_str(&format!("Address={a}\n")),
        None => network.push_str("DHCP=yes\n"),
    }
    if let Some(g) = gateway {
        network.push_str(&format!("Gateway={g}\n"));
    }
    for d in &dns {
        network.push_str(&format!("DNS={d}\n"));
    }
    fs::write(network_dir.join("10-jetos.network"), network)?;

    let nft_dir = dir.join("etc/nftables");
    fs::create_dir_all(&nft_dir)?;
    let ports = if firewall_ports.is_empty() {
        "drop".to_string()
    } else {
        format!("tcp dport {{ {} }} accept", firewall_ports.join(", "))
    };
    fs::write(
        nft_dir.join("jetos-firewall.nft"),
        format!("table inet jetos {{\n  chain input {{\n    type filter hook input priority 0;\n    {ports}\n  }}\n}}\n"),
    )?;

    let dns_json = dns
        .iter()
        .map(|d| JSON::quote(d))
        .collect::<Vec<_>>()
        .join(",");
    let ports_json = firewall_ports
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let wireless = wireless_ssid
        .map(|ssid| JSON::object_of(&[("ssid", &ssid), ("secret", "/run/jetos-secrets/wifi")]))
        .unwrap_or_else(|| "null".to_string());
    let facts_dir = dir.join("network");
    fs::create_dir_all(&facts_dir)?;
    fs::write(
        facts_dir.join("facts.json"),
        format!(
            "{{\"backend\":\"systemd-networkd\",\"interface\":{},\"dns\":[{}],\"firewall_allowed_tcp_ports\":[{}],\"wireless\":{}}}",
            JSON::quote(&interface),
            dns_json,
            ports_json,
            wireless
        ),
    )
}

pub(super) fn write_systemd_timer_socket_units(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&unit_dir)?;
    for svc in system.services.iter().filter(|s| s.enable) {
        if let Some(calendar) = service_extra(svc, &["timer", "schedule"]) {
            fs::write(
                unit_dir.join(format!("{}.timer", svc.name)),
                format!(
                    "[Unit]\nDescription=jetos timer for {}\n\n[Timer]\nOnCalendar={}\nUnit={}.service\n\n[Install]\nWantedBy=timers.target\n",
                    svc.name, calendar, svc.name
                ),
            )?;
            enable_unit(&unit_dir, "timers.target", &format!("{}.timer", svc.name))?;
        }
        if let Some(listen) = service_extra(svc, &["socket", "listen"]) {
            fs::write(
                unit_dir.join(format!("{}.socket", svc.name)),
                format!(
                    "[Unit]\nDescription=jetos socket for {}\n\n[Socket]\nListenStream={}\n\n[Install]\nWantedBy=sockets.target\n",
                    svc.name, listen
                ),
            )?;
            enable_unit(&unit_dir, "sockets.target", &format!("{}.socket", svc.name))?;
        }
    }
    Ok(())
}

pub(super) fn write_hardware_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let hw_dir = dir.join("hardware");
    let bin_dir = dir.join("sw/bin");
    let boot_spec_dir = dir.join("boot/specialisations");
    fs::create_dir_all(&hw_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&boot_spec_dir)?;
    let firmware = option_value(system, &["kernel.firmware"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let drivers = option_value(system, &["kernel.drivers"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let firmware_json = firmware
        .iter()
        .map(|f| JSON::quote(f))
        .collect::<Vec<_>>()
        .join(",");
    let drivers_json = drivers
        .iter()
        .map(|d| JSON::quote(d))
        .collect::<Vec<_>>()
        .join(",");
    let hardware_options = prefixed_options(system, "hardware.");
    let profile_keys = [
        format!("hardware.{}.profile", system.name),
        "hardware.profile".to_string(),
    ];
    let profile_keys = profile_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let profiles = option_value(system, &profile_keys)
        .map(|v| parse_list_items(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["generic-pc".to_string()]);
    let profiles_json = profiles
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let specialisations =
        prefixed_options(system, &format!("hardware.{}.specialisation.", system.name));
    let specialisations_json = specialisations
        .iter()
        .map(|(name, enabled)| {
            JSON::object_of(&[
                ("name", name),
                ("enabled", enabled),
                ("boot_entry", &format!("boot/specialisations/{name}.conf")),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        hw_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.hardware\",\"host\":{},\"firmware\":[{}],\"drivers\":[{}],\"profiles\":[{}],\"specialisations\":[{}],\"scan_source\":\"hardware/{}.jet\",\"doctor\":\"sw/bin/jetos-hardware-doctor\",\"audit\":\"declared hardware facts enter generation proof before activation\"}}",
            JSON::quote(&system.name),
            firmware_json,
            drivers_json,
            profiles_json,
            specialisations_json,
            system.name
        ),
    )?;
    fs::write(
        hw_dir.join("firmware.manifest"),
        firmware
            .iter()
            .map(|f| format!("{f}\tdeclared\n"))
            .collect::<String>(),
    )?;
    fs::write(
        hw_dir.join("drivers.manifest"),
        drivers
            .iter()
            .map(|d| format!("{d}\tdeclared\n"))
            .collect::<String>(),
    )?;
    fs::write(
        hw_dir.join("profiles.manifest"),
        profiles
            .iter()
            .map(|p| format!("{p}\tdeclared\n"))
            .collect::<String>(),
    )?;
    for profile in &profiles {
        fs::write(
            hw_dir.join(format!("profile-{}.json", safe_filename(profile))),
            format!(
                "{{\"kind\":\"jetos.hardware-profile\",\"name\":{},\"source\":\"first-party hardware profile\",\"applies_to\":{},\"drivers\":[{}],\"firmware\":[{}],\"proof\":\"profile-applied\"}}",
                JSON::quote(profile),
                JSON::quote(&system.name),
                drivers_json,
                firmware_json
            ),
        )?;
    }
    let mut source = format!(
        "// generated by jetos-hardware-scan; canonical source may be copied into config.jet\nmodule hardware_{} {{\n",
        safe_identifier(&system.name)
    );
    source.push_str(&format!(
        "    hardware.{}.scan.generated: true\n",
        system.name
    ));
    for driver in &drivers {
        source.push_str(&format!(
            "    hardware.{}.driver.{}: true\n",
            system.name,
            safe_identifier(driver)
        ));
    }
    for firmware in &firmware {
        source.push_str(&format!(
            "    hardware.{}.firmware.{}: true\n",
            system.name,
            safe_identifier(firmware)
        ));
    }
    for profile in &profiles {
        source.push_str(&format!(
            "    hardware.{}.profile: \"{}\"\n",
            system.name, profile
        ));
    }
    source.push_str("}\n");
    fs::write(hw_dir.join(format!("{}.jet", system.name)), source)?;
    fs::write(
        hw_dir.join("declared-options.json"),
        format!(
            "{{\"kind\":\"jetos.hardware-options\",\"options\":[{}]}}",
            option_rows_json(&hardware_options)
        ),
    )?;
    fs::write(
        hw_dir.join("specialisations.json"),
        format!(
            "{{\"kind\":\"jetos.hardware-specialisations\",\"host\":{},\"items\":[{}],\"proof\":\"boot-selectable-variants\"}}",
            JSON::quote(&system.name),
            specialisations_json
        ),
    )?;
    for (name, enabled) in &specialisations {
        fs::write(
            boot_spec_dir.join(format!("{name}.conf")),
            format!(
                "title {} — {} ({name})\nhost {}\nenabled {}\ngeneration /run/current-system\nproof hardware-specialisation\n",
                jetos_release_label(false), system.name, system.name, enabled
            ),
        )?;
    }
    let host = shell_single_quote(&system.name);
    let scanner = format!(
        "#!/usr/bin/env sh\nset -eu\nhost=${{1:-{host}}}\nscan_root=${{JETOS_HW_ROOT:-/}}\nout=${{JETOS_HARDWARE_OUT:-$PWD/hardware-$host.jet}}\nmkdir -p \"$(dirname \"$out\")\"\nmods=''\nif [ -r \"$scan_root/proc/modules\" ]; then\n  mods=$(awk '{{print $1}}' \"$scan_root/proc/modules\" | sort -u | paste -sd, -)\nfi\nblocks=''\nif [ -d \"$scan_root/sys/class/block\" ]; then\n  blocks=$(find \"$scan_root/sys/class/block\" -mindepth 1 -maxdepth 1 -printf '%f\\n' | sort | paste -sd, -)\nfi\ngpus=''\nif [ -d \"$scan_root/sys/class/drm\" ]; then\n  gpus=$(find \"$scan_root/sys/class/drm\" -mindepth 1 -maxdepth 1 -name 'card*' -printf '%f\\n' | sort | paste -sd, -)\nfi\n{{\n  printf '%s\\n' \"// generated by jetos-hardware-scan\"\n  printf '%s\\n' \"module hardware_$host {{\"\n  printf '    hardware.%s.scan.modules: \"%s\"\\n' \"$host\" \"$mods\"\n  printf '    hardware.%s.scan.blockDevices: \"%s\"\\n' \"$host\" \"$blocks\"\n  printf '    hardware.%s.scan.gpus: \"%s\"\\n' \"$host\" \"$gpus\"\n  printf '%s\\n' \"}}\"\n}} > \"$out\"\nprintf '{{\"kind\":\"jetos.hardware-scan\",\"host\":\"%s\",\"out\":\"%s\",\"modules\":\"%s\",\"block_devices\":\"%s\",\"gpus\":\"%s\"}}\\n' \"$host\" \"$out\" \"$mods\" \"$blocks\" \"$gpus\"\n"
    );
    let scanner_path = bin_dir.join("jetos-hardware-scan");
    fs::write(&scanner_path, scanner)?;
    make_executable(&scanner_path)?;
    let doctor = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nscan_root=${JETOS_HW_ROOT:-/}\nreport=${JETOS_HARDWARE_REPORT:-$root/hardware/drift-report.json}\nmkdir -p \"$(dirname \"$report\")\"\nmissing=''\nif [ -f \"$root/hardware/drivers.manifest\" ]; then\n  while IFS='	' read -r driver _state; do\n    [ -n \"$driver\" ] || continue\n    if [ -r \"$scan_root/proc/modules\" ]; then\n      if ! awk '{print $1}' \"$scan_root/proc/modules\" | grep -qx \"$driver\"; then\n        missing=\"$missing $driver\"\n      fi\n    fi\n  done < \"$root/hardware/drivers.manifest\"\nfi\nstate=match\nif [ -n \"$missing\" ]; then state=drift; fi\nprintf '{\"kind\":\"jetos.hardware-doctor\",\"state\":\"%s\",\"missing_drivers\":\"%s\",\"proof\":\"hardware-drift-checked\"}\\n' \"$state\" \"${missing# }\" > \"$report\"\ncat \"$report\"\n";
    let doctor_path = bin_dir.join("jetos-hardware-doctor");
    fs::write(&doctor_path, doctor)?;
    make_executable(&doctor_path)
}
