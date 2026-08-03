//! publish / vendor / audit / sbom / yank supply-chain subcommand handlers (E2-M8).

use std::fs;
use std::path::{Path, PathBuf};
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

/// Run the same project test command that a package author runs locally.
/// Publishing must not turn successful semantic checking into a fake test
/// result: the generated test harnesses must compile and execute before the
/// immutable registry mutation below.
fn run_publish_tests(root: &Path, entry: &Path) -> bool {
    if !entry.is_file() {
        eprintln!(
            "  tests: failed — no project entry `{}` was found",
            entry.display()
        );
        return false;
    }

    let jet_bin = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("  tests: failed — couldn't locate the running Jet executable: {error}");
            return false;
        }
    };

    let entry_arg = entry.to_string_lossy().into_owned();
    let output = match Command::new(jet_bin)
        .args(["test", entry_arg.as_str()])
        .current_dir(root)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!("  tests: failed — couldn't start `jet test`: {error}");
            return false;
        }
    };

    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if output.status.success() {
        println!("  tests: ok");
        true
    } else {
        eprintln!(
            "  tests: failed (`jet test` exited with {})",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "a signal".to_string())
        );
        false
    }
}

fn render_signing_diagnostic(diagnostic: &jet::Diagnostics::Diagnostic) {
    if diagnostic.code == "E1292" {
        eprint!("{}", jet::Publish::Sign::render_e1292());
    } else {
        eprint!(
            "{}",
            jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", std::slice::from_ref(diagnostic))
        );
    }
}

