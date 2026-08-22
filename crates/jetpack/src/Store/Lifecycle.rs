//! C3 lifecycle root journal.
//!
//! Producers replace roots in two durable steps. A prepared replacement
//! protects the union of the old and proposed target sets. Commit narrows that
//! protection to the exact proposed set. Manual external roots use the same
//! journal and lock through the typed atomic update seam below.

use super::{
    legacy_user_hangar_dir, legacy_user_root, Closure, PreparedProfileGenerationRoot, Roots,
};
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod Directory;
use Directory::PinnedDirectory;

const DB_DIR: &str = "lifecycle-db";
const JOURNAL_DIR: &str = "journal";
const PARTIAL_SUFFIX: &str = ".partial";
const TXN_SUFFIX: &str = ".txn";
const SNAPSHOT_FILE: &str = "snapshot";
const SNAPSHOT_PARTIAL: &str = "snapshot.partial";
const MAX_TRANSACTION_BYTES: usize = 2 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECOVERY_BYTES: usize = 32 * 1024 * 1024;
const MAX_JOURNAL_MEMBERS: usize = 512;
const COMPACT_AFTER_TRANSACTIONS: usize = 128;
const MAX_ROOTS: usize = 4096;
const MAX_TARGETS_PER_ROOT: usize = 4096;
const MAX_TOTAL_TARGETS: usize = 65_536;
const MAX_RECOVERY_WORK: usize = 1_000_000;

fn node_metadata(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn existing_hangar_destination(path: &Path) -> io::Result<bool> {
    let Some(metadata) = node_metadata(path)? else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "native Hangar path `{}` is a symlink",
            path.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "native Hangar path `{}` is not a directory",
            path.display()
        )));
    }
    Ok(true)
}

fn find_legacy_source(legacy_source: &Path) -> io::Result<Option<PathBuf>> {
    let mut sources = vec![
        legacy_source.to_path_buf(),
        PathBuf::from(crate::Syntax::HANGAR_DIR),
    ];
    sources.dedup();
    for candidate in sources {
        if node_metadata(&candidate)?.is_some() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Move a pre-D-ECO-HANGARPATH1 user Hangar into the native per-user data
/// path. The old tree stays in place, so an operator can roll back by removing
/// the new tree and restoring the old resolver. A staging tree makes a crash
/// visible instead of presenting a partial migration as a live Hangar.
pub fn migrate_legacy_hangar(roots: &Roots) -> std::io::Result<bool> {
    if !roots.dev_mode {
        return Ok(false);
    }
    let destination = roots.hangar_dir();
    let parent = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("native Hangar path has no parent"))?;
    let stage = parent.join(format!(
        ".{}-migration.partial",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("hangar")
    ));
    let legacy_source = legacy_user_hangar_dir();
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        if node_metadata(&stage)?.is_some() {
            return Err(std::io::Error::other(format!(
                "incomplete Hangar migration remains at `{}`; inspect or remove it before retrying",
                stage.display()
            )));
        }
        if existing_hangar_destination(&destination)? {
            return Ok(false);
        }
        let Some(source) = find_legacy_source(&legacy_source)? else {
            return Ok(false);
        };
        if source == destination {
            return Ok(false);
        }
        let migrate_unlocked = || {
            let Some(source_metadata) = node_metadata(&source)? else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("legacy Hangar source `{}` disappeared", source.display()),
                ));
            };
            if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
                return Err(io::Error::other(format!(
                    "legacy Hangar source `{}` is not a directory",
                    source.display()
                )));
            }
            fs::create_dir_all(parent)?;
            copy_migration_tree(&source, &stage, &source)?;
            super::fsync_tree(&stage)?;
            fs::rename(&stage, &destination)?;
            super::sync_store_directory(parent)?;
            Ok(true)
        };
        if source == legacy_source {
            let source_root = legacy_user_root();
            if source_root.as_path() == roots.root.as_path() {
                migrate_unlocked()
            } else {
                super::super::RuntimePolicy::with_lock(&source_root, "hangar", migrate_unlocked)
            }
        } else {
            migrate_unlocked()
        }
    })
}

fn copy_migration_tree(source: &Path, destination: &Path, root: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        if target.is_absolute() {
            if !absolute_target_stays_in_nix_store(&target) {
                return Err(std::io::Error::other(format!(
                    "legacy Hangar symlink `{}` escapes the approved compatibility root",
                    source.display()
                )));
            }
        } else if !relative_target_stays_in_root(source, &target, root) {
            return Err(std::io::Error::other(format!(
                "legacy Hangar symlink `{}` escapes its migration root",
                source.display()
            )));
        }
        create_migration_symlink(&target, destination, source)?;
        return Ok(());
    }
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_migration_tree(&entry.path(), &destination.join(entry.file_name()), root)?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }
    if metadata.is_file() {
        fs::copy(source, destination)?;
        fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "legacy Hangar contains unsupported node `{}`",
        source.display()
    )))
}

fn relative_target_stays_in_root(link: &Path, target: &Path, root: &Path) -> bool {
    let Some(parent) = link.parent() else {
        return false;
    };
    let Ok(relative_parent) = parent.strip_prefix(root) else {
        return false;
    };
    let mut normalized = root.join(relative_parent);
    for component in target.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => normalized.push(name),
            std::path::Component::ParentDir => {
                if normalized == root || !normalized.pop() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if !normalized.starts_with(root) {
        return false;
    }
    let canonical_root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(_) => return false,
    };
    existing_path_stays_in_root(&parent.join(target), &canonical_root)
}

fn absolute_target_stays_in_nix_store(target: &Path) -> bool {
    let compatibility_root = Path::new("/nix/store");
    let Ok(relative) = target.strip_prefix(compatibility_root) else {
        return false;
    };
    let mut normalized = compatibility_root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => normalized.push(name),
            std::path::Component::ParentDir => {
                if normalized == compatibility_root || !normalized.pop() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if !normalized.starts_with(compatibility_root) {
        return false;
    }
    let Ok(canonical_root) = fs::canonicalize(compatibility_root) else {
        return true;
    };
    existing_path_stays_in_root(target, &canonical_root)
}

fn existing_path_stays_in_root(path: &Path, canonical_root: &Path) -> bool {
    let mut probe = path.to_path_buf();
    loop {
        match fs::canonicalize(&probe) {
            Ok(canonical) => return canonical.starts_with(canonical_root),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(parent) = probe.parent().map(Path::to_path_buf) else {
                    return false;
                };
                if parent == probe {
                    return false;
                }
                probe = parent;
            }
            Err(_) => return false,
        }
    }
}

