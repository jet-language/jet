use super::parse::Parsed;
use crate::Output::{self, Theme};
use crate::Store::{self, Roots};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `jetpack list` — show realized store entries.
pub(super) fn cmd_list(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    let entries = Store::list(&roots);
    if entries.is_empty() {
        theme.status("no realized packages yet.");
        return 0;
    }
    theme.status(&format!("{} realized package(s):", entries.len()));
    let name_w = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let ver_w = entries
        .iter()
        .map(|e| {
            if e.version.is_empty() {
                1
            } else {
                e.version.len()
            }
        })
        .max()
        .unwrap_or(1);
    for e in entries {
        let v = if e.version.is_empty() {
            "—"
        } else {
            &e.version
        };
        theme.detail(&format!(
            "{}  {}  {}",
            theme.bold(&format!("{:<name_w$}", e.name)),
            format!("{v:<ver_w$}"),
            theme.gray(&e.reference)
        ));
    }
    0
}

/// `jetpack cache` — host-owned role bindings and signed NAR transfers.
/// Repository state can request roles, but only this host config chooses
/// mirrors, trust keys, credentials, and write authority.
pub(super) fn cmd_cache(theme: &Theme, parsed: &Parsed) -> i32 {
    let action = parsed.positional.first().map(String::as_str).unwrap_or("list");
    let roots = Store::resolve();
    match action {
        "bind" => {
            let Some(role) = parsed.positional.get(1) else {
                return cache_usage(theme, "`cache bind` needs a role and mirror", "jet cache bind public /absolute/cache");
            };
            let mirrors = parsed.positional[2..].to_vec();
            if mirrors.is_empty() {
                return cache_usage(theme, "`cache bind` needs at least one mirror", "jet cache bind public /absolute/cache");
            }
            if !theme.confirm_apply(parsed.flags.assume_yes) {
                return 0;
            }
            match Store::bind_cache(
                &roots,
                role,
                mirrors,
                parsed.flags.archive_key.as_deref().map(Path::new),
                parsed.flags.cache_credential.clone(),
                parsed.flags.cache_write,
            ) {
                Ok(binding) => {
                    if parsed.flags.json {
                        println!("{}", Store::cache_binding_json(&binding));
                    } else {
                        theme.status(&format!(
                            "bound cache role `{}` to {} ordered mirror(s){}",
                            binding.role,
                            binding.mirrors.len(),
                            if binding.allow_write { " with write authority" } else { "" }
                        ));
                    }
                    0
                }
                Err(error) => cache_error(theme, "bind", error),
            }
        }
        "list" => match Store::list_cache_bindings(&roots) {
            Ok(bindings) => {
                if parsed.flags.json {
                    let values = bindings
                        .iter()
                        .map(Store::cache_binding_json)
                        .collect::<Vec<_>>()
                        .join(",");
                    println!("[{values}]");
                } else if bindings.is_empty() {
                    theme.status("no host-owned cache bindings.");
                } else {
                    for binding in &bindings {
                        theme.detail(&format!(
                            "{}  {} mirror(s){}{}",
                            theme.bold(&binding.role),
                            binding.mirrors.len(),
                            if binding.allow_write { "  write" } else { "  read-only" },
                            binding
                                .credential_provider
                                .as_deref()
                                .map(|provider| format!("  credential:{provider}"))
                                .unwrap_or_default()
                        ));
                    }
                }
                0
            }
            Err(error) => cache_error(theme, "list", error),
        },
        "remove" => {
            let Some(role) = parsed.positional.get(1) else {
                return cache_usage(theme, "`cache remove` needs a role", "jet cache remove public --yes");
            };
            if !theme.confirm_apply(parsed.flags.assume_yes) {
                return 0;
            }
            match Store::remove_cache_binding(&roots, role) {
                Ok(true) => {
                    theme.status(&format!("removed cache role `{role}`"));
                    0
                }
                Ok(false) => {
                    theme.status(&format!("cache role `{role}` was not bound"));
                    0
                }
                Err(error) => cache_error(theme, "remove", error),
            }
        }
        "publish" | "verify" | "substitute" => {
            let Some(target) = parsed.positional.get(1) else {
                return cache_usage(theme, "cache transfer needs an entry, reference, or output digest", "jet cache verify <entry> --role public");
            };
            let role = parsed.flags.cache_role.as_deref().unwrap_or("public");
            match action {
                "publish" => {
                    if !theme.confirm_apply(parsed.flags.assume_yes) {
                        return 0;
                    }
                    report_cache(
                        theme,
                        parsed,
                        action,
                        Store::publish_cache_entry(&roots, target, role),
                    )
                }
                "verify" => report_cache(
                    theme,
                    parsed,
                    action,
                    Store::verify_cache_transfer(&roots, target, role),
                ),
                "substitute" => {
                    let Some(destination) = parsed.flags.archive_to.as_deref() else {
                        return cache_usage(theme, "`cache substitute` needs `--to <directory>`", "jet cache substitute <entry> --role public --to /tmp/output --yes");
                    };
                    if !theme.confirm_apply(parsed.flags.assume_yes) {
                        return 0;
                    }
                    report_cache(
                        theme,
                        parsed,
                        action,
                        Store::substitute_cache_entry(&roots, target, role, destination),
                    )
                }
                _ => unreachable!(),
            }
        }
        _ => cache_usage(
            theme,
            &format!("`cache {action}` is not a cache command"),
            "jet cache bind|list|remove|publish|verify|substitute",
        ),
    }
}

