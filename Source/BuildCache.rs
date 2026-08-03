//! Incremental build cache (M14).
//!
//! Layout: `~/.cache/jet/build/<semantic-key>/bin` plus `bin.sha256`.
//! The key is supplied by the compiler; the sidecar proves cached artifact
//! bytes were not truncated or corrupted between builds.

use crate::SHA256::sha256_hex;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CACHE_KEY_LEN: usize = 64;
const DIGEST_LEN: usize = 64;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Returns `~/.cache/jet/build`.
/// If `JET_CACHE_DIR` is set, that directory is used instead (for testing).
pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("JET_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("jet").join("build")
}

/// Content hash for a generated Rust source + profile tag (e.g. `"default"`,
/// `"small"`, `"release"`, `"debug"`, `"opt:full"` for named profiles).
/// D-BUILDPROFILE1: different profiles produce different binaries and must
/// not share cache entries.
pub fn cache_key(source: &str, profile_tag: &str) -> String {
    let mut data = Vec::with_capacity(source.len() + profile_tag.len() + 32);
    data.extend_from_slice(b"jet-build-cache-v2");
    for value in [source.as_bytes(), profile_tag.as_bytes()] {
        data.extend_from_slice(&(value.len() as u64).to_be_bytes());
        data.extend_from_slice(value);
    }
    sha256_hex(&data)
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn cache_entry_dir(key: &str) -> Option<PathBuf> {
    is_lower_hex(key, CACHE_KEY_LEN).then(|| cache_dir().join(key))
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn safe_cache_dir(dir: &Path) -> Result<(), String> {
    let root = cache_dir();
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!("unsafe build-cache root {}", root.display()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("could not inspect {}: {error}", root.display())),
    }
    match fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(format!("unsafe build-cache entry {}", dir.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not inspect {}: {error}", dir.display())),
    }
}

/// Path to the cached binary for `key`.
pub fn cached_bin(key: &str) -> PathBuf {
    cache_entry_dir(key)
        .unwrap_or_else(|| cache_dir().join("__invalid-cache-key__"))
        .join("bin")
}

fn cached_digest(key: &str) -> Option<PathBuf> {
    cache_entry_dir(key).map(|dir| dir.join("bin.sha256"))
}

fn parse_digest_record(bytes: &[u8]) -> Option<&str> {
    if bytes.len() != DIGEST_LEN + 1 || bytes[DIGEST_LEN] != b'\n' {
        return None;
    }
    let digest = std::str::from_utf8(&bytes[..DIGEST_LEN]).ok()?;
    is_lower_hex(digest, DIGEST_LEN).then_some(digest)
}

fn verified_cached_bytes(key: &str) -> Option<(Vec<u8>, fs::Permissions)> {
    let dir = cache_entry_dir(key)?;
    let digest = dir.join("bin.sha256");
    let bin = dir.join("bin");
    if !regular_file(&digest) || !regular_file(&bin) {
        return None;
    }
    let digest_bytes = fs::read(digest).ok()?;
    let expected = parse_digest_record(&digest_bytes)?;
    let mut source = fs::File::open(dir.join("bin")).ok()?;
    let permissions = source.metadata().ok()?.permissions();
    let mut bytes = Vec::new();
    source.read_to_end(&mut bytes).ok()?;
    (sha256_hex(&bytes) == expected).then_some((bytes, permissions))
}

fn temp_path(path: &Path, label: &str) -> Option<PathBuf> {
    let parent = path.parent()?;
    let name = path.file_name()?.to_str()?;
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    Some(parent.join(format!(".{name}.{label}.{}.{}", std::process::id(), id)))
}

fn publish_bytes(
    dest: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
    label: &str,
) -> Result<(), String> {
    let tmp = temp_path(dest, label).ok_or_else(|| {
        format!("could not derive a temporary path for {}", dest.display())
    })?;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|error| format!("could not stage {}: {error}", dest.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("could not stage {}: {error}", dest.display()))?;
        file.sync_all()
            .map_err(|error| format!("could not flush {}: {error}", dest.display()))?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&tmp, permissions).map_err(|error| {
                format!("could not preserve {} permissions: {error}", dest.display())
            })?;
        }
        fs::rename(&tmp, dest)
            .map_err(|error| format!("could not publish {}: {error}", dest.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Copy a cached binary to `dest` when present. Returns `true` on cache hit.
pub fn try_copy_cached(key: &str, dest: &Path) -> bool {
    let Some(dir) = cache_entry_dir(key) else {
        return false;
    };
    if safe_cache_dir(&dir).is_err() {
        return false;
    }
    let Some((bytes, permissions)) = verified_cached_bytes(key) else {
        return false;
    };
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    publish_bytes(dest, &bytes, Some(permissions), "copy").is_ok()
}

/// Store a freshly built binary in the cache.
///
/// Atomic: read source bytes once, hash those bytes, write those same bytes to
/// a per-process temp file, then rename. A concurrent `try_copy_cached` reader
/// therefore never observes a half-written `bin`. Preserve source permissions
/// so cached native artifacts stay executable.
pub fn store_cached(key: &str, bin: &Path) -> Result<(), String> {
    let dir = cache_entry_dir(key)
        .ok_or_else(|| "refusing to store a cache entry with an invalid key".to_string())?;
    if !regular_file(bin) {
        return Err(format!("refusing non-regular build artifact {}", bin.display()));
    }
    safe_cache_dir(&dir)?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let mut source = fs::File::open(bin)
        .map_err(|error| format!("could not read {}: {error}", bin.display()))?;
    let permissions = source
        .metadata()
        .map_err(|error| format!("could not stat {}: {error}", bin.display()))?
        .permissions();
    let mut bytes = Vec::new();
    source
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {}: {error}", bin.display()))?;
    let digest = sha256_hex(&bytes);
    let dest = dir.join("bin");
    publish_bytes(&dest, &bytes, Some(permissions), "store")?;
    let digest_path = cached_digest(key).expect("validated cache key has a digest path");
    publish_bytes(
        &digest_path,
        format!("{digest}\n").as_bytes(),
        None,
        "digest",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_changes_with_profile() {
        let a = cache_key("fn main() {}", "default");
        let b = cache_key("fn main() {}", "small");
        let c = cache_key("fn main() {}", "release");
        let d = cache_key("fn main() {}", "debug");
        let e = cache_key("fn main() {}", "ci");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_ne!(a, e);
        assert_ne!(c, d);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn cache_key_is_stable() {
        let k1 = cache_key("fn main() {}", "default");
        let k2 = cache_key("fn main() {}", "default");
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_integrity_rejects_changed_bytes() {
        let path = std::env::temp_dir().join(format!(
            "jet-build-cache-integrity-{}",
            std::process::id()
        ));
        fs::write(&path, b"valid").unwrap();
        let digest = sha256_hex(b"valid");
        assert_eq!(
            parse_digest_record(format!("{digest}\n").as_bytes()),
            Some(digest.as_str())
        );
        fs::write(&path, b"corrupt").unwrap();
        assert_ne!(sha256_hex(&fs::read(&path).unwrap()), digest);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn cache_digest_rejects_loose_or_uppercase_records() {
        let digest = sha256_hex(b"valid");
        assert!(parse_digest_record(format!("{digest} \n").as_bytes()).is_none());
        assert!(
            parse_digest_record(format!("{}\n", digest.to_ascii_uppercase()).as_bytes())
                .is_none()
        );
    }
}