fn create_migration_symlink(
    target: &Path,
    destination: &Path,
    source: &Path,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // `source` only tells Windows whether to make a dir or file symlink.
        let _ = source;
        std::os::unix::fs::symlink(target, destination)
    }
    #[cfg(windows)]
    {
        let target_is_dir = fs::metadata(source)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false);
        if target_is_dir {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, destination, source);
        Err(std::io::Error::other(
            "legacy Hangar migration needs symlink support on this host",
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RootKind {
    ProjectLock,
    ProfileGeneration,
    Toolchain,
    ExternalConsumer,
    Manual,
}

impl RootKind {
    fn wire(self) -> &'static str {
        match self {
            Self::ProjectLock => "project-lock",
            Self::ProfileGeneration => "profile-generation",
            Self::Toolchain => "toolchain",
            Self::ExternalConsumer => "external-consumer",
            Self::Manual => "manual",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "project-lock" => Ok(Self::ProjectLock),
            "profile-generation" => Ok(Self::ProfileGeneration),
            "toolchain" => Ok(Self::Toolchain),
            "external-consumer" => Ok(Self::ExternalConsumer),
            "manual" => Ok(Self::Manual),
            _ => Err(format!("unknown lifecycle root kind `{value}`")),
        }
    }
}

macro_rules! text_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        pub(crate) struct $name(String);

        impl $name {
            pub(crate) fn new(value: impl Into<String>) -> io::Result<Self> {
                let value = value.into();
                validate_identity($label, &value)?;
                Ok(Self(value))
            }

            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

text_id!(RootId, "root id");
text_id!(ProducerId, "producer");
text_id!(RootWitness, "witness");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Incarnation(u64);

impl Incarnation {
    pub(crate) fn new(value: u64) -> io::Result<Self> {
        if value == 0 {
            return Err(invalid("lifecycle incarnation must be nonzero"));
        }
        Ok(Self(value))
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LifecycleTimestamp(u64);

impl LifecycleTimestamp {
    pub(crate) fn from_unix_seconds(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

/// Compare-and-swap token for a durable root incarnation.
///
/// The first component is the never-reused incarnation. The second component
/// is the durable root revision. Both are persisted in the lifecycle state so
/// a retry after a crash cannot accidentally accept an older mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RootEtag {
    incarnation: u64,
    revision: u64,
}

impl RootEtag {
    pub(crate) fn new(incarnation: u64, revision: u64) -> io::Result<Self> {
        Incarnation::new(incarnation)?;
        if revision == 0 {
            return Err(invalid("lifecycle root revision must be nonzero"));
        }
        Ok(Self {
            incarnation,
            revision,
        })
    }

    pub(crate) fn parse(value: &str) -> io::Result<Self> {
        let (incarnation, revision) = value
            .split_once('.')
            .ok_or_else(|| invalid("lifecycle root etag must be `<incarnation>.<revision>`"))?;
        if revision.contains('.') {
            return Err(invalid("lifecycle root etag has too many components"));
        }
        let incarnation = incarnation
            .parse::<u64>()
            .map_err(|_| invalid("lifecycle root etag has an invalid incarnation"))?;
        let revision = revision
            .parse::<u64>()
            .map_err(|_| invalid("lifecycle root etag has an invalid revision"))?;
        // A zero revision is accepted for a stale caller token such as 1.0;
        // durable roots themselves always carry a nonzero revision.
        Incarnation::new(incarnation)?;
        Ok(Self {
            incarnation,
            revision,
        })
    }

    pub(crate) fn render(self) -> String {
        format!("{}.{}", self.incarnation, self.revision)
    }

    fn next_after(root: &LifecycleRoot) -> io::Result<Self> {
        Self::new(
            root.identity
                .incarnation
                .get()
                .checked_add(1)
                .ok_or_else(|| invalid("lifecycle incarnation overflow"))?,
            root.revision
                .checked_add(1)
                .ok_or_else(|| invalid("lifecycle root revision overflow"))?,
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RootMetadata {
    pub(crate) label: Option<String>,
    pub(crate) reference: Option<String>,
    pub(crate) expires_at: Option<LifecycleTimestamp>,
}

impl RootMetadata {
    pub(crate) fn manual(
        label: &str,
        reference: &str,
        expires_at: Option<LifecycleTimestamp>,
    ) -> io::Result<Self> {
        let metadata = Self {
            label: Some(label.to_string()),
            reference: Some(reference.to_string()),
            expires_at,
        };
        metadata.validate()?;
        Ok(metadata)
    }

    fn validate(&self) -> io::Result<()> {
        if let Some(label) = &self.label {
            validate_identity("root label", label)?;
        }
        if let Some(reference) = &self.reference {
            validate_identity("root reference", reference)?;
        }
        Ok(())
    }

    pub(crate) fn is_expired(&self, now: LifecycleTimestamp) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

#[derive(Debug)]
pub(crate) struct CasConflict {
    pub(crate) expected: Option<String>,
    pub(crate) current: Option<String>,
}

impl fmt::Display for CasConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lifecycle root compare-and-swap conflict")
    }
}

impl Error for CasConflict {}

#[derive(Debug)]
pub(crate) struct RootNotFound;

impl fmt::Display for RootNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lifecycle root was not found")
    }
}

impl Error for RootNotFound {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootIdentity {
    pub(crate) kind: RootKind,
    pub(crate) id: RootId,
    pub(crate) producer: ProducerId,
    pub(crate) incarnation: Incarnation,
    pub(crate) witness: RootWitness,
}

impl RootIdentity {
    pub(crate) fn new(
        kind: RootKind,
        id: RootId,
        producer: ProducerId,
        incarnation: Incarnation,
        witness: RootWitness,
    ) -> Self {
        Self {
            kind,
            id,
            producer,
            incarnation,
            witness,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootPhase {
    Prepared,
    Committed,
    Tombstoned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifecycleRoot {
    pub(crate) identity: RootIdentity,
    /// Exact targets requested by this incarnation.
    pub(crate) targets: BTreeSet<String>,
    /// Targets protected during replay. Prepared roots contain old union new.
    pub(crate) protected_targets: BTreeSet<String>,
    pub(crate) phase: RootPhase,
    pub(crate) prepared_at: LifecycleTimestamp,
    pub(crate) committed_at: Option<LifecycleTimestamp>,
    pub(crate) tombstoned_at: Option<LifecycleTimestamp>,
    pub(crate) metadata: RootMetadata,
    pub(crate) revision: u64,
    legacy: bool,
}

impl LifecycleRoot {
    pub(crate) fn etag(&self) -> RootEtag {
        // State validation guarantees this pair is nonzero. Keeping this
        // accessor infallible makes read-only projections straightforward.
        RootEtag {
            incarnation: self.identity.incarnation.get(),
            revision: self.revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifecycleRevision(String);

impl LifecycleRevision {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoreRevision {
    pub(crate) lifecycle: LifecycleRevision,
    pub(crate) closure_head: String,
    digest: String,
}

impl StoreRevision {
    #[allow(dead_code)]
    pub(crate) fn as_str(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LifecycleSnapshot {
    pub(crate) roots: BTreeMap<RootId, LifecycleRoot>,
    pub(crate) protected_targets: BTreeSet<String>,
    pub(crate) revision: LifecycleRevision,
    pub(crate) store_revision: StoreRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRoot {
    pub(crate) identity: RootIdentity,
    pub(crate) targets: Vec<String>,
    pub(crate) observed_at: LifecycleTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JournalEntry {
    Prepare {
        identity: RootIdentity,
        proposed: BTreeSet<String>,
        protected: BTreeSet<String>,
        metadata: RootMetadata,
        revision: u64,
        at: LifecycleTimestamp,
    },
    Commit {
        id: RootId,
        incarnation: Incarnation,
        witness: RootWitness,
        at: LifecycleTimestamp,
    },
    Tombstone {
        id: RootId,
        incarnation: Incarnation,
        witness: RootWitness,
        at: LifecycleTimestamp,
    },
    Legacy {
        identity: RootIdentity,
        targets: BTreeSet<String>,
        at: LifecycleTimestamp,
    },
}

/// One complete replacement request. The caller supplies the stable typed
/// identity and expected token; this module owns incarnation allocation,
/// old-union-new protection, and the prepare/commit journal sequence.
pub(crate) struct RootUpdate {
    pub(crate) identity: RootIdentity,
    pub(crate) targets: Vec<String>,
    pub(crate) metadata: RootMetadata,
    #[allow(dead_code)]
    pub(crate) expected_etag: Option<String>,
    pub(crate) at: LifecycleTimestamp,
}

pub(crate) fn prepare(
    roots: &Roots,
    identity: RootIdentity,
    targets: Vec<String>,
    at: LifecycleTimestamp,
) -> io::Result<LifecycleSnapshot> {
    with_lifecycle_lock(roots, |known, closure_head| {
        prepare_unlocked(roots, identity, targets, at, known)?;
        snapshot_unlocked(roots, known, closure_head, &[])
    })
}

pub(crate) fn commit(
    roots: &Roots,
    id: &RootId,
    incarnation: Incarnation,
    witness: &RootWitness,
    at: LifecycleTimestamp,
) -> io::Result<LifecycleSnapshot> {
    with_lifecycle_lock(roots, |known, closure_head| {
        let entry = JournalEntry::Commit {
            id: id.clone(),
            incarnation,
            witness: witness.clone(),
            at,
        };
        let mut state = load_state(roots, known)?;
        if !entry_already_applied(&state, &entry) {
            apply_entry(&mut state, entry.clone())?;
            persist_entry(roots, &entry, &state, WriteControl::none())?;
        } else {
            compact_if_needed(roots, &state)?;
        }
        snapshot_from_state(state, closure_head)
    })
}

#[allow(dead_code)]
pub(crate) fn remove_root(
    roots: &Roots,
    id: &RootId,
    incarnation: Incarnation,
    witness: &RootWitness,
    at: LifecycleTimestamp,
) -> io::Result<LifecycleSnapshot> {
    with_lifecycle_lock(roots, |known, closure_head| {
        let entry = JournalEntry::Tombstone {
            id: id.clone(),
            incarnation,
            witness: witness.clone(),
            at,
        };
        let mut state = load_state(roots, known)?;
        if !entry_already_applied(&state, &entry) {
            apply_entry(&mut state, entry.clone())?;
            persist_entry(roots, &entry, &state, WriteControl::none())?;
        } else {
            compact_if_needed(roots, &state)?;
        }
        snapshot_from_state(state, closure_head)
    })
}

/// Atomically replace one typed root. The prepare and commit records are
/// serialized while one Hangar lock is held. If the process dies between
/// records, the prepared union remains protected and the same request resumes
/// by witness/metadata/target identity instead of silently rebinding.
#[allow(dead_code)]
pub(crate) fn atomic_update(
    roots: &Roots,
    update: RootUpdate,
) -> io::Result<LifecycleSnapshot> {
    let expected = update
        .expected_etag
        .as_deref()
        .map(RootEtag::parse)
        .transpose()?;
    with_lifecycle_lock(roots, |known, closure_head| {
        atomic_update_unlocked(roots, update, expected, known, closure_head)
    })
}

fn atomic_update_unlocked(
    roots: &Roots,
    update: RootUpdate,
    expected: Option<RootEtag>,
    known: &BTreeSet<String>,
    closure_head: String,
) -> io::Result<LifecycleSnapshot> {
    let proposed = checked_targets(update.targets.clone(), known)?;
    let mut state = load_state(roots, known)?;
    let current = state.get(&update.identity.id).cloned();

    let (identity, revision, protected) = match current.as_ref() {
        Some(root) if root.phase == RootPhase::Prepared => {
            if !same_request(root, &update, &proposed) {
                return Err(cas_conflict(expected, Some(root.etag())));
            }
            let current_etag = root.etag();
            let previous_etag = previous_etag(root);
            if expected.is_some_and(|value| {
                value != current_etag && Some(value) != previous_etag
            }) {
                return Err(cas_conflict(expected, Some(current_etag)));
            }
            (
                root.identity.clone(),
                root.revision,
                root.protected_targets.clone(),
            )
        }
        Some(root) => {
            let current_etag = root.etag();
            let same = root.phase == RootPhase::Committed
                && same_request(root, &update, &proposed);
            let previous_etag = previous_etag(root);
            if expected.is_some_and(|value| {
                value != current_etag && !(same && Some(value) == previous_etag)
            }) {
                return Err(cas_conflict(expected, Some(current_etag)));
            }
            if same {
                return snapshot_from_state(state, closure_head);
            }
            if root.phase != RootPhase::Tombstoned && expected != Some(current_etag) {
                return Err(cas_conflict(expected, Some(current_etag)));
            }
            if root.identity.kind != update.identity.kind
                || root.identity.producer != update.identity.producer
            {
                return Err(invalid("lifecycle root identity changed kind or producer"));
            }
            let next = RootEtag::next_after(root)?;
            let identity = RootIdentity::new(
                update.identity.kind,
                update.identity.id.clone(),
                update.identity.producer.clone(),
                Incarnation::new(next.incarnation)?,
                update.identity.witness.clone(),
            );
            let mut protected = proposed.clone();
            protected.extend(root.protected_targets.iter().cloned());
            (identity, next.revision, protected)
        }
        None => {
            if expected.is_some() {
                return Err(cas_conflict(expected, None));
            }
            (
                RootIdentity::new(
                    update.identity.kind,
                    update.identity.id.clone(),
                    update.identity.producer.clone(),
                    Incarnation::new(1)?,
                    update.identity.witness.clone(),
                ),
                1,
                proposed.clone(),
            )
        }
    };
    let prepare = JournalEntry::Prepare {
        identity: identity.clone(),
        proposed,
        protected,
        metadata: update.metadata,
        revision,
        at: update.at,
    };
    if !entry_already_applied(&state, &prepare) {
        apply_entry(&mut state, prepare.clone())?;
        persist_entry(roots, &prepare, &state, WriteControl::none())?;
    }
    let commit = JournalEntry::Commit {
        id: identity.id.clone(),
        incarnation: identity.incarnation,
        witness: identity.witness.clone(),
        at: update.at,
    };
    if !entry_already_applied(&state, &commit) {
        apply_entry(&mut state, commit.clone())?;
        persist_entry(roots, &commit, &state, WriteControl::none())?;
    }
    snapshot_from_state(state, closure_head)
}

/// Prepare an immutable root only if its stable identity is absent. The
/// existence check and journal append share one lock; callers cannot observe
/// an absent root and then race a different producer into its key.
pub(crate) fn prepare_if_absent(
    roots: &Roots,
    identity: RootIdentity,
    targets: Vec<String>,
    at: LifecycleTimestamp,
) -> io::Result<LifecycleSnapshot> {
    with_lifecycle_lock(roots, |known, closure_head| {
        let proposed = checked_targets(targets.clone(), known)?;
        let state = load_state(roots, known)?;
        if let Some(root) = state.get(&identity.id) {
            if root.phase == RootPhase::Tombstoned
                || root.identity != identity
                || root.targets != proposed
                || root.metadata != RootMetadata::default()
            {
                return Err(invalid("lifecycle root disagrees with immutable metadata"));
            }
            return snapshot_from_state(state, closure_head);
        }
        prepare_unlocked(roots, identity, targets, at, known)?;
        let state = load_state(roots, known)?;
        snapshot_from_state(state, closure_head)
    })
}

/// Tombstone a typed root with one compare-and-swap check. Repeating the same
/// removal after the tombstone is durable is an idempotent success.
pub(crate) fn atomic_remove(
    roots: &Roots,
    id: &RootId,
    expected_etag: &str,
    at: LifecycleTimestamp,
) -> io::Result<LifecycleSnapshot> {
    let expected = RootEtag::parse(expected_etag)?;
    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_unlocked(roots)?;
        let (known, closure_head) = Closure::lifecycle_inputs_unlocked(roots)?;
        let mut state = load_state(roots, &known)?;
        let root = state
            .get(id)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, RootNotFound))?;
        if root.etag() != expected {
            return Err(cas_conflict(Some(expected), Some(root.etag())));
        }
        if root.phase == RootPhase::Tombstoned {
            return snapshot_from_state(state, closure_head);
        }
        if root.phase == RootPhase::Prepared {
            let commit = JournalEntry::Commit {
                id: root.identity.id.clone(),
                incarnation: root.identity.incarnation,
                witness: root.identity.witness.clone(),
                at,
            };
            apply_entry(&mut state, commit.clone())?;
            persist_entry(roots, &commit, &state, WriteControl::none())?;
        }
        let tombstone = JournalEntry::Tombstone {
            id: root.identity.id.clone(),
            incarnation: root.identity.incarnation,
            witness: root.identity.witness,
            at,
        };
        if !entry_already_applied(&state, &tombstone) {
            apply_entry(&mut state, tombstone.clone())?;
            persist_entry(roots, &tombstone, &state, WriteControl::none())?;
        }
        snapshot_from_state(state, closure_head)
    })
}

fn same_request(
    root: &LifecycleRoot,
    update: &RootUpdate,
    proposed: &BTreeSet<String>,
) -> bool {
    root.identity.kind == update.identity.kind
        && root.identity.producer == update.identity.producer
        && root.identity.witness == update.identity.witness
        && root.targets == *proposed
        && root.metadata == update.metadata
}

fn previous_etag(root: &LifecycleRoot) -> Option<RootEtag> {
    (root.identity.incarnation.get() > 1 && root.revision > 1).then(|| RootEtag {
        incarnation: root.identity.incarnation.get() - 1,
        revision: root.revision - 1,
    })
}

fn cas_conflict(expected: Option<RootEtag>, current: Option<RootEtag>) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        CasConflict {
            expected: expected.map(RootEtag::render),
            current: current.map(RootEtag::render),
        },
    )
}

#[allow(dead_code)]
pub(crate) fn import_legacy_root(
    roots: &Roots,
    legacy: LegacyRoot,
) -> io::Result<LifecycleSnapshot> {
    with_lifecycle_lock(roots, |known, closure_head| {
        let targets = checked_targets(legacy.targets, known)?;
        let entry = JournalEntry::Legacy {
            identity: legacy.identity,
            targets,
            at: legacy.observed_at,
        };
        let mut state = load_state(roots, known)?;
        if !entry_already_applied(&state, &entry) {
            apply_entry(&mut state, entry.clone())?;
            persist_entry(roots, &entry, &state, WriteControl::none())?;
        } else {
            compact_if_needed(roots, &state)?;
        }
        snapshot_from_state(state, closure_head)
    })
}

#[allow(dead_code)]
pub(crate) fn list(roots: &Roots) -> io::Result<Vec<LifecycleRoot>> {
    Ok(snapshot(roots)?.roots.into_values().collect())
}

pub(crate) fn snapshot(roots: &Roots) -> io::Result<LifecycleSnapshot> {
    snapshot_with_legacy(roots, &[])
}

/// Caller already owns the Hangar lock. Clean uses this exact protected set
/// so dropping a short-lived verification lease cannot expose durable roots.
pub(super) fn protected_targets_unlocked(roots: &Roots) -> io::Result<BTreeSet<String>> {
    recover_unlocked(roots)?;
    let (known, _) = Closure::lifecycle_inputs_unlocked(roots)?;
    let state = load_state(roots, &known)?;
    validate_state_bounds(&state)?;
    let now = unix_now();
    Ok(state
        .values()
        .filter(|root| {
            root.phase != RootPhase::Tombstoned && !root.metadata.is_expired(now)
        })
        .flat_map(|root| root.protected_targets.iter().cloned())
        .collect())
}

pub(crate) fn snapshot_with_legacy(
    roots: &Roots,
    legacy: &[LegacyRoot],
) -> io::Result<LifecycleSnapshot> {
    with_lifecycle_lock(roots, |known, closure_head| {
        snapshot_unlocked(roots, known, closure_head, legacy)
    })
}

#[allow(dead_code)]
pub(crate) fn recover(roots: &Roots) -> io::Result<usize> {
    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let recovered = recover_unlocked(roots)?;
        let (known, _) = Closure::lifecycle_inputs_unlocked(roots)?;
        let state = load_state(roots, &known)?;
        compact_if_needed(roots, &state)?;
        Ok(recovered)
    })
}

fn with_lifecycle_lock<T>(
    roots: &Roots,
    operation: impl FnOnce(&BTreeSet<String>, String) -> io::Result<T>,
) -> io::Result<T> {
    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_unlocked(roots)?;
        let (known, closure_head) = Closure::lifecycle_inputs_unlocked(roots)?;
        operation(&known, closure_head)
    })
}

fn prepare_unlocked(
    roots: &Roots,
    identity: RootIdentity,
    targets: Vec<String>,
    at: LifecycleTimestamp,
    known: &BTreeSet<String>,
) -> io::Result<()> {
    prepare_unlocked_controlled(roots, identity, targets, at, known, WriteControl::none())
}

fn prepare_unlocked_controlled(
    roots: &Roots,
    identity: RootIdentity,
    targets: Vec<String>,
    at: LifecycleTimestamp,
    known: &BTreeSet<String>,
    control: WriteControl,
) -> io::Result<()> {
    let proposed = checked_targets(targets, known)?;
    let mut state = load_state(roots, known)?;
    let mut protected = proposed.clone();
    let revision = if let Some(old) = state.get(&identity.id) {
        protected.extend(old.protected_targets.iter().cloned());
        if old.phase == RootPhase::Prepared
            && old.identity == identity
            && old.targets == proposed
            && old.protected_targets == protected
        {
            old.revision
        } else {
            old.revision
                .checked_add(1)
                .ok_or_else(|| invalid("lifecycle root revision overflow"))?
        }
    } else {
        1
    };
    let entry = JournalEntry::Prepare {
        identity,
        proposed,
        protected,
        metadata: RootMetadata::default(),
        revision,
        at,
    };
    if !entry_already_applied(&state, &entry) {
        apply_entry(&mut state, entry.clone())?;
        persist_entry(roots, &entry, &state, control)
    } else {
        compact_if_needed(roots, &state)
    }
}

fn snapshot_unlocked(
    roots: &Roots,
    known: &BTreeSet<String>,
    closure_head: String,
    legacy: &[LegacyRoot],
) -> io::Result<LifecycleSnapshot> {
    let mut state = load_state(roots, known)?;
    for root in legacy {
        let targets = checked_targets(root.targets.clone(), known)?;
        if state.contains_key(&root.identity.id) {
            continue;
        }
        apply_entry(
            &mut state,
            JournalEntry::Legacy {
                identity: root.identity.clone(),
                targets,
                at: root.observed_at,
            },
        )?;
    }
    snapshot_from_state(state, closure_head)
}

fn snapshot_from_state(
    roots: BTreeMap<RootId, LifecycleRoot>,
    closure_head: String,
) -> io::Result<LifecycleSnapshot> {
    validate_state_bounds(&roots)?;
    let now = unix_now();
    let protected_targets = roots
        .values()
        .filter(|root| {
            root.phase != RootPhase::Tombstoned && !root.metadata.is_expired(now)
        })
        .flat_map(|root| root.protected_targets.iter().cloned())
        .collect::<BTreeSet<_>>();
    let canonical = canonical_state(&roots);
    let revision = LifecycleRevision(format!(
        "sha256-{}",
        SHA256::sha256_hex(canonical.as_bytes())
    ));
    let composite = format!(
        "jet-store-revision-v1\nclosure\t{}\nlifecycle\t{}\n",
        closure_head,
        revision.as_str()
    );
    let store_revision = StoreRevision {
        lifecycle: revision.clone(),
        closure_head,
        digest: format!("sha256-{}", SHA256::sha256_hex(composite.as_bytes())),
    };
    Ok(LifecycleSnapshot {
        roots,
        protected_targets,
        revision,
        store_revision,
    })
}

fn load_state(
    roots: &Roots,
    known: &BTreeSet<String>,
) -> io::Result<BTreeMap<RootId, LifecycleRoot>> {
    let journal = ensure_journal_dir(roots)?;
    let scan = scan_journal(&journal)?;
    let through = scan.snapshot.as_ref().map(|value| value.0).unwrap_or(0);
    let mut state = scan
        .snapshot
        .map(|(_, roots)| roots)
        .unwrap_or_default();
    validate_state_bounds(&state)?;
    validate_state_targets(&state, known)?;
    let mut work = state
        .values()
        .map(|root| root.targets.len() + root.protected_targets.len() + 1)
        .sum::<usize>();
    for (sequence, name, entry) in scan.transactions {
        if sequence <= through {
            continue;
        }
        work = work
            .checked_add(entry_work(&entry))
            .ok_or_else(|| invalid("lifecycle recovery work overflow"))?;
        if work > MAX_RECOVERY_WORK {
            return Err(invalid("lifecycle recovery exceeds work bound"));
        }
        validate_entry_targets(&entry, known)
            .map_err(|error| corrupt_name(&journal, &name, error))?;
        apply_entry(&mut state, entry)
            .map_err(|error| corrupt_name(&journal, &name, error.to_string()))?;
        validate_state_bounds(&state)?;
    }
    Ok(state)
}

fn entry_already_applied(
    state: &BTreeMap<RootId, LifecycleRoot>,
    entry: &JournalEntry,
) -> bool {
    match entry {
        JournalEntry::Prepare {
            identity,
            proposed,
            protected,
            metadata,
            revision,
            at: _,
        } => state.get(&identity.id).is_some_and(|root| {
            root.phase == RootPhase::Prepared
                && root.identity == *identity
                && root.targets == *proposed
                && root.protected_targets == *protected
                && root.metadata == *metadata
                && root.revision == *revision
                && root.committed_at.is_none()
                && root.tombstoned_at.is_none()
        }),
        JournalEntry::Commit {
            id,
            incarnation,
            witness,
            at: _,
        } => state.get(id).is_some_and(|root| {
            root.phase == RootPhase::Committed
                && root.identity.incarnation == *incarnation
                && root.identity.witness == *witness
                && root.committed_at.is_some()
                && root.tombstoned_at.is_none()
        }),
        JournalEntry::Tombstone {
            id,
            incarnation,
            witness,
            at: _,
        } => state.get(id).is_some_and(|root| {
            root.phase == RootPhase::Tombstoned
                && root.identity.incarnation == *incarnation
                && root.identity.witness == *witness
                && root.tombstoned_at.is_some()
        }),
        JournalEntry::Legacy {
            identity,
            targets,
            at: _,
        } => state.get(&identity.id).is_some_and(|root| {
            root.legacy
                && root.phase == RootPhase::Committed
                && root.identity == *identity
                && root.targets == *targets
                && root.protected_targets == *targets
                && root.committed_at.is_some()
        }),
    }
}

fn entry_work(entry: &JournalEntry) -> usize {
    match entry {
        JournalEntry::Prepare {
            proposed,
            protected,
            ..
        } => proposed.len() + protected.len() + 1,
        JournalEntry::Legacy { targets, .. } => targets.len() + 1,
        JournalEntry::Commit { .. } | JournalEntry::Tombstone { .. } => 1,
    }
}

fn validate_state_bounds(state: &BTreeMap<RootId, LifecycleRoot>) -> io::Result<()> {
    if state.len() > MAX_ROOTS {
        return Err(invalid("lifecycle state exceeds root-count bound"));
    }
    let mut total = 0usize;
    for root in state.values() {
        root.metadata.validate()?;
        if root.revision == 0 {
            return Err(invalid("lifecycle root revision must be nonzero"));
        }
        if root.targets.len() > MAX_TARGETS_PER_ROOT
            || root.protected_targets.len() > MAX_TARGETS_PER_ROOT * 2
        {
            return Err(invalid("lifecycle root exceeds target-count bound"));
        }
        total = total
            .checked_add(root.targets.len())
            .and_then(|value| value.checked_add(root.protected_targets.len()))
            .ok_or_else(|| invalid("lifecycle target-count overflow"))?;
        if total > MAX_TOTAL_TARGETS {
            return Err(invalid("lifecycle state exceeds total-target bound"));
        }
    }
    Ok(())
}

fn validate_state_targets(
    state: &BTreeMap<RootId, LifecycleRoot>,
    known: &BTreeSet<String>,
) -> io::Result<()> {
    for root in state.values() {
        for target in root.targets.iter().chain(&root.protected_targets) {
            validate_target(target)?;
            if !known.contains(target) {
                return Err(invalid(format!("unknown lifecycle target `{target}`")));
            }
        }
    }
    Ok(())
}

fn apply_entry(
    state: &mut BTreeMap<RootId, LifecycleRoot>,
    entry: JournalEntry,
) -> io::Result<()> {
    match entry {
        JournalEntry::Prepare {
            identity,
            proposed,
            protected,
            metadata,
            revision,
            at,
        } => {
            if proposed.is_empty() || protected.is_empty() || !proposed.is_subset(&protected) {
                return Err(invalid("prepared lifecycle root has invalid target sets"));
            }
            metadata.validate()?;
            if revision == 0 {
                return Err(invalid("prepared lifecycle root has zero revision"));
            }
            if let Some(old) = state.get(&identity.id) {
                if old.phase == RootPhase::Prepared {
                    return Err(invalid("lifecycle root already has a prepared replacement"));
                }
                if old.identity.kind != identity.kind || old.identity.producer != identity.producer {
                    return Err(invalid("lifecycle root identity changed kind or producer"));
                }
                let expected = old.identity.incarnation.get().checked_add(1)
                    .ok_or_else(|| invalid("lifecycle incarnation overflow"))?;
                if identity.incarnation.get() != expected {
                    return Err(invalid("lifecycle replacement skipped or reused an incarnation"));
                }
                let expected_revision = old
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("lifecycle root revision overflow"))?;
                if revision != expected_revision {
                    return Err(invalid("lifecycle replacement skipped or reused a revision"));
                }
                if identity.witness == old.identity.witness {
                    return Err(invalid("lifecycle replacement reused its witness"));
                }
                let last = old.tombstoned_at.or(old.committed_at).unwrap_or(old.prepared_at);
                if at < last {
                    return Err(invalid("lifecycle prepare timestamp moved backwards"));
                }
                let expected_union = old.protected_targets.union(&proposed).cloned().collect();
                if protected != expected_union {
                    return Err(invalid("prepared lifecycle root does not protect old union new"));
                }
            } else {
                if identity.incarnation.get() != 1 {
                    return Err(invalid("first lifecycle incarnation must be one"));
                }
                if revision != 1 {
                    return Err(invalid("first lifecycle revision must be one"));
                }
                if protected != proposed {
                    return Err(invalid("first lifecycle root has unexpected protected targets"));
                }
            }
            state.insert(identity.id.clone(), LifecycleRoot {
                identity,
                targets: proposed,
                protected_targets: protected,
                phase: RootPhase::Prepared,
                prepared_at: at,
                committed_at: None,
                tombstoned_at: None,
                metadata,
                revision,
                legacy: false,
            });
        }
        JournalEntry::Commit { id, incarnation, witness, at } => {
            let root = state.get_mut(&id)
                .ok_or_else(|| invalid("commit references an unknown lifecycle root"))?;
            if root.phase != RootPhase::Prepared
                || root.identity.incarnation != incarnation
                || root.identity.witness != witness
            {
                return Err(invalid("commit does not match the prepared lifecycle root"));
            }
            if at < root.prepared_at {
                return Err(invalid("lifecycle commit timestamp predates prepare"));
            }
            root.phase = RootPhase::Committed;
            root.protected_targets = root.targets.clone();
            root.committed_at = Some(at);
        }
        JournalEntry::Tombstone { id, incarnation, witness, at } => {
            let root = state.get_mut(&id)
                .ok_or_else(|| invalid("tombstone references an unknown lifecycle root"))?;
            if root.phase != RootPhase::Committed
                || root.identity.incarnation != incarnation
                || root.identity.witness != witness
            {
                return Err(invalid("tombstone does not match a committed lifecycle root"));
            }
            if at < root.committed_at.unwrap_or(root.prepared_at) {
                return Err(invalid("lifecycle tombstone timestamp predates commit"));
            }
            root.phase = RootPhase::Tombstoned;
            root.protected_targets.clear();
            root.tombstoned_at = Some(at);
        }
        JournalEntry::Legacy { identity, targets, at } => {
            if targets.is_empty() {
                return Err(invalid("legacy lifecycle root has no targets"));
            }
            if state.contains_key(&identity.id) {
                return Err(invalid("legacy import duplicates a lifecycle root"));
            }
            if identity.incarnation.get() != 1 {
                return Err(invalid("legacy lifecycle root must use incarnation one"));
            }
            state.insert(identity.id.clone(), LifecycleRoot {
                identity,
                targets: targets.clone(),
                protected_targets: targets,
                phase: RootPhase::Committed,
                prepared_at: at,
                committed_at: Some(at),
                tombstoned_at: None,
                metadata: RootMetadata::default(),
                revision: 1,
                legacy: true,
            });
        }
    }
    Ok(())
}

fn checked_targets(
    targets: Vec<String>,
    known: &BTreeSet<String>,
) -> io::Result<BTreeSet<String>> {
    if targets.is_empty() {
        return Err(invalid("lifecycle root target set must not be empty"));
    }
    if targets.len() > MAX_TARGETS_PER_ROOT {
        return Err(invalid("lifecycle root exceeds target-count bound"));
    }
    let mut exact = BTreeSet::new();
    for target in targets {
        validate_target(&target)?;
        if !exact.insert(target.clone()) {
            return Err(invalid(format!("duplicate lifecycle target `{target}`")));
        }
        if !known.contains(&target) {
            return Err(invalid(format!("unknown lifecycle target `{target}`")));
        }
    }
    Ok(exact)
}

fn validate_entry_targets(entry: &JournalEntry, known: &BTreeSet<String>) -> Result<(), String> {
    let targets = match entry {
        JournalEntry::Prepare { proposed, protected, .. } => proposed.iter().chain(protected),
        JournalEntry::Legacy { targets, .. } => targets.iter().chain(targets),
        JournalEntry::Commit { .. } | JournalEntry::Tombstone { .. } => return Ok(()),
    };
    for target in targets {
        validate_target(target).map_err(|error| error.to_string())?;
        if !known.contains(target) {
            return Err(format!("unknown lifecycle target `{target}`"));
        }
    }
    Ok(())
}

fn validate_target(target: &str) -> io::Result<()> {
    let Some(hex) = target.strip_prefix("sha256-") else {
        return Err(invalid("lifecycle target must be a sha256 digest"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(invalid("lifecycle target must use 64 lowercase hexadecimal digits"));
    }
    Ok(())
}

fn validate_identity(label: &str, value: &str) -> io::Result<()> {
    if value.is_empty() || value.len() > 512 || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(invalid(format!("invalid lifecycle {label}")));
    }
    Ok(())
}

fn unix_now() -> LifecycleTimestamp {
    LifecycleTimestamp::from_unix_seconds(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
}

fn persist_entry(
    roots: &Roots,
    entry: &JournalEntry,
    state: &BTreeMap<RootId, LifecycleRoot>,
    control: WriteControl,
) -> io::Result<()> {
    validate_state_bounds(state)?;
    validate_snapshot_wire(state)?;
    let journal = ensure_journal_dir(roots)?;
    let sequence = next_sequence(&journal)?;
    let body = render_entry(entry);
    let checksum = SHA256::sha256_hex(body.as_bytes());
    let wire = format!("{body}checksum\t{checksum}\n");
    if wire.len() > MAX_TRANSACTION_BYTES {
        return Err(invalid("lifecycle transaction exceeds byte bound"));
    }
    let base = format!("{sequence:020}-{}", &checksum[..16]);
    let partial = format!("{base}{PARTIAL_SUFFIX}");
    let final_name = format!("{base}{TXN_SUFFIX}");
    control.hit(WritePhase::BeforeWrite)?;
    let mut file = journal.create_new(&partial)?;
    file.write_all(wire.as_bytes())?;
    control.hit(WritePhase::AfterWrite)?;
    file.sync_all()?;
    control.hit(WritePhase::AfterFileSync)?;
    journal.rename_open(&file, &partial, &final_name, false)?;
    control.hit(WritePhase::AfterRename)?;
    journal.sync()?;
    control.hit(WritePhase::AfterDirectorySync)?;
    compact_if_needed(roots, state)
}

fn recover_unlocked(roots: &Roots) -> io::Result<usize> {
    let journal = ensure_journal_dir(roots)?;
    let mut recovered = 0;
    let mut names = bounded_names(&journal)?;
    names.sort();
    for name in names {
        let name = name
            .into_string()
            .map_err(|_| invalid("lifecycle journal has a non-UTF-8 name"))?;
        if valid_partial_name(&name) {
            let _ = journal.open_read(&name)?;
            journal.remove_file(&name)?;
            recovered += 1;
        } else if name != SNAPSHOT_FILE && !valid_transaction_name(&name) {
            return Err(corrupt_name(&journal, &name, "unknown journal member"));
        }
    }
    if recovered != 0 {
        journal.sync()?;
    }
    let _ = scan_journal(&journal)?;
    Ok(recovered)
}

#[cfg(test)]
fn validated_transaction_paths(roots: &Roots) -> io::Result<Vec<std::path::PathBuf>> {
    let journal = ensure_journal_dir(roots)?;
    Ok(scan_journal(&journal)?
        .transaction_names
        .into_iter()
        .map(|name| journal.path().join(name))
        .collect())
}

fn next_sequence(journal: &PinnedDirectory) -> io::Result<u64> {
    scan_journal(journal)?
        .last_sequence
        .checked_add(1)
        .ok_or_else(|| invalid("lifecycle journal sequence overflow"))
}

fn ensure_journal_dir(roots: &Roots) -> io::Result<PinnedDirectory> {
    PinnedDirectory::open_or_create(&roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR))
}

fn bounded_names(journal: &PinnedDirectory) -> io::Result<Vec<std::ffi::OsString>> {
    let names = journal.names(MAX_JOURNAL_MEMBERS + 1)?;
    if names.len() > MAX_JOURNAL_MEMBERS {
        return Err(invalid("lifecycle journal exceeds member-count bound"));
    }
    Ok(names)
}

fn valid_partial_name(name: &str) -> bool {
    name == SNAPSHOT_PARTIAL
        || (name.ends_with(PARTIAL_SUFFIX)
            && name.len() == 20 + 1 + 16 + PARTIAL_SUFFIX.len()
            && parse_sequence(name).is_some())
}

fn valid_transaction_name(name: &str) -> bool {
    name.ends_with(TXN_SUFFIX)
        && name.len() == 20 + 1 + 16 + TXN_SUFFIX.len()
        && parse_sequence(name).is_some()
        && name.as_bytes().get(20) == Some(&b'-')
        && name[21..37]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn parse_sequence(name: &str) -> Option<u64> {
    name.get(..20)?.parse().ok()
}

struct JournalScan {
    snapshot: Option<(u64, BTreeMap<RootId, LifecycleRoot>)>,
    transactions: Vec<(u64, String, JournalEntry)>,
    #[allow(dead_code)]
    transaction_names: Vec<String>,
    last_sequence: u64,
}

fn scan_journal(journal: &PinnedDirectory) -> io::Result<JournalScan> {
    let mut names = bounded_names(journal)?;
    names.sort();
    let snapshot = if names
        .iter()
        .any(|name| name.as_os_str() == std::ffi::OsStr::new(SNAPSHOT_FILE))
    {
        let raw = read_member_bounded(journal, SNAPSHOT_FILE, MAX_SNAPSHOT_BYTES)?;
        Some(
            parse_snapshot(&raw)
                .map_err(|error| corrupt_name(journal, SNAPSHOT_FILE, error))?,
        )
    } else {
        None
    };
    let through = snapshot.as_ref().map(|(sequence, _)| *sequence).unwrap_or(0);
    let mut transactions = Vec::new();
    let mut transaction_names = Vec::new();
    let mut work = 0usize;
    let mut decoded_bytes = 0usize;
    let mut last_sequence = through;
    let mut sequences = BTreeSet::new();
    for name in names {
        let name = name
            .into_string()
            .map_err(|_| invalid("lifecycle journal has a non-UTF-8 name"))?;
        if valid_partial_name(&name) {
            continue;
        }
        if name == SNAPSHOT_FILE {
            continue;
        }
        if !valid_transaction_name(&name) {
            return Err(corrupt_name(journal, &name, "invalid journal member"));
        }
        let sequence = parse_sequence(&name)
            .ok_or_else(|| corrupt_name(journal, &name, "invalid journal sequence"))?;
        if !sequences.insert(sequence) {
            return Err(corrupt_name(
                journal,
                &name,
                "duplicate journal sequence",
            ));
        }
        let raw = read_member_bounded(journal, &name, MAX_TRANSACTION_BYTES)?;
        let body = checked_body(&raw).map_err(|error| corrupt_name(journal, &name, error))?;
        let checksum = SHA256::sha256_hex(body.as_bytes());
        let expected = format!("{sequence:020}-{}{TXN_SUFFIX}", &checksum[..16]);
        if name != expected {
            return Err(corrupt_name(
                journal,
                &name,
                "journal filename disagrees with checksum",
            ));
        }
        transaction_names.push(name.clone());
        last_sequence = last_sequence.max(sequence);
        // A durable snapshot supersedes transaction semantics through its
        // sequence. Still authenticate every old file and filename, but do
        // not decode or charge it as recovery work after a crash between
        // snapshot publication and journal deletion.
        if sequence <= through {
            continue;
        }
        decoded_bytes = decoded_bytes
            .checked_add(raw.len())
            .ok_or_else(|| invalid("lifecycle journal byte work overflow"))?;
        if decoded_bytes > MAX_RECOVERY_BYTES {
            return Err(invalid("lifecycle journal exceeds aggregate byte bound"));
        }
        let entry = parse_entry(&raw).map_err(|error| corrupt_name(journal, &name, error))?;
        work = work
            .checked_add(entry_work(&entry))
            .ok_or_else(|| invalid("lifecycle journal work overflow"))?;
        if work > MAX_RECOVERY_WORK {
            return Err(invalid("lifecycle journal exceeds work bound"));
        }
        transactions.push((sequence, name, entry));
    }
    transactions.sort_by_key(|(sequence, _, _)| *sequence);
    let mut expected = through.checked_add(1)
        .ok_or_else(|| invalid("lifecycle journal sequence overflow"))?;
    for (sequence, name, _) in transactions.iter().filter(|(sequence, _, _)| *sequence > through) {
        if *sequence != expected {
            return Err(corrupt_name(
                journal,
                name,
                "journal sequence is missing, duplicate, or invalid",
            ));
        }
        expected = expected
            .checked_add(1)
            .ok_or_else(|| invalid("lifecycle journal sequence overflow"))?;
    }
    if snapshot.is_none() {
        if let Some((first, name, _)) = transactions.first() {
            if *first != 1 {
                return Err(corrupt_name(
                    journal,
                    name,
                    "journal starts after sequence one without a snapshot",
                ));
            }
        }
    }
    Ok(JournalScan {
        snapshot,
        transactions,
        transaction_names,
        last_sequence,
    })
}

fn read_member_bounded(
    journal: &PinnedDirectory,
    name: &str,
    maximum: usize,
) -> io::Result<String> {
    let file = journal.open_read(name)?;
    let length = usize::try_from(file.metadata()?.len())
        .map_err(|_| invalid("lifecycle journal member length overflows usize"))?;
    if length > maximum {
        return Err(invalid("lifecycle journal member exceeds byte bound"));
    }
    let mut bytes = Vec::with_capacity(length);
    let read_limit = maximum
        .checked_add(1)
        .ok_or_else(|| invalid("lifecycle journal byte bound overflows usize"))?;
    let read_limit = u64::try_from(read_limit)
        .map_err(|_| invalid("lifecycle journal byte bound overflows u64"))?;
    file.take(read_limit)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("lifecycle journal member exceeds byte bound"));
    }
    String::from_utf8(bytes)
        .map_err(|_| invalid("lifecycle journal member is not UTF-8"))
}

fn compact_if_needed(
    roots: &Roots,
    state: &BTreeMap<RootId, LifecycleRoot>,
) -> io::Result<()> {
    let journal = ensure_journal_dir(roots)?;
    let scan = scan_journal(&journal)?;
    let active = scan
        .transactions
        .iter()
        .filter(|(sequence, _, _)| *sequence > scan.snapshot.as_ref().map(|value| value.0).unwrap_or(0))
        .count();
    if active <= COMPACT_AFTER_TRANSACTIONS {
        return Ok(());
    }
    validate_state_bounds(state)?;
    validate_snapshot_wire(state)?;
    let body = render_snapshot(scan.last_sequence, state);
    let checksum = SHA256::sha256_hex(body.as_bytes());
    let wire = format!("{body}checksum\t{checksum}\n");
    if wire.len() > MAX_SNAPSHOT_BYTES {
        return Err(invalid("lifecycle snapshot exceeds byte bound"));
    }
    let mut file = journal.create_new(SNAPSHOT_PARTIAL)?;
    file.write_all(wire.as_bytes())?;
    file.sync_all()?;
    journal.rename_open(&file, SNAPSHOT_PARTIAL, SNAPSHOT_FILE, true)?;
    journal.sync()?;
    for (sequence, name, _) in &scan.transactions {
        if *sequence <= scan.last_sequence {
            journal.remove_file(name)?;
        }
    }
    journal.sync()
}

fn validate_snapshot_wire(state: &BTreeMap<RootId, LifecycleRoot>) -> io::Result<()> {
    // Use the longest possible sequence spelling so admission guarantees a
    // later compaction cannot fail after its transaction is already durable.
    let body = render_snapshot(u64::MAX, state);
    let wire_len = body
        .len()
        .checked_add("checksum\t".len() + 64 + 1)
        .ok_or_else(|| invalid("lifecycle snapshot byte count overflow"))?;
    if wire_len > MAX_SNAPSHOT_BYTES {
        return Err(invalid("lifecycle state cannot fit a durable snapshot"));
    }
    Ok(())
}

fn corrupt_name(
    journal: &PinnedDirectory,
    name: &str,
    detail: impl Into<String>,
) -> io::Error {
    corrupt(&journal.path().join(name), detail)
}

fn render_entry(entry: &JournalEntry) -> String {
    let mut out = String::from("jet-lifecycle-journal-v1\n");
    match entry {
        JournalEntry::Prepare { identity, proposed, protected, metadata, revision, at } => {
            out.push_str(&format!("prepare\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                identity.kind.wire(), hex(identity.id.as_str()), hex(identity.producer.as_str()),
                identity.incarnation.get(), hex(identity.witness.as_str()), revision, at.get(),
                hex_option(metadata.label.as_deref()), hex_option(metadata.reference.as_deref()),
                metadata.expires_at.map(|value| value.get().to_string()).unwrap_or_else(|| "-".to_string())));
            for target in proposed { out.push_str(&format!("target\t{}\n", target)); }
            for target in protected { out.push_str(&format!("protect\t{}\n", target)); }
        }
        JournalEntry::Commit { id, incarnation, witness, at } => {
            out.push_str(&format!("commit\t{}\t{}\t{}\t{}\n",
                hex(id.as_str()), incarnation.get(), hex(witness.as_str()), at.get()));
        }
        JournalEntry::Tombstone { id, incarnation, witness, at } => {
            out.push_str(&format!("tombstone\t{}\t{}\t{}\t{}\n",
                hex(id.as_str()), incarnation.get(), hex(witness.as_str()), at.get()));
        }
        JournalEntry::Legacy { identity, targets, at } => {
            out.push_str(&format!("legacy\t{}\t{}\t{}\t{}\t{}\t{}\n",
                identity.kind.wire(), hex(identity.id.as_str()), hex(identity.producer.as_str()),
                identity.incarnation.get(), hex(identity.witness.as_str()), at.get()));
            for target in targets { out.push_str(&format!("target\t{}\n", target)); }
        }
    }
    out
}

fn parse_entry(raw: &str) -> Result<JournalEntry, String> {
    let body = checked_body(raw)?;
    let mut lines = body.lines();
    if lines.next() != Some("jet-lifecycle-journal-v1") {
        return Err("unsupported journal version".to_string());
    }
    let header = lines.next().ok_or_else(|| "missing lifecycle operation".to_string())?;
    let fields = header.split('\t').collect::<Vec<_>>();
    let mut targets = BTreeSet::new();
    let mut protected = BTreeSet::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["target", _] if targets.len() >= MAX_TARGETS_PER_ROOT => {
                return Err("lifecycle transaction exceeds target-count bound".to_string());
            }
            ["protect", _] if protected.len() >= MAX_TARGETS_PER_ROOT * 2 => {
                return Err("lifecycle transaction exceeds protected-target bound".to_string());
            }
            ["target", target] if targets.insert((*target).to_string()) => {}
            ["protect", target] if protected.insert((*target).to_string()) => {}
            ["target", target] => return Err(format!("duplicate lifecycle target `{target}`")),
            ["protect", target] => return Err(format!("duplicate protected target `{target}`")),
            _ => return Err(format!("invalid lifecycle journal line `{line}`")),
        }
    }
    match fields.as_slice() {
        ["prepare", kind, id, producer, incarnation, witness, at] => Ok(JournalEntry::Prepare {
            identity: parse_identity(kind, id, producer, incarnation, witness)?,
            proposed: targets,
            protected,
            metadata: RootMetadata::default(),
            revision: incarnation
                .parse()
                .map_err(|_| "invalid lifecycle root revision".to_string())?,
            at: parse_timestamp(at)?,
        }),
        ["prepare", kind, id, producer, incarnation, witness, revision, at, label, reference, expires] => Ok(JournalEntry::Prepare {
            identity: parse_identity(kind, id, producer, incarnation, witness)?,
            proposed: targets,
            protected,
            metadata: parse_metadata(label, reference, expires)?,
            revision: parse_revision(revision)?,
            at: parse_timestamp(at)?,
        }),
        ["commit", id, incarnation, witness, at] if targets.is_empty() && protected.is_empty() => {
            Ok(JournalEntry::Commit { id: parse_id(id)?, incarnation: parse_incarnation(incarnation)?,
                witness: parse_witness(witness)?, at: parse_timestamp(at)? })
        }
        ["tombstone", id, incarnation, witness, at] if targets.is_empty() && protected.is_empty() => {
            Ok(JournalEntry::Tombstone { id: parse_id(id)?, incarnation: parse_incarnation(incarnation)?,
                witness: parse_witness(witness)?, at: parse_timestamp(at)? })
        }
        ["legacy", kind, id, producer, incarnation, witness, at] if protected.is_empty() => {
            Ok(JournalEntry::Legacy { identity: parse_identity(kind, id, producer, incarnation, witness)?,
                targets, at: parse_timestamp(at)? })
        }
        _ => Err("invalid lifecycle journal operation".to_string()),
    }
}

fn checked_body(raw: &str) -> Result<&str, String> {
    let Some((body, checksum)) = raw.rsplit_once("checksum\t") else {
        return Err("missing checksum".to_string());
    };
    let checksum = checksum.strip_suffix('\n').ok_or_else(|| "truncated checksum".to_string())?;
    if checksum.len() != 64 || checksum.contains('\n') {
        return Err("invalid checksum".to_string());
    }
    if SHA256::sha256_hex(body.as_bytes()) != checksum {
        return Err("checksum mismatch".to_string());
    }
    Ok(body)
}

fn render_snapshot(
    through_sequence: u64,
    roots: &BTreeMap<RootId, LifecycleRoot>,
) -> String {
    let mut out = format!(
        "jet-lifecycle-snapshot-v1\nthrough\t{through_sequence}\njet-lifecycle-state-v1\n"
    );
    for root in roots.values() {
        let committed = root
            .committed_at
            .map(|value| value.get().to_string())
            .unwrap_or_else(|| "-".to_string());
        let tombstoned = root
            .tombstoned_at
            .map(|value| value.get().to_string())
            .unwrap_or_else(|| "-".to_string());
        out.push_str(&format!(
            "root\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            root.identity.kind.wire(),
            hex(root.identity.id.as_str()),
            hex(root.identity.producer.as_str()),
            root.identity.incarnation.get(),
            hex(root.identity.witness.as_str()),
            match root.phase {
                RootPhase::Prepared => "prepared",
                RootPhase::Committed => "committed",
                RootPhase::Tombstoned => "tombstoned",
            },
            root.prepared_at.get(),
            committed,
            tombstoned,
            u8::from(root.legacy),
            root.revision,
            hex_option(root.metadata.label.as_deref()),
            hex_option(root.metadata.reference.as_deref()),
            root.metadata
                .expires_at
                .map(|value| value.get().to_string())
                .unwrap_or_else(|| "-".to_string()),
        ));
        for target in &root.targets {
            out.push_str(&format!("target\t{target}\n"));
        }
        for target in &root.protected_targets {
            out.push_str(&format!("protect\t{target}\n"));
        }
    }
    out
}

fn parse_snapshot(raw: &str) -> Result<(u64, BTreeMap<RootId, LifecycleRoot>), String> {
    let body = checked_body(raw)?;
    let mut lines = body.lines();
    if lines.next() != Some("jet-lifecycle-snapshot-v1") {
        return Err("unsupported lifecycle snapshot version".to_string());
    }
    let through = lines
        .next()
        .and_then(|line| line.strip_prefix("through\t"))
        .ok_or_else(|| "missing lifecycle snapshot sequence".to_string())?
        .parse::<u64>()
        .map_err(|_| "invalid lifecycle snapshot sequence".to_string())?;
    if through == 0 {
        return Err("lifecycle snapshot sequence must be nonzero".to_string());
    }
    if lines.next() != Some("jet-lifecycle-state-v1") {
        return Err("missing lifecycle snapshot state header".to_string());
    }
    let mut roots = BTreeMap::<RootId, LifecycleRoot>::new();
    let mut current = None::<RootId>;
    let mut work = 0usize;
    for line in lines {
        work = work
            .checked_add(1)
            .ok_or_else(|| "lifecycle snapshot work overflow".to_string())?;
        if work > MAX_RECOVERY_WORK {
            return Err("lifecycle snapshot exceeds work bound".to_string());
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            [
                "root",
                kind,
                id,
                producer,
                incarnation,
                witness,
                phase,
                prepared,
                committed,
                tombstoned,
                legacy @ ("0" | "1"),
                revision,
                label,
                reference,
                expires,
            ] => {
                if roots.len() >= MAX_ROOTS {
                    return Err("lifecycle snapshot exceeds root-count bound".to_string());
                }
                let identity = parse_identity(kind, id, producer, incarnation, witness)?;
                let id = identity.id.clone();
                if roots.contains_key(&id) {
                    return Err(format!("duplicate lifecycle snapshot root `{}`", id.as_str()));
                }
                let phase = match *phase {
                    "prepared" => RootPhase::Prepared,
                    "committed" => RootPhase::Committed,
                    "tombstoned" => RootPhase::Tombstoned,
                    _ => return Err("invalid lifecycle snapshot phase".to_string()),
                };
                roots.insert(
                    id.clone(),
                    LifecycleRoot {
                        identity,
                        targets: BTreeSet::new(),
                        protected_targets: BTreeSet::new(),
                        phase,
                        prepared_at: parse_timestamp(prepared)?,
                        committed_at: parse_optional_timestamp(committed)?,
                        tombstoned_at: parse_optional_timestamp(tombstoned)?,
                        metadata: parse_metadata(label, reference, expires)?,
                        revision: parse_revision(revision)?,
                        legacy: *legacy == "1",
                    },
                );
                current = Some(id);
            }
            [
                "root",
                kind,
                id,
                producer,
                incarnation,
                witness,
                phase,
                prepared,
                committed,
                tombstoned,
                legacy @ ("0" | "1"),
            ] => {
                if roots.len() >= MAX_ROOTS {
                    return Err("lifecycle snapshot exceeds root-count bound".to_string());
                }
                let identity = parse_identity(kind, id, producer, incarnation, witness)?;
                let id = identity.id.clone();
                if roots.contains_key(&id) {
                    return Err(format!("duplicate lifecycle snapshot root `{}`", id.as_str()));
                }
                let phase = match *phase {
                    "prepared" => RootPhase::Prepared,
                    "committed" => RootPhase::Committed,
                    "tombstoned" => RootPhase::Tombstoned,
                    _ => return Err("invalid lifecycle snapshot phase".to_string()),
                };
                roots.insert(
                    id.clone(),
                    LifecycleRoot {
                        identity,
                        targets: BTreeSet::new(),
                        protected_targets: BTreeSet::new(),
                        phase,
                        prepared_at: parse_timestamp(prepared)?,
                        committed_at: parse_optional_timestamp(committed)?,
                        tombstoned_at: parse_optional_timestamp(tombstoned)?,
                        metadata: RootMetadata::default(),
                        revision: incarnation
                            .parse()
                            .map_err(|_| "invalid lifecycle root revision".to_string())?,
                        legacy: *legacy == "1",
                    },
                );
                current = Some(id);
            }
            [kind @ ("target" | "protect"), target] => {
                let id = current
                    .as_ref()
                    .ok_or_else(|| "snapshot target precedes root".to_string())?;
                let root = roots
                    .get_mut(id)
                    .ok_or_else(|| "snapshot target has no current root".to_string())?;
                let set = if *kind == "target" {
                    &mut root.targets
                } else {
                    &mut root.protected_targets
                };
                if set.len() >= MAX_TARGETS_PER_ROOT * 2 {
                    return Err("lifecycle snapshot root exceeds target bound".to_string());
                }
                if !set.insert((*target).to_string()) {
                    return Err(format!("duplicate lifecycle snapshot target `{target}`"));
                }
            }
            _ => return Err(format!("invalid lifecycle snapshot line `{line}`")),
        }
    }
    validate_snapshot_state(&roots)?;
    validate_state_bounds(&roots).map_err(|error| error.to_string())?;
    Ok((through, roots))
}

fn parse_optional_timestamp(value: &str) -> Result<Option<LifecycleTimestamp>, String> {
    if value == "-" {
        Ok(None)
    } else {
        parse_timestamp(value).map(Some)
    }
}

fn validate_snapshot_state(roots: &BTreeMap<RootId, LifecycleRoot>) -> Result<(), String> {
    for (id, root) in roots {
        if id != &root.identity.id || root.targets.is_empty() {
            return Err("invalid lifecycle snapshot root identity or targets".to_string());
        }
        root.metadata.validate().map_err(|error| error.to_string())?;
        if root.revision == 0 {
            return Err(format!("lifecycle snapshot root `{}` has zero revision", id.as_str()));
        }
        match root.phase {
            RootPhase::Prepared
                if root.committed_at.is_none()
                    && root.tombstoned_at.is_none()
                    && !root.protected_targets.is_empty()
                    && root.targets.is_subset(&root.protected_targets) => {}
            RootPhase::Committed
                if root.committed_at.is_some()
                    && root.tombstoned_at.is_none()
                    && root.protected_targets == root.targets => {}
            RootPhase::Tombstoned
                if root.committed_at.is_some()
                    && root.tombstoned_at.is_some()
                    && root.protected_targets.is_empty() => {}
            _ => return Err(format!("invalid lifecycle snapshot root `{}`", id.as_str())),
        }
        if root.legacy && root.phase == RootPhase::Prepared {
            return Err(format!("legacy lifecycle snapshot root `{}` is prepared", id.as_str()));
        }
        if root.committed_at.is_some_and(|value| value < root.prepared_at)
            || root.tombstoned_at.is_some_and(|value| {
                value < root.committed_at.unwrap_or(root.prepared_at)
            })
        {
            return Err(format!(
                "lifecycle snapshot root `{}` has backwards timestamps",
                id.as_str()
            ));
        }
    }
    Ok(())
}

fn parse_identity(kind: &str, id: &str, producer: &str, incarnation: &str, witness: &str) -> Result<RootIdentity, String> {
    Ok(RootIdentity::new(RootKind::parse(kind)?, parse_id(id)?,
        ProducerId::new(unhex(producer)?).map_err(|error| error.to_string())?,
        parse_incarnation(incarnation)?, parse_witness(witness)?))
}

fn parse_id(value: &str) -> Result<RootId, String> {
    RootId::new(unhex(value)?).map_err(|error| error.to_string())
}

fn parse_witness(value: &str) -> Result<RootWitness, String> {
    RootWitness::new(unhex(value)?).map_err(|error| error.to_string())
}

fn parse_incarnation(value: &str) -> Result<Incarnation, String> {
    Incarnation::new(value.parse().map_err(|_| "invalid lifecycle incarnation".to_string())?)
        .map_err(|error| error.to_string())
}

fn parse_timestamp(value: &str) -> Result<LifecycleTimestamp, String> {
    Ok(LifecycleTimestamp::from_unix_seconds(value.parse()
        .map_err(|_| "invalid lifecycle timestamp".to_string())?))
}

fn parse_revision(value: &str) -> Result<u64, String> {
    let revision = value
        .parse::<u64>()
        .map_err(|_| "invalid lifecycle root revision".to_string())?;
    if revision == 0 {
        return Err("lifecycle root revision must be nonzero".to_string());
    }
    Ok(revision)
}

fn parse_metadata(label: &str, reference: &str, expires: &str) -> Result<RootMetadata, String> {
    let metadata = RootMetadata {
        label: unhex_option(label)?,
        reference: unhex_option(reference)?,
        expires_at: parse_optional_timestamp(expires)?,
    };
    metadata.validate().map_err(|error| error.to_string())?;
    Ok(metadata)
}

fn canonical_state(roots: &BTreeMap<RootId, LifecycleRoot>) -> String {
    let mut out = String::from("jet-lifecycle-state-v1\n");
    for root in roots.values() {
        out.push_str(&format!("root\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            root.identity.kind.wire(), hex(root.identity.id.as_str()), hex(root.identity.producer.as_str()),
            root.identity.incarnation.get(), hex(root.identity.witness.as_str()),
            match root.phase { RootPhase::Prepared => "prepared", RootPhase::Committed => "committed", RootPhase::Tombstoned => "tombstoned" },
            root.prepared_at.get(), root.committed_at.map(LifecycleTimestamp::get).unwrap_or(0),
            root.tombstoned_at.map(LifecycleTimestamp::get).unwrap_or(0), u8::from(root.legacy),
            root.revision, hex_option(root.metadata.label.as_deref()),
            hex_option(root.metadata.reference.as_deref()),
            root.metadata.expires_at.map(LifecycleTimestamp::get).unwrap_or(0)));
        for target in &root.targets { out.push_str(&format!("target\t{}\n", target)); }
        for target in &root.protected_targets { out.push_str(&format!("protect\t{}\n", target)); }
    }
    out
}

fn hex_option(value: Option<&str>) -> String {
    value.map(hex).unwrap_or_else(|| "-".to_string())
}

fn unhex_option(value: &str) -> Result<Option<String>, String> {
    if value == "-" {
        Ok(None)
    } else {
        Ok(Some(unhex(value)?))
    }
}

fn hex(value: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

fn unhex(value: &str) -> Result<String, String> {
    if !value.len().is_multiple_of(2) { return Err("odd hex field".to_string()); }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    String::from_utf8(bytes).map_err(|_| "lifecycle field is not UTF-8".to_string())
}

fn nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("invalid hex field".to_string()),
    }
}

fn corrupt(path: &Path, detail: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData,
        format!("lifecycle journal `{}`: {}", path.display(), detail.into()))
}

fn invalid(detail: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WritePhase {
    BeforeWrite,
    AfterWrite,
    AfterFileSync,
    AfterRename,
    AfterDirectorySync,
}

#[derive(Debug, Clone, Copy)]
struct WriteControl { fail: Option<WritePhase>, raw_os_error: i32 }

impl WriteControl {
    const fn none() -> Self { Self { fail: None, raw_os_error: 5 } }
    #[cfg(test)]
    const fn fail(phase: WritePhase, raw_os_error: i32) -> Self { Self { fail: Some(phase), raw_os_error } }
    fn hit(self, phase: WritePhase) -> io::Result<()> {
        if self.fail == Some(phase) { Err(io::Error::from_raw_os_error(self.raw_os_error)) } else { Ok(()) }
    }
}

/// A manually retained Hangar closure, projected for the external-root CLI
/// and consumers. The lifecycle WAL remains the source of truth; this is only
/// the typed Store view of one committed root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalRootView {
    pub(crate) label: String,
    pub(crate) reference: String,
    pub(crate) etag: String,
    pub(crate) closure_size: usize,
    pub(crate) prepared: bool,
    pub(crate) expires_at: Option<u64>,
}

#[derive(Debug)]
pub(crate) enum ExternalRootError {
    Conflict {
        label: String,
        expected: Option<String>,
        current: Option<String>,
    },
    ReferenceNotFound(String),
    Store(std::io::Error),
}

impl std::fmt::Display for ExternalRootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict {
                label,
                expected,
                current,
            } => {
                write!(
                    f,
                    "external root `{label}` changed before the request applied (expected {:?}, current {:?})",
                    expected,
                    current
                )
            }
            Self::ReferenceNotFound(reference) => {
                write!(f, "no Hangar entry matches `{reference}`")
            }
            Self::Store(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ExternalRootError {}

fn validate_external_root_label(label: &str) -> std::io::Result<()> {
    if label.is_empty()
        || label.len() > 128
        || label == "."
        || label == ".."
        || label.contains('/')
        || label.contains('\\')
        || label.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "external root label must be one safe path component",
        ));
    }
    Ok(())
}

fn manual_root_id(
    principal: &ProducerId,
    label: &str,
) -> std::io::Result<RootId> {
    RootId::new(format!("manual:{}:{label}", principal.as_str()))
}

fn manual_root_witness(
    principal: &ProducerId,
    label: &str,
    reference: &str,
    targets: &[String],
) -> std::io::Result<RootWitness> {
    let mut canonical = String::from("jet-manual-root-v1\n");
    for value in [principal.as_str(), label, reference] {
        canonical.push_str(&value.len().to_string());
        canonical.push('\n');
        canonical.push_str(value);
        canonical.push('\n');
    }
    for target in targets {
        canonical.push_str(target);
        canonical.push('\n');
    }
    RootWitness::new(format!(
        "sha256-{}",
        SHA256::sha256_hex(canonical.as_bytes())
    ))
}

fn external_root_targets_unlocked(
    roots: &Roots,
    reference: &str,
    graph: &Closure::ClosureGraph,
) -> std::io::Result<Vec<String>> {
    let entry = super::list_unlocked(roots)
        .into_iter()
        .find(|entry| entry.reference == reference)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no Hangar entry matches `{reference}`"),
            )
        })?;
    let mut targets = graph.closure(&entry.envelope.output_hash);
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Hangar entry `{reference}` has no closure objects"),
        ));
    }
    Ok(targets)
}