fn report_cache(
    theme: &Theme,
    parsed: &Parsed,
    action: &str,
    result: std::io::Result<Store::CacheTransferReport>,
) -> i32 {
    match result {
        Ok(report) => {
            if parsed.flags.json {
                println!("{}", Store::cache_report_json(action, &report));
            } else {
                theme.status(&format!(
                    "{action}: {} from {} ({})",
                    report.entry,
                    report.mirror,
                    human_bytes(report.bytes)
                ));
            }
            0
        }
        Err(error) => cache_error(theme, action, error),
    }
}

fn cache_error(theme: &Theme, action: &str, error: std::io::Error) -> i32 {
    theme.error(
        &format!("cache {action} failed"),
        &error.to_string(),
        "bind a verified host-owned mirror and check the role trust key; never put credentials in the endpoint.",
    );
    2
}

fn cache_usage(theme: &Theme, what: &str, fix: &str) -> i32 {
    theme.error(what, "cache roles and mirrors are host-owned and verified before use.", fix);
    2
}

/// `jetpack hangar du` — honest per-object disk usage (U22 / D-JPK-GC1).
/// Source-built objects are counted like any other, so `du` never hides them.
///
/// Hangar Store v2 also exposes:
/// - `hangar ingest <dir> --name <n> [--version <v>] [--ref <r>]`
/// - `hangar verify <digest-or-id>`
/// - `hangar referrers <digest>`
/// - `hangar recover` — sweep crashed Hangar/build staging and `.partial` objects
/// - `hangar export|import|dump|restore|copy|sign|repair` — one signed archive
pub(super) fn cmd_hangar(theme: &Theme, parsed: &Parsed) -> i32 {
    let sub = parsed.positional.first().map(String::as_str);
    match sub {
        Some("path") => {
            let path = Store::resolve().hangar_dir();
            if parsed.flags.json {
                hangar_status_json(
                    "path",
                    &format!(
                        ",\"path\":{}",
                        crate::JSON::quote(&path.display().to_string())
                    ),
                );
            } else {
                println!("{}", path.display());
            }
            0
        }
        Some("du") | None => {
            let roots = Store::resolve();
            let entries = Store::du(&roots);
            if entries.is_empty() {
                if parsed.flags.json {
                    hangar_status_json(
                        "du",
                        ",\"objects\":0,\"built\":0,\"bytes\":0,\"entries\":[]",
                    );
                } else {
                    theme.status("hangar is empty.");
                }
                return 0;
            }
            let mut total = 0u64;
            let mut built = 0usize;
            let mut machine_entries = Vec::with_capacity(entries.len());
            for e in &entries {
                total += e.bytes;
                if e.source_built {
                    built += 1;
                }
                machine_entries.push(format!(
                    "{{\"id\":{},\"bytes\":{},\"source_built\":{}}}",
                    crate::JSON::quote(&e.id),
                    e.bytes,
                    e.source_built,
                ));
                let tag = if e.source_built { " (built)" } else { "" };
                if !parsed.flags.json {
                    theme.detail(&format!(
                        "{:>10}  {}{}",
                        human_bytes(e.bytes),
                        theme.bold(&e.id),
                        theme.gray(tag)
                    ));
                }
            }
            if parsed.flags.json {
                hangar_status_json(
                    "du",
                    &format!(
                        ",\"objects\":{},\"built\":{},\"bytes\":{},\"entries\":[{}]",
                        entries.len(),
                        built,
                        total,
                        machine_entries.join(",")
                    ),
                );
                return 0;
            }
            theme.status(&format!(
                "{} object(s), {} built from source, {} total",
                entries.len(),
                built,
                human_bytes(total)
            ));
            0
        }
        Some("ingest") => cmd_hangar_ingest(theme, parsed),
        Some("verify") => cmd_hangar_verify(theme, parsed),
        Some("export") => cmd_hangar_archive(theme, parsed, "export"),
        Some("import") => cmd_hangar_archive(theme, parsed, "import"),
        Some("dump") => cmd_hangar_archive(theme, parsed, "dump"),
        Some("restore") => cmd_hangar_archive(theme, parsed, "restore"),
        Some("copy") => cmd_hangar_archive(theme, parsed, "copy"),
        Some("sign") => cmd_hangar_archive(theme, parsed, "sign"),
        Some("repair") => cmd_hangar_archive(theme, parsed, "repair"),
        Some("referrers") => cmd_hangar_referrers(theme, parsed),
        Some("register-external-root") => cmd_hangar_register_external_root(theme, parsed),
        Some("list-external-roots") => cmd_hangar_list_external_roots(theme, parsed),
        Some("unregister-external-root") => cmd_hangar_unregister_external_root(theme, parsed),
        Some("recover") => {
            let roots = Store::resolve();
            match Store::recover_hangar(&roots) {
                Ok(n) => {
                    if parsed.flags.json {
                        hangar_status_json("recover", &format!(",\"recovered\":{n}"));
                    } else {
                        theme.status(&format!(
                            "recovered {n} abandoned or committed hangar item(s)"
                        ));
                    }
                    0
                }
                Err(e) => {
                    hangar_report_error(
                        theme,
                        parsed,
                        "E1340",
                        "could not recover hangar staging",
                        &e.to_string(),
                        "check permissions on the hangar root.",
                    )
                }
            }
        }
        Some(other) => {
            hangar_report_error(
                theme,
                parsed,
                "E1340",
                &format!("`hangar {other}` is not a hangar command"),
                "hangar subcommands: `path`, `du`, `ingest`, `verify`, `export`, `import`, `dump`, `restore`, `copy`, `sign`, `repair`, `referrers`, `recover`, `register-external-root`, `list-external-roots`, `unregister-external-root`.",
                "run `jetpack hangar path`.",
            )
        }
    }
}

