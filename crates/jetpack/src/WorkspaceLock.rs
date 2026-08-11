//! Workspace entries inside the unified `.jet/lock` file (D-WORKSPACELOCK1=A).
//!
//! The read path (`load`) now lives in `jet-pkg-model::WorkspaceLock` and is
//! re-exported here for call sites that use `jetpack::WorkspaceLock::load`.
//! The write path stays here because it needs `RuntimePolicy` for file locking.

pub use jet_pkg_model::WorkspaceLock::{load, WORKSPACE_LOCK};
use jet_pkg_model::Authority::AuthorityResolver;
use jet_pkg_model::WorkspacePlan::{WorkspacePlan, WorkspaceSourceRole};

use crate::{
    Lock::{self, LockFile, LockedWorkspaceMember},
    Syntax,
};
use std::path::Path;

/// Write workspace members into `.jet/lock` from a freshly evaluated
/// `WorkspacePlan`.
/// Creates `.jet/` if it doesn't exist. A failed write is returned to the
/// caller because a stale or partial workspace lock must never masquerade as
/// a valid index.
pub fn write(workspace_root: &Path, plan: &WorkspacePlan) -> Result<(), String> {
    let lock_path = workspace_root.join(WORKSPACE_LOCK);
    let Some(lock_dir) = lock_path.parent().map(Path::to_path_buf) else {
        return Err("workspace lock has no parent directory".to_string());
    };
    super::RuntimePolicy::with_project_lock(workspace_root, "workspace-lock", || {
        std::fs::create_dir_all(&lock_dir)?;
        let resolver = AuthorityResolver::open(workspace_root).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("workspace authority cannot be opened: {error}"),
            )
        })?;
        let existing_lock = match resolver.checked_file(Path::new(WORKSPACE_LOCK)) {
            Ok(file) => Some(file),
            Err(error) if error.is_missing() => None,
            Err(error) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("workspace lock cannot be opened: {error}"),
                ))
            }
        };
        let mut lock = match &existing_lock {
            Some(file) => {
                let raw = file.text().map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("existing lock is not valid text: {error}"),
                    )
                })?;
                resolver.revalidate_file(file).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("existing lock changed during read: {error}"),
                    )
                })?;
                jet_pkg_model::Lock::parse(&raw).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("existing lock is malformed: {error}"),
                    )
                })?
            }
            None => empty_lock(),
        };
        lock.version = Lock::LOCK_VERSION;
        let source = resolver
            .resolve_workspace_source()
            .map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("workspace authority cannot be resolved: {error}"),
                )
            })?;
        if let Some(source) = &source {
            if source.role != WorkspaceSourceRole::Index {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "workspace lock members require workspace.jet as the index",
                ));
            }
            let digest = jet_pkg_model::SHA256::sha256_hex(source.source.as_bytes());
            if !plan.source_digest.is_empty() && plan.source_digest != digest {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "workspace plan authority changed before lock write",
                ));
            }
            resolver.revalidate_source(source).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("workspace authority changed before lock write: {error}"),
                )
            })?;
        } else if !plan.members.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "workspace lock members require a checked workspace.jet index",
            ));
        }
        let source_digest = if !plan.source_digest.is_empty() {
            plan.source_digest.clone()
        } else {
            "no-workspace-source".to_string()
        };
        lock.workspace_members = plan
            .members
            .iter()
            .map(|m| {
                let relative = std::path::Path::new(&m.path);
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|component| component == std::path::Component::ParentDir)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("workspace member {} has an unsafe relative path", m.name),
                    ));
                }
                let checked = resolver.checked_package(relative).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("workspace member `{}` is not a checked Package: {error}", m.name),
                    )
                })?;
                resolver.revalidate_member(&checked.member).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("workspace member `{}` changed before lock write: {error}", m.name),
                    )
                })?;
                let canonical_relative = resolver
                    .relative_identity(&checked.member.directory)
                    .map_err(|error| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("workspace member {} has no relative identity: {error}", m.name),
                        )
                    })?;
                let package = checked.facts;
                Ok(LockedWorkspaceMember {
                    name: m.name.clone(),
                    path: m.path.clone(),
                    source_digest: source_digest.clone(),
                    canonical_path: canonical_relative,
                    package_digest: package.semantic_digest(),
                })
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        if let Some(source) = &source {
            resolver.revalidate_source(source).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("workspace authority changed during lock write: {error}"),
                )
            })?;
        }
        lock.workspace_source_digest = Some(source_digest);
        lock.workspace_overlay_policy = plan.overlay_policy.clone();
        // D-CTEFFECT1: fold the Tier-1 inputs the `members:` expression recorded
        // into the lock, keeping any inputs already recorded by other call sites.
        // Dedup by path so re-writing the lock is idempotent.
        for ci in &plan.comptime_inputs {
            if !lock.comptime_inputs.iter().any(|e| e.path == ci.path) {
                lock.comptime_inputs.push(ci.clone());
            }
        }
        if let Some(existing_lock) = &existing_lock {
            resolver.revalidate_file(existing_lock).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("existing lock changed before write: {error}"),
                )
            })?;
        } else {
            match resolver.checked_file(Path::new(WORKSPACE_LOCK)) {
                Ok(file) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("workspace lock appeared during write: {}", file.path.display()),
                    ));
                }
                Err(error) if error.is_missing() => {}
                Err(error) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("workspace lock changed during write: {error}"),
                    ));
                }
            }
        }
        std::fs::write(lock_path, Lock::write(&lock))
    })
    .map_err(|error| format!("could not write workspace lock: {error}"))
}