fn external_root_targets(
    roots: &Roots,
    reference: &str,
) -> Result<Vec<String>, ExternalRootError> {
    let result = with_lifecycle_lock(roots, |_, _| {
        let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
        external_root_targets_unlocked(roots, reference, &graph)
    });
    result.map_err(|error| {
        if is_missing_external_reference(&error, reference) {
            ExternalRootError::ReferenceNotFound(reference.to_string())
        } else {
            ExternalRootError::Store(error)
        }
    })
}

fn is_missing_external_reference(error: &std::io::Error, reference: &str) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
        && error.to_string() == format!("no Hangar entry matches `{reference}`")
}

pub(crate) fn external_root_closure_size(
    roots: &Roots,
    reference: &str,
) -> Result<usize, ExternalRootError> {
    Ok(external_root_targets(roots, reference)?.len())
}

fn external_root_view(root: &LifecycleRoot) -> std::io::Result<ExternalRootView> {
    let label = root.metadata.label.clone().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manual lifecycle root has no label metadata",
        )
    })?;
    let reference = root.metadata.reference.clone().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "manual lifecycle root has no reference metadata",
        )
    })?;
    Ok(ExternalRootView {
        label,
        reference,
        etag: root.etag().render(),
        closure_size: root.targets.len(),
        prepared: root.phase == RootPhase::Prepared,
        expires_at: root.metadata.expires_at.map(|value| value.get()),
    })
}