fn cmd_hangar_register_external_root(theme: &Theme, parsed: &Parsed) -> i32 {
    let args = positional_values_after(parsed, "register-external-root");
    let (Some(label), Some(reference)) = (args.first(), args.get(1)) else {
        return hangar_report_error(
            theme,
            parsed,
            "E1340",
            "`hangar register-external-root` needs a label and reference",
            "a manual root names one existing Hangar closure to retain.",
            "run `jetpack hangar register-external-root <label> <reference> [--expires-in <duration>] --yes`.",
        );
    };
    let expires_in = match flag_value(parsed, "--expires-in") {
        Some(value) => match parse_duration(&value) {
            Ok(seconds) => Some(seconds),
            Err(error) => {
                return hangar_report_error(
                    theme,
                    parsed,
                    "E1340",
                    "invalid external-root expiry",
                    &error,
                    "use a positive duration such as `1h`, `7d`, or `1w`.",
                );
            }
        },
        None => None,
    };
    let now = unix_now();
    let expires_at = match expires_in {
        Some(seconds) => match now.checked_add(seconds) {
            Some(value) => Some(value),
            None => {
                return hangar_report_error(
                    theme,
                    parsed,
                    "E1340",
                    "external-root expiry is too far in the future",
                    "the requested duration overflows the lifecycle timestamp.",
                    "use a shorter duration.",
                );
            }
        },
        None => None,
    };
    let roots = Store::resolve();
    let closure_size = match Store::external_root_closure_size(&roots, reference) {
        Ok(size) => size,
        Err(error) => return report_external_root_error(theme, parsed, error),
    };
    let principal = external_root_principal();
    let expected_etag = flag_value(parsed, "--if-etag");
    if parsed.flags.json {
        if !parsed.flags.assume_yes {
            hangar_plan_json(
                "register-external-root",
                &format!(
                    ",\"applied\":false,\"label\":{},\"reference\":{},\"closure_objects\":{},\"expires_at\":{},\"if_etag\":{}",
                    crate::JSON::quote(label),
                    crate::JSON::quote(reference),
                    closure_size,
                    expires_at.map_or_else(|| "null".to_string(), |value| value.to_string()),
                    expected_etag
                        .as_deref()
                        .map(crate::JSON::quote)
                        .unwrap_or_else(|| "null".to_string())
                ),
            );
            return 0;
        }
    } else {
        theme.status("Plan external root");
        theme.detail(&format!("+ {label}"));
        theme.detail(&format!("closure objects: {closure_size}"));
        if let Some(seconds) = expires_in {
            theme.detail(&format!("expires in {}", render_duration(seconds)));
        } else {
            theme.detail("expires: never");
        }
        if let Some(expected) = expected_etag.as_deref() {
            theme.detail(&format!("if etag: {expected}"));
        }
        if !theme.confirm_apply(parsed.flags.assume_yes) {
            return 0;
        }
    }
    match Store::register_external_root_at(
        &roots,
        &principal,
        label,
        reference,
        expires_at,
        expected_etag.as_deref(),
        now,
    ) {
        Ok(view) => {
            if parsed.flags.json {
                hangar_status_json(
                    "register-external-root",
                    &format!(
                        ",\"label\":{},\"reference\":{},\"etag\":{},\"expires_at\":{}",
                        crate::JSON::quote(&view.label),
                        crate::JSON::quote(&view.reference),
                        crate::JSON::quote(&view.etag),
                        view.expires_at
                            .map_or_else(|| "null".to_string(), |value| value.to_string())
                    ),
                );
            } else {
                theme.status(&format!(
                    "Created external root `{}` at etag {}.",
                    view.label, view.etag
                ));
            }
            0
        }
        Err(error) => report_external_root_error(theme, parsed, error),
    }
}

fn cmd_hangar_list_external_roots(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    match Store::list_external_roots(&roots, &external_root_principal()) {
        Ok(roots) if roots.is_empty() => {
            if parsed.flags.json {
                hangar_status_json("list-external-roots", ",\"roots\":[]");
            } else {
                theme.status("no external roots.");
            }
            0
        }
        Ok(roots) => {
            let now = unix_now();
            let mut machine_roots = Vec::with_capacity(roots.len());
            for root in roots {
                machine_roots.push(format!(
                    "{{\"label\":{},\"reference\":{},\"etag\":{},\"expires_at\":{}}}",
                    crate::JSON::quote(&root.label),
                    crate::JSON::quote(&root.reference),
                    crate::JSON::quote(&root.etag),
                    root.expires_at
                        .map_or_else(|| "null".to_string(), |value| value.to_string())
                ));
                let expiry = root
                    .expires_at
                    .map(|at| format!("{}", render_expiry(at, now)))
                    .unwrap_or_else(|| "expires never".to_string());
                if !parsed.flags.json {
                    theme.detail(&format!(
                        "{}  {}  {}  etag {}",
                        root.label, root.reference, expiry, root.etag
                    ));
                }
            }
            if parsed.flags.json {
                hangar_status_json(
                    "list-external-roots",
                    &format!(",\"roots\":[{}]", machine_roots.join(",")),
                );
            }
            0
        }
        Err(error) => report_external_root_error(theme, parsed, error),
    }
}

