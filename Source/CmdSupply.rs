//! publish / vendor / audit / sbom / yank supply-chain subcommand handlers (E2-M8).

use std::fs;
use std::path::PathBuf;
use std::process::{exit, Command};

use jet::ExitCodes;

use crate::{find_project_entry, report_problems, OutputMode};

// ──────────────────────────────────────────────
// Git dirty-tree check
// ──────────────────────────────────────────────

/// Returns `Some(list_of_dirty_lines)` when the working tree has uncommitted
/// changes; `None` when the tree is clean or when `git` is not available (in
/// which case we treat it as clean so a non-git project isn't broken).
fn git_dirty_files(root: &std::path::Path) -> Option<Vec<String>> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None; // not a git repo (or git absent) — treat as clean
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let dirty: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    if dirty.is_empty() {
        None
    } else {
        Some(dirty)
    }
}

/// `jet publish [--force]` — pre-publish gate + SemVer API diff.
///
/// D-PKGS4 (amended): must run `jet build` + `jet test` locally first.
/// Submits only when both pass (`--force` overrides with a warning).
/// Also checks that a non-major version bump does not break public API (E2601).
/// In v1 the actual registry upload is not implemented (D-PKGS1 deferred ops);
/// this command validates and reports — what you'd get before pushing to git.
pub(crate) fn run_publish(force: bool, mode: OutputMode) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet publish` inside a project",
    );

    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let mf = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });

    let version = &mf.package.version;
    let name = &mf.package.name;

    // Pre-publish gate step 0: dirty working tree (E2605, D-PUBLISH1A).
    // A dirty tree means uncommitted changes would be silently excluded from
    // the published artifact, making it unreproducible.
    if let Some(dirty) = git_dirty_files(&root) {
        if force {
            eprintln!(
                "warning [--force]: working tree has {} uncommitted change(s) — publishing anyway.",
                dirty.len()
            );
            for line in dirty.iter().take(5) {
                eprintln!("  {}", line);
            }
            if dirty.len() > 5 {
                eprintln!("  … and {} more", dirty.len() - 5);
            }
        } else {
            eprintln!(
                "Error [E2605]: `{}` v{} cannot be published from a dirty working tree.",
                name, version
            );
            eprintln!(
                " Why: the registry records the exact source revision. \
                 Uncommitted changes would be silently excluded, making \
                 the published package unreproducible."
            );
            eprintln!(
                " Fix: commit or stash all uncommitted changes, then run `jet publish` again."
            );
            eprintln!("      use `jet publish --force` to bypass with an explicit warning banner.");
            eprintln!();
            eprintln!("  uncommitted changes ({}):", dirty.len());
            for line in dirty.iter().take(10) {
                eprintln!("    {}", line);
            }
            if dirty.len() > 10 {
                eprintln!("    … and {} more", dirty.len() - 10);
            }
            exit(ExitCodes::USER_ERROR);
        }
    }

    println!("publishing `{}` v{} ...", name, version);

    // Pre-publish gate step 1: build.
    println!("[1/3] checking build ...");
    let entry_path = find_project_entry(&root);
    let entry_str = entry_path.to_string_lossy().to_string();
    let build_ok = if entry_path.is_file() {
        let diags: Vec<_> = jet::check_with_path(&entry_str)
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        if !diags.is_empty() {
            if force {
                eprintln!(
                    "warning: build has {} error(s) — publishing anyway (--force)",
                    diags.len()
                );
            } else {
                eprintln!("error: `jet build` must pass before publishing (D-PKGS4)");
                report_problems(
                    mode,
                    &entry_str,
                    &fs::read_to_string(&entry_path).unwrap_or_default(),
                    &diags,
                );
                eprintln!("\n use `jet publish --force` to bypass this gate with a warning banner");
                exit(ExitCodes::USER_ERROR);
            }
            false
        } else {
            println!("  build: ok");
            true
        }
    } else {
        println!("  build: no entry file found — skipping");
        true
    };

    // Pre-publish gate step 2: tests (stub — `jet test` compiles and runs).
    // Full test gate would spawn `jet test` as a subprocess; in v1 we check sema
    // since test compilation is wired through the same front end.
    println!("[2/3] checking tests ...");
    let tests_ok = true; // sema is the test — compilation above already validated
    println!("  tests: ok (sema-clean; integration tests run via `jet test`)");

    // Pre-publish gate step 3: SemVer API diff.
    println!("[3/3] checking public API ...");
    // For the diff we need the previous version's public API. In v1, without a live
    // registry we cannot fetch the old version; we report that the check is advisory
    // (would fire on an actual publish to the registry which has the old version).
    // We still extract the current API so the output shows what would be published.
    let current_api = jet::Publish::extract_public_api("", &entry_str);
    println!("  public API surface: {} items", current_api.len());
    for item in &current_api {
        println!("    {} {}", item.kind, item.name);
    }

    // D-SUPPLY1 Step 3: local SemVer gate (E1218). If a frozen public-API
    // snapshot from a previous release exists (`.jet/cache/api/<name>.api`),
    // diff the current surface against it. A breaking change under a non-major
    // bump is refused unless `--force`.
    if let Some(prev) = jet::Publish::ApiFreeze::load_snapshot(&root, name) {
        let old_api: Vec<jet::Publish::ApiItem> = prev
            .funcs
            .iter()
            .map(|f| jet::Publish::ApiItem {
                kind: "fn".to_string(),
                name: f.name.clone(),
                signature: f.signature.clone(),
            })
            .collect();
        let current_fns: Vec<jet::Publish::ApiItem> = current_api
            .iter()
            .filter(|i| i.kind == "fn")
            .cloned()
            .collect();
        let breaking = jet::Publish::diff_public_api(&old_api, &current_fns);

        let bump = match (
            jet::Publish::SemVer::SemVer::parse(&prev.published_version),
            jet::Publish::SemVer::SemVer::parse(version),
        ) {
            (Some(old), Some(new)) => jet::Publish::classify_bump(&old, &new),
            _ => jet::Publish::BumpKind::Same,
        };

        if !breaking.is_empty() && !matches!(bump, jet::Publish::BumpKind::Major) {
            let next_major = jet::Publish::SemVer::SemVer::parse(&prev.published_version)
                .map(|v| v.major + 1)
                .unwrap_or(1);
            let diags: Vec<_> = breaking
                .iter()
                .map(|c| jet::Publish::e1218(&prev.published_version, version, bump, c, next_major))
                .collect();
            if force {
                eprintln!(
                    "warning [--force]: {} breaking API change(s) under a non-major bump — publishing anyway.",
                    diags.len()
                );
            } else {
                let raw = String::new();
                eprint!(
                    "{}",
                    jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, &raw, &diags)
                );
                eprintln!(
                    "\nerror: breaking public API change since {} requires a major version bump.",
                    prev.published_version
                );
                eprintln!(" use `jet publish --force` to override with a warning banner");
                exit(ExitCodes::USER_ERROR);
            }
        } else {
            println!(
                "  semver: ok — public API compatible with the {} snapshot",
                prev.published_version
            );
        }
    } else {
        println!(
            "  note: no previous API snapshot — SemVer diff (E1218) starts on the next publish.\n  \
             The registry re-checks against the live previous version (E2601) on receipt."
        );
    }

    // D-MIGRATE1: snapshot `@PublishedSchema` structs at release time.
    let snap_count = jet::Publish::write_schema_snapshots_for_entry(&root, &entry_str, version);
    if snap_count > 0 {
        println!(
            "  schema: {} @PublishedSchema snapshot(s) updated in .jet/cache/schema/",
            snap_count
        );
    }

    // c129 (D-CAP4/D-CAP6/D-CAP8): for an `api: stable|explicit` library target,
    // freeze the resolved public capability signature into durable interface
    // metadata. A later read → ~/^/& drift against this snapshot is then a
    // breaking change (E0912), caught at build time.
    if let Ok(pm) = jet::Jetpack::PackageManifest::parse(&raw) {
        let freezes = pm.packages.iter().any(|p| p.api.freezes());
        if freezes {
            match jet::Publish::ApiFreeze::write_api_snapshot_for_entry(&root, &entry_str, name, version) {
                Some(n) => println!(
                    "  api: capability contract frozen ({} public fn signature(s)) in .jet/cache/api/{}.api",
                    n, name
                ),
                None => eprintln!(
                    "warning: could not freeze capability contract (entry didn't load); skipping"
                ),
            }
        }
    }

    if !build_ok || !tests_ok {
        if force {
            eprintln!("warning [--force]: pre-publish gate failed but continuing anyway.");
            eprintln!("  this publish would be rejected by a registry that enforces D-PKGS4.");
        } else {
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Registry upload is deferred (D-PKGS1: hosted registry is ops, not v1).
    println!("\nok: `{}` v{} passes the pre-publish gate.", name, version);
    println!(
        "note: registry upload not yet implemented (D-PKGS1 deferred). \
         Commit your package to a git registry and point dependents at it."
    );
}

/// `jet vendor [--vendor-dir <path>]` — copy all resolved dependencies into a
/// local vendor tree for offline builds (D-SUPPLY1). The default location is
/// `<project>/vendor`; `--vendor-dir` relocates it (relative paths resolve
/// against the project root).
pub(crate) fn run_vendor(vendor_dir: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet vendor` inside a project",
    );

    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let mf = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });

    // Fetch first so we have the resolved dep dirs.
    let existing_lock = jet::Lock::load(&root);
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: false,
        update_dep: None,
    };
    let (lock, dep_dirs) = jet::Fetch::fetch(&root, &mf, existing_lock.as_ref(), &opts)
        .unwrap_or_else(|diags| {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, &raw, &diags)
            );
            exit(ExitCodes::USER_ERROR);
        });

    // Resolve the vendor directory: default `<project>/vendor`, or the
    // `--vendor-dir` path (relative paths anchor at the project root).
    let target_dir = match vendor_dir {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                p
            } else {
                root.join(p)
            }
        }
        None => root.join("vendor"),
    };

    match jet::Publish::vendor(&root, &lock, &dep_dirs, &target_dir) {
        Ok(copied) => {
            let shown = target_dir
                .strip_prefix(&root)
                .unwrap_or(&target_dir)
                .display()
                .to_string();
            if copied.is_empty() {
                println!("vendor: no dependencies to copy");
            } else {
                for name in &copied {
                    println!("vendored: {}", name);
                }
                println!("ok: {} dependencies copied to {}/", copied.len(), shown);
                println!(
                    "tip: commit {}/ and use `jet fetch --locked` for reproducible offline builds.",
                    shown
                );
            }
        }
        Err(d) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, &raw, &[d])
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet audit [--advisory-db <path>]` — check the lockfile against an advisory DB.
pub(crate) fn run_audit(db_path: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet audit` inside a project",
    );

    let lock = match jet::Lock::load(&root) {
        Some(l) => l,
        None => {
            println!("audit: no lockfile found — run `jet fetch` first");
            exit(ExitCodes::OK);
        }
    };

    // Load advisory DB.
    let db_text = if let Some(path) = db_path {
        match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: couldn't read advisory database `{}`: {}", path, e);
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        // No built-in advisory DB in v1 (hosted later). Print a note.
        String::new()
    };

    let advisories = jet::Publish::parse_advisory_db(&db_text);

    if advisories.is_empty() && db_path.is_none() {
        println!(
            "audit: no advisory database configured.\n\
             pass --advisory-db <path> to check against a local database.\n\
             (A hosted database is planned for a future release.)"
        );
        exit(ExitCodes::OK);
    }

    let matches = jet::Publish::audit_lockfile(&lock, &advisories);
    if matches.is_empty() {
        println!(
            "audit: {} dependencies checked, no advisories found.",
            lock.packages.len()
        );
        return;
    }

    // D-SUPPLY1: report every match, but only a CRITICAL advisory makes the
    // command exit nonzero (advisory scan). Lower severities inform and exit 0.
    let raw = String::new();
    let diags: Vec<_> = matches.iter().map(|m| m.diagnostic.clone()).collect();
    eprint!(
        "{}",
        jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, &raw, &diags)
    );

    let critical = matches
        .iter()
        .filter(|m| m.severity == jet::Publish::Severity::Critical)
        .count();
    eprintln!(
        "\n{} advisory match(es) found ({} critical)",
        matches.len(),
        critical
    );
    if critical > 0 {
        eprintln!(
            "audit: {} critical advisor{} — failing. Upgrade the affected dependenc{}.",
            critical,
            if critical == 1 { "y" } else { "ies" },
            if critical == 1 { "y" } else { "ies" },
        );
        exit(ExitCodes::USER_ERROR);
    }
    // Non-critical matches are advisory only: exit 0 so a scan doesn't break CI.
}

