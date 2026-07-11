fn shell_single_quote(s: &str) -> String {
    let mut quoted = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn option_value(system: &SystemPlan, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| resolved_option_value(system, key))
}

fn resolved_option_value(system: &SystemPlan, key: &str) -> Option<String> {
    resolved_option(system, key).map(|r| r.value)
}

struct ResolvedOption {
    key: String,
    value: String,
    tier: String,
    priority: i64,
    contenders: Vec<OptionContender>,
}

struct OptionContender {
    value: String,
    tier: String,
    priority: i64,
    module_id: String,
    source_order: usize,
    winner: bool,
}

impl ResolvedOption {
    fn to_json(&self) -> String {
        let contenders = self
            .contenders
            .iter()
            .map(|c| {
                format!(
                    "{{\"value\":{},\"tier\":{},\"priority\":{},\"module_id\":{},\"source_order\":{},\"winner\":{}}}",
                    JSON::quote(&c.value),
                    JSON::quote(&c.tier),
                    c.priority,
                    JSON::quote(&c.module_id),
                    c.source_order,
                    if c.winner { "true" } else { "false" }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"key\":{},\"value\":{},\"tier\":{},\"priority\":{},\"contenders\":[{}]}}",
            JSON::quote(&self.key),
            JSON::quote(&self.value),
            JSON::quote(&self.tier),
            self.priority,
            contenders
        )
    }
}

fn resolved_option(system: &SystemPlan, key: &str) -> Option<ResolvedOption> {
    let mut contenders = Vec::new();
    for option in &system.options {
        if option.key == key {
            contenders.push(OptionContender {
                value: clean_value(&option.value),
                tier: option.priority.label(),
                priority: option.priority.weight(),
                module_id: option.module_id.clone(),
                source_order: option.source_order,
                winner: false,
            });
        }
    }
    let highest = contenders.iter().map(|c| c.priority).max()?;
    let mut highest_values = contenders
        .iter()
        .filter(|c| c.priority == highest)
        .map(|c| c.value.as_str())
        .collect::<Vec<_>>();
    highest_values.sort_unstable();
    highest_values.dedup();
    if highest_values.len() > 1 {
        return None;
    }
    let winner_idx = contenders
        .iter()
        .enumerate()
        .filter(|(_, contender)| contender.priority == highest)
        .min_by(|(_, a), (_, b)| {
            a.module_id
                .cmp(&b.module_id)
                .then_with(|| a.source_order.cmp(&b.source_order))
        })
        .map(|(idx, _)| idx)?;
    contenders[winner_idx].winner = true;
    let winner = &contenders[winner_idx];
    Some(ResolvedOption {
        key: key.to_string(),
        value: winner.value.clone(),
        tier: winner.tier.clone(),
        priority: winner.priority,
        contenders,
    })
}

fn option_conflict(system: &SystemPlan) -> Option<(String, i64, Vec<String>)> {
    let mut keys = system.options.iter().map(|o| o.key.clone()).collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    for key in keys {
        let contenders = system.options.iter().filter(|o| o.key == key).collect::<Vec<_>>();
        let highest = contenders.iter().map(|o| o.priority.weight()).max()?;
        let mut values = contenders
            .iter()
            .filter(|o| o.priority.weight() == highest)
            .map(|o| clean_value(&o.value))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        if values.len() > 1 {
            return Some((key, highest, values));
        }
    }
    None
}

fn is_option_priority_metadata(key: &str) -> bool {
    key.ends_with(".tier")
        || key.ends_with(".priority")
        || key.ends_with(".override")
        || key.ends_with(".disabled")
}

fn option_type(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('[') {
        "List".to_string()
    } else if trimmed == "true" || trimmed == "false" {
        "Bool".to_string()
    } else if trimmed.parse::<i64>().is_ok() {
        "Int".to_string()
    } else if trimmed.starts_with('.') {
        "Enum".to_string()
    } else {
        "String".to_string()
    }
}

fn option_default(namespace: &str) -> String {
    match namespace {
        "network" => "dhcp-and-closed-firewall",
        "services" => "disabled",
        "users" | "user" => "absent",
        "filesystem" | "storage" => "guided-root",
        "boot" | "kernel" => "safe-profile",
        "performance" => "safe",
        "theme" => "default",
        "apps" | "workload" => "none",
        _ => "unset",
    }
    .to_string()
}

fn option_doc(key: &str) -> String {
    let namespace = key.split('.').next().unwrap_or("");
    match namespace {
        "network" => "Network identity, DNS, wireless, and firewall policy.",
        "services" => "System service declaration projected to systemd units and proof.",
        "users" => "System account identity used by login and generated roots.",
        "user" => "Per-user environment profile applied by jetos-user-apply.",
        "filesystem" => "Mounted filesystems, swap, timezone, and root projection.",
        "storage" => "Installer and activation storage tree.",
        "boot" | "kernel" => "Bootloader, kernel, initrd, firmware, and driver selection.",
        "performance" => "Safe performance profile plus expert sysctl/zram/scheduler overrides.",
        "theme" => "Reusable theme projection for GTK, terminals, editors, DM, and Studio.",
        "apps" => "Foreign app ecosystem policy and app permissions.",
        "workload" => "Container or microVM workload declaration.",
        "deploy" => "Fleet deploy target, rollout, health, and rollback policy.",
        _ => "JetOS system option.",
    }
    .to_string()
}

fn prefixed_options(system: &SystemPlan, prefix: &str) -> Vec<(String, String)> {
    system
        .options
        .iter()
        .filter_map(|o| {
            o.key
                .strip_prefix(prefix)
                .map(|key| (key.to_string(), clean_value(&o.value)))
        })
        .collect()
}

fn option_rows_json(rows: &[(String, String)]) -> String {
    rows.iter()
        .map(|(key, value)| JSON::object_of(&[("key", key), ("value", value)]))
        .collect::<Vec<_>>()
        .join(",")
}

fn strings_json(values: &[String]) -> String {
    values
        .iter()
        .map(|value| JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn boot_profile(system: &SystemPlan) -> BootProfile {
    let loader = option_value(system, &["boot.loader"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Limine".to_string());
    let kernel = option_value(system, &["boot.kernel", "kernel.package"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "CachyOS".to_string());
    let init = option_value(system, &["init.path"]).unwrap_or_else(|| "/sbin/init".to_string());
    let initrd_modules = option_value(system, &["boot.initrd.modules"])
        .map(|v| parse_list_items(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["virtio_blk".to_string(), "ext4".to_string()]);
    BootProfile {
        loader,
        kernel,
        init,
        initrd_modules,
    }
}

fn collect_names(system: &SystemPlan, namespace: &str) -> Vec<String> {
    let prefix = format!("{namespace}.");
    let mut names = Vec::new();
    for option in &system.options {
        let Some(rest) = option.key.strip_prefix(&prefix) else {
            continue;
        };
        let Some((name, _)) = rest.split_once('.') else {
            continue;
        };
        if !name.is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

fn user_names(system: &SystemPlan) -> Vec<String> {
    let mut names = collect_names(system, "users");
    for name in collect_names(system, "user") {
        if !names.iter().any(|n| n == &name) {
            names.push(name);
        }
    }
    names.sort();
    names
}

fn render_user_profile_json(system: &SystemPlan, user: &str) -> String {
    let home = option_value(
        system,
        &[&format!("user.{user}.home"), &format!("users.{user}.home")],
    )
    .unwrap_or_else(|| format!("/home/{user}"));
    let shell = option_value(
        system,
        &[
            &format!("user.{user}.shell"),
            &format!("users.{user}.shell"),
        ],
    )
    .map(|s| package_path_or_literal(&s))
    .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
    let packages = option_value(
        system,
        &[
            &format!("user.{user}.packages"),
            &format!("users.{user}.packages"),
        ],
    )
    .map(|v| parse_list_items(&v))
    .unwrap_or_default();
    let services = option_value(
        system,
        &[
            &format!("user.{user}.services"),
            &format!("users.{user}.services"),
        ],
    )
    .map(|v| parse_list_items(&v))
    .unwrap_or_default();
    let files = prefixed_options(system, &format!("user.{user}.files."));
    let packages_json = packages
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let services_json = services
        .iter()
        .map(|s| JSON::quote(s))
        .collect::<Vec<_>>()
        .join(",");
    let files_json = files
        .iter()
        .map(|(key, value)| JSON::object_of(&[("path", key), ("source", value)]))
        .collect::<Vec<_>>()
        .join(",");
    render_user_profile_json_parts(
        user,
        &home,
        &shell,
        &packages_json,
        &services_json,
        &files_json,
    )
}

fn render_user_profile_json_parts(
    user: &str,
    home: &str,
    shell: &str,
    packages_json: &str,
    services_json: &str,
    files_json: &str,
) -> String {
    format!(
        "{{\"kind\":\"jetos.user-generation\",\"user\":{},\"home\":{},\"shell\":{},\"packages\":[{}],\"services\":[{}],\"files\":[{}],\"standalone_commands\":[\"plan\",\"build\",\"switch\",\"rollback\",\"prove\"],\"proof\":\"user-generation-ready\"}}",
        JSON::quote(user),
        JSON::quote(home),
        JSON::quote(shell),
        packages_json,
        services_json,
        files_json
    )
}

fn parse_list_items(value: &str) -> Vec<String> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(value);
    // Split on commas OUTSIDE quotes only — kernel params like
    // `"lsm=landlock,yama,bpf"` are one item.
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in inner.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                items.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|item| clean_symbol(&clean_value(item)))
        .filter(|item| !item.is_empty())
        .collect()
}

fn package_path_or_literal(value: &str) -> String {
    if let Some(name) = value.strip_prefix("packages.") {
        format!("/run/current-system/sw/bin/{name}")
    } else {
        value.to_string()
    }
}

fn service_extra(service: &ServicePlan, keys: &[&str]) -> Option<String> {
    service
        .extra
        .iter()
        .find(|(k, _)| keys.iter().any(|wanted| k == wanted))
        .map(|(_, v)| clean_value(v))
}

fn clean_value(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.to_string()
    } else {
        trimmed.to_string()
    }
}

fn clean_symbol(value: &str) -> String {
    let cleaned = clean_value(value);
    let trimmed = cleaned.trim().trim_start_matches('.');
    trimmed
        .strip_prefix("users.")
        .unwrap_or(trimmed)
        .to_string()
}

fn safe_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn safe_identifier(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(true)
    {
        out.insert(0, '_');
    }
    out
}

fn clean_bool_json(value: &str) -> &'static str {
    if clean_symbol(value).eq_ignore_ascii_case("true") {
        "true"
    } else {
        "false"
    }
}

fn render_proof(system: &SystemPlan, realized: &[RealizedPackage], plan: &EnvPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!("jetos proof for {}\n", system.name));
    out.push_str(&format!("target: {}\n", system.target));
    let risks = risk_classes(system);
    out.push_str(&format!(
        "risk: {}\n",
        if risks.is_empty() {
            "low".to_string()
        } else {
            risks.join(", ")
        }
    ));
    out.push_str("plan: pass\n");
    out.push_str("packages:\n");
    for p in realized {
        out.push_str(&format!("  {} -> {}\n", p.reference, p.out));
    }
    out.push_str("options:\n");
    for o in &system.options {
        out.push_str(&format!("  {} = {}\n", o.key, o.value));
    }
    out.push_str("services:\n");
    for s in &system.services {
        out.push_str(&format!("  {} enable={}\n", s.name, s.enable));
    }
    out.push_str("images:\n");
    for image in &plan.images {
        if image.kind == ImageKind::Iso && image.from == system.name {
            out.push_str(&format!("  {} {}\n", image.name, image.format));
        }
    }
    out
}

fn risk_classes(system: &SystemPlan) -> Vec<String> {
    let mut risks = Vec::new();
    for option in &system.options {
        let key = option.key.as_str();
        if key.starts_with("filesystem.") && !risks.iter().any(|r| r == "filesystem") {
            risks.push("filesystem".to_string());
        }
        if key.contains("boot") && !risks.iter().any(|r| r == "boot") {
            risks.push("boot".to_string());
        }
        if key.contains("kernel") && !risks.iter().any(|r| r == "kernel") {
            risks.push("kernel".to_string());
        }
        if (key.starts_with("storage.") || key.starts_with("performance."))
            && !risks.iter().any(|r| r == "performance/storage")
        {
            risks.push("performance/storage".to_string());
        }
        if (key.starts_with("user.") || key.starts_with("apps.") || key.starts_with("workload."))
            && !risks.iter().any(|r| r == "user-app-workload")
        {
            risks.push("user-app-workload".to_string());
        }
    }
    if system.services.iter().any(|s| s.enable) {
        risks.push("service-risk".to_string());
    }
    risks
}
