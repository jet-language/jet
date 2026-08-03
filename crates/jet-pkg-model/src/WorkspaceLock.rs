//! Read-only workspace lock access (D-WORKSPACELOCK1=A).
//!
//! External tools (IDEs, CI scripts, Canvas) can call `WorkspaceLock::load`
//! to get a static workspace index from `.jet/lock` without evaluating Jet
//! and without depending on `jetpack`'s engine crate.
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

/// Load workspace members from `.jet/lock` in `workspace_root`. Returns `None`
/// when the file is absent. Returns an empty plan on parse failure (the lock is
/// best-effort; callers should fall back to evaluating `workspace.jet`).
pub fn load(workspace_root: &Path) -> Option<WorkspacePlan> {
    let lock = Lock::load(workspace_root)?;
    let workspace_source = workspace_root.join(Syntax::WORKSPACE_FILE);
    if workspace_source.exists() && lock.workspace_members.is_empty() {
        return None;
    }
    let source_digest = match std::fs::read(&workspace_source) {
        Ok(source) => crate::SHA256::sha256_hex(&source),
        Err(_) if workspace_source.exists() => return None,
        Err(_) => "no-workspace-source".to_string(),
    };
    if !lock.workspace_members.is_empty() {
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
            if !physical.starts_with(&root) || physical == root {
                return None;
            }
            if physical.to_string_lossy() != member.canonical_path {
                return None;
            }
            let package = PackageFacts::load(&physical)?.ok()?;
            if package.name != member.name || !package.members.is_empty() {
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
    Some(WorkspacePlan {
        comptime_inputs: lock.comptime_inputs.clone(),
        overlay_policy: Default::default(),
        source_digest: lock
            .workspace_members
            .first()
            .map(|member| member.source_digest.clone())
            .unwrap_or(source_digest),
        members: lock
            .workspace_members
            .into_iter()
            .map(|m: LockedWorkspaceMember| WorkspaceMember {
                name: m.name,
                path: m.path,
                canonical_path: m.canonical_path,
            })
            .collect(),
    })
}