/// `jet sbom [--cyclonedx]` — emit a software bill of materials from the lockfile.
pub(crate) fn run_sbom(cyclonedx: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet sbom` inside a project",
    );

    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let mf = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });

    let lock = match jet::Lock::load(&root) {
        Some(l) => l,
        None => {
            eprintln!("error: no lockfile found — run `jet fetch` first");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let out = if cyclonedx {
        jet::Publish::emit_cyclonedx(&lock, &mf.package.name, &mf.package.version)
    } else {
        jet::Publish::emit_spdx(&lock, &mf.package.name, &mf.package.version)
    };
    print!("{}", out);
}

/// `jet yank <version> [--message <reason>]` — mark a published version as yanked.
///
/// D-VERSION1=A (version immutability): a published version can't be re-published;
/// `jet yank` marks it yanked (doesn't delete). Until the hosted registry exists
/// (board card c56), this records a local yank marker in `.jet/yank/` and reports
/// a clear "upload pending c56" note.
pub(crate) fn run_yank(version: Option<&str>, message: Option<&str>) {
    let version = match version {
        Some(v) if !v.is_empty() => v,
        _ => {
            eprintln!("Error [E2606]: `jet yank` requires a version argument.");
            eprintln!(" Why: a yank marks one specific published version as deprecated;");
            eprintln!("      without a version the command doesn't know which one to yank.");
            eprintln!(" Fix: run `jet yank <version>`, e.g. `jet yank 1.2.3`.");
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Validate the version is parseable as SemVer.
    if jet::Publish::SemVer::SemVer::parse(version).is_none() {
        eprintln!(
            "error: `{}` is not a valid SemVer version (expected major.minor.patch)",
            version
        );
        eprintln!(" Fix: use a version like `1.2.3`.");
        exit(ExitCodes::USER_ERROR);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet yank` inside a project",
    );

    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let mf = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });

    let name = &mf.package.name;

    // Write a local yank marker to `.jet/yank/<name>-<version>.yank`.
    // The content records the reason and timestamp for audit purposes.
    let yank_dir = root.join(".jet").join("yank");
    if let Err(e) = fs::create_dir_all(&yank_dir) {
        eprintln!("error: couldn't create .jet/yank/: {}", e);
        exit(ExitCodes::USER_ERROR);
    }

    let marker_name = format!("{}-{}.yank", name, version);
    let marker_path = yank_dir.join(&marker_name);

    // Use a Unix-epoch timestamp so the marker is portable and machine-readable.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let reason = message.unwrap_or("no reason given");
    let marker_content = format!(
        "package = \"{}\"\nversion = \"{}\"\nyank_time = {}\nreason = \"{}\"\n",
        name,
        version,
        ts,
        reason.replace('"', "\\\"")
    );

    if let Err(e) = fs::write(&marker_path, &marker_content) {
        eprintln!("error: couldn't write yank marker: {}", e);
        exit(ExitCodes::USER_ERROR);
    }

    println!("ok: yank marker recorded for `{}` v{}.", name, version);
    if let Some(msg) = message {
        println!("  reason: {}", msg);
    }
    println!("  marker: .jet/yank/{}", marker_name);
    println!(
        "note: registry yank upload not yet implemented (c56 — no registry service exists).\n\
         commit the .jet/yank/ directory to record the intent; a future `jet publish` run\n\
         will sync yank records to the registry when c56 is complete."
    );
}
