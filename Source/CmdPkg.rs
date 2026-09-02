//! add / remove / fetch / update / store / gc package-manager subcommand
//! handlers (M12.1).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;

use crate::flag_value;
struct PackageInput {
    path: PathBuf,
    raw: String,
    inline: Option<jet::Package::InlinePackageBlock>,
    manifest: jet::Manifest::Manifest,
}

fn package_input(root: &Path) -> PackageInput {
    let entry = crate::find_project_entry(root);
    let entry_raw = fs::read_to_string(&entry).unwrap_or_else(|error| {
        crate::cli_error!("E2105", "couldn't read {}: {}", entry.display(), error);
        exit(ExitCodes::USER_ERROR);
    });
    let inline = jet::Package::extract_inline_package(&entry_raw).unwrap_or_else(|error| {
        eprint!(
            "{}",
            jet::render_diagnostics(
                &entry.display().to_string(),
                &entry_raw,
                &[error.diagnostic()],
            )
        );
        exit(ExitCodes::USER_ERROR);
    });
    let facts = jet::Loader::package_facts_for_entry(&entry)
        .unwrap_or_else(|diagnostics| {
            eprint!(
                "{}",
                jet::render_diagnostics(
                    &entry.display().to_string(),
                    &entry_raw,
                    &diagnostics,
                )
            );
            exit(ExitCodes::USER_ERROR);
        })
        .unwrap_or_else(|| {
            crate::cli_error!(
                "E2105",
                "couldn't resolve package facts for {}",
                entry.display()
            );
            exit(ExitCodes::USER_ERROR);
        });
    if let Some(block) = inline {
        let raw = block.body(&entry_raw).to_string();
        let manifest = jet::Package::to_manifest(&facts, &raw).unwrap_or_else(|diagnostic| {
            eprint!(
                "{}",
                jet::render_diagnostics(&entry.display().to_string(), &raw, &[diagnostic])
            );
            exit(ExitCodes::USER_ERROR);
        });
        return PackageInput {
            path: entry,
            raw: entry_raw,
            inline: Some(block),
            manifest,
        };
    }
    let path = jet::Loader::manifest_path(root).unwrap_or_else(|| {
        crate::cli_error!("E2105", "couldn't locate {}", jet::Syntax::PACKAGE_FILE);
        exit(ExitCodes::USER_ERROR);
    });
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| {
        crate::cli_error!("E2105", "couldn't read {}: {}", path.display(), error);
        exit(ExitCodes::USER_ERROR);
    });
    let manifest = jet::Package::to_manifest(&facts, &raw).unwrap_or_else(|diagnostic| {
        eprint!(
            "{}",
            jet::render_diagnostics(&path.display().to_string(), &raw, &[diagnostic])
        );
        exit(ExitCodes::USER_ERROR);
    });
    PackageInput {
        path,
        raw,
        inline: None,
        manifest,
    }
}