fn map_external_root_error(label: &str, error: std::io::Error) -> ExternalRootError {
    if let Some(conflict) = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<CasConflict>())
    {
        return ExternalRootError::Conflict {
            label: label.to_string(),
            expected: conflict.expected.clone(),
            current: conflict.current.clone(),
        };
    }
    ExternalRootError::Store(error)
}

/// Atomically create or replace one manually retained Hangar closure.
/// Lifecycle owns the typed identity, CAS check, expiry metadata, and
/// prepare/commit journal sequence while this Store adapter resolves the
/// reference to the complete closure under Hangar authority.
pub(crate) fn register_external_root_at(
    roots: &Roots,
    principal: &str,
    label: &str,
    reference: &str,
    expires_at: Option<u64>,
    expected_etag: Option<&str>,
    at: u64,
) -> Result<ExternalRootView, ExternalRootError> {
    validate_external_root_label(label).map_err(ExternalRootError::Store)?;
    let producer = ProducerId::new(principal.to_string())
        .map_err(ExternalRootError::Store)?;
    let metadata = RootMetadata::manual(
        label,
        reference,
        expires_at.map(LifecycleTimestamp::from_unix_seconds),
    )
    .map_err(ExternalRootError::Store)?;
    let id = manual_root_id(&producer, label).map_err(ExternalRootError::Store)?;
    let expected = expected_etag
        .map(RootEtag::parse)
        .transpose()
        .map_err(ExternalRootError::Store)?;
    let snapshot = with_lifecycle_lock(roots, |known, closure_head| {
        let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
        let targets = external_root_targets_unlocked(roots, reference, &graph)?;
        let witness = manual_root_witness(&producer, label, reference, &targets)?;
        let update = RootUpdate {
            identity: RootIdentity::new(
                RootKind::Manual,
                id.clone(),
                producer,
                Incarnation::new(1)?,
                witness,
            ),
            targets,
            metadata,
            expected_etag: expected_etag.map(str::to_string),
            at: LifecycleTimestamp::from_unix_seconds(at),
        };
        atomic_update_unlocked(roots, update, expected, known, closure_head)
    })
    .map_err(|error| {
        if is_missing_external_reference(&error, reference) {
            ExternalRootError::ReferenceNotFound(reference.to_string())
        } else {
            map_external_root_error(label, error)
        }
    })?;
    let root = snapshot.roots.get(&id).ok_or_else(|| {
        ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("manual lifecycle root `{label}` disappeared after update"),
        ))
    })?;
    external_root_view(root).map_err(ExternalRootError::Store)
}

