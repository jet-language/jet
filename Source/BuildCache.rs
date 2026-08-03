//! Incremental build cache (M14).
//!
//! Layout: `~/.cache/jet/build/<semantic-key>/bin` plus `bin.sha256`.
//! The key is supplied by the compiler; the sidecar proves cached artifact
//! bytes were not truncated or corrupted between builds.

use crate::SHA256::sha256_hex;
use std::fs;
use std::path::{Path, PathBuf};

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

/// Path to the cached binary for `key`.
pub fn cached_bin(key: &str) -> PathBuf {
    cache_dir().join(key).join("bin")
}

fn cached_digest(key: &str) -> PathBuf {
    cache_dir().join(key).join("bin.sha256")
}

fn matches_digest(path: &Path, expected: &str) -> bool {
    fs::read(path)
        .ok()
        .map(|bytes| sha256_hex(&bytes) == expected.trim())
        .unwrap_or(false)
}

/// Copy a cached binary to `dest` when present. Returns `true` on cache hit.
pub fn try_copy_cached(key: &str, dest: &Path) -> bool {
    let src = cached_bin(key);
    let expected = match fs::read_to_string(cached_digest(key)) {
        Ok(expected) => expected,
        Err(_) => return false,
    };
    if !src.is_file() || !matches_digest(&src, &expected) {
        return false;
    }
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    fs::copy(&src, dest).is_ok()
}

/// Store a freshly built binary in the cache.
///
/// Atomic: the binary is copied to a per-process temp file first, then renamed
/// into place. A concurrent `try_copy_cached` reader therefore never observes a
/// half-written `bin`. Two processes storing the *same* key are storing the
/// same content by construction (the key is content-addressed), so a
/// last-writer-wins rename is safe.
pub fn store_cached(key: &str, bin: &Path) -> Result<(), String> {
    let dir = cache_dir().join(key);
    fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let bytes = fs::read(bin)
        .map_err(|error| format!("could not read {}: {error}", bin.display()))?;
    let digest = sha256_hex(&bytes);
    let dest = dir.join("bin");
    let tmp = dir.join(format!("bin.tmp.{}", std::process::id()));
    if let Err(error) = fs::copy(bin, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("could not stage {}: {error}", bin.display()));
    }
    if let Err(error) = fs::rename(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("could not publish {}: {error}", dest.display()));
    }
    let digest_path = cached_digest(key);
    let digest_tmp = dir.join(format!("bin.sha256.tmp.{}", std::process::id()));
    if let Err(error) = fs::write(&digest_tmp, format!("{digest}\n")) {
        let _ = fs::remove_file(&digest_tmp);
        return Err(format!("could not write {}: {error}", digest_path.display()));
    }
    if let Err(error) = fs::rename(&digest_tmp, &digest_path) {
        let _ = fs::remove_file(&digest_tmp);
        return Err(format!("could not publish {}: {error}", digest_path.display()));
    }
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
        assert!(matches_digest(&path, &digest));
        fs::write(&path, b"corrupt").unwrap();
        assert!(!matches_digest(&path, &digest));
        let _ = fs::remove_file(path);
    }
}
