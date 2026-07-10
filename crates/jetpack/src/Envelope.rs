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

use crate::SHA256;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

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
    encode_node(&root, &root, Path::new(""), &mut archive, &mut hardlinks)?;
    for link in hardlinks.values() {
        if link.seen != link.total {
            return Err(format!(
                "hardlink `{}` has {} link(s), but only {} are inside the output",
                link.first.display(), link.total, link.seen
            ));
        }
    }
    Ok(format!("sha256-{}", SHA256::sha256_hex(&archive)))
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
) -> Result<(), String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("cannot inspect `{}`: {e}", path.display()))?;
    let kind = meta.file_type();
    let rel_bytes = path_bytes(rel);
    if kind.is_dir() {
        let before = directory_identity(&meta);
        let resolved = fs::canonicalize(path)
            .map_err(|e| format!("cannot resolve directory `{}`: {e}", path.display()))?;
        if !resolved.starts_with(root) {
            return Err(format!("directory `{}` escapes output root", path.display()));
        }
        record_header(archive, b'D', &rel_bytes, mode_of(&meta));
        let mut entries = fs::read_dir(path)
            .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
        entries.sort_by_key(|entry| path_bytes(Path::new(&entry.file_name())));
        for entry in entries {
            let child_rel = rel.join(entry.file_name());
            encode_node(root, &entry.path(), &child_rel, archive, hardlinks)?;
        }
        let after = fs::symlink_metadata(path)
            .map_err(|e| format!("directory `{}` changed while hashing: {e}", path.display()))?;
        if before != directory_identity(&after) {
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
        let bytes = read_file_stable(path, &meta)?;
        push_bytes(archive, &bytes);
    } else {
        return Err(format!(
            "unsupported special file in output: `{}`",
            path.display()
        ));
    }
    Ok(())
}

fn read_file_stable(path: &Path, expected: &fs::Metadata) -> Result<Vec<u8>, String> {
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

#[cfg(unix)]
fn directory_identity(meta: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    (meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn directory_identity(meta: &fs::Metadata) -> (u64, u64) {
    (meta.len(), u64::from(meta.permissions().readonly()))
}

#[cfg(unix)]
fn stable_file_identity(meta: &fs::Metadata) -> (u64, u64, u64, i64, i64, u32) {
    use std::os::unix::fs::MetadataExt as _;
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.mode(),
    )
}

#[cfg(not(unix))]
fn stable_file_identity(meta: &fs::Metadata) -> (u64, u64, u64, i64, i64, u32) {
    (
        0,
        0,
        meta.len(),
        meta.modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_secs() as i64),
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