fn cmd_hangar_unregister_external_root(theme: &Theme, parsed: &Parsed) -> i32 {
    let args = positional_values_after(parsed, "unregister-external-root");
    let Some(label) = args.first() else {
        return hangar_report_error(
            theme,
            parsed,
            "E1340",
            "`hangar unregister-external-root` needs a label",
            "removing a manual root needs its stable label and current etag.",
            "run `jetpack hangar unregister-external-root <label> --etag <etag> --yes`.",
        );
    };
    let Some(etag) = flag_value(parsed, "--etag") else {
        return hangar_report_error(
            theme,
            parsed,
            "E1340",
            "`hangar unregister-external-root` needs `--etag`",
            "root removal is compare-and-swap protected.",
            "read `jetpack hangar list-external-roots`, then pass its etag.",
        );
    };
    if !hangar_confirm_apply(theme, parsed, "unregister-external-root") {
        return 0;
    }
    let roots = Store::resolve();
    match Store::unregister_external_root_at(
        &roots,
        &external_root_principal(),
        label,
        &etag,
        unix_now(),
    ) {
        Ok(()) => {
            if parsed.flags.json {
                hangar_status_json(
                    "unregister-external-root",
                    &format!(",\"label\":{}", crate::JSON::quote(label)),
                );
            } else {
                theme.status(&format!("Removed external root `{label}`."));
            }
            0
        }
        Err(error) => report_external_root_error(theme, parsed, error),
    }
}

fn report_external_root_error(
    theme: &Theme,
    parsed: &Parsed,
    error: Store::ExternalRootError,
) -> i32 {
    match error {
        Store::ExternalRootError::Conflict {
            label,
            current,
            ..
        } => {
            let fix = match current {
                Some(current) => format!(
                    "Read the current etag `{current}` and retry with `--if-etag {current}`."
                ),
                None => "Read the current roots, then retry the mutation.".to_string(),
            };
            hangar_report_error(
                theme,
                parsed,
                "E1320",
                &format!("external root `{label}` changed before this request"),
                "No requested root mutation was applied.",
                &fix,
            )
        }
        Store::ExternalRootError::ReferenceNotFound(reference) => hangar_report_error(
            theme,
            parsed,
            "E1340",
            "could not retain that external root",
            &format!("no Hangar entry matches `{reference}`."),
            "run `jetpack list` and use the exact package reference.",
        ),
        Store::ExternalRootError::Store(error) => hangar_report_error(
            theme,
            parsed,
            "E1340",
            "could not update the external root",
            &error.to_string(),
            "repair the Hangar state or check its permissions, then retry.",
        ),
    }
}