pub(crate) fn list_external_roots(
    roots: &Roots,
    principal: &str,
) -> Result<Vec<ExternalRootView>, ExternalRootError> {
    let producer = ProducerId::new(principal.to_string())
        .map_err(ExternalRootError::Store)?;
    snapshot(roots)
        .map_err(ExternalRootError::Store)?
        .roots
        .values()
        .filter(|root| {
            root.identity.kind == RootKind::Manual
                && root.identity.producer == producer
                && root.phase != RootPhase::Tombstoned
        })
        .map(external_root_view)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ExternalRootError::Store)
}

pub(crate) fn unregister_external_root_at(
    roots: &Roots,
    principal: &str,
    label: &str,
    expected_etag: &str,
    at: u64,
) -> Result<(), ExternalRootError> {
    validate_external_root_label(label).map_err(ExternalRootError::Store)?;
    let producer = ProducerId::new(principal.to_string())
        .map_err(ExternalRootError::Store)?;
    let id = manual_root_id(&producer, label).map_err(ExternalRootError::Store)?;
    let snapshot = snapshot(roots).map_err(ExternalRootError::Store)?;
    let Some(root) = snapshot.roots.get(&id) else {
        return Err(ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("external root `{label}` was not found"),
        )));
    };
    if root.identity.kind != RootKind::Manual
        || root.identity.producer != producer
    {
        return Err(ExternalRootError::Store(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("external root `{label}` is owned by another producer"),
        )));
    }
    atomic_remove(
        roots,
        &id,
        expected_etag,
        LifecycleTimestamp::from_unix_seconds(at),
    )
    .map_err(|error| map_external_root_error(label, error))?;
    Ok(())
}

