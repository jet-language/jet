use super::etc_boot_facts::{cachyos_kernel_entry, kernel_package_json};
use super::generations_activation::current_generation_path;
use super::options_rendering::{
    clean_value, collect_names, option_value, package_path_or_literal, service_extra,
    shell_single_quote,
};
use super::root_projection::enable_unit;
use super::store_realize::RealizedPackage;
use jet_env_model::ModuleEval::SystemPlan;
use crate::JSON;
use std::fs;
use std::path::Path;

pub(super) fn write_activation_diff(
    dir: &Path,
    published_dir: &Path,
    system: &SystemPlan,
    realized: &[RealizedPackage],
) -> std::io::Result<()> {
    let previous = current_generation_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let enabled_services = system.services.iter().filter(|s| s.enable).count();
    let mut diff = String::new();
    diff.push_str(&format!("host: {}\n", system.name));
    diff.push_str(&format!("previous: {previous}\n"));
    diff.push_str(&format!("next: {}\n", published_dir.display()));
    diff.push_str(&format!("packages: {}\n", realized.len()));
    diff.push_str(&format!("services: {enabled_services}\n"));
    diff.push_str(&format!("options: {}\n", system.options.len()));
    for pkg in realized {
        diff.push_str(&format!("  package {} -> {}\n", pkg.reference, pkg.out));
    }
    for svc in system.services.iter().filter(|s| s.enable) {
        diff.push_str(&format!("  service {} enable=true\n", svc.name));
    }
    for option in &system.options {
        diff.push_str(&format!("  option {} = {}\n", option.key, option.value));
    }
    fs::write(dir.join("activation-diff.txt"), diff)
}

pub(super) fn write_health_checks(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str("jetos health checks\n");
    for svc in system.services.iter().filter(|s| s.enable) {
        let check = service_extra(svc, &["health", "ready"])
            .unwrap_or_else(|| format!("systemctl is-active {}", svc.name));
        out.push_str(&format!("service {}: {}\n", svc.name, check));
    }
    let host = option_value(system, &["network.hostName", "network.hostname"])
        .unwrap_or_else(|| system.name.clone());
    out.push_str(&format!("network hostname: {host}\n"));
    for name in collect_names(system, "health") {
        if let Some(command) = option_value(system, &[&format!("health.{name}.command")]) {
            out.push_str(&format!("check {name}: {command}\n"));
        }
    }
    fs::write(dir.join("health-checks.txt"), out)
}

pub(super) fn write_provenance(
    dir: &Path,
    system: &SystemPlan,
    realized: &[RealizedPackage],
) -> std::io::Result<()> {
    let packages = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("out", &p.out),
                ("bin", &p.bin),
                ("output_hash", &p.envelope.output_hash),
                ("provenance", &p.envelope.provenance),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let kernel = cachyos_kernel_entry(realized)
        .map(kernel_package_json)
        .unwrap_or_else(|| "null".to_string());
    let text = format!(
        "{{\"host\":{},\"target\":{},\"kernel\":{},\"packages\":[{}],\"compat_escape_hatches\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        kernel,
        packages,
        compat_hatches_json(system)
    );
    fs::write(dir.join("provenance.json"), text)
}

pub(super) fn compat_hatches_json(system: &SystemPlan) -> String {
    let mut hatches = Vec::new();
    for option in &system.options {
        if option.key.starts_with("packages.overlay.")
            || option.key.starts_with("packages.specialArgs.")
            || option.key.starts_with("packages.nixModule.")
        {
            hatches.push(JSON::object_of(&[
                ("key", &option.key),
                ("value", &clean_value(&option.value)),
                ("audit", "compatibility escape hatch"),
                ("provenance_visible", "true"),
                ("studio_visible", "true"),
                ("native_replacement", "tracked"),
            ]));
        }
    }
    hatches.join(",")
}

pub(super) fn write_systemd_units(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&unit_dir)?;
    for svc in &system.services {
        if !svc.enable {
            continue;
        }
        let exec = svc
            .extra
            .iter()
            .find(|(k, _)| k == "exec" || k == "command")
            .map(|(_, v)| v.trim_matches('"').to_string())
            .unwrap_or_else(|| "/usr/bin/env true".to_string());
        fs::write(
            unit_dir.join(format!("{}.service", svc.name)),
            format!(
                "[Unit]\nDescription=jetos service {}\n\n[Service]\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
                svc.name, exec
            ),
        )?;
        enable_unit(
            &unit_dir,
            "multi-user.target",
            &format!("{}.service", svc.name),
        )?;
    }
    Ok(())
}