/// `jet registry publish [--force]` — pre-publish gate + SemVer API diff.
///
/// D-PKGS4 (amended): must run `jet build` + `jet test` locally first.
/// Submits only when both pass (`--force` overrides with a warning).
/// Also checks that a non-major version bump does not break public API (E2601).
/// After the gate passes it pushes an index entry to the git registry (card c56):
/// clone/pull the sparse index, append the version line, commit, push. Version
/// immutability (D-VERSION1) is enforced here (E1234).
pub(crate) fn run_publish(force: bool, no_sign: bool, mode: OutputMode) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet registry publish` inside a project",
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
                " Fix: commit or stash all uncommitted changes, then run `jet registry publish` again."
            );
            eprintln!("      use `jet registry publish --force` to bypass with an explicit warning banner.");
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

    // Registry metadata is always signed. `--no-sign` only disables the
    // optional author signature on the package entry.
    let registry = jet::Publish::resolve_publish_registry();
    let generated_key = if !jet::Publish::Sign::key_exists(&registry.name) {
        match jet::Publish::Sign::keygen(&registry.name, false) {
            Ok(generated) => Some(generated),
            Err(diagnostic) => {
                render_signing_diagnostic(&diagnostic);
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        None
    };
    let registry_public = match jet::Publish::Sign::read_public_key(&registry.name) {
        Some(public) => public,
        None => {
            eprintln!("error: registry signing key has no public-key file");
            exit(ExitCodes::USER_ERROR);
        }
    };
    if let Err(error) = jet::Publish::ensure_registry_root_key(&registry.name, &registry_public) {
        eprintln!("error: registry root-key pin failed: {error}");
        exit(ExitCodes::USER_ERROR);
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
                eprintln!("\n use `jet registry publish --force` to bypass this gate with a warning banner");
                exit(ExitCodes::USER_ERROR);
            }
            false
        } else {
            println!("  build: ok");
            true
        }
    } else {
        eprintln!(
            "  build: failed — no project entry `{}` was found",
            entry_path.display()
        );
        false
    };

    // Pre-publish gate step 2: run the project's real test command before any
    // immutable registry index mutation.
    println!("[2/3] checking tests ...");
    let tests_ok = run_publish_tests(&root, &entry_path);

    // Pre-publish gate step 3: SemVer API diff.
    println!("[3/3] checking public API ...");
    // For the diff we need the previous version's public API. In v1, without a live
    // registry we cannot fetch the old version; we report that the check is advisory
    // (would fire on an actual publish to the registry which has the old version).
    // We still extract the current API so the output shows what would be published.
    let current_api = jet::Publish::extract_public_api_for_package("", &entry_str, name);
    println!("  public API surface: {} items", current_api.len());
    for item in &current_api {
        println!("    {} {}", item.kind, item.name);
    }

    // D-SUPPLY1 Step 3: local SemVer gate (E1218). If a frozen public-API
    // snapshot from a previous release exists (`.jet/cache/api/<name>.api`),
    // diff the current surface against it. A breaking change under a non-major
    // bump is refused unless `--force`.
    if let Some(prev) = jet::Publish::ApiFreeze::load_snapshot(&root, name) {
        let mut old_api: Vec<jet::Publish::ApiItem> = prev
            .funcs
            .iter()
            .map(|f| jet::Publish::ApiItem {
                kind: "fn".to_string(),
                name: f.name.clone(),
                signature: f.signature.clone(),
            })
            .collect();
        let mut current_fns: Vec<jet::Publish::ApiItem> = current_api
            .iter()
            .filter(|i| i.kind == "fn")
            .cloned()
            .collect();
        if prev.api_version < jet::Publish::ApiFreeze::API_SNAPSHOT_VERSION {
            for item in &mut old_api {
                item.name = jet::Publish::ApiFreeze::legacy_api_name(&item.name).to_string();
                item.signature = jet::Publish::ApiFreeze::legacy_api_signature(&item.signature);
            }
            for item in &mut current_fns {
                item.name = jet::Publish::ApiFreeze::legacy_api_name(&item.name).to_string();
                item.signature = jet::Publish::ApiFreeze::legacy_api_signature(&item.signature);
            }
        }
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
                eprintln!(" use `jet registry publish --force` to override with a warning banner");
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

    // D-MIGRATE1: snapshot `#PublishedSchema` structs at release time.
    let snap_count = jet::Publish::write_schema_snapshots_for_entry(&root, &entry_str, version);
    if snap_count > 0 {
        println!(
            "  schema: {} #PublishedSchema snapshot(s) updated in .jet/cache/schema/",
            snap_count
        );
    }

    // S2/D-MEM1: snapshot this publish's public-fn surface unconditionally —
    // pub-metadata semver diffing (feeds the next publish's E1218 check above),
    // no `api:` opt-in gate anymore.
    match jet::Publish::ApiFreeze::write_api_snapshot_for_entry(&root, &entry_str, name, version) {
        Some(n) => println!(
            "  api: {} public fn signature(s) snapshotted in .jet/cache/api/{}.api",
            n, name
        ),
        None => eprintln!("warning: could not snapshot public API (entry didn't load); skipping"),
    }

    if !build_ok || !tests_ok {
        if force {
            eprintln!("warning [--force]: pre-publish gate failed but continuing anyway.");
            eprintln!("  this publish would be rejected by a registry that enforces D-PKGS4.");
        } else {
            exit(ExitCodes::USER_ERROR);
        }
    }

    println!("\nok: `{}` v{} passes the pre-publish gate.", name, version);

    // c56 (D-JPK-CACHE1=A / D-VERSION1=A): push the index entry to the git
    // registry. The registry is a git repo — publishing is: clone/pull the
    // sparse index, append the version line, commit, push.
    println!("publishing to registry index `{}` ...", registry.url);

    let source_repo = match jet::Publish::ensure_index_clone(&registry) {
        Ok(r) => r,
        Err(d) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[d])
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Version immutability (D-VERSION1): refuse to overwrite a published,
    // non-yanked version. This is the enforcement point the decision promised
    // but couldn't land without a real push target.
    match jet::Publish::find_published(&source_repo, name, version) {
        Ok(Some(existing)) if !existing.yanked => {
            eprint!(
                "{}",
                jet::render_diagnostics(
                    jet::Syntax::PAYLOAD_FILE,
                    "",
                    &[jet::Publish::e1234(name, version)]
                )
            );
            exit(ExitCodes::USER_ERROR);
        }
        Ok(_) => {}
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    let (content_hash, fingerprint) = publish_index_hashes(&root, name);

    let checkout = match jet::Publish::prepare_publish_checkout(&registry) {
        Ok(checkout) => checkout,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let repo = checkout.path();

    // The source artifact and its index line are one registry transaction.
    // Stage and hash the artifact before touching metadata so a fresh machine
    // can consume the same source bytes rather than an index-only promise.
    if let Err(error) = jet::Publish::publish_artifact(
        repo,
        &root,
        name,
        version,
        &content_hash,
    ) {
        let diagnostic = jet::Publish::e2607("registry source artifact", &error.to_string());
        eprint!(
            "{}",
            jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
        );
        exit(ExitCodes::USER_ERROR);
    }

    // c146 (D-PKGSIGN1): tier-A author signing. Auto-keygen silently on first
    // publish, then sign the content hash; `--no-sign` opts out (tier-B checksum
    // still applies unconditionally). The public key is TOFU-pinned into the
    // index on the FIRST published version of a package and never rewritten.
    let (public_key, signature) = if no_sign {
        println!("  signing: skipped (--no-sign); tier-B checksum still applies.");
        (String::new(), String::new())
    } else {
        let reg = &registry.name;
        if let Some((seed_path, _pub_path, pub_hex)) = generated_key {
            println!("  signing: generated a new key for registry `{}`.", reg);
            println!("    public key: {}", pub_hex);
            println!(
                "    `jet registry key backup` writes this to {} — losing it means losing your ability to publish signed updates.",
                seed_path.display()
            );
        }
        let (seed_path, _pub_path) = jet::Publish::Sign::key_paths(reg);
        let sig = match jet::Publish::Sign::sign(&seed_path, &content_hash) {
            Ok(s) => s,
            Err(d) => {
                eprint!(
                    "{}",
                    jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[d])
                );
                exit(ExitCodes::USER_ERROR);
            }
        };
        // TOFU: record the public key only if this package has none pinned yet.
        let entries = match jet::Publish::Index::read_entries(repo, name) {
            Ok(entries) => entries,
            Err(error) => {
                let diagnostic = jet::Publish::e2607("registry index", &error.to_string());
                eprint!(
                    "{}",
                    jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
                );
                exit(ExitCodes::USER_ERROR);
            }
        };
        let already_pinned = jet::Publish::Index::pinned_public_key(&entries).is_some();
        let pub_field = if already_pinned {
            String::new()
        } else {
            jet::Publish::Sign::read_public_key(reg).unwrap_or_default()
        };
        println!("  signing: signed content hash with the `{}` key.", reg);
        (pub_field, sig)
    };

    let entry = jet::Publish::IndexEntry {
        name: name.clone(),
        version: version.clone(),
        content_hash,
        fingerprint,
        yanked: false,
        public_key,
        signature,
    };
    if let Err(e) = jet::Publish::Index::write_index_entry(repo, &entry) {
        eprintln!("error: couldn't write the registry index entry: {}", e);
        exit(ExitCodes::USER_ERROR);
    }
    let artifact = jet::Publish::Registry::artifact_path(repo, name, version)
        .unwrap_or_else(|error| {
            eprintln!("error: invalid registry artifact path: {error}");
            exit(ExitCodes::USER_ERROR);
        });
    let index = jet::Publish::Index::index_entry_path(repo, name).unwrap_or_else(|error| {
        eprintln!("error: invalid registry index path: {error}");
        exit(ExitCodes::USER_ERROR);
    });
    let metadata = match jet::Publish::refresh_registry_metadata(repo, &registry.name) {
        Ok(metadata) => metadata,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let mut publish_paths = vec![artifact, index];
    publish_paths.extend(metadata.paths);
    if let Err(d) = jet::Publish::push_index(
        &registry,
        repo,
        &format!("publish {} {}", name, version),
        &publish_paths,
        Some(&entry),
    ) {
        eprint!(
            "{}",
            jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[d])
        );
        exit(ExitCodes::USER_ERROR);
    }

    println!("ok: published `{}` v{} to {}", name, version, registry.url);
}