fn empty_lock() -> LockFile {
    LockFile {
        version: Lock::LOCK_VERSION,
        packages: Vec::new(),
        root_dependencies: Vec::new(),
        workspace_members: Vec::new(),
        workspace_source_digest: None,
        workspace_overlay_policy: Default::default(),
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
        write(&tmp, &plan).unwrap();
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
        write(&tmp, &plan).unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.members.len(), 2);
        assert_eq!(loaded.members[0].name, "hello");
        assert_eq!(loaded.members[0].path, "packages/hello");
        assert_eq!(loaded.members[1].name, "ranker");
        assert_eq!(loaded.members[1].path, "packages/ranker");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn roundtrip_with_workspace_root_member() {
        let tmp = std::env::temp_dir().join(format!(
            "wlock-root-member-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(Syntax::PACKAGE_FILE), "name: \"root\"\n").unwrap();
        let plan = WorkspacePlan {
            members: vec![member("root", ".")],
            ..Default::default()
        };
        write(&tmp, &plan).unwrap();
        let loaded = load(&tmp).expect("lock reload must preserve a root member");
        assert_eq!(loaded.members.len(), 1);
        assert_eq!(loaded.members[0].name, "root");
        assert_eq!(loaded.members[0].path, ".");
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
        let package_digest = jet_pkg_model::Package::PackageFacts::load(&canonical)
            .unwrap()
            .unwrap()
            .semantic_digest();
        std::fs::write(
            tmp.join(Syntax::UNIFIED_LOCK_FILE),
            format!(
                "version = 1\nworkspace_source_digest = \"no-workspace-source\"\n\n[[workspace_member]]\nname = \"hello\"\npath = \"packages/hello\"\nsource_digest = \"no-workspace-source\"\ncanonical_path = \"packages/hello\"\npackage_digest = \"{}\"\n",
                package_digest,
            ),
        )
        .unwrap();
        let plan = load(&tmp).unwrap();
        assert_eq!(plan.members.len(), 1);
        assert_eq!(plan.members[0].name, "hello");
        let moved = std::env::temp_dir().join(format!(
            "wlock-moved-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(moved.join(".jet")).unwrap();
        std::fs::create_dir_all(moved.join("packages/hello")).unwrap();
        std::fs::copy(
            tmp.join("packages/hello").join(Syntax::PACKAGE_FILE),
            moved.join("packages/hello").join(Syntax::PACKAGE_FILE),
        )
        .unwrap();
        std::fs::copy(
            tmp.join(Syntax::UNIFIED_LOCK_FILE),
            moved.join(Syntax::UNIFIED_LOCK_FILE),
        )
        .unwrap();
        assert!(
            load(&moved).is_some(),
            "workspace lock must survive checkout relocation"
        );
        std::fs::remove_dir_all(moved).ok();
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
        write(&tmp, &plan).unwrap();
        let raw = std::fs::read_to_string(tmp.join(Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(raw.contains("[[comptime_inputs]]"), "{raw}");
        assert!(raw.contains("assets/index.json"), "{raw}");
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.comptime_inputs.len(), 1);
        assert_eq!(loaded.comptime_inputs[0].path, "assets/index.json");
        // Idempotent re-write does not duplicate the input.
        write(&tmp, &loaded).unwrap();
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
        write(&tmp, &plan).unwrap();

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
    fn lock_load_rejects_a_workspace_digest_when_the_source_is_missing() {
        let tmp = std::env::temp_dir().join(format!(
            "wlock-missing-source-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(tmp.join(".jet")).unwrap();
        std::fs::write(
            tmp.join(Syntax::UNIFIED_LOCK_FILE),
            "version = 1\nworkspace_source_digest = \"sha256-stale\"\n",
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
        write_member_manifest(&tmp, "packages/hello", "hello");
        write(&tmp, &plan).unwrap();
        let raw = std::fs::read_to_string(tmp.join(Syntax::UNIFIED_LOCK_FILE)).unwrap();
        assert!(raw.contains("[[package]]"), "{raw}");
        assert!(raw.contains("name = \"dep\""), "{raw}");
        assert!(raw.contains("[[workspace_member]]"), "{raw}");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn write_rejects_malformed_existing_lock_without_overwriting_it() {
        let tmp = std::env::temp_dir().join(format!(
            "wlock-malformed-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(tmp.join(".jet")).unwrap();
        let lock_path = tmp.join(Syntax::UNIFIED_LOCK_FILE);
        let raw = "version = 1\n[[package]]\nname = \"broken\"\n";
        std::fs::write(&lock_path, raw).unwrap();
        let plan = WorkspacePlan::default();

        let error = write(&tmp, &plan).unwrap_err();

        assert!(error.contains("existing lock is malformed"), "{error}");
        assert_eq!(std::fs::read_to_string(&lock_path).unwrap(), raw);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
