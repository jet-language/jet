//! D-BROWSER-AUTO1=A (#1187): locked browser provisioning through Jetpack.
//!
//! Records exact browser binaries in `.jet/lock` `[[browser]]` blocks so
//! automation resolves a project-pinned install — never a host PATH scrape.

use crate::Lock::{self, LockEnvelope, LockedBrowser};
use crate::SHA256;
use crate::Syntax;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserLockError {
    UnknownEngine(String),
    MissingBinary(PathBuf),
    MissingLock(String),
    HashMismatch { engine: String, expected: String, actual: String },
    SizeMismatch { engine: String, expected: u64, actual: u64 },
    Io(String),
    InvalidProtocol(String),
}

impl std::fmt::Display for BrowserLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserLockError::UnknownEngine(engine) => {
                write!(
                    f,
                    "browser engine `{engine}` is not supported (use {})",
                    Syntax::BROWSER_ENGINES.join(", ")
                )
            }
            BrowserLockError::MissingBinary(path) => {
                write!(f, "browser binary `{}` was not found", path.display())
            }
            BrowserLockError::MissingLock(engine) => {
                write!(
                    f,
                    "no locked browser for `{engine}` in {}",
                    Syntax::UNIFIED_LOCK_FILE
                )
            }
            BrowserLockError::HashMismatch {
                engine,
                expected,
                actual,
            } => write!(
                f,
                "locked browser `{engine}` hash drifted (expected {expected}, found {actual})"
            ),
            BrowserLockError::SizeMismatch {
                engine,
                expected,
                actual,
            } => write!(
                f,
                "locked browser `{engine}` size drifted (expected {expected}, found {actual})"
            ),
            BrowserLockError::Io(message) => write!(f, "{message}"),
            BrowserLockError::InvalidProtocol(protocol) => {
                write!(f, "browser protocol `{protocol}` is not a BiDi profile")
            }
        }
    }
}

pub fn normalize_engine(raw: &str) -> Result<&'static str, BrowserLockError> {
    let lower = raw.to_ascii_lowercase();
    Syntax::BROWSER_ENGINES
        .iter()
        .copied()
        .find(|engine| *engine == lower)
        .ok_or_else(|| BrowserLockError::UnknownEngine(raw.to_string()))
}

pub fn validate_protocol(protocol: &str) -> Result<(), BrowserLockError> {
    if matches!(protocol, "bidi-2025.5" | "bidi-2024.11") {
        Ok(())
    } else {
        Err(BrowserLockError::InvalidProtocol(protocol.to_string()))
    }
}

fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        other => other,
    };
    format!("{}-{}", std::env::consts::ARCH, os)
}

fn hash_file(path: &Path) -> Result<(String, u64), BrowserLockError> {
    let bytes = fs::read(path).map_err(|error| {
        BrowserLockError::Io(format!("could not read {}: {error}", path.display()))
    })?;
    let size = bytes.len() as u64;
    let digest = format!("sha256-{}", SHA256::sha256_hex(&bytes));
    Ok((digest, size))
}

/// Lock a local browser binary into the project lock (deterministic pin).
pub fn lock_binary(
    project_root: &Path,
    engine: &str,
    binary: &Path,
    version: &str,
    protocol: &str,
    provenance: &str,
) -> Result<LockedBrowser, BrowserLockError> {
    let engine = normalize_engine(engine)?.to_string();
    validate_protocol(protocol)?;
    if !binary.is_file() {
        return Err(BrowserLockError::MissingBinary(binary.to_path_buf()));
    }
    let absolute = fs::canonicalize(binary).map_err(|error| {
        BrowserLockError::Io(format!("could not resolve {}: {error}", binary.display()))
    })?;
    let (output_hash, size) = hash_file(&absolute)?;
    let locked = LockedBrowser {
        engine,
        version: version.to_string(),
        binary: absolute.to_string_lossy().into_owned(),
        protocol: protocol.to_string(),
        size,
        envelope: LockEnvelope {
            output_hash,
            platform: platform(),
            signature: String::new(),
            provenance: provenance.to_string(),
        },
    };
    Lock::record_browser(project_root, locked.clone());
    Ok(locked)
}

/// Resolve and re-verify a locked browser. Hash and size must still match.
pub fn resolve(project_root: &Path, engine: &str) -> Result<LockedBrowser, BrowserLockError> {
    let engine = normalize_engine(engine)?;
    let lock = Lock::load(project_root).ok_or_else(|| {
        BrowserLockError::MissingLock(engine.to_string())
    })?;
    let locked = lock
        .browsers
        .into_iter()
        .find(|entry| entry.engine == engine)
        .ok_or_else(|| BrowserLockError::MissingLock(engine.to_string()))?;
    verify(&locked)?;
    Ok(locked)
}