/// Source hash + plan fingerprint for the index entry. Reuses the lock's
/// recorded values (the exact `LockedPackage` fields) when a lock exists; falls
/// back to hashing the source tree so `jet registry publish` works before a first
/// `jet fetch`.
fn publish_index_hashes(root: &std::path::Path, name: &str) -> (String, String) {
    if let Some(lock) = jet::Lock::load(root) {
        if let Some(pkg) = lock
            .packages
            .iter()
            .find(|p| p.name == name || matches!(p.source, jet::Lock::LockSource::Root))
        {
            if !pkg.fingerprint.is_empty() {
                let content_hash = pkg
                    .content_hash
                    .clone()
                    .filter(|hash| !hash.is_empty())
                    .unwrap_or_else(|| jet::SHA256::tree_hash(root));
                return (content_hash, pkg.fingerprint.clone());
            }
        }
    }
    let tree_hash = jet::SHA256::tree_hash(root);
    let fingerprint = jet::Lock::compute_fingerprint(&tree_hash, &[], "");
    (tree_hash, fingerprint)
}

/// `jet registry keygen [--registry <name>] [--force]` — create the Ed25519 signing key
/// used to sign published packages (c146, D-PKGSIGN1). Refuses to overwrite an
/// existing key without `--force` (E1248).
pub(crate) fn run_keygen(registry: Option<&str>, force: bool) {
    let reg = registry.unwrap_or(jet::Publish::Sign::DEFAULT_REGISTRY);
    match jet::Publish::Sign::keygen(reg, force) {
        Ok((seed_path, pub_path, pub_hex)) => {
            println!("created a signing key for registry `{}`.", reg);
            println!("  public key:  {}", pub_hex);
            println!("  secret key:  {}", seed_path.display());
            println!("  public file: {}", pub_path.display());
            println!(
                "`jet registry key backup` writes this to {} — losing it means losing your ability to publish signed updates.",
                seed_path.display()
            );
        }
        Err(d) => {
            render_signing_diagnostic(&d);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet registry key backup [<dest>] [--registry <name>]` — copy the secret signing key
/// to `<dest>` (default `./jet-signing-key.backup`) so the publisher can store
/// it somewhere safe (c146, D-PKGSIGN1). The backup is not encrypted — it is the
/// user's own copy to protect.
pub(crate) fn run_key_backup(dest: Option<&str>, registry: Option<&str>) {
    let reg = registry.unwrap_or(jet::Publish::Sign::DEFAULT_REGISTRY);
    let (seed_path, _pub_path) = jet::Publish::Sign::key_paths(reg);
    if !seed_path.is_file() {
        eprintln!(
            "error: no signing key for registry `{}` — run `jet registry keygen` first.",
            reg
        );
        exit(ExitCodes::USER_ERROR);
    }
    let dest = dest
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./jet-signing-key.backup"));
    match fs::copy(&seed_path, &dest) {
        Ok(_) => {
            println!("backed up the `{}` signing key to {}", reg, dest.display());
            println!(
                "warning: store this file somewhere safe (e.g. a password manager). Anyone who has it can publish signed updates as you."
            );
        }
        Err(e) => {
            eprintln!(
                "error: couldn't copy the signing key to {}: {}",
                dest.display(),
                e
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet registry vendor [--vendor-dir <path>]` — copy all resolved dependencies into a
/// local vendor tree for offline builds (D-SUPPLY1). The default location is
/// `<project>/vendor`; `--vendor-dir` relocates it (relative paths resolve
/// against the project root).
pub(crate) fn run_vendor(vendor_dir: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet registry vendor` inside a project",
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

/// `jet inspect audit [--advisory-db <path>]` — check the lockfile against an advisory DB.
pub(crate) fn run_audit(db_path: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet inspect audit` inside a project",
    );

    let lock = match jet::Lock::load(&root) {
        Some(l) => l,
        None => {
            println!("audit: no lockfile found — run `jet fetch` first");
            exit(ExitCodes::OK);
        }
    };

    // Load the explicitly selected local database, the environment-selected
    // local database, or the project-local default. No network lookup happens
    // in an audit command.
    let configured_path = db_path
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("JET_ADVISORY_DB").map(PathBuf::from))
        .or_else(|| {
            let path = root.join(".jet").join("advisories.db");
            path.is_file().then_some(path)
        });
    let db_text = if let Some(path) = configured_path.as_deref() {
        match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: couldn't read advisory database `{}`: {}", path, e);
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        String::new()
    };

    let advisories = match jet::Publish::parse_advisory_db(&db_text) {
        Ok(advisories) => advisories,
        Err(diagnostic) => {
            let advisory_source = configured_path.as_deref().map_or_else(
                || "advisory database".to_string(),
                |path| path.display().to_string(),
            );
            eprint!(
                "{}",
                jet::render_diagnostics(
                    &advisory_source,
                    &db_text,
                    &[diagnostic]
                )
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    if advisories.is_empty() && configured_path.is_none() {
        println!(
            "audit: no advisory database configured.\n\
             pass --advisory-db <path>, set JET_ADVISORY_DB, or add `.jet/advisories.db`\n\
             to check against a local database."
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

/// `jet inspect sbom [--cyclonedx]` — emit a software bill of materials from the lockfile.
pub(crate) fn run_sbom(cyclonedx: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet inspect sbom` inside a project",
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

/// `jet registry yank <version> [--message <reason>]` — mark a published version as yanked.
///
/// D-VERSION1=A (version immutability): a published version can't be re-published;
/// `jet registry yank` flips its `yanked` flag in the registry index in place (never
/// deletes the line), then commits and pushes (card c56).
pub(crate) fn run_yank(version: Option<&str>, message: Option<&str>) {
    let version = match version {
        Some(v) if !v.is_empty() => v,
        _ => {
            eprintln!("Error [E2606]: `jet registry yank` requires a version argument.");
            eprintln!(" Why: a yank marks one specific published version as deprecated;");
            eprintln!("      without a version the command doesn't know which one to yank.");
            eprintln!(" Fix: run `jet registry yank <version>`, e.g. `jet registry yank 1.2.3`.");
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
        "error: no `pkg.jet` found — run `jet registry yank` inside a project",
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

    // c56 (D-VERSION1=A): a yank flips the `yanked` flag on the version's line
    // in the registry index — the line is never deleted, so the version number
    // stays taken (immutable) but drops out of new resolution.
    let registry = jet::Publish::resolve_publish_registry();
    if let Err(error) = jet::Publish::read_registry_root_key(&registry.name) {
        eprintln!("error: registry root-key pin is unavailable: {error}");
        exit(ExitCodes::USER_ERROR);
    }
    let checkout = match jet::Publish::prepare_publish_checkout(&registry) {
        Ok(checkout) => checkout,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let repo = checkout.path();
    match jet::Publish::Index::mark_yanked(repo, name, version) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!(
                "error: `{}` v{} is not published in the registry index — nothing to yank.",
                name, version
            );
            eprintln!(
                " Fix: run `jet registry publish` for the version first, or check the version number."
            );
            exit(ExitCodes::USER_ERROR);
        }
        Err(e) => {
            eprintln!("error: couldn't update the registry index: {}", e);
            exit(ExitCodes::USER_ERROR);
        }
    }

    let entry = match jet::Publish::Index::find_entry(repo, name, version) {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            eprintln!("error: yanked registry entry disappeared during publication");
            exit(ExitCodes::USER_ERROR);
        }
        Err(error) => {
            eprintln!("error: couldn't read the yanked registry entry: {error}");
            exit(ExitCodes::USER_ERROR);
        }
    };
    let index = jet::Publish::Index::index_entry_path(repo, name).unwrap_or_else(|error| {
        eprintln!("error: invalid registry index path: {error}");
        exit(ExitCodes::USER_ERROR);
    });
    let metadata = match jet::Publish::refresh_registry_metadata(repo, &registry.name) {
        Ok(metadata) => metadata,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[diagnostic])
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let mut yank_paths = vec![index];
    yank_paths.extend(metadata.paths);
    if let Err(d) = jet::Publish::push_index(
        &registry,
        repo,
        &format!("yank {} {}", name, version),
        &yank_paths,
        Some(&entry),
    ) {
        eprint!(
            "{}",
            jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, "", &[d])
        );
        exit(ExitCodes::USER_ERROR);
    }

    println!("ok: yanked `{}` v{} in {}", name, version, registry.url);
    if let Some(msg) = message {
        println!("  reason: {}", msg);
    }
    println!("  the version stays reserved (immutable); it is hidden from new resolution.");
}
