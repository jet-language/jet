//! D-MIGRATE2C (ratified 2026-06-22): `jet inspect schema` subcommand handlers.
//!
//! `jet inspect schema status` — report every `#PublishedSchema` type in the project:
//! its pinned published version, field count, and field list. If the working-tree
//! struct has a pending breaking change vs its snapshot, note it (reuses the
//! E0910 diff, so the output matches what `jet build` would refuse).
//!
//! `jet inspect schema squash --before <ver>` — re-baseline. Rewrites every snapshot to
//! the CURRENT published struct shape and records `squashed_before = <ver>`, so
//! future builds treat the current shape as the authoritative baseline and old
//! `migration` blocks for versions `< <ver>` are no longer required. This NEVER
//! edits user source — only the committed `.jet/cache/schema/*.snapshot` files.
//!
//! There is NO `jet inspect schema check` verb — `jet build`'s E0910 is already the CI gate.

use std::path::PathBuf;
use std::process::exit;

use jet::ExitCodes;
use jet::Syntax;

use crate::find_project_entry;

/// Dispatch `jet inspect schema <verb> …`.
pub(crate) fn run_schema(args: &[String]) {
    let verb = args.first().map(|s| s.as_str());
    match verb {
        Some(v) if v == Syntax::SCHEMA_VERB_STATUS => run_status(),
        Some(v) if v == Syntax::SCHEMA_VERB_SQUASH => {
            let before = flag_value(args, "--before");
            run_squash(before.as_deref());
        }
        other => {
            if let Some(v) = other {
                eprintln!("error: `jet inspect schema {}` isn't a schema command", v);
            } else {
                eprintln!("error: `jet inspect schema` needs a verb");
            }
            eprintln!(
                " Fix: use `jet inspect schema {}` to inspect published schemas, or \
                 `jet inspect schema {} --before <version>` to re-baseline them",
                Syntax::SCHEMA_VERB_STATUS,
                Syntax::SCHEMA_VERB_SQUASH
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// Locate the project root (dir containing package.jet or migration-era pkg.jet)
/// or exit with a clear error.
fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    crate::require_manifest_root(
        &cwd,
        "error: no package.jet found — run `jet inspect schema` inside a project",
    )
}

/// `jet inspect schema status` — list every snapshotted `#PublishedSchema` type.
fn run_status() {
    let root = project_root();
    let snaps = jet::Publish::load_all_snapshots(&root);

    if snaps.is_empty() {
        println!("no published schemas yet.");
        println!(
            "note: a `#PublishedSchema` struct is snapshotted into .jet/cache/schema/ on `jet registry publish`."
        );
        return;
    }

    // Diff each snapshot against the current working-tree struct so we can flag
    // pending breaking changes. Reuses the same E0910 pass `jet build` runs.
    let pending = pending_breaks(&root);

    println!("published schemas in this project:\n");
    for snap in &snaps {
        print!(
            "  {} — published {}",
            snap.type_name, snap.published_version
        );
        if let Some(before) = &snap.squashed_before {
            print!(" (squashed before {})", before);
        }
        println!();
        println!("    {} field(s):", snap.fields.len());
        for f in &snap.fields {
            println!("      {}: {}", f.name, f.ty);
        }
        if pending.iter().any(|t| t == &snap.type_name) {
            println!(
                "    pending: a breaking change vs this snapshot — `jet build` would report E0910"
            );
        }
        println!();
    }
}

/// Run the schema-migration diff against the working tree and return the set of
/// `#PublishedSchema` type names that currently have a pending breaking change.
/// Reuses the exact pass `jet build` runs, then attributes each E0910 to the
/// struct it names (every E0910 message backticks the type name).
fn pending_breaks(root: &std::path::Path) -> Vec<String> {
    let entry = find_project_entry(root);
    let entry_str = entry.to_string_lossy().to_string();
    if !entry.is_file() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(bundle) = jet::Loader::load_entry_with_overlay(&entry_str, None, true) {
        for module in &bundle.modules {
            // D-MIGRATE1 (I2 fix, bug #185): this advisory command runs before
            // a full `jet build` (no populated trait registry is available
            // here), so an empty `TraitRegistry` stands in. The Decode-only
            // "add"-op guard this enables (an added field must itself satisfy
            // `Decode`) then trusts every named type — a false negative here
            // only means `pending_breaks` under-reports; `jet build`'s own
            // call (`Bundle.rs`, with the real populated registry) is the
            // authoritative check and always runs before codegen.
            let reg = jet::Traits::TraitRegistry::default();
            let diags =
                jet::Sema::check_schema_migrations(&module.items, &bundle.project_root, &reg);
            if diags.is_empty() {
                continue;
            }
            for item in &module.items {
                if let jet::AST::Item::Struct(s) = item {
                    if s.is_published_schema
                        && diags
                            .iter()
                            .any(|d| d.what.contains(&format!("`{}`", s.name)))
                    {
                        out.push(s.name.clone());
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `jet inspect schema squash --before <ver>` — re-baseline snapshots to current shape.
fn run_squash(before: Option<&str>) {
    let Some(before) = before else {
        eprintln!(
            "error: `jet inspect schema {}` needs `--before <version>`",
            Syntax::SCHEMA_VERB_SQUASH
        );
        eprintln!(
            " Why: squash re-baselines to the current shape and marks migrations before \
             a version as no longer required — so it needs that cutoff version"
        );
        eprintln!(
            " Fix: run `jet inspect schema {} --before 2.0.0` (the version whose migrations you want to retire)",
            Syntax::SCHEMA_VERB_SQUASH
        );
        exit(ExitCodes::USER_ERROR);
    };

    let root = project_root();

    // Read the package version (the current published shape's version).
    let pack_path = jet::Loader::manifest_path(&root).unwrap_or_else(|| root.join(Syntax::PACKAGE_FILE));
    let version = match std::fs::read_to_string(&pack_path) {
        Ok(raw) => match jet::Manifest::parse(&pack_path, &raw) {
            Ok(mf) => mf.package.version,
            Err(_) => "0.0.0".to_string(),
        },
        Err(_) => "0.0.0".to_string(),
    };

    // Load the current working-tree #PublishedSchema structs so we re-baseline to
    // the *current* shape (not whatever the old snapshot held).
    let entry = find_project_entry(&root);
    let entry_str = entry.to_string_lossy().to_string();
    let bundle = match jet::Loader::load_entry_with_overlay(&entry_str, None, true) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("error: couldn't load the project to read its current schema shape");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let mut count = 0;
    for module in &bundle.modules {
        for item in &module.items {
            if let jet::AST::Item::Struct(s) = item {
                if s.is_published_schema {
                    let mut snap = jet::Publish::snapshot_from_struct(s, &version);
                    snap.squashed_before = Some(before.to_string());
                    if jet::Publish::save_snapshot(&root, &snap).is_ok() {
                        count += 1;
                        println!(
                            "  re-baselined `{}` to version {} (squashed before {})",
                            s.name, version, before
                        );
                    }
                }
            }
        }
    }

    if count == 0 {
        println!("no `#PublishedSchema` structs found — nothing to squash.");
    } else {
        println!(
            "\nok: {} snapshot(s) re-baselined. Migration blocks for versions before {} are no longer required.",
            count, before
        );
        println!("note: this rewrote .jet/cache/schema/ only — your source is unchanged. Commit the updated snapshots.");
    }
}

/// Find `--flag value` or `--flag=value` in an argument slice.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(rest) = a.strip_prefix(&format!("{}=", flag)) {
            return Some(rest.to_string());
        }
        if a == flag {
            return it.next().cloned();
        }
    }
    None
}
