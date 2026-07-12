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
        "error: no `pkg.jet` found — run `jet add` inside a project\n fix: run `jet new <name>` to create a project first",
    );

    // Parse: jet add <dep-name> --path <dir> | --git <url> [--tag <t>|--branch <b>|--rev <r>]
    let non_flag: Vec<&String> = raw_args.iter().filter(|a| !a.starts_with("--")).collect();
    let dep_name = match non_flag.get(1) {
        Some(n) => n.as_str(),
        None => {
            eprintln!("error: `jet add` needs a dependency name");
            eprintln!(" fix: try `jet add mylib --path ../mylib`");
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
            eprintln!(
                "error: git dependency `{}` needs one of: --tag, --branch, --rev",
                dep_name
            );
            exit(ExitCodes::USER_ERROR);
        };
        jet::Manifest::DepSpec::Git {
            url: url.to_string(),
            selector,
        }
    } else {
        eprintln!("error: `jet add {}` needs --path or --git", dep_name);
        eprintln!(
            " fix: try `jet add {} --path ../{}` or `jet add {} --git <url> --tag <tag>`",
            dep_name, dep_name, dep_name
        );
        exit(ExitCodes::USER_ERROR);
    };

    // Load the manifest, add the dep, write back.
    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let updated = jet::Manifest::add_dependency(&raw, dep_name, &spec);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("added `{}` to {}", dep_name, jet::Syntax::PAYLOAD_FILE);

    // Auto-fetch.
    do_fetch(&root, false);
}

pub(crate) fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(&cwd, "error: no `pkg.jet` found");

    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let updated = jet::Manifest::remove_dependency(&raw, dep_name);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("removed `{}` from {}", dep_name, jet::Syntax::PAYLOAD_FILE);

    // Re-fetch to update lock.
    do_fetch(&root, false);
}

pub(crate) fn run_fetch(locked: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(
        &cwd,
        "error: no `pkg.jet` found — run `jet fetch` inside a project",
    );
    do_fetch(&root, locked);
}

pub(crate) fn run_update(dep: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = crate::require_manifest_root(&cwd, "error: no `pkg.jet` found");

    let pack_path = root.join(jet::Syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::Syntax::PAYLOAD_FILE, e);
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
    };
    match jet::Fetch::fetch(&root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
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
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, &src, &diags)
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `jet hangar verify` (D-CLI-STORE2=A, was `jet store verify`).
pub(crate) fn run_hangar_verify() {
    let store_dir = jet::Store::store_dir();
    let entries = jet::Store::list_entries();
    if entries.is_empty() {
        println!("hangar is empty ({})", store_dir.display());
        return;
    }
    println!("verifying {} hangar entries...", entries.len());
    // Without lockfile context we can only verify tree hashes against themselves.
    // Full verification requires the lock file; this checks for obvious corruption.
    let mut ok = 0;
    let mut bad = 0;
    for entry in &entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let th = jet::SHA256::tree_hash(entry);
        if th.starts_with("sha256-") {
            ok += 1;
        } else {
            eprintln!("  bad: {}", name);
            bad += 1;
        }
    }
    println!("{} ok, {} bad", ok, bad);
    if bad > 0 {
        exit(ExitCodes::USER_ERROR);
    }
}

/// `jet hangar generations` (D-PURE3=B / D-CLI-STORE2=A, was `jet store generations`).
pub(crate) fn run_hangar_generations() {
    let gens = jet::Store::list_generations();
    if gens.is_empty() {
        println!("no hangar generations recorded yet");
        println!("hint: generations are recorded when packages are installed");
        return;
    }
    println!("{} generation(s):", gens.len());
    for g in &gens {
        println!(
            "  gen {}: {} (hash: {})",
            g.number, g.timestamp, g.entry_hash
        );
    }
}

/// `jet hangar rollback <gen>` (D-PURE3=B / D-CLI-STORE2=A, was `jet store rollback`).
pub(crate) fn run_hangar_rollback(gen_str: &str) {
    let gen_number = match gen_str.parse::<u64>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("error: `jet hangar rollback` needs a generation number");
            eprintln!(" fix: run `jet hangar generations` to see available generations");
            exit(ExitCodes::USAGE);
        }
    };
    match jet::Store::rollback_to(gen_number) {
        Ok(g) => {
            println!(
                "rolled back to generation {} (entry hash: {})",
                g.number, g.entry_hash
            );
            println!("hint: the store is append-only; run `jet fetch` to restore the generation's packages");
        }
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!(" fix: run `jet hangar generations` to see available generations");
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn do_fetch(root: &Path, locked: bool) {
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
    let existing_lock = jet::Lock::load(root);
    let opts = jet::Fetch::FetchOptions {
        locked,
        update: false,
        update_dep: None,
    };
    match jet::Fetch::fetch(root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if locked {
                println!("lock verified");
            } else {
                println!("fetched all dependencies");
            }
        }
        Err(diags) => {
            eprint!(
                "{}",
                jet::render_diagnostics(jet::Syntax::PAYLOAD_FILE, &raw, &diags)
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}
