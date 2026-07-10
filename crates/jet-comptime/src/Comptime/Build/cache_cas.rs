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
        match secure_read_file(&self.root, &path) {
            Ok(existing) => if ContentDigest::from_bytes(&existing) != digest {
                atomic_restore_file(&self.root, &path, bytes)?;
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                atomic_restore_file(&self.root, &path, bytes)?;
            }
            Err(error) => return Err(error),
        }
        Ok(digest)
    }

    pub fn read_blob(&self, digest: &ContentDigest) -> io::Result<Vec<u8>> {
        let bytes = secure_read_file(&self.root, &self.blob_path(digest)?)?;
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
            atomic_restore_file(base, &path, &bytes)?;
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

#[cfg(unix)]
pub(super) fn secure_read_file(base: &Path, path: &Path) -> io::Result<Vec<u8>> {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    let name = |value: &std::ffi::OsStr| CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in cache path"));
    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root"))?;
    let file_name = relative.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cache path has no file name"))?;
    let root = fs::OpenOptions::new().read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC).open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let file_name = name(file_name)?;
    let fd = unsafe { openat(dirfd, file_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cache entry is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn secure_read_file(base: &Path, path: &Path) -> io::Result<Vec<u8>> {
    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root"))?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else { continue };
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cache path contains a symlink"));
        }
    }
    fs::read(current)
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

    #[test]
    fn known_digest_blob_and_action_record_symlinks_are_never_read() {
        let root = temp("read-link");
        let cas = LocalCas::new(root.join("cas"));
        let digest = cas.put_blob(b"host-bytes").unwrap();
        let blob = cas.blob_path(&digest).unwrap();
        fs::remove_file(&blob).unwrap();
        let host = root.join("host");
        fs::write(&host, b"host-bytes").unwrap();
        symlink(&host, &blob).unwrap();
        assert!(cas.read_blob(&digest).is_err());

        let records = root.join("records");
        fs::create_dir_all(&records).unwrap();
        let key = ActionKey("act-sha256:known".to_string());
        let host_record = root.join("host-record");
        fs::write(&host_record, format!("{}\n", key.as_str())).unwrap();
        let record = records.join("known");
        symlink(host_record, &record).unwrap();
        assert!(read_action_record(&records, &record, key).is_none());
    }

    #[test]
    fn concurrent_same_size_restores_use_unique_create_new_temps() {
        let root = temp("concurrent-restore");
        let output = root.join("out");
        let payloads = (0..16).map(|index| format!("payload-{index:08}").into_bytes()).collect::<Vec<_>>();
        std::thread::scope(|scope| {
            let jobs = payloads.iter().map(|payload| {
                let root = &root;
                let output = &output;
                scope.spawn(move || atomic_restore_file(root, output, payload))
            }).collect::<Vec<_>>();
            for job in jobs { job.join().unwrap().unwrap(); }
        });
        let final_bytes = fs::read(&output).unwrap();
        assert!(payloads.contains(&final_bytes));
        assert!(!fs::read_dir(&root).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".jet-restore-")));
    }
}

#[cfg(unix)]
fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
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
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists { return Err(error); }
                }
                fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            }
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let final_name = name(file_name)?;
    let (temp_name, fd) = loop {
        let mut random = [0u8; 16];
        std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut random)?;
        let nonce = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let temp_name = CString::new(format!(".jet-restore-{nonce}")).unwrap();
        let fd = unsafe { openat(dirfd, temp_name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
        if fd >= 0 { break (temp_name, fd); }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists { return Err(error); }
    };
    let mut temp = unsafe { fs::File::from_raw_fd(fd) };
    temp.write_all(bytes)?;
    temp.sync_all()?;
    drop(temp);
    if unsafe { renameat(dirfd, temp_name.as_ptr(), dirfd, final_name.as_ptr()) } != 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { unlinkat(dirfd, temp_name.as_ptr(), 0) };
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    prepare_output_destination(base, path)?;
    let mut random = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut random)?;
    let nonce = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let temp = path.with_extension(format!("jet-cache-restore-{nonce}.tmp"));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    std::io::Write::write_all(&mut file, bytes)?;
    if fs::symlink_metadata(path).is_ok() { fs::remove_file(path)?; }
    fs::rename(temp, path)
}
