//! Workspace entries inside the unified `.jet/lock` file (D-WORKSPACELOCK1=A).
//!
//! External tools that need a static workspace index (IDEs, CI scripts) can
//! read the `[[workspace_member]]` entries without evaluating Jet.
//!
//! Format:
//! ```toml
//! version = 1
//!
//! [[workspace_member]]
//! name = "hello"
//! path = "packages/hello"
//!
//! [[workspace_member]]
//! name = "ranker"
//! path = "packages/ranker"
//! ```

use crate::{
    Lock::{self, LockFile, LockedWorkspaceMember},
    Syntax,
};
use std::path::Path;

use super::WorkspaceFile::{WorkspaceMember, WorkspacePlan};

/// The lock file path within the workspace root.
pub const WORKSPACE_LOCK: &str = Syntax::UNIFIED_LOCK_FILE;

/// Write workspace members into `.jet/lock` from a freshly evaluated
/// `WorkspacePlan`.
/// Creates `.jet/` if it doesn't exist. Silently ignores write failures
/// (the lock is best-effort; the source of truth is `workspace.jet`).
pub fn write(workspace_root: &Path, plan: &WorkspacePlan) {
    let lock_path = workspace_root.join(WORKSPACE_LOCK);
    let Some(lock_dir) = lock_path.parent() else {
        return;
    };
    if std::fs::create_dir_all(lock_dir).is_err() {
        return;
    }
    let mut lock = Lock::load(workspace_root).unwrap_or_else(empty_lock);
    lock.version = Lock::LOCK_VERSION;
    lock.workspace_members = plan
        .members
        .iter()
        .map(|m| LockedWorkspaceMember {
            name: m.name.clone(),
            path: m.path.clone(),
        })
        .collect();
    // D-CTEFFECT1: fold the Tier-1 inputs the `members:` expression recorded
    // into the lock, keeping any inputs already recorded by other call sites.
    // Dedup by path so re-writing the lock is idempotent.
    for ci in &plan.comptime_inputs {
        if !lock.comptime_inputs.iter().any(|e| e.path == ci.path) {
            lock.comptime_inputs.push(ci.clone());
        }
    }
    let _ = std::fs::write(lock_path, Lock::write(&lock));
}

/// Load workspace members from `.jet/lock` in `workspace_root`. Returns `None`
/// when the file is absent. Returns an empty plan on parse failure (the lock is
/// best-effort; callers should fall back to evaluating `workspace.jet`).
pub fn load(workspace_root: &Path) -> Option<WorkspacePlan> {
    let lock = Lock::load(workspace_root)?;
    Some(WorkspacePlan {
        comptime_inputs: lock.comptime_inputs.clone(),
        members: lock
            .workspace_members
            .into_iter()
            .map(|m| WorkspaceMember {
                name: m.name,
                path: m.path,
            })
            .collect(),
    })
}

fn empty_lock() -> LockFile {
    LockFile {
        version: Lock::LOCK_VERSION,
        packages: Vec::new(),
        root_dependencies: Vec::new(),
        workspace_members: Vec::new(),
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
    }
}

// ── check that Syntax::PAYLOAD_FILE is accessible (used for doc purposes) ─────
const _: &str = Syntax::WORKSPACE_FILE;

#[cfg(test)]
mod tests {
    use super::super::WorkspaceFile::WorkspaceMember;
    use super::*;

    fn member(name: &str, path: &str) -> WorkspaceMember {
        WorkspaceMember {
            name: name.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn roundtrip_empty() {
        let plan = WorkspacePlan {
            members: vec![],
            ..Default::default()
        };
        let tmp = std::env::temp_dir().join(format!(
            "wlock-empty-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        write(&tmp, &plan);
        let lock_path = tmp.join(Syntax::UNIFIED_LOCK_FILE);
        assert!(
            lock_path.exists(),
            "workspace lock must be folded into {}",
            Syntax::UNIFIED_LOCK_FILE
        );
        let loaded = load(&tmp).unwrap();
        assert!(loaded.members.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn roundtrip_with_members() {
        let plan = WorkspacePlan {
            members: vec![
                member("hello", "packages/hello"),
                member("ranker", "packages/ranker"),
            ],
            ..Default::default()
        };
        let tmp = std::env::temp_dir().join(format!(
            "wlock-members-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        write(&tmp, &plan);
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.members.len(), 2);
        assert_eq!(loaded.members[0].name, "hello");
        assert_eq!(loaded.members[0].path, "packages/hello");
        assert_eq!(loaded.members[1].name, "ranker");
        assert_eq!(loaded.members[1].path, "packages/ranker");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn load_reads_workspace_members_from_unified_lock() {
        let tmp = std::env::temp_dir().join(format!(
            "wlock-parse-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(tmp.join(".jet")).unwrap();
        std::fs::write(
            tmp.join(Syntax::UNIFIED_LOCK_FILE),
            "version = 1\n\n[[workspace_member]]\nname = \"hello\"\npath = \"packages/hello\"\n",
        )
        .unwrap();
        let plan = load(&tmp).unwrap();
        assert_eq!(plan.members.len(), 1);
        assert_eq!(plan.members[0].name, "hello");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn comptime_inputs_round_trip_through_lock() {
        // Slice A (D-CTEFFECT1): a Tier-1 input a `members:` expression recorded
        // is written into `.jet/lock` and read back by `load`.
        use crate::AST::ComptimeInput;
        let tmp = std::env::temp_dir().join(format!(
            "wlock-ct-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let plan = WorkspacePlan {
            members: vec![member("hello", "packages/hello")],
            comptime_inputs: vec![ComptimeInput {
                path: "assets/index.json".to_string(),
                hash: "sha256-deadbeef".to_string(),
            }],
        };
        write(&tmp, &plan);
        let raw = std::fs::read_to_string(tmp.join(Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(raw.contains("[[comptime_inputs]]"), "{raw}");
        assert!(raw.contains("assets/index.json"), "{raw}");
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.comptime_inputs.len(), 1);
        assert_eq!(loaded.comptime_inputs[0].path, "assets/index.json");
        // Idempotent re-write does not duplicate the input.
        write(&tmp, &loaded);
        let reloaded = load(&tmp).unwrap();
        assert_eq!(reloaded.comptime_inputs.len(), 1);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn write_preserves_existing_package_entries() {
        let tmp = std::env::temp_dir().join(format!(
            "wlock-preserve-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(tmp.join(".jet")).unwrap();
        std::fs::write(
            tmp.join(Syntax::UNIFIED_LOCK_FILE),
            "version = 1\n\n[[package]]\nname = \"dep\"\nversion = \"1.2.3\"\nsource = { path = \"../dep\" }\nfingerprint = \"sha256-x\"\ndependencies = []\n\n[root]\ndependencies = [\"dep\"]\n",
        )
        .unwrap();
        let plan = WorkspacePlan {
            members: vec![member("hello", "packages/hello")],
            ..Default::default()
        };
        write(&tmp, &plan);
        let raw = std::fs::read_to_string(tmp.join(Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(raw.contains("[[package]]"), "{raw}");
        assert!(raw.contains("name = \"dep\""), "{raw}");
        assert!(raw.contains("[[workspace_member]]"), "{raw}");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