fn external_root_principal() -> String {
    std::env::var("JETPACK_PRINCIPAL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "jetpack-cli".to_string())
}

fn positional_values_after(parsed: &Parsed, sub: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut seen_sub = false;
    let mut skip_next = false;
    for arg in &parsed.positional {
        if skip_next {
            skip_next = false;
            continue;
        }
        if !seen_sub {
            if arg == sub {
                seen_sub = true;
            }
            continue;
        }
        if arg.starts_with("--") {
            if !arg.contains('=') {
                skip_next = true;
            }
            continue;
        }
        values.push(arg.clone());
    }
    values
}

fn parse_duration(value: &str) -> Result<u64, String> {
    let (number, suffix) = value.split_at(
        value
            .find(|byte: char| !byte.is_ascii_digit())
            .unwrap_or(value.len()),
    );
    let amount = number
        .parse::<u64>()
        .map_err(|_| format!("`{value}` is not a valid duration"))?;
    if amount == 0 {
        return Err("external-root expiry must be positive".to_string());
    }
    let multiplier = match suffix {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        "w" => 7 * 24 * 60 * 60,
        _ => return Err(format!("`{value}` needs a unit: s, m, h, d, or w")),
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| format!("`{value}` is too large"))
}

fn render_duration(seconds: u64) -> String {
    let (amount, unit) = if seconds % (7 * 24 * 60 * 60) == 0 {
        (seconds / (7 * 24 * 60 * 60), "week")
    } else if seconds % (24 * 60 * 60) == 0 {
        (seconds / (24 * 60 * 60), "day")
    } else if seconds % (60 * 60) == 0 {
        (seconds / (60 * 60), "hour")
    } else if seconds % 60 == 0 {
        (seconds / 60, "minute")
    } else {
        (seconds, "second")
    };
    format!("{amount} {unit}{}", if amount == 1 { "" } else { "s" })
}

fn render_expiry(expires_at: u64, now: u64) -> String {
    if expires_at <= now {
        "expired".to_string()
    } else {
        format!("expires in {}", render_duration(expires_at - now))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn hangar_status_json(action: &str, fields: &str) {
    println!(
        "{}",
        jet_foundation::Report::render_status_json("ok", true, action, fields)
    );
}

fn hangar_plan_json(action: &str, fields: &str) {
    println!(
        "{}",
        jet_foundation::Report::render_status_json("plan", true, action, fields)
    );
}

fn hangar_confirm_apply(theme: &Theme, parsed: &Parsed, action: &str) -> bool {
    if !parsed.flags.json {
        return theme.confirm_apply(parsed.flags.assume_yes);
    }
    if parsed.flags.assume_yes {
        return true;
    }
    hangar_plan_json(action, ",\"applied\":false");
    false
}

fn hangar_report_error(
    theme: &Theme,
    parsed: &Parsed,
    code: &str,
    what: &str,
    why: &str,
    fix: &str,
) -> i32 {
    if parsed.flags.json {
        let diagnostic = jet_foundation::Diagnostics::Diagnostic::error(
            code,
            what.to_string(),
            why.to_string(),
            fix.to_string(),
            None,
        );
        print!(
            "{}",
            jet_foundation::Diagnostics::render_all_json(
                &jet_foundation::Diagnostics::ReportPath::from_process(""),
                "",
                &[diagnostic],
            )
        );
    } else {
        theme.error_coded(code, what, why, fix);
    }
    2
}

fn hangar_ingest_error(theme: &Theme, parsed: &Parsed, error: &Store::IngestError) -> i32 {
    hangar_report_error(
        theme,
        parsed,
        error.code(),
        &error.what(),
        &error.why(),
        error.fix(),
    )
}

fn cmd_hangar_ingest(theme: &Theme, parsed: &Parsed) -> i32 {
    let name = match parsed
        .flags
        .os_name
        .clone()
        .or_else(|| flag_value(parsed, "--name"))
    {
        Some(n) if !n.is_empty() => n,
        _ => {
            return hangar_report_error(
                theme,
                parsed,
                "E1340",
                "`hangar ingest` needs `--name`",
                "every hangar object has a package name in its record.",
                "pass `--name <pkg>`.",
            );
        }
    };
    let version = flag_value(parsed, "--version").unwrap_or_default();
    let platform_artifact_kind =
        flag_value(parsed, "--platform-artifact-kind").unwrap_or_default();
    let dir = match positional_path_after(parsed, "ingest") {
        Some(p) => p,
        None => {
            return hangar_report_error(
                theme,
                parsed,
                "E1340",
                "`hangar ingest` needs a source directory",
                "atomic staged ingest copies a local tree into the hangar.",
                "run `jetpack hangar ingest <dir> --name <pkg>`.",
            );
        }
    };
    let reference =
        flag_value(parsed, "--ref").unwrap_or_else(|| dir.display().to_string());
    let mut outputs = std::collections::BTreeMap::new();
    outputs.insert("out".to_string(), dir.clone());
    if let Some(dev) = flag_value(parsed, "--output-dev") {
        outputs.insert("dev".to_string(), PathBuf::from(dev));
    }
    let roots = Store::resolve();
    let req = Store::IngestRequest {
        name,
        version,
        reference,
        cache_identity: Store::CacheIdentity::default(),
        references: Vec::new(),
        outputs,
        signature: String::new(),
        provenance: String::new(),
        platform_artifact_kind,
    };
    match Store::ingest_tree(&roots, &req) {
        Ok(ingested) => {
            if parsed.flags.json {
                hangar_status_json(
                    "ingest",
                    &format!(
                        ",\"id\":{},\"output_hash\":{},\"deduplicated\":{}",
                        crate::JSON::quote(&ingested.entry.id),
                        crate::JSON::quote(&ingested.entry.envelope.output_hash),
                        ingested.deduplicated
                    ),
                );
            } else {
                let tag = if ingested.deduplicated {
                    " (deduplicated)"
                } else {
                    ""
                };
                theme.status(&format!(
                    "ingested {} → {}{}",
                    theme.bold(&ingested.entry.id),
                    ingested.entry.envelope.output_hash,
                    tag
                ));
            }
            0
        }
        Err(err) => hangar_ingest_error(theme, parsed, &err),
    }
}

fn cmd_hangar_verify(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let Some(target) = positional_path_after(parsed, "verify") else {
        let entries = match Store::list_checked(&roots) {
            Ok(entries) => entries,
            Err(error) => {
                return hangar_report_error(
                    theme,
                    parsed,
                    "E1340",
                    "could not read the Hangar",
                    &error.to_string(),
                    "repair the Hangar journal, then retry verification.",
                );
            }
        };
        let mut verified = 0usize;
        for entry in &entries {
            if let Err(error) = Store::verify_archive(
                &roots,
                &entry.id,
                parsed.flags.archive_key.as_deref(),
            ) {
                return report_archive_error(theme, parsed, "verify", error);
            }
            verified += 1;
        }
        if parsed.flags.json {
            hangar_status_json("verify", &format!(",\"objects\":{verified}"));
        } else {
            theme.status(&format!("verified {verified} Hangar object(s)"));
        }
        return 0;
    };
    let target = target.to_string_lossy().into_owned();
    let entries = Store::list(&roots);
    if PathBuf::from(&target).is_file() {
        return report_archive_result(
            theme,
            parsed,
            "verify",
            Store::verify_archive(
                &roots,
                &target,
                parsed.flags.archive_key.as_deref(),
            ),
        );
    }
    let Some(entry) = entries.iter().find(|e| {
        e.id == target
            || e.envelope.output_hash == target
            || e.reference == target
            || format!("{}@{}", e.name, e.version) == target
    }) else {
        return hangar_report_error(
            theme,
            parsed,
            "E1340",
            &format!("no hangar object `{target}`"),
            "verify only checks realized hangar records.",
            "run `jetpack list` to see ids.",
        );
    };
    match Store::verify_archive(&roots, &entry.id, parsed.flags.archive_key.as_deref()) {
        Ok(report) => {
            if parsed.flags.json {
                println!("{}", Store::report_json("verify", &report));
            } else {
                theme.status(&format!("verified {}", entry.envelope.output_hash));
            }
            0
        }
        Err(error) => report_archive_error(theme, parsed, "verify", error),
    }
}

fn cmd_hangar_archive(theme: &Theme, parsed: &Parsed, action: &str) -> i32 {
    let roots = Store::resolve();
    let target = positional_values_after(parsed, action).into_iter().next();
    let key = parsed.flags.archive_key.as_deref();
    match action {
        "export" => {
            let Some(target) = target else {
                return archive_usage(theme, parsed, "`hangar export` needs an entry id, reference, or output digest", "jetpack hangar export <entry> --to <archive.hangar> --yes");
            };
            let Some(destination) = parsed.flags.archive_to.as_deref() else {
                return archive_usage(theme, parsed, "`hangar export` needs `--to <archive.hangar>`", "write one signed archive, then import it on the target Hangar");
            };
            if !hangar_confirm_apply(theme, parsed, action) {
                return 0;
            }
            let result = Store::export_archive(&roots, &target, !parsed.flags.archive_no_deps, key)
                .and_then(|(bytes, report)| {
                    Store::write_archive_file(destination, &bytes)?;
                    Ok(report)
                });
            report_archive_result(theme, parsed, action, result)
        }
        "import" => {
            let Some(source) = target else {
                return archive_usage(theme, parsed, "`hangar import` needs an archive path", "jetpack hangar import <archive.hangar> --yes");
            };
            let bytes = match Store::read_archive_file(std::path::Path::new(&source)) {
                Ok(bytes) => bytes,
                Err(error) => return report_archive_error(theme, parsed, action, error),
            };
            if !hangar_confirm_apply(theme, parsed, action) {
                return 0;
            }
            report_archive_result(
                theme,
                parsed,
                action,
                Store::import_archive(&roots, &bytes, key, parsed.flags.archive_allow_unsigned),
            )
        }
        "dump" => {
            if parsed.flags.json {
                return hangar_report_error(
                    theme,
                    parsed,
                    "E1340",
                    "`hangar dump --json` cannot stream archive bytes",
                    "JSON mode reserves stdout for one `jet.report/v1` machine result.",
                    "omit `--json` when redirecting the signed archive, or use `export --json` for a status report.",
                );
            }
            let Some(target) = target else {
                return archive_usage(theme, parsed, "`hangar dump` needs an entry id or reference", "jetpack hangar dump <entry> > closure.hangar");
            };
            match Store::export_archive(&roots, &target, !parsed.flags.archive_no_deps, key) {
                Ok((bytes, _)) => {
                    if let Err(error) = std::io::stdout().write_all(&bytes) {
                        return report_archive_error(theme, parsed, action, error);
                    }
                    0
                }
                Err(error) => report_archive_error(theme, parsed, action, error),
            }
        }
        "restore" => {
            let mut bytes = Vec::new();
            if let Err(error) = std::io::stdin()
                .lock()
                .take((Store::MAX_ARCHIVE_BYTES as u64).saturating_add(1))
                .read_to_end(&mut bytes)
            {
                return report_archive_error(
                    theme,
                    parsed,
                    action,
                    std::io::Error::other(format!("could not read archive from stdin: {error}")),
                );
            }
            if bytes.len() > Store::MAX_ARCHIVE_BYTES {
                return report_archive_error(
                    theme,
                    parsed,
                    action,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "archive from stdin exceeds the 1 GiB Hangar limit",
                    ),
                );
            }
            if !hangar_confirm_apply(theme, parsed, action) {
                return 0;
            }
            report_archive_result(
                theme,
                parsed,
                action,
                Store::import_archive(&roots, &bytes, key, parsed.flags.archive_allow_unsigned),
            )
        }
        "copy" => {
            let Some(target) = target else {
                return archive_usage(theme, parsed, "`hangar copy` needs an entry id or reference", "jetpack hangar copy <entry> --to <hangar-root> --yes");
            };
            let Some(destination) = parsed.flags.archive_to.as_deref() else {
                return archive_usage(theme, parsed, "`hangar copy` needs `--to <hangar-root>`", "copy uses the signed export/import backbone");
            };
            if !hangar_confirm_apply(theme, parsed, action) {
                return 0;
            }
            report_archive_result(
                theme,
                parsed,
                action,
                Store::copy_archive(&roots, &target, destination, key),
            )
        }
        "sign" => {
            let Some(target) = target else {
                return archive_usage(theme, parsed, "`hangar sign` needs an entry id or archive path", "jetpack hangar sign <entry-or-archive> [--to <path>] --yes");
            };
            if !hangar_confirm_apply(theme, parsed, action) {
                return 0;
            }
            report_archive_result(
                theme,
                parsed,
                action,
                Store::sign_archive(
                    &roots,
                    &target,
                    parsed.flags.archive_to.as_deref(),
                    key,
                ),
            )
        }
        "repair" => {
            let Some(target) = target else {
                return archive_usage(theme, parsed, "`hangar repair` needs an entry id or reference", "jetpack hangar repair <entry> --from <signed.hangar> --yes");
            };
            if !hangar_confirm_apply(theme, parsed, action) {
                return 0;
            }
            report_archive_result(
                theme,
                parsed,
                action,
                Store::repair_archive(
                    &roots,
                    &target,
                    parsed.flags.archive_from.as_deref(),
                    key,
                ),
            )
        }
        _ => unreachable!("archive action is closed at the Hangar dispatcher"),
    }
}

fn report_archive_result(
    theme: &Theme,
    parsed: &Parsed,
    action: &str,
    result: std::io::Result<Store::ArchiveReport>,
) -> i32 {
    match result {
        Ok(report) => {
            if parsed.flags.json {
                println!("{}", Store::report_json(action, &report));
            } else {
                theme.status(&format!(
                    "{action}: {} object(s), {}{}",
                    report.objects,
                    human_bytes(report.bytes),
                    if report.signed { ", signed" } else { "" }
                ));
            }
            0
        }
        Err(error) => report_archive_error(theme, parsed, action, error),
    }
}

fn report_archive_error(
    theme: &Theme,
    parsed: &Parsed,
    action: &str,
    error: std::io::Error,
) -> i32 {
    if action == "verify" && error.to_string().starts_with("hangar ingest ") {
        let message = error.to_string();
        hangar_report_error(
            theme,
            parsed,
            "E1315",
            &message,
            &message,
            "Fix the rejected tree (path law, special files, or unsupported xattrs) and ingest again.",
        );
        return 2;
    }
    hangar_report_error(
        theme,
        parsed,
        "E1340",
        &format!("Hangar {action} failed"),
        &error.to_string(),
        "check the archive signature and paths, or retry from a verified source archive.",
    )
}

fn archive_usage(theme: &Theme, parsed: &Parsed, what: &str, fix: &str) -> i32 {
    hangar_report_error(
        theme,
        parsed,
        "E1340",
        what,
        "Hangar archive operations are plan-before-apply and content verified.",
        fix,
    )
}

fn cmd_hangar_referrers(theme: &Theme, parsed: &Parsed) -> i32 {
    let digest = match positional_path_after(parsed, "referrers") {
        Some(d) => d.to_string_lossy().into_owned(),
        None => {
            return hangar_report_error(
                theme,
                parsed,
                "E1340",
                "`hangar referrers` needs an output digest",
                "referrers lists objects that declare a dependency on this digest.",
                "run `jetpack hangar referrers sha256-…`.",
            );
        }
    };
    let roots = Store::resolve();
    let refs = match Store::referrers_of(&roots, &digest) {
        Ok(refs) => refs,
        Err(error) => {
            return hangar_report_error(
                theme,
                parsed,
                "E1340",
                "could not read the hangar closure graph",
                &error.to_string(),
                "run `jetpack hangar verify` before querying referrers.",
            );
        }
    };
    if refs.is_empty() {
        if parsed.flags.json {
            hangar_status_json(
                "referrers",
                &format!(
                    ",\"digest\":{},\"referrers\":[]",
                    crate::JSON::quote(&digest)
                ),
            );
        } else {
            theme.status("no referrers.");
        }
        return 0;
    }
    if parsed.flags.json {
        let values = refs
            .iter()
            .map(|reference| crate::JSON::quote(reference))
            .collect::<Vec<_>>();
        hangar_status_json(
            "referrers",
            &format!(
                ",\"digest\":{},\"referrers\":[{}]",
                crate::JSON::quote(&digest),
                values.join(",")
            ),
        );
    } else {
        for r in refs {
            theme.detail(&r);
        }
    }
    0
}

fn flag_value(parsed: &Parsed, name: &str) -> Option<String> {
    let args = &parsed.positional;
    let mut i = 0;
    while i < args.len() {
        if args[i] == name {
            return args.get(i + 1).cloned();
        }
        if let Some(rest) = args[i].strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
        i += 1;
    }
    None
}

fn positional_path_after(parsed: &Parsed, sub: &str) -> Option<PathBuf> {
    let mut seen_sub = false;
    let mut skip_next = false;
    for arg in &parsed.positional {
        if skip_next {
            skip_next = false;
            continue;
        }
        if !seen_sub {
            if arg == sub {
                seen_sub = true;
            }
            continue;
        }
        if arg.starts_with("--") {
            if !arg.contains('=') {
                skip_next = true;
            }
            continue;
        }
        return Some(PathBuf::from(arg));
    }
    None
}

/// Render a byte count as a short human string (B/K/M/G).
fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// `jetpack vendor [<dir>]` — write vendored + hash-pinned sources for every
/// source-built hangar object (D-BFS1 / T4). Each object's realized tree is
/// copied into `<dir>/<name>/` and a `<dir>/<name>.sha256` records the A4 output
/// hash, so a later build is reproducible offline from pinned sources.
pub(super) fn cmd_vendor(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let cwd = std::env::current_dir().unwrap_or_default();
    let vendor_dir = match parsed.positional.first() {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                p
            } else {
                cwd.join(p)
            }
        }
        None => cwd.join("vendor"),
    };
    let built: Vec<_> = Store::list(&roots)
        .into_iter()
        .filter(|e| e.envelope.provenance.contains("core-"))
        .collect();
    if built.is_empty() {
        theme.status("nothing to vendor: no source-built packages in the hangar.");
        return 0;
    }
    if std::fs::create_dir_all(&vendor_dir).is_err() {
        theme.error(
            "could not create the vendor directory",
            &vendor_dir.display().to_string(),
            "check write permissions here.",
        );
        return 1;
    }
    let mut count = 0;
    for e in &built {
        let dest = vendor_dir.join(&e.name);
        let _ = std::fs::remove_dir_all(&dest);
        if copy_dir(std::path::Path::new(&e.out), &dest).is_err() {
            theme.error(
                "could not vendor a package",
                &format!("copying {} failed", e.out),
                "check disk space and permissions.",
            );
            return 1;
        }
        // Hash-pin: the A4 output hash is the reproducibility anchor.
        let pin = vendor_dir.join(format!("{}.sha256", e.name));
        let _ = std::fs::write(&pin, format!("{}\n", e.envelope.output_hash));
        theme.detail(&format!(
            "vendored {} ({})",
            theme.bold(&e.name),
            theme.gray(&e.envelope.output_hash)
        ));
        count += 1;
    }
    theme.status(&format!(
        "vendored {count} source-built package(s) with pinned hashes."
    ));
    0
}

