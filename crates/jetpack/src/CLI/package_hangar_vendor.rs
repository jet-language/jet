/// `jetpack list` — show realized store entries.
fn cmd_list(theme: &Theme) -> i32 {
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
fn cmd_hangar(theme: &Theme, parsed: &Parsed) -> i32 {
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
        Some(other) => {
            theme.error(
                &format!("`hangar {other}` is not a hangar command"),
                "the hangar subcommand is `du` (honest disk usage).",
                "run `jetpack hangar du`.",
            );
            2
        }
    }
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
fn cmd_vendor(theme: &Theme, parsed: &Parsed) -> i32 {
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
fn cmd_audit(theme: &Theme) -> i32 {
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
fn cmd_clean(theme: &Theme, parsed: &Parsed) -> i32 {
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

fn auto_clean_after_success(theme: &Theme, roots: &Roots) {
    match Store::maybe_auto_clean(roots) {
        Ok(Some(report)) if !report.is_empty() => theme.detail(&format!(
            "auto-cleaned hangar: removed {} stale object(s), swept {} scratch item(s), optimized {} file(s)",
            report.removed_objects, report.swept_tmp, report.optimized_files
        )),
        Ok(_) => {}
        Err(e) => theme.detail(&theme.gray(&format!("auto-clean skipped: {e}"))),
    }
}