fn edited_package_source(input: &PackageInput, updated_body: impl FnOnce(&str) -> String) -> String {
    let Some(block) = input.inline else {
        return updated_body(&input.raw);
    };
    let mut source = input.raw.clone();
    let updated = updated_body(block.body(&input.raw));
    source.replace_range(block.body_span.start..block.body_span.end, &updated);
    source
}
/// Replace a package carrier only after re-reading the same checked source
/// object used to build the edit. This keeps inline package edits atomic and
/// prevents a concurrent writer from losing an unrelated source change.
fn write_package_source(input: &PackageInput, updated: &str) -> Result<(), String> {
    let parent = input
        .path
        .parent()
        .ok_or_else(|| "package source has no parent directory".to_string())?;
    let name = input
        .path
        .file_name()
        .ok_or_else(|| "package source has no file name".to_string())?;
    let resolver = jet::Authority::AuthorityResolver::open(parent)
        .map_err(|error| format!("could not open package source authority: {error}"))?;
    let checked = resolver
        .checked_file(Path::new(name))
        .map_err(|error| format!("could not check {}: {error}", input.path.display()))?;
    let current = checked
        .text()
        .map_err(|error| format!("could not read {}: {error}", input.path.display()))?;
    if current != input.raw {
        return Err(format!(
            "{} changed while the package edit was prepared",
            input.path.display()
        ));
    }
    resolver
        .revalidate_file(&checked)
        .map_err(|error| format!("package source changed before writing: {error}"))?;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("could not create package edit nonce: {error}"))?
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.jet-package-edit-{}-{nonce}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("could not stage {}: {error}", input.path.display()))?;
        file.write_all(updated.as_bytes())
            .map_err(|error| format!("could not stage {}: {error}", input.path.display()))?;
        file.sync_all()
            .map_err(|error| format!("could not sync {}: {error}", input.path.display()))?;
        resolver
            .revalidate_file(&checked)
            .map_err(|error| format!("package source changed before publishing: {error}"))?;
        fs::rename(&temporary, &input.path)
            .map_err(|error| format!("could not publish {}: {error}", input.path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn run_add(raw_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no package.jet found — run `jet add` inside a project\n fix: run `jet new <name>` to create a project first",
    );

    // Parse: jet add <dep-name> --path <dir> | --git <url> [--tag <t>|--branch <b>|--rev <r>]
    let non_flag: Vec<&String> = raw_args.iter().filter(|a| !a.starts_with("--")).collect();
    let dep_name = match non_flag.get(1) {
        Some(n) => n.as_str(),
        None => {
            crate::cli_error!(@fix "E2104", "`jet add` needs a dependency name", "try `jet add mylib --path ../mylib`");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let path_val = flag_value(raw_args, "--path");
    let git_val = flag_value(raw_args, "--git");
    let tag_val = flag_value(raw_args, "--tag");
    let branch_val = flag_value(raw_args, "--branch");
    let rev_val = flag_value(raw_args, "--rev");

    let spec = if let Some(p) = path_val {
        jet::Manifest::DepSpec::Path {
            path: p.to_string(),
        }
    } else if let Some(url) = git_val {
        let selector = if let Some(t) = tag_val {
            jet::Manifest::GitSelector::Tag(t.to_string())
        } else if let Some(b) = branch_val {
            jet::Manifest::GitSelector::Branch(b.to_string())
        } else if let Some(r) = rev_val {
            jet::Manifest::GitSelector::Rev(r.to_string())
        } else {
            crate::cli_error!(
                "E2104",
                "git dependency `{}` needs one of: --tag, --branch, --rev",
                dep_name
            );
            exit(ExitCodes::USER_ERROR);
        };
        jet::Manifest::DepSpec::Git {
            url: url.to_string(),
            selector,
        }
    } else {
        crate::cli_error!(@fix "E2104", format!("`jet add {}` needs --path or --git", dep_name), format!("try `jet add {} --path ../{}` or `jet add {} --git <url> --tag <tag>`", dep_name, dep_name, dep_name));
        exit(ExitCodes::USER_ERROR);
    };

    let input = package_input(&root);
    let updated = edited_package_source(&input, |raw| {
        jet::Manifest::add_dependency(raw, dep_name, &spec)
    });
    if let Err(error) = write_package_source(&input, &updated) {
        crate::cli_error!("E2105", "couldn't write {}: {}", input.path.display(), error);
        exit(ExitCodes::USER_ERROR);
    }
    println!("added `{}` to {}", dep_name, input.path.display());

    // Auto-fetch.
    do_fetch(&root, false);
}

pub(crate) fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(&cwd, "error: no package.jet found");

    let input = package_input(&root);
    if !input.manifest.dependencies.contains_key(dep_name) {
        crate::cli_error!(
            @fix "E2104",
            format!("dependency `{dep_name}` is not present in {}", jet::Syntax::PACKAGE_FILE),
            format!("remove a dependency listed in {}, or add `{dep_name}` first", jet::Syntax::PACKAGE_FILE)
        );
        exit(ExitCodes::USER_ERROR);
    }
    let updated = edited_package_source(&input, |raw| {
        jet::Manifest::remove_dependency(raw, dep_name)
    });
    if let Err(error) = write_package_source(&input, &updated) {
        crate::cli_error!("E2105", "couldn't write {}: {}", input.path.display(), error);
        exit(ExitCodes::USER_ERROR);
    }
    println!("removed `{}` from {}", dep_name, input.path.display());

    // Re-fetch to update lock.
    do_fetch(&root, false);
}
pub(crate) fn run_fetch(locked: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no package.jet found — run `jet fetch` inside a project",
    );
    do_fetch(&root, locked);
}

pub(crate) fn run_update(dep: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(&cwd, "error: no package.jet found");

    let input = package_input(&root);
    let pack_path = input.path;
    let raw = input.raw;
    let mf = input.manifest;
    let existing_lock = jet::Lock::load(&root);
    let opts = jet::Fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: dep.map(str::to_string),
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    match jet::Fetch::fetch(&root, &mf, existing_lock.as_ref(), &opts) {
        Ok((lock, _)) => {
            print_registry_tiers(&lock);
            if let Some(d) = dep {
                println!("updated `{}`", d);
            } else {
                println!("updated all moving selectors");
            }
        }
        Err(diags) => {
            let src = raw;
            eprint!(
                "{}",
                jet::render_diagnostics(&pack_path.display().to_string(), &src, &diags)
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn do_fetch(root: &Path, locked: bool) {
    let input = package_input(root);
    let pack_path = input.path;
    let raw = input.raw;
    let mf = input.manifest;
    let existing_lock = jet::Lock::load(root);
    let opts = jet::Fetch::FetchOptions {
        locked,
        update: false,
        update_dep: None,
        resolution: jet::Publish::ResolveMode::Conservative,
    };
    match jet::Fetch::fetch(root, &mf, existing_lock.as_ref(), &opts) {
        Ok((lock, _)) => {
            print_registry_tiers(&lock);
            if locked {
                println!("lock verified");
            } else {
                println!("fetched all dependencies");
            }
        }
        Err(diags) => {
            eprint!(
                "{}",
                jet::render_diagnostics(&pack_path.display().to_string(), &raw, &diags)
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn print_registry_tiers(lock: &jet::Lock::LockFile) {
    for package in &lock.packages {
        let jet::Lock::LockSource::Registry {
            registry,
            tier,
            gate_status,
            ..
        } = &package.source
        else {
            continue;
        };
        println!(
            "resolved `{}` {} from registry `{}` (tier: {}; gate status: {})",
            package.name, package.version, registry, tier, gate_status
        );
    }
}
