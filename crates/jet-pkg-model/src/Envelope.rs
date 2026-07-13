//! D-JPK-CACHE1=A (U24) — the A4 hangar-object envelope.
//!
//! Every realized hangar object carries a small identity block that makes it a
//! cache-substitutable artifact: the content hash of its output tree, the
//! platform it was built for, a detached-signature slot (filled by package
//! signing, card #13), and provenance (how it was produced). These fields are
//! frozen into the hangar/lock schema now — the binary-cache protocol that
//! consumes them is a later card (D-JPK-CACHE1 protocol slice). A
//! build-from-source output is an envelope-carrying object exactly like a
//! substituted one, so the resolver never has to care which path produced it.
//!
//! E4-JP1 Hangar Store v2: the canonical archive digests uncompressed logical
//! bytes (sparse holes hash as zero-filled logical content; physical allocation
//! is not identity). Path law rejects case-fold collisions, reserved names,
//! trailing-dot/space Windows aliases, and unrepresentable cross-platform
//! names — Unicode normalization is never implicit. Security/quarantine xattrs
//! are excluded from identity; other semantic xattrs require an explicit
//! platform artifact kind or the ingest fails closed.

use crate::SHA256;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Path-law failure (E1299). Stable codes so CLI + tests can match without
/// scraping free-form text alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathLawError {
    pub code: &'static str,
    pub path: String,
    pub detail: String,
}

impl PathLawError {
    pub fn what(&self) -> String {
        format!("store path rejected: `{}`", self.path)
    }

    pub fn why(&self) -> String {
        format!(
            "Hangar Store v2 path law ({}) forbids this name: {}.",
            self.code, self.detail
        )
    }

    pub fn fix(&self) -> &'static str {
        "Rename the entry to a portable store path: no reserved Windows names, no trailing `.`/` `, no case-fold collisions with a sibling, and no `.`/`..` components."
    }
}

/// Reject a single path component under Hangar path law (POSIX bytes; Windows
/// reserved/trailing rules applied so the same tree is representable both ways).
pub fn validate_path_component(name: &[u8]) -> Result<(), PathLawError> {
    if name.is_empty() {
        return Err(PathLawError {
            code: "empty",
            path: String::new(),
            detail: "empty path component".into(),
        });
    }
    if name.contains(&0) {
        return Err(PathLawError {
            code: "nul",
            path: lossy(name),
            detail: "NUL byte in path component".into(),
        });
    }
    if name == b"." || name == b".." {
        return Err(PathLawError {
            code: "dot",
            path: lossy(name),
            detail: "`.` and `..` are not store path components".into(),
        });
    }
    if name.ends_with(b".") || name.ends_with(b" ") {
        return Err(PathLawError {
            code: "trailing-alias",
            path: lossy(name),
            detail: "trailing `.` or space is a Windows alias and is rejected".into(),
        });
    }
    if is_windows_reserved(name) {
        return Err(PathLawError {
            code: "reserved",
            path: lossy(name),
            detail: "Windows reserved device name".into(),
        });
    }
    // No implicit Unicode normalization: NFC and NFD that differ in bytes are
    // distinct names. Cross-platform unrepresentable: reject unpaired surrogates
    // when the name claims to be UTF-8 text with escapes — raw non-UTF8 bytes
    // are allowed on POSIX and recorded as opaque bytes.
    Ok(())
}

/// Validate every component of a relative store path (no absolute, no empty).
pub fn validate_rel_path(rel: &Path) -> Result<(), PathLawError> {
    if rel.is_absolute() {
        return Err(PathLawError {
            code: "absolute",
            path: rel.display().to_string(),
            detail: "store paths must be relative".into(),
        });
    }
    let mut saw = false;
    for comp in rel.components() {
        use std::path::Component;
        match comp {
            Component::Normal(os) => {
                saw = true;
                validate_path_component(&path_component_bytes(os))?;
            }
            Component::CurDir | Component::ParentDir => {
                return Err(PathLawError {
                    code: "dot",
                    path: rel.display().to_string(),
                    detail: "`.` and `..` are not store path components".into(),
                });
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(PathLawError {
                    code: "absolute",
                    path: rel.display().to_string(),
                    detail: "store paths must be relative".into(),
                });
            }
        }
    }
    if !saw && rel.as_os_str().is_empty() {
        return Ok(()); // output root itself
    }
    Ok(())
}

