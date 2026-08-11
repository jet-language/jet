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

/// Find the nearest Package, environment, or workspace root for commands run
/// below a project directory. Explicit refs and project plans must use the
/// same source facts from that root.
pub(super) fn project_root(start: &Path) -> PathBuf {
    nearest_project_root(start)
        .unwrap_or_else(|| start.to_path_buf())
}

/// Workspace member lookup has a wider boundary than a member Package. Keep
/// it on the nearest workspace declaration so a package inside a monorepo
/// still sees the monorepo's member index.
pub(super) fn workspace_root(start: &Path) -> PathBuf {
    nearest_workspace_root(start)
        .unwrap_or_else(|| start.to_path_buf())
}

fn workspace_source_present(dir: &Path) -> bool {
    WorkspaceFile::resolve_workspace_source(dir).is_some()
}

fn nearest_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start
        .canonicalize()
        .unwrap_or_else(|_| start.to_path_buf());
    loop {
        if dir.join(Syntax::PACKAGE_FILE).is_file()
            || dir.join(Syntax::ENV_FILE).is_file()
            || workspace_source_present(&dir)
        {
            return Some(dir);
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }
    None
}

fn nearest_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start
        .canonicalize()
        .unwrap_or_else(|_| start.to_path_buf());
    loop {
        if workspace_source_present(&dir) {
            return Some(dir);
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }
    None
}

/// Load and evaluate the declaration-resolved workspace source from `dir`,
/// emit workspace entries into `.jet/lock`, and return the `WorkspacePlan`.
/// Returns `None` when no source declares a workspace. Prints the diagnostic to
/// stderr and returns `Err(2)` when discovery or evaluation fails.
pub fn load_workspace(dir: &Path) -> Option<Result<WorkspaceFile::WorkspacePlan, i32>> {
    if let Some(Ok(source)) = WorkspaceFile::resolve_workspace_source(dir) {
        if source.role != WorkspaceFile::WorkspaceSourceRole::Index {
            return None;
        }
    }
    let result = WorkspaceFile::load(dir)?;
    match result {
        Ok(plan) => {
            match WorkspaceLock::write(dir, &plan) {
                Ok(()) => Some(Ok(plan)),
                Err(error) => {
                    let lock_path = dir.join(Syntax::UNIFIED_LOCK_FILE);
                    let diagnostic = crate::Lock::e1202_workspace_write(
                        &lock_path.display().to_string(),
                        &error,
                    );
                    eprint!(
                        "{}",
                        crate::Diagnostics::render_all(
                            Syntax::WORKSPACE_FILE,
                            "",
                            std::slice::from_ref(&diagnostic),
                        )
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
    let dir = project_root(&std::env::current_dir().unwrap_or_default());
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
/// from the declaration-resolved source when present, else read from
/// the `.jet/lock` mirror, else empty. Lets bare (`logging`) and path-form
/// (`packages/logging`) refs resolve against workspace members.
pub(super) fn cwd_workspace_index() -> RefSpec::WorkspaceIndex {
    let dir = workspace_root(&std::env::current_dir().unwrap_or_default());
    let plan = match WorkspaceFile::load(&dir) {
        Some(Ok(plan)) => Some(plan),
        // A malformed workspace source is source failure, never permission to
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
                let looks_like_workspace_lock = match std::fs::read_to_string(&path) {
                    Ok(source) => crate::Lock::looks_like_workspace_lock(&source),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        let mut diagnostic = crate::Lock::e1202_workspace(
                            &path.display().to_string(),
                        );
                        diagnostic.why = format!(
                            "the workspace lock could not be read, so its authority is not trusted: {error}"
                        );
                        eprint!(
                            "{}",
                            crate::Diagnostics::render_all(
                                Syntax::WORKSPACE_FILE,
                                "",
                                std::slice::from_ref(&diagnostic),
                            )
                        );
                        std::process::exit(2);
                    }
                };
                if looks_like_workspace_lock {
                    let diagnostic = crate::Lock::e1202_workspace(&path.display().to_string());
                    eprint!(
                        "{}",
                        crate::Diagnostics::render_all(
                            Syntax::WORKSPACE_FILE,
                            "",
                            std::slice::from_ref(&diagnostic),
                        )
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

#[cfg(test)]
mod tests {
    use super::{load_workspace, project_root, workspace_root};
    use crate::Syntax;
    use std::fs;

    #[test]
    fn project_root_walks_from_nested_command_directory() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-project-root-{}",
            std::process::id()
        ));
        let nested = root.join("packages/app/src");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("env.jet"), "module env.dev {}\n").unwrap();
        assert_eq!(project_root(&nested), root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_root_walks_to_the_nearest_package_file() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-package-root-{}",
            std::process::id()
        ));
        let nested = root.join("packages/app/src");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join(Syntax::PACKAGE_FILE), "name: \"app\"\n").unwrap();
        assert_eq!(project_root(&nested), root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_root_survives_a_nested_package_boundary() {
        let root = std::env::temp_dir().join(format!(
            "jetpack-workspace-root-{}",
            std::process::id()
        ));
        let package = root.join("packages/app");
        let nested = package.join("src");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            root.join("authority.jet"),
            "module workspace { policy: .{ deny: #(Exec) } }\n",
        )
        .unwrap();
        fs::write(package.join(Syntax::PACKAGE_FILE), "name: \"app\"\n").unwrap();
        assert_eq!(project_root(&nested), package);
        assert_eq!(workspace_root(&nested), root);
        assert!(WorkspaceFile::load(&root).is_none());
        assert!(load_workspace(&root).is_none());
        let _ = fs::remove_dir_all(root);
    }
}
