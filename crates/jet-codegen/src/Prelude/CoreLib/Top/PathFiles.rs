// ── Typed Path API (D-PATHFS1) ────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct JetPath {
    inner: std::path::PathBuf,
}
impl JetShow for JetPath {
    fn jet_show(&self) -> String {
        self.inner.to_string_lossy().to_string()
    }
}
fn jet_path_from(s: &String) -> JetPath {
    JetPath {
        inner: std::path::PathBuf::from(s),
    }
}
fn jet_path_join(p: &JetPath, other: &String) -> JetPath {
    JetPath {
        inner: p.inner.join(other.as_str()),
    }
}
fn jet_path_parent(p: &JetPath) -> Option<JetPath> {
    p.inner.parent().map(|par| JetPath {
        inner: par.to_path_buf(),
    })
}
fn jet_path_extension(p: &JetPath) -> Option<String> {
    p.inner.extension().map(|e| e.to_string_lossy().to_string())
}
fn jet_path_stem(p: &JetPath) -> Option<String> {
    p.inner.file_stem().map(|s| s.to_string_lossy().to_string())
}

static JET_ATOMIC_TEMP_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

struct JetAtomicTemp {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
    committed: bool,
}

impl JetAtomicTemp {
    fn create(dir: &std::path::Path) -> std::io::Result<Self> {
        for _ in 0..128 {
            let sequence = JET_ATOMIC_TEMP_COUNTER
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!(
                ".jet_tmp_{}_{}",
                std::process::id(),
                sequence
            ));
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                        committed: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not reserve an atomic-write temporary file",
        ))
    }

    fn file_mut(&mut self) -> &mut std::fs::File {
        self.file.as_mut().expect("atomic temp file must be open")
    }

    fn close(&mut self) {
        self.file.take();
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for JetAtomicTemp {
    fn drop(&mut self) {
        self.close();
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

// JET_VETTED_UNSAFE_BEGIN: jet_atomic_windows
#[cfg(windows)]
mod jet_atomic_windows {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PATH_NOT_FOUND: i32 = 3;
    extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }

    fn wide(path: &std::path::Path) -> std::io::Result<Vec<u16>> {
        let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Windows path contains an embedded NUL",
            ));
        }
        encoded.push(0);
        Ok(encoded)
    }

    pub fn replace(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
        let target_exists = target.exists();
        let temp = wide(temp)?;
        let target = wide(target)?;
        if target_exists {
            let replaced = unsafe {
                ReplaceFileW(
                    target.as_ptr(),
                    temp.as_ptr(),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if replaced != 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if !matches!(
                error.raw_os_error(),
                Some(ERROR_FILE_NOT_FOUND) | Some(ERROR_PATH_NOT_FOUND)
            ) {
                return Err(error);
            }
        }
        let moved = unsafe {
            MoveFileExW(
                temp.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
// JET_VETTED_UNSAFE_END: jet_atomic_windows

#[cfg(not(windows))]
fn jet_atomic_replace(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(temp, target)
}

#[cfg(windows)]
fn jet_atomic_replace(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    jet_atomic_windows::replace(temp, target)
}

#[cfg(unix)]
fn jet_atomic_sync_parent(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn jet_atomic_sync_parent(_dir: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

fn jet_path_write_atomic(p: &JetPath, content: &Vec<u8>) -> Result<(), jet_std::IoError> {
    use std::io::Write;

    let path_s = p.inner.to_string_lossy();
    let parent = p.inner.parent().ok_or_else(|| {
        jet_std::io_error_at(
            jet_std::IoOperation::Write,
            &path_s,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path has no parent directory",
            ),
        )
    })?;
    // Path::parent("file") is Some(""). Treat that lexical parent as the
    // current directory so the post-rename directory sync cannot turn a
    // successful relative replacement into an error.
    let dir = if parent.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        parent
    };
    let existing_permissions = std::fs::metadata(&p.inner)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temp = JetAtomicTemp::create(dir)
        .map_err(|error| jet_std::io_error_at(jet_std::IoOperation::Write, dir.to_string_lossy().as_ref(), error))?;
    temp.file_mut()
        .write_all(content)
        .map_err(|error| jet_std::io_error_at(jet_std::IoOperation::Write, temp.path.to_string_lossy().as_ref(), error))?;
    if let Some(permissions) = existing_permissions {
        temp.file_mut()
            .set_permissions(permissions)
            .map_err(|error| jet_std::io_error_at(jet_std::IoOperation::Write, temp.path.to_string_lossy().as_ref(), error))?;
    }
    temp.file_mut()
        .sync_all()
        .map_err(|error| jet_std::io_error_at(jet_std::IoOperation::Flush, temp.path.to_string_lossy().as_ref(), error))?;
    temp.close();
    jet_atomic_replace(&temp.path, &p.inner).map_err(|error| jet_std::io_error_at(jet_std::IoOperation::Write, &path_s, error))?;
    temp.commit();
    jet_atomic_sync_parent(dir).map_err(|error| jet_std::io_error_at(jet_std::IoOperation::Flush, &path_s, error))
}
fn jet_path_walk(p: &JetPath) -> Vec<JetPath> {
    let mut result = Vec::new();
    let mut stack = vec![p.inner.clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        let canonical = match std::fs::canonicalize(&dir) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !visited.insert(canonical) {
            continue; // symlink loop — skip
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            result.push(JetPath {
                inner: path.clone(),
            });
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    result
}
// ─────────────────────────────────────────────────────────────────────────────

fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IoError> {
    let f = std::fs::File::open(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Read, path, e))?;
    Ok(JetFileReader {
        inner: std::io::BufReader::new(f),
        path: path.clone(),
    })
}
fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::File::create(path).map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(f),
        path: path.clone(),
    })
}
fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, path, e))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(f),
        path: path.clone(),
    })
}
fn jet_std_file_reader_read_line(
    r: &mut JetFileReader,
) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::io_error_at(jet_std::IoOperation::Read, &r.path, e)),
    }
}
fn jet_std_file_writer_write_line(
    w: &mut JetFileWriter,
    line: &String,
) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner
        .write_all(line.as_bytes())
        .and_then(|_| w.inner.write_all(b"\n"))
        .map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Write, &w.path, e))
}
fn jet_std_file_writer_flush(w: &mut JetFileWriter) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.flush().map_err(|e| jet_std::io_error_at(jet_std::IoOperation::Flush, &w.path, e))
}