/// `jetpack audit` — read the build provenance of every realized object
/// (D-BUILDSCOPE1 audit contract, T4): source ref + recipe/action identity,
/// output hash, platform, closure, and signed producer facts. **Executes
/// nothing** — a pure read of the hangar records, so it is safe to run against
/// untrusted builds.
pub(super) fn cmd_audit(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    let entries = match Store::list_checked(&roots) {
        Ok(entries) => entries,
        Err(error) => {
            theme.error(
                "could not read Hangar provenance",
                &error.to_string(),
                "repair the Hangar journal or metadata, then retry audit.",
            );
            return 2;
        }
    };
    if entries.is_empty() {
        theme.status("audit: hangar is empty, nothing to read.");
        return 0;
    }
    theme.status(&format!(
        "audit: {} realized object(s) (read-only, no build ran):",
        entries.len()
    ));
    for e in &entries {
        theme.detail(&format!("{}", theme.bold(&e.id)));
        theme.detail(&format!(
            "  provenance: {}",
            if e.envelope.provenance.is_empty() {
                "<none recorded>"
            } else {
                &e.envelope.provenance
            }
        ));
        theme.detail(&format!(
            "  output-hash: {}",
            theme.gray(&e.envelope.output_hash)
        ));
        theme.detail(&format!(
            "  platform:    {}",
            theme.gray(&e.envelope.platform)
        ));
        theme.detail(&format!(
            "  closure:     {}",
            if e.references.is_empty() {
                "<none recorded>".to_string()
            } else {
                e.references.join(",")
            }
        ));
        match Store::ProducerRecord::decode(&e.producer_record) {
            Ok(producer) => {
                for (label, key) in [
                    ("source", "cache.source"),
                    ("recipe", "action.recipe"),
                    ("action", "cache.action"),
                    ("builder", "cache.builder"),
                    ("sandbox", "cache.sandbox"),
                    ("policy", "cache.policy"),
                    ("reproducibility", "cache.reproducibility"),
                    ("toolchain", "toolchain_facts"),
                ] {
                    let value = match key {
                        "toolchain_facts" => producer.toolchain_facts.as_str(),
                        _ => producer
                            .facts
                            .get(key)
                            .map(String::as_str)
                            .unwrap_or("<missing>"),
                    };
                    theme.detail(&format!("  {label}:       {value}"));
                }
            }
            Err(error) => theme.detail(&format!("  provenance:   <invalid producer record: {error}>")),
        }
    }
    0
}

