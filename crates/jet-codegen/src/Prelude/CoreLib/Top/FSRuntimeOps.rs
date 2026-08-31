// Filesystem operations shared by AOT, JIT, and the interpreter.
//
// The surrounding engine supplies `jet_std`, `jet_fault_should_fail`, and the
// raw `jet_fs_*` kernels. Error classification and fault policy live here.

pub(crate) fn jet_std_fs_absolute(path: &String) -> Result<String, jet_std::IOError> {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Resolve, None, e))?
            .join(p)
    };
    Ok(abs.to_string_lossy().to_string())
}


fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

pub(crate) fn jet_std_fs_rename(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    jet_fs_rename(from, to)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, from, error))
}

pub(crate) fn jet_std_fs_fsync(path: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Flush, path, e))
}

pub(crate) fn jet_std_fs_glob(
    pattern: &String,
) -> Result<Vec<String>, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(pattern.clone()),
            "fault injected: FS.Read",
        ));
    }
    jet_fs_glob(pattern)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, pattern, error))
}

pub(crate) fn jet_std_fs_canonicalize(path: &String) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    jet_fs_canonicalize(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Resolve, path, error))
}
pub(crate) struct JetFsStat {
    pub(crate) size: i64,
    pub(crate) modified_ms: i64,
    pub(crate) created_ms: i64,
    pub(crate) readonly: bool,
    pub(crate) is_file: bool,
    pub(crate) is_dir: bool,
    pub(crate) is_symlink: bool,
    pub(crate) kind: String,
    pub(crate) mode: i64,
}

pub(crate) fn jet_fs_stat(path: &String) -> Result<JetFsStat, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    let ft = meta.file_type();
    let modified_ms = meta.modified().ok().and_then(system_time_ms).unwrap_or(0);
    let created_ms = meta.created().ok().and_then(system_time_ms).unwrap_or(0);
    let kind = if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    };
    Ok(JetFsStat {
        size: meta.len() as i64,
        modified_ms,
        created_ms,
        readonly: meta.permissions().readonly(),
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        kind: kind.to_string(),
        mode: mode_of(&meta),
    })
}

pub(crate) fn jet_std_fs_set_mode(
    path: &String,
    mode: i64,
) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    #[cfg(unix)]
    {
        if mode < 0 || mode > i64::from(u32::MAX) {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some(path.clone()),
                "file mode must be between 0 and u32::MAX",
            ));
        }
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(mode as u32);
        return std::fs::set_permissions(path, permissions)
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error));
    }
    #[cfg(not(unix))]
    {
        if mode != 0 && mode != 1 {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some(path.clone()),
                "file mode must contain only the readonly bit on this platform",
            ));
        }
        let mut permissions = std::fs::metadata(path)
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?
            .permissions();
        permissions.set_readonly(mode == 1);
        return std::fs::set_permissions(path, permissions)
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error));
    }
}
pub(crate) fn jet_std_fs_symlink(
    from: &String,
    to: &String,
) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to).map_err(|e| {
            jet_std::io_error_at(jet_std::IOOperation::Write, to, e)
        })
    }
    #[cfg(windows)]
    {
        let meta = std::fs::metadata(from)
            .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, from, e))?;
        if meta.is_dir() {
            std::os::windows::fs::symlink_dir(from, to).map_err(|e| {
                jet_std::io_error_at(jet_std::IOOperation::Write, to, e)
            })
        } else {
            std::os::windows::fs::symlink_file(from, to).map_err(|e| {
                jet_std::io_error_at(jet_std::IOOperation::Write, to, e)
            })
        }
    }
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() as i64
}

#[cfg(not(unix))]
fn mode_of(meta: &std::fs::Metadata) -> i64 {
    // Non-Unix metadata has no portable permission bits; expose readonly as 0/1.
    i64::from(u8::from(meta.permissions().readonly()))
}