pub(crate) fn reconcile_profile_generation_root(
    roots: &Roots,
    owner: &str,
    profile: &str,
    generation: u64,
    witness: &str,
    mut targets: Vec<String>,
    at: u64,
) -> std::io::Result<Option<PreparedProfileGenerationRoot>> {
    targets.sort();
    targets.dedup();
    let id = RootId::new(format!(
        "profile-generation:{owner}:{profile}:{generation}"
    ))?;
    let incarnation = Incarnation::new(1)?;
    let witness = RootWitness::new(witness)?;
    let identity = RootIdentity::new(
        RootKind::ProfileGeneration,
        id.clone(),
        ProducerId::new("jetpack-profile-generation")?,
        incarnation,
        witness.clone(),
    );
    let snapshot = prepare_if_absent(
        roots,
        identity,
        targets,
        LifecycleTimestamp::from_unix_seconds(at),
    )?;
    let root = snapshot
        .roots
        .get(&id)
        .ok_or_else(|| std::io::Error::other("generation root disappeared after prepare"))?;
    if root.phase == RootPhase::Committed {
        Ok(None)
    } else {
        Ok(Some(PreparedProfileGenerationRoot {
            id,
            incarnation,
            witness,
        }))
    }
}


#[cfg(test)]
mod tests {

    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> (Roots, BTreeSet<String>) {
        let root = std::env::temp_dir().join(format!("jet-lifecycle-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let roots = Roots { root, dev_mode: true };
        let known = [digest('a'), digest('b'), digest('c')].into_iter().collect::<BTreeSet<_>>();
        install_closure_fixture(&roots, &known);
        (roots, known)
    }

    fn digest(ch: char) -> String { format!("sha256-{}", ch.to_string().repeat(64)) }

    fn identity(id: &str, incarnation: u64, witness: &str) -> RootIdentity {
        RootIdentity::new(RootKind::Manual, RootId::new(id).unwrap(), ProducerId::new("test-producer").unwrap(),
            Incarnation::new(incarnation).unwrap(), RootWitness::new(witness).unwrap())
    }

    fn install_closure_fixture(roots: &Roots, targets: &BTreeSet<String>) {
        let journal = roots.hangar_dir().join("closure-db/journal");
        fs::create_dir_all(&journal).unwrap();
        let mut body = String::from("jet-closure-journal-v1\nkind\tdelta\n");
        for target in targets {
            body.push_str(&format!(
                "object\t{}\t{}\t1\n",
                super::hex(target),
                super::hex(&format!("/fixture/{target}")),
            ));
        }
        let checksum = SHA256::sha256_hex(body.as_bytes());
        fs::write(journal.join(format!("{:020}-{}.txn", 1, &checksum[..16])), format!("{body}checksum\t{checksum}\n")).unwrap();
    }

    fn cleanup(roots: &Roots) { let _ = fs::remove_dir_all(&roots.root); }

    fn persist_controlled(
        roots: &Roots,
        known: &BTreeSet<String>,
        entry: JournalEntry,
        control: WriteControl,
    ) -> io::Result<()> {
        let mut state = load_state(roots, known)?;
        if !entry_already_applied(&state, &entry) {
            apply_entry(&mut state, entry.clone())?;
            persist_entry(roots, &entry, &state, control)?;
        }
        Ok(())
    }

    #[test]
    fn replacement_recovery_obeys_old_union_new_law() {
        let (roots, known) = fixture();
        let first = identity("root", 1, "w1");
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(2)).unwrap();

        let before = snapshot(&roots).unwrap();
        assert_eq!(before.protected_targets, BTreeSet::from([digest('a')]));
        let second = identity("root", 2, "w2");
        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            recover_unlocked(&roots)?;
            prepare_unlocked_controlled(&roots, second.clone(), vec![digest('b')], LifecycleTimestamp(3), &known, WriteControl::none())
        }).unwrap();
        let prepared = snapshot(&roots).unwrap();
        assert_eq!(prepared.protected_targets, BTreeSet::from([digest('a'), digest('b')]));
        commit(&roots, &second.id, second.incarnation, &second.witness, LifecycleTimestamp(4)).unwrap();
        assert_eq!(snapshot(&roots).unwrap().protected_targets, BTreeSet::from([digest('b')]));
        cleanup(&roots);
    }

    #[test]
    fn partial_and_enospc_failures_recover_old_state() {
        let (roots, known) = fixture();
        let first = identity("root", 1, "w1");
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(2)).unwrap();
        let second = identity("root", 2, "w2");
        let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            prepare_unlocked_controlled(&roots, second, vec![digest('b')], LifecycleTimestamp(3), &known,
                WriteControl::fail(WritePhase::AfterWrite, 28))
        }).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(28));
        assert_eq!(recover(&roots).unwrap(), 1);
        assert_eq!(snapshot(&roots).unwrap().protected_targets, BTreeSet::from([digest('a')]));
        cleanup(&roots);
    }

    #[test]
    fn published_prepare_survives_post_rename_failure_as_union() {
        let (roots, known) = fixture();
        let first = identity("root", 1, "w1");
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(2)).unwrap();
        let second = identity("root", 2, "w2");
        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            prepare_unlocked_controlled(&roots, second, vec![digest('b')], LifecycleTimestamp(3), &known,
                WriteControl::fail(WritePhase::AfterRename, 5))
        }).unwrap_err();
        assert_eq!(snapshot(&roots).unwrap().protected_targets, BTreeSet::from([digest('a'), digest('b')]));
        cleanup(&roots);
    }

    #[test]
    fn durable_ambiguous_prepare_retry_is_witness_bound_and_idempotent() {
        let (roots, known) = fixture();
        let first = identity("root", 1, "w1");
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(2)).unwrap();
        let second = identity("root", 2, "w2");
        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            prepare_unlocked_controlled(
                &roots,
                second.clone(),
                vec![digest('b')],
                LifecycleTimestamp(3),
                &known,
                WriteControl::fail(WritePhase::AfterDirectorySync, 5),
            )
        })
        .unwrap_err();
        let retried = prepare(
            &roots,
            second.clone(),
            vec![digest('b')],
            LifecycleTimestamp(30),
        )
        .unwrap();
        assert_eq!(
            retried.protected_targets,
            BTreeSet::from([digest('a'), digest('b')])
        );
        assert_eq!(retried.roots[&second.id].prepared_at, LifecycleTimestamp(3));
        assert!(prepare(
            &roots,
            identity("root", 2, "different-witness"),
            vec![digest('b')],
            LifecycleTimestamp(3),
        )
        .is_err());
        assert_eq!(validated_transaction_paths(&roots).unwrap().len(), 3);
        cleanup(&roots);
    }

    #[test]
    fn durable_commit_tombstone_and_legacy_retries_ignore_new_timestamp() {
        let (roots, known) = fixture();
        let root = identity("root", 1, "w1");
        prepare(&roots, root.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            persist_controlled(
                &roots,
                &known,
                JournalEntry::Commit {
                    id: root.id.clone(),
                    incarnation: root.incarnation,
                    witness: root.witness.clone(),
                    at: LifecycleTimestamp(2),
                },
                WriteControl::fail(WritePhase::AfterDirectorySync, 5),
            )
        })
        .unwrap_err();
        let committed = commit(
            &roots,
            &root.id,
            root.incarnation,
            &root.witness,
            LifecycleTimestamp(20),
        )
        .unwrap();
        assert_eq!(committed.roots[&root.id].committed_at, Some(LifecycleTimestamp(2)));

        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            persist_controlled(
                &roots,
                &known,
                JournalEntry::Tombstone {
                    id: root.id.clone(),
                    incarnation: root.incarnation,
                    witness: root.witness.clone(),
                    at: LifecycleTimestamp(3),
                },
                WriteControl::fail(WritePhase::AfterDirectorySync, 5),
            )
        })
        .unwrap_err();
        let tombstoned = remove_root(
            &roots,
            &root.id,
            root.incarnation,
            &root.witness,
            LifecycleTimestamp(30),
        )
        .unwrap();
        assert_eq!(tombstoned.roots[&root.id].tombstoned_at, Some(LifecycleTimestamp(3)));
        cleanup(&roots);

        let (roots, known) = fixture();
        let legacy = LegacyRoot {
            identity: identity("legacy", 1, "legacy-witness"),
            targets: vec![digest('b')],
            observed_at: LifecycleTimestamp(4),
        };
        crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            persist_controlled(
                &roots,
                &known,
                JournalEntry::Legacy {
                    identity: legacy.identity.clone(),
                    targets: BTreeSet::from([digest('b')]),
                    at: legacy.observed_at,
                },
                WriteControl::fail(WritePhase::AfterDirectorySync, 5),
            )
        })
        .unwrap_err();
        let mut retry = legacy.clone();
        retry.observed_at = LifecycleTimestamp(40);
        let imported = import_legacy_root(&roots, retry).unwrap();
        let durable = &imported.roots[&legacy.identity.id];
        assert_eq!(durable.prepared_at, LifecycleTimestamp(4));
        assert_eq!(durable.committed_at, Some(LifecycleTimestamp(4)));
        cleanup(&roots);
    }

    #[test]
    fn recover_replays_semantics_and_rejects_checksummed_invalid_transition() {
        let (roots, _) = fixture();
        let journal = roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR);
        fs::create_dir_all(&journal).unwrap();
        let invalid_entry = JournalEntry::Commit {
            id: RootId::new("missing").unwrap(),
            incarnation: Incarnation::new(1).unwrap(),
            witness: RootWitness::new("missing-witness").unwrap(),
            at: LifecycleTimestamp(1),
        };
        let body = render_entry(&invalid_entry);
        let checksum = SHA256::sha256_hex(body.as_bytes());
        fs::write(
            journal.join(format!("{:020}-{}.txn", 1, &checksum[..16])),
            format!("{body}checksum\t{checksum}\n"),
        )
        .unwrap();
        assert_eq!(recover(&roots).unwrap_err().kind(), io::ErrorKind::InvalidData);
        cleanup(&roots);
    }

    #[test]
    fn hostile_corruption_truncation_duplicates_and_unknowns_fail_closed() {
        let (roots, _) = fixture();
        let id = identity("root", 1, "w1");
        assert!(prepare(&roots, id.clone(), vec![], LifecycleTimestamp(1)).is_err());
        assert!(prepare(&roots, id.clone(), vec![digest('c'), digest('c')], LifecycleTimestamp(1)).is_err());
        assert!(prepare(&roots, id.clone(), vec![digest('d')], LifecycleTimestamp(1)).is_err());
        prepare(&roots, id, vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        let journal = roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR);
        let path = validated_transaction_paths(&roots).unwrap().pop().unwrap();
        let original = fs::read(&path).unwrap();
        let mut bad_checksum = original.clone();
        bad_checksum[0] = b'X';
        fs::write(&path, bad_checksum).unwrap();
        assert_eq!(
            snapshot(&roots).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(&path, &original).unwrap();
        fs::write(&path, &original[..original.len() / 2]).unwrap();
        let error = snapshot(&roots).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!error.to_string().is_empty());
        fs::write(&path, original).unwrap();
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&path, journal.join("hostile.txn")).unwrap();
            assert_eq!(snapshot(&roots).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
        cleanup(&roots);
    }

    #[test]
    fn fresh_store_snapshot_is_durable_and_deterministic() {
        let root = std::env::temp_dir().join(format!(
            "jet-lifecycle-fresh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let roots = Roots {
            root,
            dev_mode: true,
        };
        let first = snapshot(&roots).unwrap();
        let second = snapshot(&roots).unwrap();
        assert!(first.roots.is_empty());
        assert!(first.protected_targets.is_empty());
        assert_eq!(first, second);
        assert!(roots
            .hangar_dir()
            .join(DB_DIR)
            .join(JOURNAL_DIR)
            .is_dir());
        cleanup(&roots);
    }

    #[test]
    fn explicit_byte_member_and_target_bounds_fail_closed() {
        let (roots, _) = fixture();
        let oversized_targets = vec![digest('a'); MAX_TARGETS_PER_ROOT + 1];
        assert!(prepare(
            &roots,
            identity("bounded", 1, "w1"),
            oversized_targets,
            LifecycleTimestamp(1),
        )
        .unwrap_err()
        .to_string()
        .contains("target-count bound"));

        let journal = roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR);
        fs::create_dir_all(&journal).unwrap();
        fs::write(
            journal.join("00000000000000000001-0000000000000000.txn"),
            vec![b'x'; MAX_TRANSACTION_BYTES + 1],
        )
        .unwrap();
        assert!(snapshot(&roots)
            .unwrap_err()
            .to_string()
            .contains("byte bound"));
        fs::remove_dir_all(&journal).unwrap();
        fs::create_dir_all(&journal).unwrap();
        for index in 0..=MAX_JOURNAL_MEMBERS {
            fs::write(journal.join(format!("hostile-{index}")), b"x").unwrap();
        }
        assert!(snapshot(&roots)
            .unwrap_err()
            .to_string()
            .contains("member-count bound"));
        cleanup(&roots);
    }

    #[test]
    fn bounded_recovery_compacts_long_history_without_state_drift() {
        let (roots, _) = fixture();
        let mut current = identity("root", 1, "w1");
        prepare(&roots, current.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(
            &roots,
            &current.id,
            current.incarnation,
            &current.witness,
            LifecycleTimestamp(2),
        )
        .unwrap();
        for incarnation in 2..=66 {
            current = identity("root", incarnation, &format!("w{incarnation}"));
            let target = if incarnation % 2 == 0 {
                digest('b')
            } else {
                digest('a')
            };
            prepare(
                &roots,
                current.clone(),
                vec![target.clone()],
                LifecycleTimestamp(incarnation * 2 - 1),
            )
            .unwrap();
            commit(
                &roots,
                &current.id,
                current.incarnation,
                &current.witness,
                LifecycleTimestamp(incarnation * 2),
            )
            .unwrap();
        }
        let snapshot = snapshot(&roots).unwrap();
        assert_eq!(snapshot.roots[&current.id].identity, current);
        assert_eq!(snapshot.protected_targets.len(), 1);
        let journal = ensure_journal_dir(&roots).unwrap();
        let scan = scan_journal(&journal).unwrap();
        assert!(scan.snapshot.is_some());
        assert!(scan.transactions.len() < 10);
        cleanup(&roots);
    }

    #[test]
    fn published_snapshot_supersedes_lingering_authenticated_transactions() {
        let (roots, known) = fixture();
        let root = identity("root", 1, "w1");
        prepare(&roots, root.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &root.id, root.incarnation, &root.witness, LifecycleTimestamp(2)).unwrap();
        let state = load_state(&roots, &known).unwrap();
        let journal = ensure_journal_dir(&roots).unwrap();
        let scan = scan_journal(&journal).unwrap();
        assert_eq!(scan.last_sequence, 2);
        let body = render_snapshot(scan.last_sequence, &state);
        let checksum = SHA256::sha256_hex(body.as_bytes());
        fs::write(
            journal.path().join(SNAPSHOT_FILE),
            format!("{body}checksum\t{checksum}\n"),
        )
        .unwrap();

        let crashed = scan_journal(&journal).unwrap();
        assert_eq!(crashed.transaction_names.len(), 2);
        assert!(crashed.transactions.is_empty());
        assert_eq!(load_state(&roots, &known).unwrap(), state);
        cleanup(&roots);
    }

    #[test]
    fn admission_rejects_state_that_cannot_fit_future_snapshot() {
        let mut state = BTreeMap::new();
        let targets = (0..8)
            .map(|index| format!("sha256-{index:064x}"))
            .collect::<BTreeSet<_>>();
        for index in 0..MAX_ROOTS {
            let prefix = format!("root-{index:04}-");
            let id = format!("{prefix}{}", "i".repeat(512 - prefix.len()));
            let producer = format!("producer-{}", "p".repeat(503));
            let witness = format!("witness-{}", "w".repeat(504));
            let identity = RootIdentity::new(
                RootKind::Manual,
                RootId::new(id).unwrap(),
                ProducerId::new(producer).unwrap(),
                Incarnation::new(1).unwrap(),
                RootWitness::new(witness).unwrap(),
            );
            state.insert(
                identity.id.clone(),
                LifecycleRoot {
                    identity,
                    targets: targets.clone(),
                    protected_targets: targets.clone(),
                    phase: RootPhase::Committed,
                    prepared_at: LifecycleTimestamp(1),
                    committed_at: Some(LifecycleTimestamp(2)),
                    tombstoned_at: None,
                    metadata: RootMetadata::default(),
                    revision: 1,
                    legacy: false,
                },
            );
        }
        validate_state_bounds(&state).unwrap();
        assert!(validate_snapshot_wire(&state)
            .unwrap_err()
            .to_string()
            .contains("cannot fit"));
    }

    #[test]
    fn invalid_transitions_and_identity_reuse_fail_closed() {
        let (roots, _) = fixture();
        let first = identity("root", 1, "w1");
        assert!(commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(1)).is_err());
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(2)).unwrap();
        assert!(prepare(&roots, identity("root", 2, "w2"), vec![digest('b')], LifecycleTimestamp(3)).is_err());
        assert!(commit(&roots, &first.id, first.incarnation, &RootWitness::new("wrong").unwrap(), LifecycleTimestamp(3)).is_err());
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(3)).unwrap();
        assert!(prepare(&roots, identity("root", 3, "w3"), vec![digest('b')], LifecycleTimestamp(4)).is_err());
        assert!(remove_root(&roots, &first.id, first.incarnation, &RootWitness::new("wrong").unwrap(), LifecycleTimestamp(4)).is_err());
        cleanup(&roots);
    }

    #[test]
    fn tombstone_and_legacy_roots_preserve_audit_and_conservative_protection() {
        let (roots, _) = fixture();
        let first = identity("root", 1, "w1");
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(2)).unwrap();
        let removed = remove_root(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(3)).unwrap();
        assert!(removed.protected_targets.is_empty());
        assert_eq!(removed.roots[&first.id].targets, BTreeSet::from([digest('a')]));
        assert_eq!(removed.roots[&first.id].phase, RootPhase::Tombstoned);
        let legacy = LegacyRoot { identity: identity("legacy", 1, "legacy-witness"), targets: vec![digest('b')], observed_at: LifecycleTimestamp(4) };
        let transient = snapshot_with_legacy(&roots, &[legacy.clone()]).unwrap();
        assert_eq!(transient.protected_targets, BTreeSet::from([digest('b')]));
        let persisted = import_legacy_root(&roots, legacy).unwrap();
        assert_eq!(persisted.protected_targets, BTreeSet::from([digest('b')]));
        cleanup(&roots);
    }

    #[test]
    fn revisions_are_deterministic_and_include_closure_head() {
        let (left, _) = fixture();
        let (right, _) = fixture();
        for roots in [&left, &right] {
            let root = identity("root", 1, "w1");
            prepare(roots, root.clone(), vec![digest('b'), digest('a')], LifecycleTimestamp(1)).unwrap();
            commit(roots, &root.id, root.incarnation, &root.witness, LifecycleTimestamp(2)).unwrap();
        }
        let left_snapshot = snapshot(&left).unwrap();
        let right_snapshot = snapshot(&right).unwrap();
        assert_eq!(left_snapshot.revision, right_snapshot.revision);
        assert_eq!(left_snapshot.store_revision, right_snapshot.store_revision);
        assert!(left_snapshot.store_revision.as_str().starts_with("sha256-"));
        cleanup(&left); cleanup(&right);
    }

    #[test]
    fn concurrent_replacements_serialize_to_one_valid_transition() {
        let (roots, _) = fixture();
        let first = identity("root", 1, "w1");
        prepare(&roots, first.clone(), vec![digest('a')], LifecycleTimestamp(1)).unwrap();
        commit(&roots, &first.id, first.incarnation, &first.witness, LifecycleTimestamp(2)).unwrap();
        let roots = Arc::new(roots);
        let handles = [("w2-left", 'b'), ("w2-right", 'c')].into_iter().map(|(witness, target)| {
            let roots = Arc::clone(&roots);
            std::thread::spawn(move || prepare(&roots, identity("root", 2, witness), vec![digest(target)], LifecycleTimestamp(3)))
        }).collect::<Vec<_>>();
        let results = handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let snapshot = snapshot(&roots).unwrap();
        assert_eq!(snapshot.roots[&RootId::new("root").unwrap()].phase, RootPhase::Prepared);
        assert!(snapshot.protected_targets.contains(&digest('a')));
        assert_eq!(snapshot.protected_targets.len(), 2);
        cleanup(&roots);
    }
}
