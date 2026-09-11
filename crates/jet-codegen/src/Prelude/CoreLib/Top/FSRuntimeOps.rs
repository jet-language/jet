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

/// D-FILES-SCOPE1: capture only the explicit FS.Read resource roots from the
/// checked Authority. The handle owns that attenuation and never accepts a
/// caller-supplied root.
pub(crate) fn jet_std_fs_scope(authority: &JetAuthority) -> JetFileScope {
    JetFileScope::from_authority(authority)
}

/// D-FILES-SCOPE1: read through the descriptor-relative no-follow kernel.
/// Fault injection and IOError projection stay in this adapter; path policy
/// and root isolation stay in `jet_fs_scope_read`.
pub(crate) fn jet_std_fs_scope_read(
    scope: &JetFileScope,
    path: &String,
) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let bytes = jet_fs_scope_read(scope, path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))?;
    String::from_utf8(bytes).map_err(|error| {
        jet_std::io_error_at(
            jet_std::IOOperation::Read,
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })
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

pub(crate) fn jet_std_fs_read_link_path(path: &String) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    std::fs::read_link(path)
        .map(|target| target.to_string_lossy().to_string())
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))
}

pub(crate) fn jet_std_fs_hard_link_path(
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
    std::fs::hard_link(from, to)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, to, error))
}

fn jet_std_fs_temp_path(prefix: &String) -> String {
    let clean: String = prefix
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("{}_{}_{}", clean, std::process::id(), nanos))
        .to_string_lossy()
        .to_string()
}

pub(crate) fn jet_std_fs_temp_dir_path(prefix: &String) -> Result<String, jet_std::IOError> {
    let path = jet_std_fs_temp_path(prefix);
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::create_dir(&path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &path, error))?;
    Ok(path)
}

pub(crate) fn jet_std_fs_temp_file_path(prefix: &String) -> Result<String, jet_std::IOError> {
    let path = jet_std_fs_temp_path(prefix);
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &path, error))?;
    Ok(path)
}

pub(crate) fn jet_std_fs_lock_path(path: &String) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    Ok(path.clone())
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

pub(crate) fn jet_std_fs_glob(pattern: &String) -> Result<Vec<String>, jet_std::IOError> {
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
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
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

pub(crate) fn jet_std_fs_set_mode(path: &String, mode: i64) -> Result<(), jet_std::IOError> {
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
pub(crate) fn jet_std_fs_symlink(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to)
            .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
    }
    #[cfg(windows)]
    {
        let meta = std::fs::metadata(from)
            .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, from, e))?;
        if meta.is_dir() {
            std::os::windows::fs::symlink_dir(from, to)
                .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
        } else {
            std::os::windows::fs::symlink_file(from, to)
                .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
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
pub(crate) fn jet_std_fs_read_at(
    path: &String,
    offset: i64,
    len: i64,
) -> Result<Vec<u8>, jet_std::IOError> {
    use std::io::{Read, Seek, SeekFrom};
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let mut f = std::fs::File::open(path)
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = f
        .read(&mut buf)
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    buf.truncate(n);
    Ok(buf)
}
pub(crate) fn jet_std_fs_read_bytes(path: &String) -> Result<Vec<u8>, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    std::fs::read(path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))
}

/// D-FOUND-VIEW1: open one shared read-only mapping. The adapter owns only
/// fault classification; native lifetime and path identity stay in JetStd.
pub(crate) fn jet_std_fs_map(
    path: &String,
) -> Result<jet_std::JetMappedFile, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    jet_std::jet_std_files_map(path)
}

/// Construct a checked map window without materializing its bytes.
pub(crate) fn jet_std_fs_map_window(
    map: &jet_std::JetMappedFile,
    offset: i64,
    length: i64,
) -> Result<jet_std::JetMappedByteView, jet_std::IOError> {
    jet_std::jet_std_files_map_window(map, offset, length)
}
/// Borrowed half-open byte window; the map owner remains in the returned slice.
pub(crate) fn jet_std_fs_map_window_view(
    map: &jet_std::JetMappedFile,
    start: i64,
    end: i64,
) -> Result<&[u8], jet_std::IOError> {
    jet_std::jet_std_files_map_window_view(map, start, end)
}

/// Borrowed offset/length byte window; no byte buffer is materialized.
pub(crate) fn jet_std_fs_map_window_len_view(
    map: &jet_std::JetMappedFile,
    offset: i64,
    length: i64,
) -> Result<&[u8], jet_std::IOError> {
    jet_std::jet_std_files_map_window_len_view(map, offset, length)
}

/// Borrowed mapped-file lines tie their item lifetime to the map binding.
pub(crate) fn jet_std_fs_map_lines_view<'a>(
    map: &'a jet_std::JetMappedFile,
) -> JetViewIter<'a, &'a [u8]> {
    jet_view_iter_from_iter(jet_std::jet_std_files_map_lines_view(map))
}

pub(crate) fn jet_std_fs_map_len(map: &jet_std::JetMappedFile) -> i64 {
    map.len()
}

pub(crate) fn jet_std_fs_map_is_empty(map: &jet_std::JetMappedFile) -> bool {
    map.is_empty()
}

/// Keep mapped line iteration lazy; each yielded carrier pins its map lease.
pub(crate) fn jet_std_fs_map_lines(map: &jet_std::JetMappedFile) -> jet_std::JetMappedLines {
    jet_std::jet_std_files_map_lines(map)
}
