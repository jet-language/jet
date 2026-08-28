use super::{
    admission_reservation, admission_size, ensure_hangar_capacity, fsync_tree,
    make_tree_writable_for_removal, recover_seals, remove_seal, seal_node, sync_store_directory,
    write_seal, Closure, Ingest, Receipt, Roots, StoreEntry, OBJECTS_DIR, PARTIAL_SUFFIX,
    STAGE_DIR,
};
use crate::SHA256;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TRANSACTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A verified object supplied by a format adapter. The adapter owns how the
/// bytes were fetched and hashed; this transaction owns their durable
/// publication.
#[derive(Debug, Clone)]
pub(crate) struct AdmissionObject {
    pub source: PathBuf,
    pub digest: String,
    pub bytes: u64,
    pub allow_semantic_xattrs: bool,
    pub repair_corrupt: bool,
}

/// A format-specific receipt whose atomic publication is shared with package
/// receipts. Nix builds the closure receipt bytes; Hangar publishes them.
#[derive(Debug, Clone)]
pub(crate) struct AdmissionReceipt {
    pub digest: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionFailurePoint {
    AfterObjectPublication,
    AfterReceiptPublication,
    AfterClosureRegistration,
}

#[cfg(test)]
thread_local! {
    static ADMISSION_FAILURE: std::cell::RefCell<Option<AdmissionFailurePoint>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_admission_failure<T>(
    point: AdmissionFailurePoint,
    operation: impl FnOnce() -> T,
) -> T {
    ADMISSION_FAILURE.with(|slot| {
        let previous = slot.replace(Some(point));
        let result = operation();
        slot.replace(previous);
        result
    })
}

fn injected_failure(point: AdmissionFailurePoint) -> std::io::Result<()> {
    #[cfg(test)]
    if ADMISSION_FAILURE.with(|slot| *slot.borrow()) == Some(point) {
        return Err(std::io::Error::other(format!(
            "injected Hangar admission failure after {point:?}"
        )));
    }
    #[cfg(not(test))]
    let _ = point;
    Ok(())
}

/// The one Hangar commit boundary shared by native ingest, Nix admission, and
/// realized-package publication. Adapters stage and certify format-specific
/// input, then hand this type canonical object identities and candidate
/// records. This type owns the shared durable order and rollback state.
pub(crate) struct AdmissionTransaction<'a> {
    roots: &'a Roots,
    stage: PathBuf,
    objects: Vec<AdmissionObject>,
    published: Vec<PublishedObject>,
    receipts: Vec<PathBuf>,
    committed: bool,
}

#[derive(Debug)]
struct PublishedObject {
    destination: PathBuf,
    original_source: Option<PathBuf>,
    held_source: Option<PathBuf>,
    backup: Option<PathBuf>,
    pending: Option<PathBuf>,
    created: bool,
}

impl<'a> AdmissionTransaction<'a> {
    pub(crate) fn new(roots: &'a Roots) -> std::io::Result<Self> {
        Ingest::ensure_real_directory(&roots.hangar_dir(), "Hangar root")?;
        let stage_parent = roots.hangar_dir().join(STAGE_DIR);
        Ingest::ensure_real_directory(&stage_parent, "Hangar admission staging")?;
        let stage = loop {
            let candidate = stage_parent.join(format!(
                "admission-{}-{}",
                std::process::id(),
                TRANSACTION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        Ok(Self {
            roots,
            stage,
            objects: Vec::new(),
            published: Vec::new(),
            receipts: Vec::new(),
            committed: false,
        })
    }

    /// Recover the shared object staging surface before any adapter starts a
    /// new admission. Nix's live-pid fetch staging remains its adapter-owned
    /// surface and is swept by the Nix adapter.
    pub(crate) fn recover_unlocked(roots: &Roots) -> std::io::Result<usize> {
        let hangar = roots.hangar_dir();
        let mut swept = 0usize;
        swept += recover_seals(&hangar)?;
        let stage = hangar.join(STAGE_DIR);
        swept += sweep_abandoned_directory(&stage, "Hangar admission staging")?;
        let objects = hangar.join(OBJECTS_DIR);
        let metadata = match fs::symlink_metadata(&objects) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(swept),
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Hangar object pool is not a real directory; repair the path before recovery",
            ));
        }
        for entry in fs::read_dir(&objects)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(PARTIAL_SUFFIX) {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || metadata.is_file() {
                fs::remove_file(&path)?;
            } else if metadata.is_dir() {
                remove_node(&path)?;
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar partial object `{}` is not removable",
                        path.display()
                    ),
                ));
            }
            swept += 1;
        }
        Ok(swept)
    }

    /// Queue one adapter-verified object and return its deterministic CAS
    /// path. No bytes become visible until `commit`.
    pub(crate) fn stage_object(
        &mut self,
        object: AdmissionObject,
    ) -> std::io::Result<PathBuf> {
        validate_digest(&object.digest)?;
        fs::symlink_metadata(&object.source).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!(
                    "cannot inspect staged Hangar object `{}`: {error}",
                    object.source.display()
                ),
            )
        })?;
        let destination = self
            .roots
            .hangar_dir()
            .join(OBJECTS_DIR)
            .join(&object.digest);
        self.objects.push(object);
        Ok(destination)
    }

    /// Publish staged objects, extra format receipts, package receipts, and
    /// the closure delta in one shared order. The adapter supplies only the
    /// representation-specific registration mode and already-built entries.
    pub(crate) fn commit(
        &mut self,
        entries: &mut [StoreEntry],
        extra_receipts: &[AdmissionReceipt],
        excluded: Option<&Path>,
        mode: Closure::RegistrationMode,
        fresh_action_key: Option<&str>,
    ) -> std::io::Result<bool> {
        if self.committed {
            return Err(std::io::Error::other("Hangar admission transaction already committed"));
        }
        self.publish_objects(excluded)?;
        injected_failure(AdmissionFailurePoint::AfterObjectPublication)?;

        for receipt in extra_receipts {
            if self.publish_receipt(receipt)? {
                self.receipts.push(
                    self.roots
                        .hangar_dir()
                        .join(Closure::RECEIPTS_DIR)
                        .join(&receipt.digest),
                );
            }
        }
        for entry in entries.iter_mut() {
            if Receipt::prepare_entry_receipt_with_status(self.roots, entry)? {
                self.receipts.push(
                    self.roots
                        .hangar_dir()
                        .join(Closure::RECEIPTS_DIR)
                        .join(&entry.receipt),
                );
            }
        }
        injected_failure(AdmissionFailurePoint::AfterReceiptPublication)?;

        let registered = Closure::register_entries_unlocked_with_mode(
            self.roots,
            entries,
            mode,
            fresh_action_key,
        )?;
        self.committed = true;
        if !registered {
            self.remove_new_receipts();
        }
        self.finish();
        injected_failure(AdmissionFailurePoint::AfterClosureRegistration)?;
        Ok(registered)
    }

    /// Small compatibility seam for the old projection test helpers. It uses
    /// the same object publication and rollback implementation but has no
    /// package metadata to register.
    #[cfg(test)]
    pub(crate) fn commit_objects(
        &mut self,
        excluded: Option<&Path>,
    ) -> std::io::Result<Vec<PathBuf>> {
        self.publish_objects(excluded)?;
        injected_failure(AdmissionFailurePoint::AfterObjectPublication)?;
        let destinations = self
            .objects
            .iter()
            .map(|object| {
                self.roots
                    .hangar_dir()
                    .join(OBJECTS_DIR)
                    .join(&object.digest)
            })
            .collect();
        self.committed = true;
        self.finish();
        Ok(destinations)
    }

    fn publish_objects(&mut self, excluded: Option<&Path>) -> std::io::Result<()> {
        let objects_dir = self.roots.hangar_dir().join(OBJECTS_DIR);
        Ingest::ensure_real_directory(&objects_dir, "Hangar object pool")?;
        let mut incoming = 0u64;
        let mut already_counted = 0u64;
        let mut counted = BTreeSet::new();
        for object in &self.objects {
            if object.repair_corrupt {
                Ingest::invalidate_verified_digest(
                    &objects_dir.join(&object.digest),
                );
            }
            if !counted.insert(object.digest.clone()) {
                continue;
            }
            let destination = objects_dir.join(&object.digest);
            if !self.object_is_verified(&destination, object)? {
                incoming = incoming.checked_add(object.bytes).ok_or_else(|| {
                    std::io::Error::other("Hangar admission size overflowed")
                })?;
            }
            if object.source != destination
                && self.source_is_internal(&object.source)?
                && !excluded.is_some_and(|path| object.source.starts_with(path))
            {
                already_counted = already_counted
                    .checked_add(admission_size(&object.source)?)
                    .ok_or_else(|| std::io::Error::other("Hangar admission size overflowed"))?;
            }
        }
        incoming = incoming.saturating_sub(already_counted);
        ensure_hangar_capacity(
            self.roots,
            admission_reservation(incoming),
            excluded,
        )?;
        for index in 0..self.objects.len() {
            self.publish_object(index)?;
        }
        if !self.published.is_empty() {
            sync_store_directory(&objects_dir)?;
        }
        Ok(())
    }

    fn object_is_verified(
        &self,
        destination: &Path,
        object: &AdmissionObject,
    ) -> std::io::Result<bool> {
        match fs::symlink_metadata(destination) {
            Ok(_) => Ok(Ingest::verified_output_hash_persistent(
                destination,
                Some(&self.roots.hangar_dir()),
                object.allow_semantic_xattrs,
            )
            .is_ok_and(|actual| actual == object.digest)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn source_is_internal(&self, source: &Path) -> std::io::Result<bool> {
        // A symlink is an input node, not a tree owned by Hangar. Do not
        // canonicalize it: that would follow a symlink-root (including a
        // dangling one) and either misclassify it or reject valid no-follow
        // input before the adapter can publish the link itself.
        if fs::symlink_metadata(source)?.file_type().is_symlink() {
            return Ok(false);
        }
        let hangar = fs::canonicalize(self.roots.hangar_dir())?;
        Ok(fs::canonicalize(source)?.starts_with(hangar))
    }

    fn publish_object(&mut self, index: usize) -> std::io::Result<()> {
        let object = self.objects[index].clone();
        let objects_dir = self.roots.hangar_dir().join(OBJECTS_DIR);
        let destination = objects_dir.join(&object.digest);
        let internal = self.source_is_internal(&object.source)?;

        if object.source == destination {
            if self.object_is_verified(&destination, &object)? {
                return Ok(());
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "canonical Hangar object `{}` does not match its digest",
                    destination.display()
                ),
            ));
        }

        let existing = match fs::symlink_metadata(&destination) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if existing.is_some() && self.object_is_verified(&destination, &object)? {
            if internal {
                let held = self.stage.join("consumed").join(index.to_string());
                fs::create_dir_all(held.parent().expect("consumed path has a parent"))?;
                make_tree_writable_for_removal(&object.source)?;
                fs::rename(&object.source, &held)?;
                if let Some(parent) = object.source.parent() {
                    sync_store_directory(parent)?;
                }
                self.published.push(PublishedObject {
                    destination,
                    original_source: Some(object.source),
                    held_source: Some(held),
                    backup: None,
                    pending: None,
                    created: false,
                });
            }
            return Ok(());
        }
        if existing.is_some() && !object.repair_corrupt {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "existing Hangar object `{}` does not match its digest",
                    destination.display()
                ),
            ));
        }

        let backup = if existing.is_some() {
            let backup = self.stage.join("replaced").join(index.to_string());
            fs::create_dir_all(backup.parent().expect("replacement path has a parent"))?;
            make_tree_writable_for_removal(&destination)?;
            fs::rename(&destination, &backup)?;
            Some(backup)
        } else {
            None
        };
        let original_source = internal.then(|| object.source.clone());
        let record_index = self.published.len();
        self.published.push(PublishedObject {
            destination: destination.clone(),
            original_source,
            held_source: None,
            backup,
            pending: None,
            created: false,
        });
        if self.published[record_index].backup.is_some() {
            sync_store_directory(&objects_dir)?;
        }

        let partial = objects_dir.join(format!(
            "{}{}",
            object.digest, PARTIAL_SUFFIX
        ));
        if fs::symlink_metadata(&partial).is_ok() {
            remove_node(&partial)?;
        }
        self.published[record_index].pending = Some(partial.clone());

        if internal {
            make_tree_writable_for_removal(&object.source)?;
            fs::rename(&object.source, &partial)?;
            if let Some(parent) = object.source.parent() {
                sync_store_directory(parent)?;
            }
        } else {
            Ingest::copy_nofollow_tree(&object.source, &partial)
                .map_err(|error| std::io::Error::other(error.what()))?;
        }
        seal_node(&partial)?;
        let actual = Ingest::verified_output_hash(
            &partial,
            Some(&self.roots.hangar_dir()),
            object.allow_semantic_xattrs,
        )?;
        if actual != object.digest {
            return Err(std::io::Error::other(format!(
                "Hangar object `{}` re-hashed as `{actual}`",
                object.digest
            )));
        }
        fsync_tree(&partial)?;
        sync_store_directory(&objects_dir)?;
        fs::rename(&partial, &destination)?;
        self.published[record_index].pending = None;
        self.published[record_index].created = true;
        write_seal(&destination, &self.roots.hangar_dir(), &object.digest)?;
        Ingest::refresh_verified_digest(
            &destination,
            &self.roots.hangar_dir(),
            object.allow_semantic_xattrs,
            &object.digest,
        )?;
        sync_store_directory(&objects_dir)?;
        Ok(())
    }

    fn publish_receipt(&self, receipt: &AdmissionReceipt) -> std::io::Result<bool> {
        let expected = format!("sha256-{}", SHA256::sha256_hex(&receipt.bytes));
        if expected != receipt.digest {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar admission receipt digest is `{}`, expected `{expected}`",
                    receipt.digest
                ),
            ));
        }
        let receipts = self.roots.hangar_dir().join(Closure::RECEIPTS_DIR);
        Ingest::ensure_real_directory(&receipts, "Hangar receipt directory")?;
        let path = receipts.join(&receipt.digest);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Hangar receipt `{}` is not a regular file", receipt.digest),
                ));
            }
            Ok(_) => {
                if fs::read(&path)? != receipt.bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Hangar receipt `{}` is corrupt", receipt.digest),
                    ));
                }
                return Ok(false);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let partial = receipts.join(format!(
            ".{}-{}-{}.partial",
            receipt.digest,
            std::process::id(),
            TRANSACTION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)?;
            file.write_all(&receipt.bytes)?;
            file.sync_all()?;
            match fs::rename(&partial, &path) {
                Ok(()) => {
                    sync_store_directory(&receipts)?;
                    Ok(true)
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let _ = fs::remove_file(&partial);
                    let existing = fs::read(&path)?;
                    if existing != receipt.bytes {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Hangar receipt `{}` changed during publication", receipt.digest),
                        ));
                    }
                    Ok(false)
                }
                Err(error) => Err(error),
            }
        })();
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result
    }

    fn remove_new_receipts(&mut self) {
        for path in self.receipts.drain(..) {
            let _ = fs::remove_file(path);
        }
    }

    fn finish(&mut self) {
        for published in self.published.drain(..) {
            if let Some(path) = published.held_source {
                let _ = remove_node(&path);
            }
            if let Some(path) = published.backup {
                let _ = remove_node(&path);
            }
        }
        let _ = remove_node(&self.stage);
    }

    fn rollback(&mut self) {
        for published in self.published.drain(..).rev() {
            if let Some(pending) = published.pending {
                if fs::symlink_metadata(&pending).is_ok() {
                    if let Some(original) = published.original_source.as_ref() {
                        if fs::symlink_metadata(original).is_err() {
                            let _ = fs::rename(&pending, original);
                        } else {
                            let _ = remove_node(&pending);
                        }
                    } else {
                        let _ = remove_node(&pending);
                    }
                }
            }
            if published.created {
                let _ = remove_seal(&published.destination, &self.roots.hangar_dir());
                if let Some(original) = published.original_source.as_ref() {
                    if fs::symlink_metadata(&published.destination).is_ok()
                        && fs::symlink_metadata(original).is_err()
                    {
                        let _ = make_tree_writable_for_removal(&published.destination);
                        let _ = fs::rename(&published.destination, original);
                    } else {
                        let _ = remove_node(&published.destination);
                    }
                } else {
                    let _ = remove_node(&published.destination);
                }
            }
            if let Some(held) = published.held_source {
                if let Some(original) = published.original_source.as_ref() {
                    if fs::symlink_metadata(&held).is_ok()
                        && fs::symlink_metadata(original).is_err()
                    {
                        let _ = fs::rename(&held, original);
                    } else {
                        let _ = remove_node(&held);
                    }
                } else {
                    let _ = remove_node(&held);
                }
            }
            if let Some(backup) = published.backup {
                if fs::symlink_metadata(&published.destination).is_err() {
                    let _ = fs::rename(&backup, &published.destination);
                } else {
                    let _ = remove_node(&backup);
                }
            }
        }
        self.remove_new_receipts();
        let _ = remove_node(&self.stage);
    }
}

impl Drop for AdmissionTransaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.rollback();
        }
    }
}

fn validate_digest(digest: &str) -> std::io::Result<()> {
    let mut components = Path::new(digest).components();
    if digest.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid Hangar object digest `{digest}`"),
        ));
    }
    Ok(())
}

fn remove_node(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        make_tree_writable_for_removal(path)?;
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Remove only abandoned children from a staging directory. The directory
/// itself must be a real directory; a symlink is a path-escape repair stop,
/// not permission to walk or remove the target it names.
pub(super) fn sweep_abandoned_directory(path: &Path, label: &str) -> std::io::Result<usize> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{label} is not a real directory; repair the path before recovery"),
        ));
    }
    let mut swept = 0;
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(&child)?;
        } else if metadata.is_dir() {
            remove_node(&child)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{label} contains an unsupported entry `{}`",
                    child.display()
                ),
            ));
        }
        swept += 1;
    }
    Ok(swept)
}

#[cfg(test)]
#[path = "TransactionTests.rs"]
mod transaction_tests;
