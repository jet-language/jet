use super::*;

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
                fsync_dir(&objects)?;
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
            fsync_dir(&objects)?;
            fs::rename(&partial, &destination)?;
            fsync_dir(&objects)?;
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

#[cfg(unix)]
fn libc_fsync(fd: i32) -> i32 {
    #[link(name = "c")]
    extern "C" {
        fn fsync(fd: i32) -> i32;
    }
    unsafe { fsync(fd) }
}

fn fsync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd as _;
        let file = fs::File::open(path)?;
        let rc = libc_fsync(file.as_raw_fd());
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    let _ = path;
    Ok(())
}

fn reject_semantic_xattrs_tree(root: &Path) -> Result<(), IngestError> {
    fn walk(path: &Path) -> Result<(), IngestError> {
        let meta = fs::symlink_metadata(path).map_err(|e| IngestError::Io(e.to_string()))?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_file() {
            super::super::Envelope::check_xattrs(path, false).map_err(IngestError::Invalid)?;
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
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_file() {
            copy_semantic_xattrs_file(src, dst)?;
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

fn copy_semantic_xattrs_file(src: &Path, dst: &Path) -> Result<(), IngestError> {
    let names = super::super::Envelope::list_xattr_names(src).map_err(IngestError::Invalid)?;
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

#[cfg(not(target_os = "linux"))]
fn get_xattr_value(_path: &Path, _name: &str) -> Result<Vec<u8>, IngestError> {
    Ok(Vec::new())
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

#[cfg(not(target_os = "linux"))]
fn set_xattr_value(_path: &Path, _name: &str, _value: &[u8]) -> Result<(), IngestError> {
    Ok(())
}

fn copy_nofollow_tree(src: &Path, dst: &Path) -> Result<(), IngestError> {
    let meta = fs::symlink_metadata(src).map_err(|e| IngestError::Io(e.to_string()))?;
    let before = stable_meta_identity(&meta);
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
    if before != stable_meta_identity(&after) {
        return Err(IngestError::Mutated(format!(
            "`{}` changed during ingest",
            src.display()
        )));
    }
    Ok(())
}

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
        const O_NOFOLLOW: i32 = 0x20000;
        options.custom_flags(O_NOFOLLOW);
    }
    let mut file = options
        .open(src)
        .map_err(|e| IngestError::Io(format!("nofollow open `{}`: {e}", src.display())))?;
    let opened = file
        .metadata()
        .map_err(|e| IngestError::Io(e.to_string()))?;
    if stable_meta_identity(&opened) != stable_meta_identity(expected) {
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
    if stable_meta_identity(&opened) != stable_meta_identity(&after) {
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

fn stable_meta_identity(meta: &fs::Metadata) -> (u64, u64, u64, u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        (meta.dev(), meta.ino(), meta.len(), meta.mode())
    }
    #[cfg(not(unix))]
    {
        (
            0,
            0,
            meta.len(),
            u32::from(meta.permissions().readonly()),
        )
    }
}
