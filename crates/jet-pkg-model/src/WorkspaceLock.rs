//! Read-only workspace lock access (D-WORKSPACELOCK1=A).
//!
//! External tools (IDEs, CI scripts, Canvas) can call `WorkspaceLock::load`
//! to get a static workspace index from `.jet/lock` without evaluating Jet
//! and without depending on `jetpack`'s engine crate. An arbitrary authority
//! may contribute persisted overlay policy through the same lock, but never
//! contributes or validates member-index facts.
//!
//! The write path (`WorkspaceLock::write`) needs `jetpack::RuntimePolicy` for
//! file locking; it lives in `jetpack::WorkspaceLock` and re-exports this
//! read path for callers that only need to read.

use crate::{
    Lock::{self, LockedWorkspaceMember},
    Package::PackageFacts,
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
    let lock = Lock::load(workspace_root)?;
    if lock.workspace_source_digest.is_none() {
        return None;
    }
    let (workspace_source_role, workspace_source_present, source_digest) = match
        crate::WorkspacePlan::resolve_workspace_source(workspace_root)
    {
        Some(Ok(source)) => (
            Some(source.role),
            true,
            crate::SHA256::sha256_hex(source.source.as_bytes()),
        ),
        Some(Err(_)) => return None,
        None => (None, false, "no-workspace-source".to_string()),
    };
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
        .is_none_or(|role| role == crate::WorkspacePlan::WorkspaceSourceRole::Index);
    if !is_index && !lock.workspace_members.is_empty() {
        // An arbitrary authority may persist overlay facts in this lock, but
        // its source is never a D-WORKSPACE2 member index.
        return None;
    }
    if is_index && !lock.workspace_members.is_empty() {
        let root = workspace_root.canonicalize().ok()?;
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
            let physical = workspace_root.join(relative).canonicalize().ok()?;
            if !physical.starts_with(&root) {
                return None;
            }
            let canonical_relative = physical
                .strip_prefix(&root)
                .ok()?
                .to_string_lossy()
                .into_owned();
            let canonical_relative = if canonical_relative.is_empty() {
                ".".to_string()
            } else {
                canonical_relative
            };
            if canonical_relative != member.canonical_path {
                return None;
            }
            let package = PackageFacts::load(&physical)?.ok()?;
            if package.name != member.name || !package.members.is_empty() {
                return None;
            }
            if member.package_digest.is_empty()
                || package.semantic_digest() != member.package_digest
            {
                return None;
            }
            if physical_paths.iter().any(|existing| existing == &physical)
                || names.iter().any(|existing| existing == &member.name)
            {
                return None;
            }
            physical_paths.push(physical);
            names.push(member.name.clone());
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
    Some(WorkspacePlan {
        comptime_inputs: lock.comptime_inputs.clone(),
        overlay_policy: lock.workspace_overlay_policy,
        source_digest: plan_source_digest,
        members,
    })
}
