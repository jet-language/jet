//! Workspace entries inside the unified `.jet/lock` file (D-WORKSPACELOCK1=A).
//!
//! The read path (`load`) now lives in `jet-pkg-model::WorkspaceLock` and is
//! re-exported here for call sites that use `jetpack::WorkspaceLock::load`.
//! The write path stays here because it needs `RuntimePolicy` for file locking.

pub use jet_pkg_model::WorkspaceLock::{load, WORKSPACE_LOCK};
use jet_pkg_model::WorkspacePlan::{WorkspacePlan};

use crate::{
    Lock::{self, LockFile, LockedWorkspaceMember},
    Syntax,
};
use std::path::Path;

/// Write workspace members into `.jet/lock` from a freshly evaluated
/// `WorkspacePlan`.
/// Creates `.jet/` if it doesn't exist. Silently ignores write failures
/// (the lock is best-effort; the source of truth is `workspace.jet`).
pub fn write(workspace_root: &Path, plan: &WorkspacePlan) {
    let lock_path = workspace_root.join(WORKSPACE_LOCK);
    let Some(lock_dir) = lock_path.parent().map(Path::to_path_buf) else {
        return;
    };
    let _ = super::RuntimePolicy::with_project_lock(workspace_root, "workspace-lock", || {
        if std::fs::create_dir_all(lock_dir).is_err() {
            return Ok(());
        }
        let mut lock = Lock::load(workspace_root).unwrap_or_else(empty_lock);
        lock.version = Lock::LOCK_VERSION;
        let source_digest = if !plan.source_digest.is_empty() {
            plan.source_digest.clone()
        } else if let Ok(source) = std::fs::read(workspace_root.join(Syntax::WORKSPACE_FILE)) {
            jet_pkg_model::SHA256::sha256_hex(&source)
        } else {
            "no-workspace-source".to_string()
        };
        lock.workspace_members = plan
            .members
            .iter()
            .map(|m| LockedWorkspaceMember {
                name: m.name.clone(),
                path: m.path.clone(),
                source_digest: source_digest.clone(),
                canonical_path: if !m.canonical_path.is_empty() {
                    m.canonical_path.clone()
                } else {
                    workspace_root
                        .join(&m.path)
                        .canonicalize()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default()
                },
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
        std::fs::write(lock_path, Lock::write(&lock))
    });
}

fn empty_lock() -> LockFile {
    LockFile {
        version: Lock::LOCK_VERSION,
        packages: Vec::new(),
        root_dependencies: Vec::new(),
        workspace_members: Vec::new(),
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
        browsers: Vec::new(),
        source_channels: Vec::new(),
    }
}

// ── check that Syntax::PAYLOAD_FILE is accessible (used for doc purposes) ─────
const _: &str = Syntax::WORKSPACE_FILE;

#[cfg(test)]
mod tests {
    use jet_pkg_model::WorkspacePlan::{WorkspaceMember, WorkspacePlan};
    use super::*;

    fn member(name: &str, path: &str) -> WorkspaceMember {
        WorkspaceMember {
            name: name.to_string(),
            path: path.to_string(),
            canonical_path: String::new(),
        }
    }

    fn write_member_manifest(root: &std::path::Path, path: &str, name: &str) {
        let dir = root.join(path);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(Syntax::PACKAGE_FILE),
            format!("name: \"{name}\"\n"),
        )
        .unwrap();
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
        write_member_manifest(&tmp, "packages/hello", "hello");
        write_member_manifest(&tmp, "packages/ranker", "ranker");
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
        write_member_manifest(&tmp, "packages/hello", "hello");
        let canonical = tmp.join("packages/hello").canonicalize().unwrap();
        std::fs::write(
            tmp.join(Syntax::UNIFIED_LOCK_FILE),
            format!(
                "version = 1\n\n[[workspace_member]]\nname = \"hello\"\npath = \"packages/hello\"\nsource_digest = \"no-workspace-source\"\ncanonical_path = \"{}\"\n",
                canonical.display()
            ),
        )
        .unwrap();
        let plan = load(&tmp).unwrap();
        assert_eq!(plan.members.len(), 1);
        assert_eq!(plan.members[0].name, "hello");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn comptime_inputs_round_trip_through_lock() {
        use crate::AST::ComptimeInput;
        let tmp = std::env::temp_dir().join(format!(
            "wlock-ct-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        write_member_manifest(&tmp, "packages/hello", "hello");
        let plan = WorkspacePlan {
            members: vec![member("hello", "packages/hello")],
            comptime_inputs: vec![ComptimeInput {
                path: "assets/index.json".to_string(),
                hash: "sha256-deadbeef".to_string(),
            }],
            overlay_policy: Default::default(),
            source_digest: String::new(),
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
    fn lock_load_revalidates_member_manifest_and_flat_membership() {
        let tmp = std::env::temp_dir().join(format!(
            "wlock-member-validation-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        write_member_manifest(&tmp, "packages/hello", "hello");
        let plan = WorkspacePlan {
            members: vec![member("hello", "packages/hello")],
            ..Default::default()
        };
        write(&tmp, &plan);

        std::fs::write(
            tmp.join("packages/hello").join(Syntax::PACKAGE_FILE),
            "name: \"other\"\n",
        )
        .unwrap();
        assert!(load(&tmp).is_none());

        std::fs::create_dir_all(tmp.join("packages/hello/child")).unwrap();
        std::fs::write(
            tmp.join("packages/hello/child").join(Syntax::PACKAGE_FILE),
            "name: \"child\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/hello").join(Syntax::PACKAGE_FILE),
            "name: \"hello\"\nmembers: [\"child\"]\n",
        )
        .unwrap();
        assert!(load(&tmp).is_none());
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
