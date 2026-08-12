use super::parse::Flags;
use crate::EnvFile::EnvFile;
use crate::ManifestTOML;
use crate::Provider;
use crate::RefSpec::{self, ProviderKind};
use crate::Syntax;
use crate::WorkspaceFile;
use crate::WorkspaceLock;
use crate::Diagnostics::Diagnostic;
use jet_pkg_model::Authority::AuthorityResolver;
use jet_pkg_model::WorkspacePlan::{WorkspaceSource, WorkspaceSourceRole};
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
    match nearest_project_root(start) {
        Ok(Some(root)) => root,
        Ok(None) => start.to_path_buf(),
        Err(diagnostic) => report_authority_error(diagnostic),
    }
}

/// Workspace member lookup has a wider boundary than a member Package. Keep
/// it on the nearest workspace declaration so a package inside a monorepo
/// still sees the monorepo's member index.
pub(super) fn workspace_root(start: &Path) -> PathBuf {
    match nearest_workspace_root(start) {
        Ok(Some(root)) => root,
        Ok(None) => start.to_path_buf(),
        Err(diagnostic) => report_authority_error(diagnostic),
    }
}

fn report_authority_error(diagnostic: Diagnostic) -> ! {
    eprint!(
        "{}",
        crate::Diagnostics::render_all(
            Syntax::WORKSPACE_FILE,
            "",
            std::slice::from_ref(&diagnostic),
        )
    );
    std::process::exit(2)
}

type CheckedWorkspaceSource = (AuthorityResolver, WorkspaceSource);

fn checked_workspace_source(dir: &Path) -> Result<Option<CheckedWorkspaceSource>, Diagnostic> {
    match AuthorityResolver::open_for_authority_walk(dir) {
        Ok(None) => Ok(None),
        Ok(Some(resolver)) => match resolver.resolve_workspace_source() {
            Ok(Some(source)) => {
                resolver
                    .revalidate_source(&source)
                    .map_err(|error| error.diagnostic())?;
                Ok(Some((resolver, source)))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error.workspace_diagnostic()),
        },
        Err(error) if error.is_missing() => Ok(None),
        Err(error) => Err(error.diagnostic()),
    }
}

fn workspace_source_present(dir: &Path) -> Result<bool, Diagnostic> {
    Ok(checked_workspace_source(dir)?.is_some())
}

fn package_manifest_present(dir: &Path) -> Result<bool, Diagnostic> {
    match AuthorityResolver::open_for_authority_walk(dir) {
        Ok(None) => Ok(false),
        Ok(Some(resolver)) => match resolver.checked_manifest(Path::new(".")) {
            Ok(manifest) => {
                resolver
                    .revalidate_file(&manifest.file)
                    .map_err(|error| error.diagnostic())?;
                Ok(true)
            }
            Err(error) if error.is_missing() => Ok(false),
            Err(error) => Err(error.diagnostic()),
        },
        Err(error) if error.is_missing() => Ok(false),
        Err(error) => Err(error.diagnostic()),
    }
}

fn nearest_project_root(start: &Path) -> Result<Option<PathBuf>, Diagnostic> {
    let mut dir = AuthorityResolver::authority_walk_root(start)
        .map_err(|error| error.diagnostic())?;
    loop {
        if package_manifest_present(&dir)?
            || env_file_present(&dir)?
            || workspace_source_present(&dir)?
        {
            return Ok(Some(dir));
        }
        let Some(parent) = AuthorityResolver::authority_walk_parent(&dir) else {
            break;
        };
        dir = parent;
    }
    Ok(None)
}

fn env_file_present(dir: &Path) -> Result<bool, Diagnostic> {
    match AuthorityResolver::open_for_authority_walk(dir) {
        Ok(None) => Ok(false),
        Ok(Some(resolver)) => match resolver.checked_file(Path::new(Syntax::ENV_FILE)) {
            Ok(file) => {
                resolver
                    .revalidate_file(&file)
                    .map_err(|error| error.diagnostic())?;
                Ok(true)
            }
            Err(error) if error.is_missing() => Ok(false),
            Err(error) => Err(error.diagnostic()),
        },
        Err(error) if error.is_missing() => Ok(false),
        Err(error) => Err(error.diagnostic()),
    }
}

fn checked_env_file(dir: &Path) -> Result<Option<EnvFile>, Diagnostic> {
    let resolver = match AuthorityResolver::open(dir) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(error.diagnostic()),
    };
    let file = match resolver.checked_file(Path::new(Syntax::ENV_FILE)) {
        Ok(file) => file,
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(error.diagnostic()),
    };
    let source = file.text().map_err(|error| error.diagnostic())?;
    resolver
        .revalidate_file(&file)
        .map_err(|error| error.diagnostic())?;
    Ok(Some(crate::EnvFile::parse(&source)))
}

fn nearest_workspace_root(start: &Path) -> Result<Option<PathBuf>, Diagnostic> {
    Ok(nearest_workspace_source(start)?.map(|(root, _, _)| root))
}

fn nearest_workspace_source(
    start: &Path,
) -> Result<Option<(PathBuf, AuthorityResolver, WorkspaceSource)>, Diagnostic> {
    let mut dir = AuthorityResolver::authority_walk_root(start)
        .map_err(|error| error.diagnostic())?;
    loop {
        if let Some((resolver, source)) = checked_workspace_source(&dir)? {
            return Ok(Some((dir, resolver, source)));
        }
        let Some(parent) = AuthorityResolver::authority_walk_parent(&dir) else {
            break;
        };
        dir = parent;
    }
    Ok(None)
}

