#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionKey(String);

impl ActionKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        ContentDigest(format!("sha256:{}", SHA256::sha256_hex(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "digest must start with `sha256:`"));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "sha256 digest must contain exactly 64 hexadecimal digits"));
        }
        Ok(ContentDigest(format!("sha256:{}", hex.to_ascii_lowercase())))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Succeeded { exit_code: i32 },
    Failed { exit_code: i32 },
    RestoredFromCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHitReason {
    LocalActionRecordMatched,
    DeclaredOutputsRestored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMissReason {
    NoLocalActionRecord,
    ActionKeyChanged,
    DeclaredOutputMissing,
    RemoteDenied,
    UncachedAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCacheStatus {
    Hit(CacheHitReason),
    Miss(CacheMissReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCacheProvenance {
    pub status: ActionCacheStatus,
    pub remote_policy: RemoteCachePolicy,
}

impl ActionCacheProvenance {
    pub fn hit(reason: CacheHitReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Hit(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }

    pub fn miss(reason: CacheMissReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Miss(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteActionRequest {
    CacheRead,
    CacheWrite,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDeniedReason {
    MissingGrantAndSandboxProof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCacheDenied {
    pub request: RemoteActionRequest,
    pub reason: RemoteDeniedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCachePolicy {
    DisabledUntilGrantAndSandboxProof,
}

impl RemoteCachePolicy {
    pub fn disabled_until_grant_and_sandbox_proof() -> Self {
        RemoteCachePolicy::DisabledUntilGrantAndSandboxProof
    }

    pub fn check(self, request: RemoteActionRequest) -> Result<(), RemoteCacheDenied> {
        match self {
            RemoteCachePolicy::DisabledUntilGrantAndSandboxProof => Err(RemoteCacheDenied {
                request,
                reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutputRecord {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInputSnapshot {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResultRecord {
    pub key: ActionKey,
    pub outcome: ActionOutcome,
    pub outputs: Vec<ActionOutputRecord>,
    pub provenance: ActionCacheProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCas {
    root: PathBuf,
}

impl LocalCas {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalCas { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_blob(&self, bytes: &[u8]) -> io::Result<ContentDigest> {
        let digest = ContentDigest::from_bytes(bytes);
        let path = self.blob_path(&digest)?;
        if let Ok(metadata) = fs::symlink_metadata(&self.root) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "CAS root is not a real directory"));
            }
        } else {
            if let Some(parent) = self.root.parent() {
                if fs::symlink_metadata(parent).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "CAS parent is a symlink"));
                }
            }
            fs::create_dir_all(&self.root)?;
        }
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "CAS blob is not a real file"));
            }
            let existing = fs::read(&path)?;
            if ContentDigest::from_bytes(&existing) != digest {
                atomic_restore_file(&self.root, &path, bytes, bytes.len() as u64)?;
            }
        } else {
            atomic_restore_file(&self.root, &path, bytes, bytes.len() as u64)?;
        }
        Ok(digest)
    }

    pub fn read_blob(&self, digest: &ContentDigest) -> io::Result<Vec<u8>> {
        let bytes = fs::read(self.blob_path(digest)?)?;
        let actual = ContentDigest::from_bytes(&bytes);
        if &actual != digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("CAS blob digest mismatch: expected {}", digest.as_str()),
            ));
        }
        Ok(bytes)
    }

    pub fn snapshot_declared_inputs(
        &self,
        base: &Path,
        action: &BuildAction,
    ) -> io::Result<Vec<ActionInputSnapshot>> {
        let mut inputs = Vec::new();
        for input in &action.inputs {
            let path = resolve_under(base, input.as_str())?;
            let bytes = fs::read(path)?;
            let digest = self.put_blob(&bytes)?;
            inputs.push(ActionInputSnapshot {
                path: input.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(inputs)
    }

    pub fn capture_declared_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        key: ActionKey,
        outcome: ActionOutcome,
        provenance: ActionCacheProvenance,
    ) -> io::Result<ActionResultRecord> {
        let mut outputs = Vec::new();
        for output in &action.outputs {
            let path = resolve_under(base, output.as_str())?;
            let bytes = fs::read(path)?;
            let digest = self.put_blob(&bytes)?;
            outputs.push(ActionOutputRecord {
                path: output.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(ActionResultRecord {
            key,
            outcome,
            outputs,
            provenance,
        })
    }

    pub fn restore_declared_outputs(
        &self,
        base: &Path,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        self.restore_outputs(base, record)
    }

    pub fn restore_action_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        if record.outputs.len() != action.outputs.len()
            || record.outputs.iter().zip(&action.outputs).any(|(recorded, declared)| recorded.path != *declared)
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "cached output record does not exactly match action declarations"));
        }
        self.restore_outputs(base, record)
    }

    fn restore_outputs(&self, base: &Path, record: &ActionResultRecord) -> io::Result<()> {
        for output in &record.outputs {
            let path = resolve_under(base, output.path.as_str())?;
            let bytes = self.read_blob(&output.digest)?;
            atomic_restore_file(base, &path, &bytes, output.byte_len)?;
        }
        Ok(())
    }

    fn blob_path(&self, digest: &ContentDigest) -> io::Result<PathBuf> {
        let digest = ContentDigest::parse(digest.as_str())?;
        let hex = digest.0.strip_prefix("sha256:").expect("validated prefix");
        let (prefix, rest) = hex.split_at(2);
        Ok(self.root
            .join("blobs")
            .join("sha256")
            .join(prefix)
            .join(rest))
    }
}

#[cfg(all(test, unix))]
mod hostile_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("jet-cas-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn put_blob_rejects_cache_root_and_blob_tree_symlinks() {
        let outside = temp("outside");
        let root_parent = temp("root-link");
        symlink(&outside, root_parent.join("cache")).unwrap();
        assert!(LocalCas::new(root_parent.join("cache")).put_blob(b"secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

        let root = temp("blob-link");
        let cas = root.join("cache");
        fs::create_dir_all(&cas).unwrap();
        symlink(&outside, cas.join("blobs")).unwrap();
        assert!(LocalCas::new(&cas).put_blob(b"secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }
}

#[cfg(unix)]
fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8], nonce: u64) -> io::Result<()> {
    use std::ffi::CString;
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn mkdirat(dirfd: i32, pathname: *const i8, mode: u32) -> i32;
        fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32;
    }
    fn name(value: &std::ffi::OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in build output path"))
    }

    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "build output escapes root"))?;
    let file_name = relative.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "build output has no file name"))?;
    let root = fs::OpenOptions::new().read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC).open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let mut fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
                if unsafe { mkdirat(dirfd, part.as_ptr(), 0o755) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            }
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let final_name = name(file_name)?;
    let temp_name = CString::new(format!(".jet-restore-{}-{nonce}", std::process::id())).unwrap();
    let fd = unsafe { openat(dirfd, temp_name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    let mut temp = unsafe { fs::File::from_raw_fd(fd) };
    temp.write_all(bytes)?;
    temp.sync_all()?;
    drop(temp);
    if unsafe { unlinkat(dirfd, final_name.as_ptr(), 0) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::NotFound {
            let _ = unsafe { unlinkat(dirfd, temp_name.as_ptr(), 0) };
            return Err(error);
        }
    }
    if unsafe { renameat(dirfd, temp_name.as_ptr(), dirfd, final_name.as_ptr()) } != 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { unlinkat(dirfd, temp_name.as_ptr(), 0) };
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8], nonce: u64) -> io::Result<()> {
    prepare_output_destination(base, path)?;
    let temp = path.with_extension(format!("jet-cache-restore-{}-{nonce}.tmp", std::process::id()));
    fs::write(&temp, bytes)?;
    if fs::symlink_metadata(path).is_ok() { fs::remove_file(path)?; }
    fs::rename(temp, path)
}
