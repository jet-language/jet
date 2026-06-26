//! Incremental build cache (M14).
//!
//! Layout: `~/.cache/jet/build/<sha256-of-source+flags>/bin`
//! Key = SHA-256 of generated Rust source + build profile tag string.

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
    let mut data = Vec::with_capacity(source.len() + profile_tag.len() + 1);
    data.extend_from_slice(source.as_bytes());
    data.push(0);
    data.extend_from_slice(profile_tag.as_bytes());
    sha256_hex(&data)
}

/// Path to the cached binary for `key`.
pub fn cached_bin(key: &str) -> PathBuf {
    cache_dir().join(key).join("bin")
}

/// Copy a cached binary to `dest` when present. Returns `true` on cache hit.
pub fn try_copy_cached(key: &str, dest: &Path) -> bool {
    let src = cached_bin(key);
    if !src.is_file() {
        return false;
    }
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    fs::copy(&src, dest).is_ok()
}

/// Store a freshly built binary in the cache (best-effort).
pub fn store_cached(key: &str, bin: &Path) {
    let dir = cache_dir().join(key);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let dest = dir.join("bin");
    let _ = fs::copy(bin, &dest);
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
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_ne!(c, d);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn cache_key_is_stable() {
        let k1 = cache_key("fn main() {}", "default");
        let k2 = cache_key("fn main() {}", "default");
        assert_eq!(k1, k2);
    }
}
