use super::parse::Parsed;
use crate::Output::{self, Theme};
use crate::Store::{self, Roots};
use std::path::PathBuf;

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

/// `jetpack hangar du` — honest per-object disk usage (U22 / D-JPK-GC1).
/// Source-built objects are counted like any other, so `du` never hides them.
///
/// Hangar Store v2 also exposes:
/// - `hangar ingest <dir> --name <n> [--version <v>] [--ref <r>]`
/// - `hangar verify <digest-or-id>`
/// - `hangar referrers <digest>`
/// - `hangar recover` — sweep crashed staging / `.partial` objects
pub(super) fn cmd_hangar(theme: &Theme, parsed: &Parsed) -> i32 {
    let sub = parsed.positional.first().map(String::as_str);
    match sub {
        Some("du") | None => {
            let roots = Store::resolve();
            let entries = Store::du(&roots);
            if entries.is_empty() {
                theme.status("hangar is empty.");
                return 0;
            }
            let mut total = 0u64;
            let mut built = 0usize;
            for e in &entries {
                total += e.bytes;
                if e.source_built {
                    built += 1;
                }
                let tag = if e.source_built { " (built)" } else { "" };
                theme.detail(&format!(
                    "{:>10}  {}{}",
                    human_bytes(e.bytes),
                    theme.bold(&e.id),
                    theme.gray(tag)
                ));
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
        Some("referrers") => cmd_hangar_referrers(theme, parsed),
        Some("recover") => {
            let roots = Store::resolve();
            match Store::recover_hangar_staging(&roots) {
                Ok(n) => {
                    theme.status(&format!("recovered {n} abandoned stage/partial item(s)"));
                    0
                }
                Err(e) => {
                    theme.error(
                        "could not recover hangar staging",
                        &e.to_string(),
                        "check permissions on the hangar root.",
                    );
                    2
                }
            }
        }
        Some(other) => {
            theme.error(
                &format!("`hangar {other}` is not a hangar command"),
                "hangar subcommands: `du`, `ingest`, `verify`, `referrers`, `recover`.",
                "run `jetpack hangar du`.",
            );
            2
        }
    }
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
            theme.error(
                "`hangar ingest` needs `--name`",
                "every hangar object has a package name in its record.",
                "pass `--name <pkg>`.",
            );
            return 2;
        }
    };
    let version = flag_value(parsed, "--version").unwrap_or_default();
    let platform_artifact_kind =
        flag_value(parsed, "--platform-artifact-kind").unwrap_or_default();
    let dir = match positional_path_after(parsed, "ingest") {
        Some(p) => p,
        None => {
            theme.error(
                "`hangar ingest` needs a source directory",
                "atomic staged ingest copies a local tree into the hangar.",
                "run `jetpack hangar ingest <dir> --name <pkg>`.",
            );
            return 2;
        }
    };
    let reference =
        flag_value(parsed, "--ref").unwrap_or_else(|| format!("path:{}", dir.display()));
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
            0
        }
        Err(err) => {
            err.report(theme);
            2
        }
    }
}

fn cmd_hangar_verify(theme: &Theme, parsed: &Parsed) -> i32 {
    let target = match positional_path_after(parsed, "verify") {
        Some(t) => t.to_string_lossy().into_owned(),
        None => {
            theme.error(
                "`hangar verify` needs an id or output digest",
                "verification re-hashes the hangar object and compares the envelope.",
                "run `jetpack hangar verify <id-or-sha256-…>`.",
            );
            return 2;
        }
    };
    let roots = Store::resolve();
    let entries = Store::list(&roots);
    let Some(entry) = entries
        .iter()
        .find(|e| e.id == target || e.envelope.output_hash == target)
    else {
        theme.error(
            &format!("no hangar object `{target}`"),
            "verify only checks realized hangar records.",
            "run `jetpack list` to see ids.",
        );
        return 2;
    };
    match Store::verify_hangar_object(&roots, entry) {
        Ok(()) => {
            theme.status(&format!("verified {}", entry.envelope.output_hash));
            0
        }
        Err(err) => {
            err.report(theme);
            2
        }
    }
}

fn cmd_hangar_referrers(theme: &Theme, parsed: &Parsed) -> i32 {
    let digest = match positional_path_after(parsed, "referrers") {
        Some(d) => d.to_string_lossy().into_owned(),
        None => {
            theme.error(
                "`hangar referrers` needs an output digest",
                "referrers lists objects that declare a dependency on this digest.",
                "run `jetpack hangar referrers sha256-…`.",
            );
            return 2;
        }
    };
    let roots = Store::resolve();
    let refs = Store::referrers_of(&roots, &digest);
    if refs.is_empty() {
        theme.status("no referrers.");
        return 0;
    }
    for r in refs {
        theme.detail(&r);
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
/// (D-BUILDSCOPE1 audit contract, T4): source ref + recipe id, output hash,
/// platform, and locked source hash. **Executes nothing** — a pure read of the
/// hangar records, so it is safe to run against untrusted builds.
pub(super) fn cmd_audit(theme: &Theme) -> i32 {
    let roots = Store::resolve();
    let entries = Store::list(&roots);
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
                "cleaned hangar: removed {} stale object(s), freed {}, swept {} scratch item(s), optimized {} file(s)",
                report.removed_objects,
                human_bytes(report.removed_bytes + report.swept_tmp_bytes),
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
            "auto-cleaned hangar: removed {} stale object(s), swept {} scratch item(s), optimized {} file(s)",
            report.removed_objects, report.swept_tmp, report.optimized_files
        )),
        Ok(_) => {}
        Err(e) => theme.detail(&theme.gray(&format!("auto-clean skipped: {e}"))),
    }
}