pub(super) fn write_terminal_environment(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let terminal_dir = dir.join("terminal");
    let etc = dir.join("etc");
    let unit_dir = etc.join("systemd/system");
    fs::create_dir_all(&terminal_dir)?;
    fs::create_dir_all(&etc)?;
    fs::create_dir_all(&unit_dir)?;

    let users = collect_names(system, "users");
    let login_user = users.first().cloned().unwrap_or_else(|| "root".to_string());
    let shell = if login_user == "root" {
        "/bin/sh".to_string()
    } else {
        option_value(system, &[&format!("users.{login_user}.shell")])
            .map(|s| package_path_or_literal(&s))
            .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string())
    };

    let mut shells = String::from("/bin/sh\n/run/current-system/sw/bin/sh\n");
    if !shells.lines().any(|line| line == shell) {
        shells.push_str(&shell);
        shells.push('\n');
    }
    fs::write(etc.join("shells"), shells)?;
    let prompt_label = format!("JetOS {}", system.name);
    let prompt_bash = format!(
        "\\[\\033[1;36m\\]JetOS\\[\\033[0m\\] \\[\\033[2m\\]{}\\[\\033[0m\\] \\w \\$ ",
        system.name
    );
    let prompt_plain = format!("JetOS {} $ ", system.name);
    fs::write(
        etc.join("profile"),
        format!(
            "export PATH=/run/current-system/sw/bin:/bin:/sbin:/usr/bin:/usr/sbin\nexport JETOS_GENERATION=/run/current-system\nexport JETOS_HOST={}\nexport JETOS_BRAND=JetOS\nexport JETOS_PROMPT={}\nif [ -n \"${{BASH_VERSION:-}}\" ]; then\n    PS1={}\nelse\n    PS1={}\nfi\nexport PS1\n",
            shell_single_quote(&system.name),
            shell_single_quote(&prompt_label),
            shell_single_quote(&prompt_bash),
            shell_single_quote(&prompt_plain)
        ),
    )?;
    fs::write(
        etc.join("issue"),
        format!(
            "JetOS {}\nproof-backed system shell\n\\n \\l\n",
            system.name
        ),
    )?;
    fs::write(
        etc.join("motd"),
        format!(
            "JetOS {}\n/run/current-system is live\nsource-owned, proof-backed\n",
            system.name
        ),
    )?;
    fs::write(etc.join("securetty"), "ttyS0\ntty1\n")?;

    let home_root = dir.join("home");
    for user in &users {
        let home = option_value(system, &[&format!("users.{user}.home")])
            .unwrap_or_else(|| format!("/home/{user}"));
        if let Some(rel) = home.strip_prefix('/') {
            fs::create_dir_all(dir.join(rel))?;
            fs::write(
                dir.join(rel).join(".profile"),
                ". /etc/profile\ncd \"$HOME\" 2>/dev/null || true\n",
            )?;
        } else {
            fs::create_dir_all(home_root.join(user))?;
        }
    }

    fs::write(
        unit_dir.join("serial-getty@ttyS0.service"),
        format!(
            "[Unit]\nDescription=jetos serial login on %I\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\n\n[Service]\nExecStart=/run/current-system/sw/bin/agetty --autologin {} --noclear %I 115200 linux\nType=idle\nRestart=always\n\n[Install]\nWantedBy=getty.target\n",
            login_user
        ),
    )?;
    enable_unit(&unit_dir, "getty.target", "serial-getty@ttyS0.service")?;
    fs::write(
        unit_dir.join("getty@tty1.service"),
        format!(
            "[Unit]\nDescription=jetos virtual-console login on %I\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\n\n[Service]\nExecStart=/run/current-system/sw/bin/agetty --autologin {} --noclear %I linux\nType=idle\nRestart=always\n\n[Install]\nWantedBy=getty.target\n",
            login_user
        ),
    )?;
    enable_unit(&unit_dir, "getty.target", "getty@tty1.service")?;

    fs::write(
        terminal_dir.join("facts.json"),
        format!(
            "{{\"login_user\":{},\"shell\":{},\"serial_tty\":\"ttyS0\",\"virtual_tty\":\"tty1\",\"profile\":\"/etc/profile\",\"prompt\":{},\"motd\":\"/etc/motd\",\"unit_dir\":\"etc/systemd/system\",\"proof\":\"terminal-login-ready\"}}",
            JSON::quote(&login_user),
            JSON::quote(&shell),
            JSON::quote(&prompt_plain)
        ),
    )
}
