use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static SOURCE_TRANSACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub(super) enum SourceWriteError {
    Conflict,
    Io(io::Error),
}

impl From<io::Error> for SourceWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(super) fn with_source_transaction<T>(
    action: impl FnOnce() -> Result<T, SourceWriteError>,
) -> Result<T, SourceWriteError> {
    let lock = SOURCE_TRANSACTION_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().map_err(|_| {
        SourceWriteError::Io(io::Error::new(
            io::ErrorKind::Other,
            "Canvas source transaction lock was poisoned",
        ))
    })?;
    action()
}

pub(super) fn write_source_if_unchanged(
    path: &Path,
    expected: &str,
    candidate: &str,
) -> Result<(), SourceWriteError> {
    with_source_transaction(|| {
        replace_source_if_unchanged_locked(path, Some(expected), Some(candidate))
    })
}

pub(super) fn replace_source_if_unchanged_locked(
    path: &Path,
    expected: Option<&str>,
    candidate: Option<&str>,
) -> Result<(), SourceWriteError> {
    let current = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(SourceWriteError::Io(error)),
    };
    let matches = match expected {
        Some(expected) => current.as_deref() == Some(expected.as_bytes()),
        None => current.is_none(),
    };
    if !matches {
        return Err(SourceWriteError::Conflict);
    }

    match candidate {
        Some(candidate) if current.as_deref() != Some(candidate.as_bytes()) => {
            atomic_replace(path, candidate.as_bytes())
        }
        Some(_) => Ok(()),
        None => {
            if current.is_some() {
                fs::remove_file(path).map_err(SourceWriteError::Io)
            } else {
                Ok(())
            }
        }
    }
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), SourceWriteError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| {
            SourceWriteError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source path has no file name",
            ))
        })?
        .to_string_lossy();

    let mode = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temporary = None;
    for _ in 0..100 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.canvas-{sequence}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(mode) = mode.as_ref() {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(mode.mode());
        }
        match options.open(&candidate) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                    let _ = fs::remove_file(&candidate);
                    return Err(SourceWriteError::Io(error));
                }
                temporary = Some(candidate);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(SourceWriteError::Io(error)),
        }
    }
    let Some(temporary) = temporary else {
        return Err(SourceWriteError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a Canvas source temporary file",
        )));
    };
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(SourceWriteError::Io(error));
    }
    if let Ok(parent_file) = File::open(parent) {
        let _ = parent_file.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "jet-canvas-source-model-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root.join("main.jet")
    }

    #[test]
    fn compare_and_publish_preserves_source_on_conflict() {
        let path = test_path();
        fs::write(&path, "before\n").unwrap();

        write_source_if_unchanged(&path, "before\n", "after\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");

        let error = write_source_if_unchanged(&path, "before\n", "lost\n").unwrap_err();
        assert!(matches!(error, SourceWriteError::Conflict));
        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn compare_and_publish_supports_new_and_removed_source() {
        let path = test_path();
        replace_source_if_unchanged_locked(&path, None, Some("new\n")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        replace_source_if_unchanged_locked(&path, Some("new\n"), None).unwrap();
        assert!(!path.exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
