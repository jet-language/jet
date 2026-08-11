//! Read-only workspace lock access (D-WORKSPACELOCK1=A).
//!
//! External tools (IDEs, CI scripts, Canvas) can call `WorkspaceLock::load`
//! to get a static workspace index from `.jet/lock` without evaluating Jet
//! and without depending on `jetpack`'s engine crate. Only the canonical
//! `workspace.jet` index may supply member-index facts.
//!
//! The write path (`WorkspaceLock::write`) needs `jetpack::RuntimePolicy` for
//! file locking; it lives in `jetpack::WorkspaceLock` and re-exports this
//! read path for callers that only need to read.

use crate::{
    Authority::AuthorityResolver,
    Lock::{self, LockedWorkspaceMember},
    Syntax,
    WorkspacePlan::{WorkspaceMember, WorkspacePlan},
};
use std::path::Path;

/// The lock file path within the workspace root.
pub const WORKSPACE_LOCK: &str = Syntax::UNIFIED_LOCK_FILE;

/// Load workspace facts from `.jet/lock` in `workspace_root`. Returns `None`
/// when the file is absent, malformed, stale, or missing the identity fields
/// required for a safe workspace index. Callers must not silently treat that
/// state as an empty workspace.
pub fn load(workspace_root: &Path) -> Option<WorkspacePlan> {
    let resolver = AuthorityResolver::open(workspace_root).ok()?;
    let lock_file = match resolver.checked_file(Path::new(WORKSPACE_LOCK)) {
        Ok(file) => file,
        Err(error) if error.is_missing() => return None,
        Err(_) => return None,
    };
    let raw = lock_file.text().ok()?;
    resolver.revalidate_file(&lock_file).ok()?;
    let lock = Lock::parse(&raw).ok()?;
    resolver.revalidate_file(&lock_file).ok()?;
    if lock.workspace_source_digest.is_none() {
        return None;
    }
    let source = match resolver.resolve_workspace_source() {
        Ok(source) => source,
        Err(_) => return None,
    };
    if let Some(source) = &source {
        resolver.revalidate_source(source).ok()?;
    }
    let workspace_source_role = source.as_ref().map(|source| source.role);
    if workspace_source_role
        .is_some_and(|role| role != crate::WorkspacePlan::WorkspaceSourceRole::Index)
    {
        return None;
    }
    let workspace_source_present = source.is_some();
    let source_digest = source
        .as_ref()
        .map(|source| crate::SHA256::sha256_hex(source.source.as_bytes()))
        .unwrap_or_else(|| "no-workspace-source".to_string());
    if !workspace_source_present
        && lock.workspace_source_digest.as_deref().is_some_and(|digest| {
            digest != "no-workspace-source"
        })
    {
        return None;
    }
    if workspace_source_present
        && lock.workspace_source_digest.as_deref() != Some(source_digest.as_str())
    {
        return None;
    }
    let is_index = workspace_source_role
        == Some(crate::WorkspacePlan::WorkspaceSourceRole::Index);
    if !is_index && !lock.workspace_members.is_empty() {
        // Only a checked workspace index may persist member-index facts.
        return None;
    }
    if is_index && !lock.workspace_members.is_empty() {
        let mut physical_paths = Vec::new();
        let mut names = Vec::new();
        for member in &lock.workspace_members {
            let relative = Path::new(&member.path);
            if member.name.is_empty()
                || member.path.is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
            {
                return None;
            }
            if member.source_digest != source_digest {
                return None;
            }
            if let Some(source) = &source {
                resolver.revalidate_source(source).ok()?;
            }
            let checked = resolver.checked_package(relative).ok()?;
            resolver.revalidate_member(&checked.member).ok()?;
            let physical_path = checked.member.directory.path.clone();
            let canonical_relative = resolver.relative_identity(&checked.member.directory).ok()?;
            if canonical_relative != member.canonical_path {
                return None;
            }
            let package = checked.facts;
            if package.name != member.name || !package.members.is_empty() {
                return None;
            }
            if member.package_digest.is_empty()
                || package.semantic_digest() != member.package_digest
            {
                return None;
            }
            if physical_paths.iter().any(|existing| existing == &physical_path)
                || names.iter().any(|existing| existing == &member.name)
            {
                return None;
            }
            physical_paths.push(physical_path);
            names.push(member.name.clone());
            if let Some(source) = &source {
                resolver.revalidate_source(source).ok()?;
            }
        }
    }
    let plan_source_digest = lock
        .workspace_members
        .first()
        .map(|member| member.source_digest.clone())
        .unwrap_or(source_digest);
    let members = if is_index {
        lock.workspace_members
            .into_iter()
            .map(|m: LockedWorkspaceMember| WorkspaceMember {
                name: m.name,
                path: m.path,
                canonical_path: m.canonical_path,
            })
            .collect()
    } else {
        Vec::new()
    };
    resolver.revalidate_file(&lock_file).ok()?;
    Some(WorkspacePlan {
        comptime_inputs: lock.comptime_inputs.clone(),
        overlay_policy: lock.workspace_overlay_policy,
        source_digest: plan_source_digest,
        members,
    })
}
