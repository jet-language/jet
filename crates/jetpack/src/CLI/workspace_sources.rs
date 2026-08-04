use super::parse::Flags;
use crate::EnvFile;
use crate::ManifestTOML;
use crate::Provider;
use crate::RefSpec::{self, ProviderKind};
use crate::Syntax;
use crate::WorkspaceFile;
use crate::WorkspaceLock;
use std::path::{Path, PathBuf};

/// Resolve the fixtures dir (explicit flag, env, or none). `--offline` only
/// requires fixtures for Nix-backed refs; core refs use the source cache.
pub(super) fn fixtures_for(flags: &Flags) -> Option<PathBuf> {
    Provider::fixtures_from_env(flags.fixtures.clone())
}

/// Load and evaluate `workspace.jet` from `dir`, emit workspace entries into
/// `.jet/lock`, and return the `WorkspacePlan`. Returns `None` when the file is absent. Prints
/// the diagnostic to stderr and returns `Err(2)` if the file exists but fails
/// to evaluate (D-WORKSPACE1=B clean break: workspace.jet is the sole index).
pub fn load_workspace(dir: &Path) -> Option<Result<WorkspaceFile::WorkspacePlan, i32>> {
    let result = WorkspaceFile::load(dir)?;
    match result {
        Ok(plan) => {
            match WorkspaceLock::write(dir, &plan) {
                Ok(()) => Some(Ok(plan)),
                Err(error) => {
                    eprintln!(
                        "error: workspace evaluation succeeded but its unified lock could not be written: {error}"
                    );
                    Some(Err(2))
                }
            }
        }
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    Syntax::WORKSPACE_FILE,
                    "",
                    std::slice::from_ref(&d)
                )
            );
            Some(Err(2))
        }
    }
}

/// Load `[sources]` from `jetpack.toml` in `dir`, print any parse errors, and
/// return the resulting `SourceTable`. Returns an empty table when the file is
/// absent (not an error). Prints E1214/E1215 to stderr and returns `Err(2)` if
/// the file exists but has errors; the non-error entries are still returned so
/// the caller can decide whether to hard-exit or soft-degrade.
pub(super) fn load_toml_sources(dir: &Path) -> Result<RefSpec::SourceTable, (RefSpec::SourceTable, i32)> {
    let Some((manifest, errors)) = ManifestTOML::load(dir) else {
        return Ok(RefSpec::SourceTable::empty());
    };

    // Convert `target@provider` entries in `[sources]` to SourceTable decls.
    // We use `Infer` as the provider kind so U9 inference runs at realize time,
    // matching the typed `module { … }` surface behaviour.
    let decls = manifest.sources.into_iter().filter_map(|(name, raw_ref)| {
        match RefSpec::classify_provider_ref(&raw_ref) {
            Ok(pr) => {
                let upstream = format!("{}:{}", pr.provider.label(), pr.target);
                Some((name, upstream, ProviderKind::Infer))
            }
            Err(_) => None, // malformed ref: skip silently (E1214 covers the line)
        }
    });
    let table = RefSpec::SourceTable::from_decls(decls);

    if errors.is_empty() {
        Ok(table)
    } else {
        let rendered = ManifestTOML::render_errors(Syntax::JETPACK_TOML, &errors);
        eprint!("{}", rendered);
        Err((table, 2))
    }
}

/// The named-source table declared by the current project's env file (empty
/// when there is none). Used so explicit CLI refs are project-aware.
/// Also merges any `[sources]` declared in `jetpack.toml` (additive — env.jet
/// inline declarations win on conflict).
pub(super) fn cwd_table() -> RefSpec::SourceTable {
    let dir = std::env::current_dir().unwrap_or_default();
    let mut table = EnvFile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(RefSpec::SourceTable::empty);
    // Merge jetpack.toml [sources] as defaults (non-overriding).
    // Ignore parse errors here — cwd_table is used for explicit CLI refs;
    // load_project_plan handles the hard-exit case for project-scoped commands.
    let toml_table = match load_toml_sources(&dir) {
        Ok(t) | Err((t, _)) => t,
    };
    table.merge_defaults(toml_table);
    table
}

/// The workspace member index for the current directory (Slice B). Evaluated
/// from `workspace.jet` when present (discovery-by-declaration), else read from
/// the `.jet/lock` mirror, else empty. Lets bare (`logging`) and path-form
/// (`packages/logging`) refs resolve against workspace members.
pub(super) fn cwd_workspace_index() -> RefSpec::WorkspaceIndex {
    let dir = std::env::current_dir().unwrap_or_default();
    let plan = match WorkspaceFile::load(&dir) {
        Some(Ok(plan)) => Some(plan),
        // A malformed `workspace.jet` is source failure, never permission to
        // reuse a stale lock mirror.
        Some(Err(diagnostic)) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    Syntax::WORKSPACE_FILE,
                    "",
                    std::slice::from_ref(&diagnostic)
                )
            );
            std::process::exit(2);
        }
        None => {
            let lock = WorkspaceLock::load(&dir);
            if lock.is_none() {
                let path = dir.join(Syntax::UNIFIED_LOCK_FILE);
                let looks_like_workspace_lock = std::fs::read_to_string(&path)
                    .map(|source| {
                        source.contains("[[workspace_member]]")
                            || source.contains("workspace_source_digest")
                            || source.contains("workspace_overlay")
                    })
                    .unwrap_or(false);
                if looks_like_workspace_lock {
                    eprintln!(
                        "error: workspace lock `{}` is malformed or stale; refusing an empty member index",
                        path.display()
                    );
                    eprintln!(
                        " fix: run `jetpack env` from the workspace root to regenerate it after fixing workspace/package sources"
                    );
                    std::process::exit(2);
                }
            }
            lock
        }
    };
    match plan {
        Some(plan) => RefSpec::WorkspaceIndex::from_members(
            plan.members.into_iter().map(|m| (m.name, m.path)),
        ),
        None => RefSpec::WorkspaceIndex::empty(),
    }
}
