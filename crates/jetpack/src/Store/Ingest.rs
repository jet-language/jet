use super::*;

pub(crate) fn try_entry_output_hash(roots: &Roots, entry: &StoreEntry) -> Result<String, String> {
    let hangar = roots.hangar_dir();
    let canonical_hangar = fs::canonicalize(&hangar).unwrap_or_else(|_| hangar.clone());
    let out = Path::new(&entry.out);
    if out.starts_with(&hangar) || out.starts_with(&canonical_hangar) {
        // Hangar-owned objects may share payload inodes with its cas pool.
        super::super::Envelope::try_output_hash_of_in_hangar(
            &entry.out,
            &canonical_hangar,
            !entry.platform_artifact_kind.is_empty(),
        )
    } else {
        super::super::Envelope::try_output_hash_of(&entry.out)
    }
}

#[derive(Default)]
pub(super) struct MovePathPermissions {
    original: Vec<(PathBuf, fs::Permissions)>,
}

impl MovePathPermissions {
    pub(super) fn make_writable(&mut self, path: &Path, root: &Path) -> std::io::Result<()> {
        if !path.starts_with(root) {
            return Err(std::io::Error::other("quarantine path escapes Hangar root"));
        }
        let mut current = Some(path);
        while let Some(directory) = current {
            let metadata = fs::symlink_metadata(directory)?;
            if metadata.is_dir()
                && !self.original.iter().any(|(saved, _)| saved == directory)
            {
                let original = metadata.permissions();
                let mut writable = original.clone();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    writable.set_mode(writable.mode() | 0o200);
                }
                #[cfg(not(unix))]
                writable.set_readonly(false);
                fs::set_permissions(directory, writable)?;
                self.original.push((directory.to_path_buf(), original));
            }
            if directory == root {
                return Ok(());
            }
            current = directory.parent();
        }
        Err(std::io::Error::other("quarantine path has no Hangar parent"))
    }

    pub(super) fn renamed(&mut self, from: &Path, to: &Path) {
        for (path, _) in &mut self.original {
            if let Ok(suffix) = path.strip_prefix(from) {
                *path = to.join(suffix);
            }
        }
    }

    pub(super) fn restore(&mut self) -> std::io::Result<()> {
        let mut first_error = None;
        for (path, permissions) in self.original.drain(..).rev() {
            if fs::symlink_metadata(&path).is_ok() {
                if let Err(error) = fs::set_permissions(path, permissions) {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for MovePathPermissions {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Remove an invalid local cache candidate so provider realization cannot
/// mistake the same tampered directory for a fresh hit. Never removes external
/// outputs such as `/nix/store`; their provider must realize them again.
pub fn quarantine_invalid_entry(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
) -> std::io::Result<()> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let expected_id = entry_id(
            &entry.name,
            &entry.version,
            &entry.reference,
            &entry.out,
        );
        if entry.id != expected_id || Path::new(&entry.id).components().count() != 1 {
            return Err(std::io::Error::other("invalid cache record identity"));
        }
        Closure::recover_closure_journal_unlocked(roots)?;
        let Some(current) = list_unlocked(roots)
            .into_iter()
            .find(|candidate| candidate.id == entry.id)
        else {
            return Ok(());
        };
        if current.envelope.output_hash != entry.envelope.output_hash
            || entry_action_key(&current) != entry_action_key(entry)
        {
            return Ok(());
        }
        let current_expected_id = entry_id(
            &current.name,
            &current.version,
            &current.reference,
            &current.out,
        );
        if current.id != current_expected_id {
            return Err(std::io::Error::other("invalid cache record identity"));
        }
        let proof = verify_cache_entry(roots, &current, &current.reference, expectation);
        if proof.trusted() {
            return Ok(());
        }
        let quarantine_output = proof.output_exists && !proof.output_digest;
        let hangar = roots.hangar_dir();
        let mut permissions = MovePathPermissions::default();
        let operation = (|| {
            permissions.make_writable(&hangar, &hangar)?;
            let quarantine = hangar.join("quarantine");
            fs::create_dir_all(&quarantine)?;
            let stamp = now_secs();
            let record = hangar.join(&current.id);
            if fs::symlink_metadata(&record).is_ok() {
                permissions.make_writable(&record, &hangar)?;
                let destination = quarantine.join(format!("record-{}-{stamp}", current.id));
                fs::rename(&record, &destination)?;
                permissions.renamed(&record, &destination);
            }
            let canonical_output = hangar
                .join(OBJECTS_DIR)
                .join(&current.envelope.output_hash);
            let owned_output = (Path::new(&current.out) == canonical_output)
                .then(|| PathBuf::from(&current.out))
                .or_else(|| expectation.owned_output.clone());
            if quarantine_output {
                if let Some(owned) = &owned_output {
                    if fs::symlink_metadata(owned).is_ok() {
                        let canonical_hangar = fs::canonicalize(&hangar)?;
                        let canonical_owned = fs::canonicalize(owned)?;
                        if !owned.starts_with(&hangar)
                            || !canonical_owned.starts_with(&canonical_hangar)
                        {
                            return Err(std::io::Error::other(
                                "derived cache output escapes canonical Hangar root",
                            ));
                        }
                        let name = owned
                            .file_name()
                            .and_then(|name| name.to_str())
                            .ok_or_else(|| std::io::Error::other("invalid owned output name"))?;
                        permissions.make_writable(owned, &hangar)?;
                        let destination = quarantine.join(format!("output-{name}-{stamp}"));
                        fs::rename(owned, &destination)?;
                        permissions.renamed(owned, &destination);
                    }
                }
            }
            Closure::tombstone_closure_record_unlocked(roots, &current.id)?;
            Ok(())
        })();
        let restored = permissions.restore();
        match (operation, restored) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(restore)) => Err(std::io::Error::other(format!(
                "{error}; restoring Hangar permissions failed: {restore}"
            ))),
        }
    })
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(any(test, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsIngestContract {
    open_reparse_point: u32,
    reparse_attribute: u32,
    stable_volume_and_file_id: bool,
}

#[cfg(any(test, windows))]
fn windows_ingest_contract() -> WindowsIngestContract {
    WindowsIngestContract {
        open_reparse_point: 0x0020_0000,
        reparse_attribute: 0x0000_0400,
        stable_volume_and_file_id: true,
    }
}

type StableMetaIdentity = (u64, u64, u64, u32);

#[cfg(any(test, windows))]
fn windows_stable_identity(
    volume: Option<u32>,
    file: Option<u64>,
    len: u64,
    attributes: u32,
) -> Result<StableMetaIdentity, IngestError> {
    let volume = volume.ok_or_else(|| {
        IngestError::Invalid("Windows file identity has no volume serial number".into())
    })?;
    let file = file.ok_or_else(|| {
        IngestError::Invalid("Windows file identity has no file index".into())
    })?;
    Ok((u64::from(volume), file, len, attributes))
}

/// Inputs for a Hangar Store v2 ingest. `outputs` must include `"out"`.
/// `cache_identity` is the store-side deriver/action identity (JP2 owns the
/// full action IR — this only records the fingerprints Hangar already stores).
#[derive(Debug, Clone)]
pub struct IngestRequest {
    pub name: String,
    pub version: String,
    pub reference: String,
    pub cache_identity: CacheIdentity,
    pub references: Vec<String>,
    /// Named output roots to ingest (`out` required).
    pub outputs: BTreeMap<String, PathBuf>,
    pub signature: String,
    pub provenance: String,
    /// Explicit platform artifact kind. Empty rejects semantic xattrs;
    /// non-empty is the product surface that keeps them (plan E4-JP1).
    pub platform_artifact_kind: String,
}

impl IngestRequest {
    /// Semantic xattrs are kept only when an explicit platform artifact kind
    /// is set on the ingest request / CLI.
    pub fn allow_semantic_xattrs(&self) -> bool {
        !self.platform_artifact_kind.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestedObject {
    pub entry: StoreEntry,
    /// True when an existing content-identical object was reused.
    pub deduplicated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestError {
    PathLaw(super::super::Envelope::PathLawError),
    Mutated(String),
    Invalid(String),
    Io(String),
}

impl IngestError {
    pub fn code(&self) -> &'static str {
        match self {
            IngestError::PathLaw(_) => "E1299",
            IngestError::Mutated(_) => "E1315",
            IngestError::Invalid(_) | IngestError::Io(_) => "E1315",
        }
    }

    pub fn what(&self) -> String {
        match self {
            IngestError::PathLaw(err) => err.what(),
            IngestError::Mutated(msg) => format!("hangar ingest aborted: {msg}"),
            IngestError::Invalid(msg) => format!("hangar ingest rejected: {msg}"),
            IngestError::Io(msg) => format!("hangar ingest failed: {msg}"),
        }
    }

    pub fn why(&self) -> String {
        match self {
            IngestError::PathLaw(err) => err.why(),
            IngestError::Mutated(_) => {
                "Race-safe no-follow ingest re-stats open handles and aborts if the source mutates (E4-JP1)."
                    .into()
            }
            IngestError::Invalid(msg) => msg.clone(),
            IngestError::Io(msg) => msg.clone(),
        }
    }

    pub fn fix(&self) -> &'static str {
        match self {
            IngestError::PathLaw(err) => err.fix(),
            IngestError::Mutated(_) => {
                "Re-run the ingest against a stable source tree; do not modify files while Hangar copies them."
            }
            IngestError::Invalid(_) => {
                "Fix the rejected tree (path law, special files, or unsupported xattrs) and ingest again."
            }
            IngestError::Io(_) => "Check hangar permissions and free disk space, then retry.",
        }
    }

    pub fn report(&self, theme: &super::super::Output::Theme) {
        theme.error_coded(self.code(), &self.what(), &self.why(), self.fix());
    }
}

impl From<std::io::Error> for IngestError {
    fn from(err: std::io::Error) -> Self {
        IngestError::Io(err.to_string())
    }
}

/// Sweep abandoned staging / `.partial` object dirs left by a crash.
pub fn recover_hangar_staging(roots: &Roots) -> std::io::Result<usize> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_hangar_staging_unlocked(roots)
    })
}

pub(super) fn recover_hangar_staging_unlocked(roots: &Roots) -> std::io::Result<usize> {
    let hangar = roots.hangar_dir();
    let mut swept = 0usize;
    let stage = hangar.join(STAGE_DIR);
    if stage.is_dir() {
        for ent in fs::read_dir(&stage)? {
            let ent = ent?;
            let path = ent.path();
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
                swept += 1;
            } else {
                fs::remove_file(&path)?;
                swept += 1;
            }
        }
    }
    let objects = hangar.join(OBJECTS_DIR);
    if objects.is_dir() {
        for ent in fs::read_dir(&objects)? {
            let ent = ent?;
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.ends_with(PARTIAL_SUFFIX) {
                let path = ent.path();
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
                swept += 1;
            }
        }
    }
    Ok(swept)
}

