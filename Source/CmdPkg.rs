//! add / remove / fetch / update / store / gc package-manager subcommand
//! handlers (M12.1).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;

use crate::flag_value;

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

    // Load the manifest, add the dep, write back.
    let pack_path = jet::Loader::manifest_path(&root).expect("manifest root has a Package file");
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read {}: {}", pack_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });
    let updated = jet::Manifest::add_dependency(&raw, dep_name, &spec);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write {}: {}", pack_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("added `{}` to {}", dep_name, pack_path.display());

    // Auto-fetch.
    do_fetch(&root, false);
}

pub(crate) fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(&cwd, "error: no package.jet found");

    let pack_path = jet::Loader::manifest_path(&root).expect("manifest root has a Package file");
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read {}: {}", pack_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    let manifest = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });
    if !manifest.dependencies.contains_key(dep_name) {
        crate::cli_error!(
            @fix "E2104",
            format!("dependency `{dep_name}` is not present in {}", jet::Syntax::PACKAGE_FILE),
            format!("remove a dependency listed in {}, or add `{dep_name}` first", jet::Syntax::PACKAGE_FILE)
        );
        exit(ExitCodes::USER_ERROR);
    }
    let updated = jet::Manifest::remove_dependency(&raw, dep_name);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write {}: {}", pack_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("removed `{}` from {}", dep_name, pack_path.display());

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

    let pack_path = jet::Loader::manifest_path(&root).expect("manifest root has a Package file");
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read {}: {}", pack_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    let mf = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprintln!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });
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
            let src = String::new();
            eprint!(
                "{}",
                jet::render_diagnostics(&pack_path.display().to_string(), &src, &diags)
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn do_fetch(root: &Path, locked: bool) {
    let pack_path = jet::Loader::manifest_path(root).expect("manifest root has a Package file");
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read {}: {}", pack_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    let mf = jet::Manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(ExitCodes::USER_ERROR);
    });
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