/// Recursively copy a directory tree (std-only, preserves Unix modes).
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&from)?.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// `jetpack clean` — collect stale hangar objects and optimize owned bytes.
pub(super) fn cmd_clean(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    match Store::clean_plan(&roots) {
        Ok(plan) if plan.is_empty() => {
            theme.status("hangar clean plan: nothing to change.");
            return 0;
        }
        Ok(plan) => {
            theme.status("Plan hangar clean");
            let name_w = 16;
            if plan.removed_objects > 0 {
                theme.plan_row(
                    Output::PlanMark::Remove,
                    "stale-objects",
                    name_w,
                    &format!("{} object(s)", plan.removed_objects),
                    "removed",
                );
                theme.detail(&format!("would free {}", human_bytes(plan.removed_bytes)));
            }
            if plan.removed_receipts > 0 {
                theme.plan_row(
                    Output::PlanMark::Remove,
                    "orphan-receipts",
                    name_w,
                    &format!("{} receipt(s)", plan.removed_receipts),
                    "removed",
                );
                theme.detail(&format!(
                    "would free {}",
                    human_bytes(plan.removed_receipt_bytes)
                ));
            }
            if plan.swept_tmp > 0 {
                theme.plan_row(
                    Output::PlanMark::Remove,
                    "build-scratch",
                    name_w,
                    &format!("{} item(s)", plan.swept_tmp),
                    "removed",
                );
                theme.detail(&format!("would free {}", human_bytes(plan.swept_tmp_bytes)));
            }
            if plan.optimized_files > 0 {
                theme.plan_row(
                    Output::PlanMark::Change,
                    "deduplicate",
                    name_w,
                    &format!("{} file(s)", plan.optimized_files),
                    "hardlinked",
                );
                theme.detail(&format!(
                    "would save {}",
                    human_bytes(plan.optimized_bytes)
                ));
            }
            if !theme.confirm_apply(parsed.flags.assume_yes) {
                return 0;
            }
        }
        Err(e) => {
            theme.error(
                "could not plan the hangar clean",
                &format!("{e}"),
                "check permissions on the hangar root.",
            );
            return 1;
        }
    }
    match Store::clean(&roots) {
        Ok(report) => {
            theme.ok(&format!(
                "cleaned hangar: removed {} stale object(s) and {} orphan receipt(s), freed {}, swept {} scratch item(s), optimized {} file(s)",
                report.removed_objects,
                report.removed_receipts,
                human_bytes(
                    report.removed_bytes + report.removed_receipt_bytes + report.swept_tmp_bytes,
                ),
                report.swept_tmp,
                report.optimized_files
            ));
            if report.optimized_bytes > 0 {
                theme.detail(&format!(
                    "optimized duplicate Jet-owned files: saved {}",
                    human_bytes(report.optimized_bytes)
                ));
            }
            0
        }
        Err(e) => {
            theme.error(
                "could not clean the hangar",
                &format!("{e}"),
                "check permissions on the hangar root.",
            );
            1
        }
    }
}

pub(super) fn auto_clean_after_success(theme: &Theme, roots: &Roots) {
    match Store::maybe_auto_clean(roots) {
        Ok(Some(report)) if !report.is_empty() => theme.detail(&format!(
            "auto-cleaned hangar: removed {} stale object(s) and {} orphan receipt(s), swept {} scratch item(s), optimized {} file(s)",
            report.removed_objects,
            report.removed_receipts,
            report.swept_tmp,
            report.optimized_files
        )),
        Ok(_) => {}
        Err(e) => theme.detail(&theme.gray(&format!("auto-clean skipped: {e}"))),
    }
}