/// Atomic staged ingest: no-follow copy → path-law + canonical digest →
/// fsync/rename into `hangar/objects/<digest>/`, package record, referrers.
/// Same primary digest reuses the object (path-independent dedupe).
pub fn ingest_tree(roots: &Roots, req: &IngestRequest) -> Result<IngestedObject, IngestError> {
    let mut outcome: Option<Result<IngestedObject, IngestError>> = None;
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        outcome = Some(ingest_tree_unlocked(roots, req));
        Ok(())
    })
    .map_err(|e| IngestError::Io(e.to_string()))?;
    outcome.unwrap_or_else(|| Err(IngestError::Io("ingest produced no result".into())))
}

fn ingest_tree_unlocked(
    roots: &Roots,
    req: &IngestRequest,
) -> Result<IngestedObject, IngestError> {
    if !req.outputs.contains_key("out") {
        return Err(IngestError::Invalid(
            "ingest requires a named output `out`".into(),
        ));
    }
    if let Some(name) = req.outputs.keys().find(|name| !valid_output_name(name)) {
        return Err(IngestError::Invalid(format!(
            "named output `{name}` must be one path component"
        )));
    }
    let hangar = roots.hangar_dir();
    fs::create_dir_all(&hangar)?;
    recover_hangar_staging_unlocked(roots)?;

    let stamp = format!("{}-{}", now_secs(), std::process::id());
    let stage = hangar.join(STAGE_DIR).join(&stamp);
    fs::create_dir_all(&stage)?;

    let result = (|| {
        let mut named_digests = BTreeMap::new();
        let mut staged_outs = BTreeMap::new();
        for (name, src) in &req.outputs {
            let dst = stage.join("outputs").join(name);
            let parent = dst.parent().ok_or_else(|| {
                IngestError::Invalid(format!("named output `{name}` has no staging parent"))
            })?;
            fs::create_dir_all(parent)?;
            // Gate semantic xattrs on the *source* tree (byte copy drops them).
            if !req.allow_semantic_xattrs() {
                reject_semantic_xattrs_tree(src)?;
            }
            copy_nofollow_tree(src, &dst)?;
            if req.allow_semantic_xattrs() {
                copy_semantic_xattrs_tree(src, &dst)?;
            }
            seal_node(&dst)?;
            let digest = super::super::Envelope::try_output_hash_of_with_policy(
                &dst.to_string_lossy(),
                req.allow_semantic_xattrs(),
                &mut |_, _| {},
            )
            .map_err(|msg| {
                if msg.contains("case-fold")
                    || msg.contains("reserved")
                    || msg.contains("trailing")
                    || msg.contains("store path rejected")
                {
                    IngestError::PathLaw(super::super::Envelope::PathLawError {
                        code: "archive",
                        path: dst.display().to_string(),
                        detail: msg,
                    })
                } else if msg.contains("changed while") || msg.contains("changed before") {
                    IngestError::Mutated(msg)
                } else {
                    IngestError::Invalid(msg)
                }
            })?;
            named_digests.insert(name.clone(), digest);
            staged_outs.insert(name.clone(), dst);
        }
        let primary = named_digests
            .get("out")
            .cloned()
            .ok_or_else(|| IngestError::Invalid("missing `out` digest".into()))?;
        let objects = hangar.join(OBJECTS_DIR);
        fs::create_dir_all(&objects)?;
        let final_obj = objects.join(&primary);
        let deduplicated = final_obj.is_dir();
        for (name, staged) in &staged_outs {
            let digest = named_digests.get(name).ok_or_else(|| {
                IngestError::Invalid(format!("named output `{name}` has no digest"))
            })?;
            let destination = objects.join(digest);
            if destination.is_dir() {
                seal_node(&destination)?;
                let actual = super::super::Envelope::try_output_hash_of_in_hangar(
                    &destination.to_string_lossy(),
                    &hangar,
                    req.allow_semantic_xattrs(),
                )
                .map_err(IngestError::Invalid)?;
                if &actual != digest {
                    return Err(IngestError::Invalid(format!(
                        "existing object `{digest}` re-hashed as `{actual}`"
                    )));
                }
                fsync_tree(&destination)?;
                super::sync_store_directory(&objects)?;
                make_tree_writable_for_removal(staged)?;
                fs::remove_dir_all(staged)?;
                continue;
            }
            let partial = objects.join(format!("{digest}{PARTIAL_SUFFIX}"));
            if partial.exists() {
                let _ = fs::remove_dir_all(&partial);
            }
            // This filesystem denies renaming a read-only directory. Reopen
            // only inside the Hangar lock, publish, then restore the sealed
            // mode before the canonical path or metadata becomes visible.
            make_tree_writable_for_removal(staged)?;
            fs::rename(staged, &partial)?;
            seal_node(&partial)?;
            let actual = super::super::Envelope::try_output_hash_of_in_hangar(
                &partial.to_string_lossy(),
                &hangar,
                req.allow_semantic_xattrs(),
            )
            .map_err(IngestError::Invalid)?;
            if &actual != digest {
                return Err(IngestError::Invalid(format!(
                    "sealed object `{digest}` re-hashed as `{actual}`"
                )));
            }
            fsync_tree(&partial)?;
            super::sync_store_directory(&objects)?;
            fs::rename(&partial, &destination)?;
            super::sync_store_directory(&objects)?;
        }

        let out_path = final_obj.to_string_lossy().into_owned();
        let bin = final_obj.join("bin");
        let envelope = super::super::Envelope::Envelope {
            output_hash: primary.clone(),
            platform: if req.cache_identity.platform.is_empty() {
                super::super::Envelope::host_platform()
            } else {
                req.cache_identity.platform.clone()
            },
            signature: req.signature.clone(),
            provenance: if req.provenance.is_empty() {
                format!("{} via hangar-ingest", req.reference)
            } else {
                req.provenance.clone()
            },
        };
        let id = entry_id(&req.name, &req.version, &req.reference, &out_path);
        let dir = hangar.join(&id);
        let now = now_secs();
        let realized_at = read_meta(&dir).and_then(|m| m.realized_at).unwrap_or(now);
        if dir.exists() {
            // Refresh record only.
        } else {
            fs::create_dir_all(&dir)?;
        }
        let entry = StoreEntry {
            id: id.clone(),
            name: req.name.clone(),
            version: req.version.clone(),
            reference: req.reference.clone(),
            out: out_path,
            bin: if bin.is_dir() {
                bin.to_string_lossy().into_owned()
            } else {
                String::new()
            },
            rlib: String::new(),
            envelope,
            cache_identity: req.cache_identity.clone(),
            references: req.references.clone(),
            named_outputs: named_digests.clone(),
            platform_artifact_kind: req.platform_artifact_kind.clone(),
            producer_record: canonical_producer(
                "hangar-ingest",
                &format!("cas:{primary}"),
                &primary,
                &req.cache_identity,
                req.outputs
                    .keys()
                    .map(|name| (format!("output.{name}"), named_digests[name].clone()))
                    .collect(),
            )?,
            realized_at,
            last_used_at: now,
        };
        register_entry_unlocked(roots, &entry)?;
        Ok(IngestedObject { entry, deduplicated })
    })();

    // Always scrub this stage dir (success moved trees out; failure quarantines).
    if result.is_err() {
        let quarantine = hangar.join("quarantine").join(format!("ingest-{stamp}"));
        if let Some(parent) = quarantine.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if stage.exists() {
            if fs::rename(&stage, &quarantine).is_err() {
                let _ = fs::remove_dir_all(&stage);
            }
        }
    } else if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

fn valid_output_name(name: &str) -> bool {
    if name.bytes().any(|byte| matches!(byte, b'/' | b'\\')) {
        return false;
    }
    let path = Path::new(name);
    let mut components = path.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return false;
    };
    component == path.as_os_str() && components.next().is_none()
}

