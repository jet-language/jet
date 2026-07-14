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
    Some(WorkspacePlan {
        comptime_inputs: lock.comptime_inputs.clone(),
        overlay_policy: Default::default(),
        members: lock
            .workspace_members
            .into_iter()
            .map(|m: LockedWorkspaceMember| WorkspaceMember {
                name: m.name,
                path: m.path,
            })
            .collect(),
    })
}