/// Reject case-fold collisions among sibling directory entry names.
pub fn reject_casefold_collisions(names: &[Vec<u8>]) -> Result<(), PathLawError> {
    let mut folded: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for name in names {
        let key: Vec<u8> = name.iter().map(ascii_fold).collect();
        if let Some(prior) = folded.get(&key) {
            if prior != name {
                return Err(PathLawError {
                    code: "case-fold",
                    path: lossy(name),
                    detail: format!(
                        "collides with `{}` under ASCII case-folding",
                        lossy(prior)
                    ),
                });
            }
        } else {
            folded.insert(key, name.clone());
        }
    }
    Ok(())
}

fn ascii_fold(b: &u8) -> u8 {
    b.to_ascii_lowercase()
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn is_windows_reserved(name: &[u8]) -> bool {
    let stem = match name.iter().position(|&b| b == b'.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    let upper: Vec<u8> = stem.iter().map(|b| b.to_ascii_uppercase()).collect();
    matches!(
        upper.as_slice(),
        b"CON"
            | b"PRN"
            | b"AUX"
            | b"NUL"
            | b"COM1"
            | b"COM2"
            | b"COM3"
            | b"COM4"
            | b"COM5"
            | b"COM6"
            | b"COM7"
            | b"COM8"
            | b"COM9"
            | b"LPT1"
            | b"LPT2"
            | b"LPT3"
            | b"LPT4"
            | b"LPT5"
            | b"LPT6"
            | b"LPT7"
            | b"LPT8"
            | b"LPT9"
    )
}

fn path_component_bytes(os: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        os.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        // Windows: keep WTF-16 → lossy UTF-8 only for validation messaging;
        // encode_node still records OsStr via path_bytes.
        os.to_string_lossy().as_bytes().to_vec()
    }
}

/// Security / quarantine xattr prefixes excluded from hangar identity.
const EXCLUDED_XATTR_PREFIXES: &[&str] = &[
    "security.",
    "trusted.",
    "system.nfs4_acl",
    "com.apple.quarantine",
    "com.apple.macl",
    "com.apple.provenance",
];

/// True when `name` is a security/quarantine xattr Hangar strips from identity.
pub fn is_excluded_xattr(name: &str) -> bool {
    EXCLUDED_XATTR_PREFIXES
        .iter()
        .any(|prefix| name == *prefix || name.starts_with(prefix))
}

/// List xattr names on `path` (symlink-nofollow). Empty on platforms without
/// xattrs or when none are present.
pub fn list_xattr_names(path: &Path) -> Result<Vec<String>, String> {
    #[cfg(target_os = "linux")]
    {
        list_xattr_names_linux(path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(Vec::new())
    }
}

/// Reject unsupported semantic xattrs unless `allow_semantic_xattrs` (explicit
/// platform artifact kind). Excluded security/quarantine names are ignored.
pub fn check_xattrs(path: &Path, allow_semantic_xattrs: bool) -> Result<(), String> {
    let names = list_xattr_names(path)?;
    let semantic: Vec<_> = names
        .into_iter()
        .filter(|n| !is_excluded_xattr(n))
        .collect();
    if semantic.is_empty() || allow_semantic_xattrs {
        return Ok(());
    }
    Err(format!(
        "unsupported semantic xattr(s) on `{}`: {} — set an explicit platform artifact kind to keep them",
        path.display(),
        semantic.join(", ")
    ))
}

#[cfg(target_os = "linux")]
fn list_xattr_names_linux(path: &Path) -> Result<Vec<String>, String> {
    use std::os::unix::ffi::OsStrExt as _;
    type LibcChar = i8;
    #[link(name = "c")]
    extern "C" {
        fn llistxattr(path: *const LibcChar, list: *mut LibcChar, size: usize) -> isize;
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("path `{}` contains NUL", path.display()))?;
    let size = unsafe { llistxattr(c_path.as_ptr(), std::ptr::null_mut(), 0) };
    if size < 0 {
        let err = std::io::Error::last_os_error();
        // ENODATA / ENOTSUP / EOPNOTSUPP → no xattrs
        if matches!(err.raw_os_error(), Some(61) | Some(95) | Some(524)) {
            return Ok(Vec::new());
        }
        // Some filesystems return 0 size with errno set differently; treat as empty.
        return Ok(Vec::new());
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0i8; size as usize];
    let wrote = unsafe { llistxattr(c_path.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    if wrote < 0 {
        return Ok(Vec::new());
    }
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, wrote as usize) };
    Ok(bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect())
}

/// Sparse-file policy (JP1): holes are hashed as zero-filled logical bytes.
/// Physical sparseness is never part of identity — a sparse file and a dense
/// file with the same logical zeros share a digest.
pub fn file_has_sparse_holes(path: &Path) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        file_has_sparse_holes_linux(path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
fn file_has_sparse_holes_linux(path: &Path) -> Result<bool, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        const O_NOFOLLOW: i32 = 0x20000;
        options.custom_flags(O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("cannot open `{}` for sparse probe: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("cannot stat `{}`: {e}", path.display()))?
        .len();
    if len == 0 {
        return Ok(false);
    }
    // SEEK_HOLE = 4 on Linux.
    const SEEK_HOLE: i32 = 4;
    use std::os::unix::io::AsRawFd as _;
    #[link(name = "c")]
    extern "C" {
        fn lseek(fd: i32, offset: i64, whence: i32) -> i64;
    }
    let hole = unsafe { lseek(file.as_raw_fd(), 0, SEEK_HOLE) };
    if hole < 0 {
        return Ok(false);
    }
    Ok((hole as u64) < len)
}

/// The A4 envelope. `Default` is the empty envelope (older records / providers
/// that predate the field), so reading a legacy `meta.json` never fails.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Envelope {
    /// Content hash of the realized output tree (`sha256-…`) — the cache key.
    pub output_hash: String,
    /// Target platform key (`<arch>-<os>`, e.g. `x86_64-linux`).
    pub platform: String,
    /// Detached-signature slot; empty until package signing (card #13) fills it.
    pub signature: String,
    /// How this output was produced: the resolved source ref + build recipe id.
    pub provenance: String,
}

impl Envelope {
    /// True when no envelope field is populated (a legacy or unrealized record).
    pub fn is_empty(&self) -> bool {
        self.output_hash.is_empty()
            && self.platform.is_empty()
            && self.signature.is_empty()
            && self.provenance.is_empty()
    }

    /// Build the envelope for a freshly realized output rooted at `out`.
    /// `reference` is the resolved source ref; `recipe_id` names the build path
    /// that produced it (`core-source`, `core-cargo-rlib`, `nix`, …).
    pub fn for_output(out: &str, reference: &str, recipe_id: &str) -> Envelope {
        Envelope {
            output_hash: output_hash_of(out),
            platform: host_platform(),
            signature: String::new(),
            provenance: format!("{reference} via {recipe_id}"),
        }
    }
}

/// The platform key for the host build target (`<arch>-<os>`). std-only (I6):
/// derived from the compile target, which is what the realized artifact runs on.
pub fn host_platform() -> String {
    super::Platform::host_key()
}

/// Content hash of a realized output root.
///
/// For a real local directory this is the full-tree content hash (every file's
/// relative path, length, and bytes) — not the compiler's `.jet`-only
/// `tree_hash`, since a realized output is `bin/`, `.rlib`, and arbitrary files.
/// Existing files and symlinks are hashed from their bytes/target. Missing
/// paths retain the legacy text identity only so old fixture records remain
/// readable; cache verification rejects a missing output before trusting it.
pub fn output_hash_of(out: &str) -> String {
    try_output_hash_of(out).unwrap_or_default()
}

/// Canonical archive digest. The byte stream records every node's relative
/// path, type, mode, and type-specific payload. Symlinks are never followed
/// while encoding and must resolve inside the root. Hardlink identity is
/// explicit; unsupported special files fail closed.
pub fn try_output_hash_of(out: &str) -> Result<String, String> {
    try_output_hash_of_with_policy(out, false, &mut |_, _| {})
}

/// Like [`try_output_hash_of`], but hangar-internal hardlink peers (cas pool /
/// sibling objects under `hangar_root`) do not fail the external-hardlink law.
/// Peers outside the hangar still reject. Digest bytes stay path-local: cas
/// peers are never encoded as in-tree `H` records.
pub fn try_output_hash_of_in_hangar(
    out: &str,
    hangar_root: &Path,
    allow_semantic_xattrs: bool,
) -> Result<String, String> {
    try_output_hash_of_with_hook(out, allow_semantic_xattrs, Some(hangar_root), &mut |_, _| {})
}

/// Like [`try_output_hash_of`], with an explicit semantic-xattr policy.
pub fn try_output_hash_of_with_policy(
    out: &str,
    allow_semantic_xattrs: bool,
    hook: &mut dyn FnMut(&Path, &'static str),
) -> Result<String, String> {
    try_output_hash_of_with_hook(out, allow_semantic_xattrs, None, hook)
}

fn try_output_hash_of_with_hook(
    out: &str,
    allow_semantic_xattrs: bool,
    hangar_root: Option<&Path>,
    hook: &mut dyn FnMut(&Path, &'static str),
) -> Result<String, String> {
    let p = Path::new(out);
    if !p.exists() && !p.is_symlink() {
        return Err(format!("output `{out}` does not exist"));
    }
    if fs::symlink_metadata(p)
        .map_err(|e| format!("cannot inspect `{out}`: {e}"))?
        .file_type()
        .is_symlink()
    {
        return Err(format!("output root `{out}` must not be a symlink"));
    }
    let root = fs::canonicalize(p).map_err(|e| format!("cannot resolve `{out}`: {e}"))?;
    let mut archive = b"jet-hangar-archive-v1\0".to_vec();
    let mut hardlinks = BTreeMap::new();
    encode_node(
        &root,
        &root,
        Path::new(""),
        &mut archive,
        &mut hardlinks,
        allow_semantic_xattrs,
        hook,
    )?;
    for (key, link) in &hardlinks {
        if link.seen == link.total {
            continue;
        }
        if let Some(hangar) = hangar_root {
            let hangar_peers = count_inode_peers_under(hangar, *key);
            if hangar_peers == link.total {
                // All nlink peers live under the hangar (cas pool / other
                // objects). External-outside-hangar reject still holds.
                continue;
            }
        }
        return Err(format!(
            "hardlink `{}` has {} link(s), but only {} are inside the output",
            link.first.display(),
            link.total,
            link.seen
        ));
    }
    Ok(format!("sha256-{}", SHA256::sha256_hex(&archive)))
}

/// Count regular files under `root` that share `(dev, ino)`.
fn count_inode_peers_under(root: &Path, key: (u64, u64)) -> u64 {
    fn walk(path: &Path, key: (u64, u64), count: &mut u64) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        if meta.file_type().is_symlink() {
            return;
        }
        if meta.is_file() {
            if file_identity(&meta) == Some(key) {
                *count += 1;
            }
            return;
        }
        if meta.is_dir() {
            let Ok(rd) = fs::read_dir(path) else {
                return;
            };
            for ent in rd.flatten() {
                walk(&ent.path(), key, count);
            }
        }
    }
    let mut count = 0u64;
    walk(root, key, &mut count);
    count
}

struct HardlinkState {
    first: PathBuf,
    seen: u64,
    total: u64,
}

fn encode_node(
    root: &Path,
    path: &Path,
    rel: &Path,
    archive: &mut Vec<u8>,
    hardlinks: &mut BTreeMap<(u64, u64), HardlinkState>,
    allow_semantic_xattrs: bool,
    hook: &mut dyn FnMut(&Path, &'static str),
) -> Result<(), String> {
    if let Err(err) = validate_rel_path(rel) {
        return Err(format!("{} — {}", err.what(), err.detail));
    }
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("cannot inspect `{}`: {e}", path.display()))?;
    let kind = meta.file_type();
    let rel_bytes = path_bytes(rel);
    // Sparse holes hash as logical zeros via ordinary reads; probe only so
    // ingest can log policy without changing the digest stream.
    if kind.is_file() {
        let _ = file_has_sparse_holes(path)?;
        check_xattrs(path, allow_semantic_xattrs)?;
    }
    if kind.is_dir() {
        let before = stable_file_identity(&meta);
        let before_entries = directory_snapshot(path)?;
        let sibling_names: Vec<Vec<u8>> = before_entries.iter().map(|e| e.name.clone()).collect();
        if let Err(err) = reject_casefold_collisions(&sibling_names) {
            return Err(format!("{} — {}", err.what(), err.detail));
        }
        let resolved = fs::canonicalize(path)
            .map_err(|e| format!("cannot resolve directory `{}`: {e}", path.display()))?;
        if !resolved.starts_with(root) {
            return Err(format!("directory `{}` escapes output root", path.display()));
        }
        record_header(archive, b'D', &rel_bytes, mode_of(&meta));
        hook(path, "directory-snapshotted");
        let mut entries = fs::read_dir(path)
            .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
        entries.sort_by_key(|entry| path_bytes(Path::new(&entry.file_name())));
        for entry in entries {
            let child_rel = rel.join(entry.file_name());
            encode_node(
                root,
                &entry.path(),
                &child_rel,
                archive,
                hardlinks,
                allow_semantic_xattrs,
                hook,
            )?;
        }
        let after = fs::symlink_metadata(path)
            .map_err(|e| format!("directory `{}` changed while hashing: {e}", path.display()))?;
        let after_entries = directory_snapshot(path)?;
        if before != stable_file_identity(&after) || before_entries != after_entries {
            return Err(format!("directory `{}` changed while hashing", path.display()));
        }
    } else if kind.is_symlink() {
        let target = fs::read_link(path)
            .map_err(|e| format!("cannot read symlink `{}`: {e}", path.display()))?;
        if target.is_absolute() {
            return Err(format!("symlink `{}` escapes output root", path.display()));
        }
        let resolved = fs::canonicalize(path)
            .map_err(|e| format!("symlink `{}` is dangling or cyclic: {e}", path.display()))?;
        if !resolved.starts_with(root) {
            return Err(format!("symlink `{}` escapes output root", path.display()));
        }
        record_header(archive, b'L', &rel_bytes, mode_of(&meta));
        push_bytes(archive, &path_bytes(&target));
    } else if kind.is_file() {
        let key = file_identity(&meta);
        if let Some(state) = key.and_then(|key| hardlinks.get_mut(&key)) {
            state.seen += 1;
            record_header(archive, b'H', &rel_bytes, mode_of(&meta));
            push_bytes(archive, &path_bytes(&state.first));
            return Ok(());
        }
        if let Some(key) = key {
            hardlinks.insert(
                key,
                HardlinkState {
                    first: rel.to_path_buf(),
                    seen: 1,
                    total: link_count(&meta),
                },
            );
        }
        record_header(archive, b'F', &rel_bytes, mode_of(&meta));
        let bytes = read_file_stable(path, &meta, hook)?;
        push_bytes(archive, &bytes);
    } else {
        return Err(format!(
            "unsupported special file in output: `{}`",
            path.display()
        ));
    }
    Ok(())
}

fn read_file_stable(
    path: &Path,
    expected: &fs::Metadata,
    hook: &mut dyn FnMut(&Path, &'static str),
) -> Result<Vec<u8>, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        const O_NOFOLLOW: i32 = 0x20000;
        options.custom_flags(O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("cannot open `{}` without following links: {e}", path.display()))?;
    let before = file
        .metadata()
        .map_err(|e| format!("cannot inspect open file `{}`: {e}", path.display()))?;
    if stable_file_identity(&before) != stable_file_identity(expected) {
        return Err(format!("file `{}` changed before hashing", path.display()));
    }
    hook(path, "file-opened");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let after = file
        .metadata()
        .map_err(|e| format!("cannot re-inspect `{}`: {e}", path.display()))?;
    if stable_file_identity(&before) != stable_file_identity(&after) {
        return Err(format!("file `{}` changed while hashing", path.display()));
    }
    Ok(bytes)
}

#[derive(Debug, PartialEq, Eq)]
struct DirectoryEntrySnapshot {
    name: Vec<u8>,
    kind: u8,
    identity: StableIdentity,
}

fn directory_snapshot(path: &Path) -> Result<Vec<DirectoryEntrySnapshot>, String> {
    let mut entries = fs::read_dir(path)
        .map_err(|e| format!("cannot snapshot directory `{}`: {e}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot snapshot directory `{}`: {e}", path.display()))?
        .into_iter()
        .map(|entry| {
            let meta = fs::symlink_metadata(entry.path())
                .map_err(|e| format!("cannot inspect `{}`: {e}", entry.path().display()))?;
            let ty = meta.file_type();
            Ok(DirectoryEntrySnapshot {
                name: path_bytes(Path::new(&entry.file_name())),
                kind: if ty.is_dir() {
                    b'D'
                } else if ty.is_file() {
                    b'F'
                } else if ty.is_symlink() {
                    b'L'
                } else {
                    b'S'
                },
                identity: stable_file_identity(&meta),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn record_header(out: &mut Vec<u8>, kind: u8, path: &[u8], mode: u32) {
    out.push(kind);
    push_bytes(out, path);
    out.extend_from_slice(&mode.to_be_bytes());
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(unix)]
fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    meta.mode()
}

#[cfg(not(unix))]
fn mode_of(meta: &fs::Metadata) -> u32 {
    u32::from(meta.permissions().readonly())
}

#[cfg(unix)]
fn file_identity(meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    (meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))
}

#[cfg(unix)]
fn link_count(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    meta.nlink()
}

#[cfg(not(unix))]
fn link_count(_meta: &fs::Metadata) -> u64 {
    1
}

type StableIdentity = (u64, u64, u64, i64, i64, i64, i64, u32);

#[cfg(unix)]
fn stable_file_identity(meta: &fs::Metadata) -> StableIdentity {
    use std::os::unix::fs::MetadataExt as _;
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
        meta.mode(),
    )
}

#[cfg(not(unix))]
fn stable_file_identity(meta: &fs::Metadata) -> StableIdentity {
    (
        0,
        0,
        meta.len(),
        meta.modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_secs() as i64),
        0,
        0,
        0,
        u32::from(meta.permissions().readonly()),
    )
}

#[cfg(not(unix))]
fn file_identity(_meta: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_key_is_arch_os() {
        let p = host_platform();
        assert!(p.contains('-'), "platform key should be <arch>-<os>: {p}");
    }

    #[test]
    fn output_hash_of_dir_reflects_contents() {
        let base = std::env::temp_dir().join(format!(
            "env-hash-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("f"), "one").unwrap();
        std::fs::write(b.join("f"), "two").unwrap();
        let ha = output_hash_of(&a.to_string_lossy());
        let hb = output_hash_of(&b.to_string_lossy());
        assert!(ha.starts_with("sha256-"));
        assert_ne!(ha, hb, "different contents must hash differently");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn output_hash_of_file_reflects_bytes_not_path_text() {
        let base = std::env::temp_dir().join(format!(
            "env-file-hash-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&base, "one").unwrap();
        let before = output_hash_of(&base.to_string_lossy());
        std::fs::write(&base, "two").unwrap();
        let after = output_hash_of(&base.to_string_lossy());
        assert_ne!(before, after, "file bytes, not path text, are identity");
        std::fs::remove_file(&base).ok();
    }

    #[test]
    fn for_output_fills_all_fields_for_a_real_tree() {
        let dir = std::env::temp_dir().join(format!(
            "env-for-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x"), "hi").unwrap();
        let e = Envelope::for_output(&dir.to_string_lossy(), "mine:hello", "core-source");
        assert!(!e.is_empty());
        assert!(e.output_hash.starts_with("sha256-"));
        assert!(!e.platform.is_empty());
        assert!(e.provenance.contains("mine:hello"));
        assert!(e.provenance.contains("core-source"));
        assert!(
            e.signature.is_empty(),
            "signature slot stays empty until #13"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_archive_covers_empty_directories() {
        let dir = scratch("empty-dir");
        fs::create_dir_all(dir.join("empty")).unwrap();
        let with_empty = try_output_hash_of(&dir.to_string_lossy()).unwrap();
        fs::remove_dir(dir.join("empty")).unwrap();
        let without_empty = try_output_hash_of(&dir.to_string_lossy()).unwrap();
        assert_ne!(with_empty, without_empty);
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_archive_covers_modes_and_hardlinks() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch("mode-hardlink");
        let file = dir.join("file");
        fs::write(&file, "same bytes").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        let plain = try_output_hash_of(&dir.to_string_lossy()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        let executable = try_output_hash_of(&dir.to_string_lossy()).unwrap();
        assert_ne!(plain, executable);

        fs::hard_link(&file, dir.join("alias")).unwrap();
        let hardlinked = try_output_hash_of(&dir.to_string_lossy()).unwrap();
        fs::remove_file(dir.join("alias")).unwrap();
        fs::copy(&file, dir.join("alias")).unwrap();
        let copied = try_output_hash_of(&dir.to_string_lossy()).unwrap();
        assert_ne!(hardlinked, copied);
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_archive_rejects_hardlinks_outside_output() {
        let dir = scratch("outside-hardlink");
        let file = dir.join("payload");
        let outside = dir.parent().unwrap().join(format!(
            "outside-hardlink-{}",
            std::process::id()
        ));
        fs::write(&file, "trusted").unwrap();
        fs::hard_link(&file, &outside).unwrap();
        let error = try_output_hash_of(&dir.to_string_lossy()).unwrap_err();
        assert!(error.contains("only 1 are inside"), "{error}");
        fs::remove_file(outside).ok();
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_archive_rejects_concurrent_add_remove_and_replace() {
        let dir = scratch("concurrent-directory");
        let keep = dir.join("keep");
        fs::write(&keep, "before").unwrap();
        let mut changed = false;
        let error = try_output_hash_of_with_hook(
            &dir.to_string_lossy(),
            false,
            None,
            &mut |path, stage| {
                if !changed && path == dir && stage == "directory-snapshotted" {
                    changed = true;
                    let transient = dir.join("transient");
                    fs::write(&transient, "added").unwrap();
                    fs::remove_file(&transient).unwrap();
                    let replacement = dir.join("replacement");
                    fs::write(&replacement, "after!").unwrap();
                    fs::rename(&replacement, &keep).unwrap();
                }
            },
        )
        .unwrap_err();
        assert!(error.contains("changed while hashing"), "{error}");
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_archive_rejects_same_size_write_with_restored_mtime() {
        use std::os::unix::fs::MetadataExt as _;
        use std::process::Command;

        let dir = scratch("restored-mtime");
        let file = dir.join("payload");
        let stamp = dir.join("stamp");
        fs::write(&file, "AAAA").unwrap();
        fs::write(&stamp, "stamp").unwrap();
        Command::new("touch")
            .args(["-r", stamp.to_str().unwrap(), file.to_str().unwrap()])
            .status()
            .unwrap();
        let expected_mtime = fs::metadata(&file).unwrap().mtime_nsec();
        let mut changed = false;
        let error = try_output_hash_of_with_hook(
            &dir.to_string_lossy(),
            false,
            None,
            &mut |path, stage| {
                if !changed && path == file && stage == "file-opened" {
                    changed = true;
                    fs::write(&file, "BBBB").unwrap();
                    Command::new("touch")
                        .args(["-r", stamp.to_str().unwrap(), file.to_str().unwrap()])
                        .status()
                        .unwrap();
                    assert_eq!(fs::metadata(&file).unwrap().mtime_nsec(), expected_mtime);
                }
            },
        )
        .unwrap_err();
        assert!(error.contains("changed while hashing"), "{error}");
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_archive_rejects_symlink_escape_cycle_and_special_file() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::UnixListener;

        let dir = scratch("hostile-nodes");
        let outside = dir.parent().unwrap().join("outside-cache-proof");
        fs::write(&outside, "outside").unwrap();
        symlink(&outside, dir.join("escape")).unwrap();
        assert!(try_output_hash_of(&dir.to_string_lossy()).is_err());
        fs::remove_file(dir.join("escape")).unwrap();

        symlink("b", dir.join("a")).unwrap();
        symlink("a", dir.join("b")).unwrap();
        assert!(try_output_hash_of(&dir.to_string_lossy()).is_err());
        fs::remove_file(dir.join("a")).unwrap();
        fs::remove_file(dir.join("b")).unwrap();

        let socket = UnixListener::bind(dir.join("socket")).unwrap();
        assert!(try_output_hash_of(&dir.to_string_lossy()).is_err());
        drop(socket);
        fs::remove_dir_all(dir).ok();
        fs::remove_file(outside).ok();
    }

    #[test]
    fn path_law_rejects_reserved_trailing_and_casefold() {
        assert!(validate_path_component(b"CON").is_err());
        assert!(validate_path_component(b"foo.").is_err());
        assert!(validate_path_component(b"foo ").is_err());
        assert!(validate_path_component(b"ok-name").is_ok());
        let err = reject_casefold_collisions(&[b"Foo".to_vec(), b"foo".to_vec()]).unwrap_err();
        assert_eq!(err.code, "case-fold");
        assert!(reject_casefold_collisions(&[b"Foo".to_vec(), b"Bar".to_vec()]).is_ok());
    }

    #[test]
    fn path_law_never_implicitly_normalizes_unicode() {
        // NFC vs NFD for "é" — distinct bytes must remain distinct names.
        let nfc = "é".as_bytes().to_vec();
        let nfd = "e\u{0301}".as_bytes().to_vec();
        assert_ne!(nfc, nfd);
        assert!(validate_path_component(&nfc).is_ok());
        assert!(validate_path_component(&nfd).is_ok());
        // Not a case-fold collision (different bytes after ASCII fold too).
        assert!(reject_casefold_collisions(&[nfc, nfd]).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn sparse_and_dense_zero_holes_share_digest() {
        use std::process::Command;
        let dir = scratch("sparse-policy");
        let sparse = dir.join("sparse");
        let dense = dir.join("dense");
        // 1 byte data + hole + 1 byte data via truncate/seek write.
        fs::write(&sparse, []).unwrap();
        let status = Command::new("truncate")
            .args(["-s", "1048576", sparse.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
        {
            use std::io::Write as _;
            let mut f = fs::OpenOptions::new().write(true).open(&sparse).unwrap();
            f.write_all(b"A").unwrap();
            use std::io::Seek as _;
            f.seek(std::io::SeekFrom::Start(1048575)).unwrap();
            f.write_all(b"Z").unwrap();
        }
        // Dense file with same logical bytes (zeros in the middle).
        let mut bytes = vec![0u8; 1_048_576];
        bytes[0] = b'A';
        bytes[1_048_575] = b'Z';
        fs::write(&dense, &bytes).unwrap();
        let hs = try_output_hash_of(&sparse.to_string_lossy()).unwrap();
        let hd = try_output_hash_of(&dense.to_string_lossy()).unwrap();
        // File roots hash file bytes only — same logical content → same digest.
        assert_eq!(hs, hd);
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn path_law_rejects_casefold_siblings_during_hash() {
        let dir = scratch("casefold-sibs");
        fs::write(dir.join("Foo"), "a").unwrap();
        fs::write(dir.join("foo"), "b").unwrap();
        // On a case-sensitive FS both exist; archive must reject.
        let err = try_output_hash_of(&dir.to_string_lossy()).unwrap_err();
        assert!(
            err.contains("case-fold") || err.contains("collides"),
            "{err}"
        );
        fs::remove_dir_all(dir).ok();
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "env-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
