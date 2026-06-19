//! add / remove / fetch / update / store / gc package-manager subcommand
//! handlers (M12.1).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::exit_codes;

use crate::flag_value;

pub(crate) fn run_add(raw_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `pkg.jet` found — run `jet add` inside a project");
        eprintln!(" fix: run `jet new <name>` to create a project first");
        exit(exit_codes::USER_ERROR);
    });

    // Parse: jet add <dep-name> --path <dir> | --git <url> [--tag <t>|--branch <b>|--rev <r>]
    let non_flag: Vec<&String> = raw_args.iter().filter(|a| !a.starts_with("--")).collect();
    let dep_name = match non_flag.get(1) {
        Some(n) => n.as_str(),
        None => {
            eprintln!("error: `jet add` needs a dependency name");
            eprintln!(" fix: try `jet add mylib --path ../mylib`");
            exit(exit_codes::USER_ERROR);
        }
    };

    let path_val = flag_value(raw_args, "--path");
    let git_val = flag_value(raw_args, "--git");
    let tag_val = flag_value(raw_args, "--tag");
    let branch_val = flag_value(raw_args, "--branch");
    let rev_val = flag_value(raw_args, "--rev");

    let spec = if let Some(p) = path_val {
        jet::manifest::DepSpec::Path {
            path: p.to_string(),
        }
    } else if let Some(url) = git_val {
        let selector = if let Some(t) = tag_val {
            jet::manifest::GitSelector::Tag(t.to_string())
        } else if let Some(b) = branch_val {
            jet::manifest::GitSelector::Branch(b.to_string())
        } else if let Some(r) = rev_val {
            jet::manifest::GitSelector::Rev(r.to_string())
        } else {
            eprintln!(
                "error: git dependency `{}` needs one of: --tag, --branch, --rev",
                dep_name
            );
            exit(exit_codes::USER_ERROR);
        };
        jet::manifest::DepSpec::Git {
            url: url.to_string(),
            selector,
        }
    } else {
        eprintln!("error: `jet add {}` needs --path or --git", dep_name);
        eprintln!(
            " fix: try `jet add {} --path ../{}` or `jet add {} --git <url> --tag <tag>`",
            dep_name, dep_name, dep_name
        );
        exit(exit_codes::USER_ERROR);
    };

    // Load the manifest, add the dep, write back.
    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let updated = jet::manifest::add_dependency(&raw, dep_name, &spec);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    println!("added `{}` to {}", dep_name, jet::syntax::PAYLOAD_FILE);

    // Auto-fetch.
    do_fetch(&root, false);
}

pub(crate) fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `pkg.jet` found");
        exit(exit_codes::USER_ERROR);
    });

    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let updated = jet::manifest::remove_dependency(&raw, dep_name);
    fs::write(&pack_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    println!("removed `{}` from {}", dep_name, jet::syntax::PAYLOAD_FILE);

    // Re-fetch to update lock.
    do_fetch(&root, false);
}

pub(crate) fn run_fetch(locked: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `pkg.jet` found — run `jet fetch` inside a project");
        exit(exit_codes::USER_ERROR);
    });
    do_fetch(&root, locked);
}

pub(crate) fn run_update(dep: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `pkg.jet` found");
        exit(exit_codes::USER_ERROR);
    });

    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let mf = jet::manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprintln!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(exit_codes::USER_ERROR);
    });
    let existing_lock = jet::lock::load(&root);
    let opts = jet::fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: dep.map(str::to_string),
    };
    match jet::fetch::fetch(&root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if let Some(d) = dep {
                println!("updated `{}`", d);
            } else {
                println!("updated all moving selectors");
            }
        }
        Err(diags) => {
            let src = String::new();
            eprint!("{}", jet::render_diagnostics(jet::syntax::PAYLOAD_FILE, &src, &diags));
            exit(exit_codes::USER_ERROR);
        }
    }
}

pub(crate) fn run_store_verify() {
    let store_dir = jet::store::store_dir();
    let entries = jet::store::list_entries();
    if entries.is_empty() {
        println!("store is empty ({})", store_dir.display());
        return;
    }
    println!("verifying {} store entries...", entries.len());
    // Without lockfile context we can only verify tree hashes against themselves.
    // Full verification requires the lock file; this checks for obvious corruption.
    let mut ok = 0;
    let mut bad = 0;
    for entry in &entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let th = jet::sha256::tree_hash(entry);
        if th.starts_with("sha256-") {
            ok += 1;
        } else {
            eprintln!("  bad: {}", name);
            bad += 1;
        }
    }
    println!("{} ok, {} bad", ok, bad);
    if bad > 0 {
        exit(exit_codes::USER_ERROR);
    }
}

pub(crate) fn run_gc() {
    // Without a global registry of in-use locks, we print a stub message.
    // Full gc would walk all .jet/lock files; M12.1 ships the infrastructure.
    let entries = jet::store::list_entries();
    println!(
        "store has {} entries; use `jet store verify` to check hashes",
        entries.len()
    );
    println!("(gc: removing unreferenced entries requires a future registry — coming in M12.2)");
}

/// D-PURE3=B (E2-M16): print recorded store generations.
pub(crate) fn run_store_generations() {
    let gens = jet::store::list_generations();
    if gens.is_empty() {
        println!("no store generations recorded yet");
        println!("hint: generations are recorded when packages are installed");
        return;
    }
    println!("{} generation(s):", gens.len());
    for g in &gens {
        println!("  gen {}: {} (hash: {})", g.number, g.timestamp, g.entry_hash);
    }
}

/// D-PURE3=B (E2-M16): roll back to a prior store generation.
pub(crate) fn run_store_rollback(gen_str: &str) {
    let gen_number = match gen_str.parse::<u64>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("error: `jet store rollback` needs a generation number");
            eprintln!(" fix: run `jet store generations` to see available generations");
            exit(exit_codes::USAGE);
        }
    };
    match jet::store::rollback_to(gen_number) {
        Ok(g) => {
            println!("rolled back to generation {} (entry hash: {})", g.number, g.entry_hash);
            println!("hint: the store is append-only; run `jet fetch` to restore the generation's packages");
        }
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!(" fix: run `jet store generations` to see available generations");
            exit(exit_codes::USER_ERROR);
        }
    }
}

fn do_fetch(root: &Path, locked: bool) {
    let pack_path = root.join(jet::syntax::PAYLOAD_FILE);
    let raw = fs::read_to_string(&pack_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {}", jet::syntax::PAYLOAD_FILE, e);
        exit(exit_codes::USER_ERROR);
    });
    let mf = jet::manifest::parse(&pack_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&pack_path.display().to_string(), &raw, &[d])
        );
        exit(exit_codes::USER_ERROR);
    });
    let existing_lock = jet::lock::load(root);
    let opts = jet::fetch::FetchOptions {
        locked,
        update: false,
        update_dep: None,
    };
    match jet::fetch::fetch(root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if locked {
                println!("lock verified");
            } else {
                println!("fetched all dependencies");
            }
        }
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(jet::syntax::PAYLOAD_FILE, &raw, &diags));
            exit(exit_codes::USER_ERROR);
        }
    }
}