pub fn list(project_root: &Path) -> Vec<LockedBrowser> {
    Lock::load(project_root)
        .map(|lock| lock.browsers)
        .unwrap_or_default()
}

pub fn verify(locked: &LockedBrowser) -> Result<(), BrowserLockError> {
    let path = Path::new(&locked.binary);
    if !path.is_file() {
        return Err(BrowserLockError::MissingBinary(path.to_path_buf()));
    }
    let meta = fs::metadata(path).map_err(|error| {
        BrowserLockError::Io(format!("could not stat {}: {error}", path.display()))
    })?;
    let size = meta.len();
    if size != locked.size {
        return Err(BrowserLockError::SizeMismatch {
            engine: locked.engine.clone(),
            expected: locked.size,
            actual: size,
        });
    }
    let (actual, _) = hash_file(path)?;
    if actual != locked.envelope.output_hash {
        return Err(BrowserLockError::HashMismatch {
            engine: locked.engine.clone(),
            expected: locked.envelope.output_hash.clone(),
            actual,
        });
    }
    Ok(())
}

/// Preferred executable names for a realized package `bin/` directory.
pub fn binary_candidates(engine: &str) -> &'static [&'static str] {
    match engine {
        "chromium" => &[
            "chromium",
            "chromium-browser",
            "chrome",
            "google-chrome",
            "google-chrome-stable",
        ],
        "firefox" => &["firefox"],
        "webkit" => &["MiniBrowser", "webkit", "WebKit"],
        _ => &[],
    }
}

/// Find the engine binary under a realized package output (usually `…/bin`).
pub fn find_engine_binary(output: &Path, engine: &str) -> Result<PathBuf, BrowserLockError> {
    let engine = normalize_engine(engine)?;
    let bin_dir = if output.join("bin").is_dir() {
        output.join("bin")
    } else {
        output.to_path_buf()
    };
    for name in binary_candidates(engine) {
        let candidate = bin_dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(BrowserLockError::MissingBinary(bin_dir.join(engine)))
}

pub fn read_version_label(binary: &Path) -> String {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output();
    match output {
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            text.lines()
                .next()
                .unwrap_or("unknown")
                .trim()
                .to_string()
        }
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("jet-browser-lock-{label}-{nanos}"));
        fs::create_dir_all(root.join(".jet")).unwrap();
        root
    }

    fn write_fake_browser(root: &Path, body: &str) -> PathBuf {
        let bin = root.join("fake-chromium");
        fs::write(&bin, body).unwrap();
        let mut perms = fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin, perms).unwrap();
        bin
    }

    #[test]
    fn lock_resolve_and_hash_drift() {
        let root = temp_root("ok");
        let bin = write_fake_browser(&root, "#!/bin/sh\necho Chromium 131.0\n");
        let locked = lock_binary(
            &root,
            "chromium",
            &bin,
            "131.0",
            Syntax::BROWSER_DEFAULT_PROTOCOL,
            "test fixture",
        )
        .expect("lock");
        assert_eq!(locked.engine, "chromium");
        let resolved = resolve(&root, "chromium").expect("resolve");
        assert_eq!(resolved.binary, locked.binary);
        assert_eq!(resolved.envelope.output_hash, locked.envelope.output_hash);

        fs::write(&bin, "#!/bin/sh\necho Chromium 999\n").unwrap();
        match resolve(&root, "chromium") {
            Err(BrowserLockError::HashMismatch { .. } | BrowserLockError::SizeMismatch { .. }) => {}
            other => panic!("expected hash/size drift, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_engine_and_missing_lock() {
        assert!(matches!(
            normalize_engine("edge"),
            Err(BrowserLockError::UnknownEngine(_))
        ));
        let root = temp_root("missing");
        assert!(matches!(
            resolve(&root, "firefox"),
            Err(BrowserLockError::MissingLock(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn find_engine_binary_prefers_bin_dir() {
        let root = temp_root("find");
        let bin_dir = root.join("out").join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let chromium = bin_dir.join("chromium");
        fs::write(&chromium, b"x").unwrap();
        let found = find_engine_binary(&root.join("out"), "chromium").unwrap();
        assert_eq!(found, chromium);
        let _ = fs::remove_dir_all(root);
    }
}