fn reject_semantic_xattrs_tree(root: &Path) -> Result<(), IngestError> {
    fn walk(path: &Path) -> Result<(), IngestError> {
        let meta = fs::symlink_metadata(path).map_err(|e| IngestError::Io(e.to_string()))?;
        super::super::Envelope::check_xattrs(path, false).map_err(IngestError::Invalid)?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            for ent in fs::read_dir(path).map_err(|e| IngestError::Io(e.to_string()))? {
                let ent = ent.map_err(|e| IngestError::Io(e.to_string()))?;
                walk(&ent.path())?;
            }
        }
        Ok(())
    }
    walk(root)
}

/// Preserve non-security xattrs onto the staged tree when an explicit platform
/// artifact kind opted in. Security/quarantine names stay excluded.
fn copy_semantic_xattrs_tree(src_root: &Path, dst_root: &Path) -> Result<(), IngestError> {
    fn walk(src: &Path, dst: &Path) -> Result<(), IngestError> {
        let meta = fs::symlink_metadata(src).map_err(|e| IngestError::Io(e.to_string()))?;
        copy_semantic_xattrs_node(src, dst)?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            for ent in fs::read_dir(src).map_err(|e| IngestError::Io(e.to_string()))? {
                let ent = ent.map_err(|e| IngestError::Io(e.to_string()))?;
                let name = ent.file_name();
                walk(&ent.path(), &dst.join(&name))?;
            }
        }
        Ok(())
    }
    walk(src_root, dst_root)
}