pub(super) fn workspace_root_snapshot(
    start: &Path,
) -> Result<(PathBuf, Option<CheckedWorkspaceSource>), Diagnostic> {
    match nearest_workspace_source(start)? {
        Some((root, resolver, source)) => Ok((root, Some((resolver, source)))),
        None => Ok((start.to_path_buf(), None)),
    }
}

pub(super) fn workspace_root_snapshot_or_exit(
    start: &Path,
) -> (PathBuf, Option<CheckedWorkspaceSource>) {
    workspace_root_snapshot(start).unwrap_or_else(|diagnostic| report_authority_error(diagnostic))
}

pub(super) fn workspace_index_required_diagnostic() -> Diagnostic {
    Diagnostic::error(
        "E1239",
        "workspace build needs workspace.jet as the index".to_string(),
        "an authority declaration is not a member index".to_string(),
        "move members into workspace.jet and keep one workspace index".to_string(),
        None,
    )
}

fn workspace_snapshot_from_source(
    resolver: &AuthorityResolver,
    expected: &WorkspaceSource,
) -> Result<WorkspaceFile::WorkspaceSnapshot, Diagnostic> {
    WorkspaceFile::load_checked_source(resolver, expected.clone())
}

#[cfg(test)]
fn load_workspace_snapshot(
    dir: &Path,
) -> Result<Option<WorkspaceFile::WorkspaceSnapshot>, Diagnostic> {
    let Some((resolver, source)) = checked_workspace_source(dir)? else {
        return Ok(None);
    };
    if source.role != WorkspaceSourceRole::Index {
        return Ok(None);
    }
    workspace_snapshot_from_source(&resolver, &source).map(Some)
}

/// Load and evaluate the declaration-resolved workspace source from `dir`,
/// emit workspace entries into `.jet/lock`, and return the `WorkspacePlan`.
/// Returns `None` when no source declares a workspace. Prints the diagnostic to
/// stderr and returns `Err(2)` when discovery or evaluation fails.
#[cfg(test)]
fn load_workspace(dir: &Path) -> Option<Result<WorkspaceFile::WorkspacePlan, i32>> {
    finish_workspace_load(dir, load_workspace_snapshot(dir))
}

pub(super) fn load_workspace_for_source(
    dir: &Path,
    checked: &CheckedWorkspaceSource,
) -> Option<Result<WorkspaceFile::WorkspacePlan, i32>> {
    let (resolver, source) = checked;
    if let Err(error) = resolver.revalidate_source(source) {
        return finish_workspace_load(dir, Err(error.diagnostic()));
    }
    if source.role != WorkspaceSourceRole::Index {
        return finish_workspace_load(
            dir,
            Err(workspace_index_required_diagnostic()),
        );
    }
    finish_workspace_load(
        dir,
        workspace_snapshot_from_source(resolver, source).map(Some),
    )
}

fn finish_workspace_load(
    dir: &Path,
    result: Result<Option<WorkspaceFile::WorkspaceSnapshot>, Diagnostic>,
) -> Option<Result<WorkspaceFile::WorkspacePlan, i32>> {
    let result = match result {
        Ok(Some(snapshot)) => Ok(snapshot.plan),
        Ok(None) => return None,
        Err(diagnostic) => Err(diagnostic),
    };
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
    let mut table = match checked_env_file(&dir) {
        Ok(Some(env)) => env.source_table(),
        Ok(None) => RefSpec::SourceTable::empty(),
        Err(diagnostic) => report_authority_error(diagnostic),
    };
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
    let (dir, source) = workspace_root_snapshot(&std::env::current_dir().unwrap_or_default())
        .unwrap_or_else(|diagnostic| report_authority_error(diagnostic));
    let (plan, allow_lock) = match source {
        Some((resolver, source)) if source.role == WorkspaceSourceRole::Index => {
            let snapshot = workspace_snapshot_from_source(&resolver, &source)
                .unwrap_or_else(|diagnostic| report_authority_error(diagnostic));
            (Some(snapshot.plan), false)
        }
        // An authority source is a boundary, not an index. Do not fall
        // through to a stale lock when one is present.
        Some(_) => (None, false),
        None => (None, true),
    };
    let plan = if allow_lock {
        let resolver = AuthorityResolver::open(&dir).unwrap_or_else(|error| {
            report_authority_error(error.diagnostic())
        });
        let lock_file = match resolver.checked_file(Path::new(Syntax::UNIFIED_LOCK_FILE)) {
            Ok(file) => Some(file),
            Err(error) if error.is_missing() => None,
            Err(error) => report_authority_error(error.diagnostic()),
        };
        match lock_file {
            Some(lock_file) => {
                let path = lock_file.path.clone();
                let source = lock_file
                    .text()
                    .unwrap_or_else(|error| report_authority_error(error.diagnostic()));
                resolver
                    .revalidate_file(&lock_file)
                    .unwrap_or_else(|error| report_authority_error(error.diagnostic()));
                let looks_like_workspace_lock = crate::Lock::looks_like_workspace_lock(&source);
                let lock = WorkspaceLock::load_checked_file(&resolver, lock_file);
                if lock.is_none() && looks_like_workspace_lock {
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
                lock
            }
            None => None,
        }
    } else {
        plan
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
    use crate::WorkspaceFile;
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