fn copy_semantic_xattrs_node(src: &Path, dst: &Path) -> Result<(), IngestError> {
    let names = semantic_xattr_names(src)?;
    for name in names {
        if super::super::Envelope::is_excluded_xattr(&name) {
            continue;
        }
        let value = get_xattr_value(src, &name)?;
        set_xattr_value(dst, &name, &value)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn semantic_xattr_names(path: &Path) -> Result<Vec<String>, IngestError> {
    super::super::Envelope::list_xattr_names(path).map_err(IngestError::Invalid)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn semantic_xattr_names(path: &Path) -> Result<Vec<String>, IngestError> {
    use std::os::unix::ffi::OsStrExt as _;
    unsafe extern "C" {
        fn listxattr(path: *const i8, list: *mut i8, size: usize, options: i32) -> isize;
    }
    const XATTR_NOFOLLOW: i32 = 0x0001;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| IngestError::Invalid("xattr path contains NUL".into()))?;
    let size = unsafe { listxattr(path.as_ptr(), std::ptr::null_mut(), 0, XATTR_NOFOLLOW) };
    if size < 0 {
        return Err(IngestError::Io(format!(
            "listxattr failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    let mut names = vec![0i8; size as usize];
    let wrote = unsafe { listxattr(path.as_ptr(), names.as_mut_ptr(), names.len(), XATTR_NOFOLLOW) };
    if wrote < 0 {
        return Err(IngestError::Io(format!(
            "listxattr failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    let bytes = unsafe { std::slice::from_raw_parts(names.as_ptr().cast::<u8>(), wrote as usize) };
    Ok(bytes
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
fn semantic_xattr_names(_path: &Path) -> Result<Vec<String>, IngestError> {
    Err(IngestError::Invalid(
        "semantic xattr ingest is unsupported on this platform".into(),
    ))
}

#[cfg(target_os = "linux")]
fn get_xattr_value(path: &Path, name: &str) -> Result<Vec<u8>, IngestError> {
    use std::os::unix::ffi::OsStrExt as _;
    type LibcChar = i8;
    #[link(name = "c")]
    extern "C" {
        fn lgetxattr(
            path: *const LibcChar,
            name: *const LibcChar,
            value: *mut u8,
            size: usize,
        ) -> isize;
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| IngestError::Invalid(format!("path `{}` contains NUL", path.display())))?;
    let c_name = std::ffi::CString::new(name)
        .map_err(|_| IngestError::Invalid(format!("xattr name `{name}` contains NUL")))?;
    let size = unsafe { lgetxattr(c_path.as_ptr(), c_name.as_ptr(), std::ptr::null_mut(), 0) };
    if size < 0 {
        return Err(IngestError::Io(format!(
            "lgetxattr `{}` on `{}`: {}",
            name,
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    let mut buf = vec![0u8; size as usize];
    let wrote = unsafe { lgetxattr(c_path.as_ptr(), c_name.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    if wrote < 0 {
        return Err(IngestError::Io(format!(
            "lgetxattr `{}` on `{}`: {}",
            name,
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    buf.truncate(wrote as usize);
    Ok(buf)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn get_xattr_value(path: &Path, name: &str) -> Result<Vec<u8>, IngestError> {
    use std::os::unix::ffi::OsStrExt as _;
    unsafe extern "C" {
        fn getxattr(
            path: *const i8,
            name: *const i8,
            value: *mut u8,
            size: usize,
            position: u32,
            options: i32,
        ) -> isize;
    }
    const XATTR_NOFOLLOW: i32 = 0x0001;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| IngestError::Invalid(format!("path `{}` contains NUL", path.display())))?;
    let c_name = std::ffi::CString::new(name)
        .map_err(|_| IngestError::Invalid(format!("xattr name `{name}` contains NUL")))?;
    let size = unsafe {
        getxattr(c_path.as_ptr(), c_name.as_ptr(), std::ptr::null_mut(), 0, 0, XATTR_NOFOLLOW)
    };
    if size < 0 {
        return Err(IngestError::Io(format!(
            "getxattr `{name}` on `{}`: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    let mut value = vec![0; size as usize];
    let wrote = unsafe {
        getxattr(c_path.as_ptr(), c_name.as_ptr(), value.as_mut_ptr(), value.len(), 0, XATTR_NOFOLLOW)
    };
    if wrote < 0 {
        return Err(IngestError::Io(format!(
            "getxattr `{name}` on `{}`: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    value.truncate(wrote as usize);
    Ok(value)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
fn get_xattr_value(_path: &Path, _name: &str) -> Result<Vec<u8>, IngestError> {
    Err(IngestError::Invalid(
        "semantic xattr ingest is unsupported on this platform".into(),
    ))
}

#[cfg(target_os = "linux")]
fn set_xattr_value(path: &Path, name: &str, value: &[u8]) -> Result<(), IngestError> {
    use std::os::unix::ffi::OsStrExt as _;
    type LibcChar = i8;
    #[link(name = "c")]
    extern "C" {
        fn lsetxattr(
            path: *const LibcChar,
            name: *const LibcChar,
            value: *const u8,
            size: usize,
            flags: i32,
        ) -> i32;
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| IngestError::Invalid(format!("path `{}` contains NUL", path.display())))?;
    let c_name = std::ffi::CString::new(name)
        .map_err(|_| IngestError::Invalid(format!("xattr name `{name}` contains NUL")))?;
    let rc = unsafe {
        lsetxattr(
            c_path.as_ptr(),
            c_name.as_ptr(),
            value.as_ptr(),
            value.len(),
            0,
        )
    };
    if rc != 0 {
        return Err(IngestError::Io(format!(
            "lsetxattr `{}` on `{}`: {}",
            name,
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn set_xattr_value(path: &Path, name: &str, value: &[u8]) -> Result<(), IngestError> {
    use std::os::unix::ffi::OsStrExt as _;
    unsafe extern "C" {
        fn setxattr(
            path: *const i8,
            name: *const i8,
            value: *const u8,
            size: usize,
            position: u32,
            options: i32,
        ) -> i32;
    }
    const XATTR_NOFOLLOW: i32 = 0x0001;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| IngestError::Invalid(format!("path `{}` contains NUL", path.display())))?;
    let c_name = std::ffi::CString::new(name)
        .map_err(|_| IngestError::Invalid(format!("xattr name `{name}` contains NUL")))?;
    if unsafe {
        setxattr(c_path.as_ptr(), c_name.as_ptr(), value.as_ptr(), value.len(), 0, XATTR_NOFOLLOW)
    } != 0
    {
        return Err(IngestError::Io(format!(
            "setxattr `{name}` on `{}`: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
fn set_xattr_value(_path: &Path, _name: &str, _value: &[u8]) -> Result<(), IngestError> {
    Err(IngestError::Invalid(
        "semantic xattr ingest is unsupported on this platform".into(),
    ))
}

fn copy_nofollow_tree(src: &Path, dst: &Path) -> Result<(), IngestError> {
    let meta = fs::symlink_metadata(src).map_err(|e| IngestError::Io(e.to_string()))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if meta.file_attributes() & windows_ingest_contract().reparse_attribute != 0 {
            return Err(IngestError::Invalid(format!(
                "reparse-point node `{}` is unsupported",
                src.display()
            )));
        }
    }
    let before = stable_meta_identity(&meta)?;
    if meta.file_type().is_dir() {
        fs::create_dir_all(dst)?;
        let mut names = Vec::new();
        for ent in fs::read_dir(src).map_err(|e| IngestError::Io(e.to_string()))? {
            let ent = ent.map_err(|e| IngestError::Io(e.to_string()))?;
            let name = ent.file_name();
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt as _;
                let bytes = name.as_os_str().as_bytes().to_vec();
                if let Err(err) = super::super::Envelope::validate_path_component(&bytes) {
                    return Err(IngestError::PathLaw(err));
                }
                names.push(bytes);
            }
            #[cfg(not(unix))]
            {
                let bytes = name.to_string_lossy().as_bytes().to_vec();
                if let Err(err) = super::super::Envelope::validate_path_component(&bytes) {
                    return Err(IngestError::PathLaw(err));
                }
                names.push(bytes);
            }
            copy_nofollow_tree(&ent.path(), &dst.join(&name))?;
        }
        if let Err(err) = super::super::Envelope::reject_casefold_collisions(&names) {
            return Err(IngestError::PathLaw(err));
        }
    } else if meta.file_type().is_symlink() {
        let target = fs::read_link(src).map_err(|e| IngestError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, dst).map_err(|e| IngestError::Io(e.to_string()))?;
        }
        #[cfg(not(unix))]
        {
            return Err(IngestError::Invalid(
                "symlink ingest needs platform support".into(),
            ));
        }
    } else if meta.file_type().is_file() {
        copy_nofollow_file(src, dst, &meta)?;
    } else {
        return Err(IngestError::Invalid(format!(
            "unsupported special file: `{}`",
            src.display()
        )));
    }
    let after = fs::symlink_metadata(src).map_err(|e| IngestError::Io(e.to_string()))?;
    if before != stable_meta_identity(&after)? {
        return Err(IngestError::Mutated(format!(
            "`{}` changed during ingest",
            src.display()
        )));
    }
    Ok(())
}

#[allow(unreachable_code)]
fn copy_nofollow_file(
    src: &Path,
    dst: &Path,
    expected: &fs::Metadata,
) -> Result<(), IngestError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(
            super::super::Envelope::nofollow_open_flag().map_err(IngestError::Invalid)?,
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
        let contract = windows_ingest_contract();
        options.custom_flags(contract.open_reparse_point);
        let file = options
            .open(src)
            .map_err(|e| IngestError::Io(format!("reparse-safe open `{}`: {e}", src.display())))?;
        if file
            .metadata()
            .map_err(|e| IngestError::Io(e.to_string()))?
            .file_attributes()
            & contract.reparse_attribute
            != 0
        {
            return Err(IngestError::Invalid(format!(
                "reparse-point file `{}` is unsupported",
                src.display()
            )));
        }
        return copy_open_file(file, src, dst, expected);
    }
    let mut file = options
        .open(src)
        .map_err(|e| IngestError::Io(format!("nofollow open `{}`: {e}", src.display())))?;
    let opened = file
        .metadata()
        .map_err(|e| IngestError::Io(e.to_string()))?;
    if stable_meta_identity(&opened)? != stable_meta_identity(expected)? {
        return Err(IngestError::Mutated(format!(
            "`{}` changed before copy",
            src.display()
        )));
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)
        .map_err(|e| IngestError::Io(e.to_string()))?;
    let after = file
        .metadata()
        .map_err(|e| IngestError::Io(e.to_string()))?;
    if stable_meta_identity(&opened)? != stable_meta_identity(&after)? {
        return Err(IngestError::Mutated(format!(
            "`{}` changed while copying",
            src.display()
        )));
    }
    fs::write(dst, &bytes).map_err(|e| IngestError::Io(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt as _, MetadataExt as _};
        let mode = expected.mode() & 0o7777;
        fs::set_permissions(dst, fs::Permissions::from_mode(mode))
            .map_err(|e| IngestError::Io(e.to_string()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn copy_open_file(
    mut file: fs::File,
    src: &Path,
    dst: &Path,
    expected: &fs::Metadata,
) -> Result<(), IngestError> {
    let opened = file.metadata().map_err(|e| IngestError::Io(e.to_string()))?;
    if stable_meta_identity(&opened)? != stable_meta_identity(expected)? {
        return Err(IngestError::Mutated(format!("`{}` changed before copy", src.display())));
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|e| IngestError::Io(e.to_string()))?;
    let after = file.metadata().map_err(|e| IngestError::Io(e.to_string()))?;
    if stable_meta_identity(&opened)? != stable_meta_identity(&after)? {
        return Err(IngestError::Mutated(format!("`{}` changed while copying", src.display())));
    }
    fs::write(dst, bytes).map_err(|e| IngestError::Io(e.to_string()))
}

fn stable_meta_identity(meta: &fs::Metadata) -> Result<StableMetaIdentity, IngestError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        Ok((meta.dev(), meta.ino(), meta.len(), meta.mode()))
    }
    #[cfg(not(unix))]
    {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            return windows_stable_identity(
                meta.volume_serial_number(),
                meta.file_index(),
                meta.len(),
                meta.file_attributes(),
            );
        }
        #[cfg(not(windows))]
        Ok((0, 0, meta.len(), u32::from(meta.permissions().readonly())))
    }
}

#[cfg(test)]
mod portability_tests {
    #[test]
    fn windows_contract_opens_reparse_points_and_tracks_stable_identity() {
        let contract = super::windows_ingest_contract();
        assert_eq!(contract.open_reparse_point, 0x0020_0000);
        assert_eq!(contract.reparse_attribute, 0x0000_0400);
        assert!(contract.stable_volume_and_file_id);
        assert_eq!(
            super::windows_stable_identity(Some(7), Some(11), 13, 17).unwrap(),
            (7, 11, 13, 17)
        );
        assert!(super::windows_stable_identity(None, Some(11), 13, 17).is_err());
        assert!(super::windows_stable_identity(Some(7), None, 13, 17).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn nofollow_flag_matches_supported_target_abi() {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        assert_eq!(super::super::super::Envelope::nofollow_open_flag().unwrap(), 0o400000);
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        assert_eq!(super::super::super::Envelope::nofollow_open_flag().unwrap(), 0x0100);
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
        )))]
        assert!(super::super::super::Envelope::nofollow_open_flag().is_err());
    }
}
