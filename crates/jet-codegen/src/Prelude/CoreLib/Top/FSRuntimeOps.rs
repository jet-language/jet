// Filesystem operations shared by AOT, JIT, and the interpreter.
//
// The surrounding engine supplies `jet_std`, `jet_fault_should_fail`, and the
// raw `jet_fs_*` kernels. Error classification and fault policy live here.

// JET_VETTED_UNSAFE_BEGIN: jet_fs_chown
// AUDIT: this region is the checked POSIX chown ABI adapter. Safe Rust cannot
// express the libc call; this adapter validates UID/GID bounds and a NUL-free
// path before calling it, and callers receive only Result<(), IOError>.
// Violating those checks could transfer ownership to an unintended path or ID.
#[cfg(unix)]
extern "C" {
    fn chown(path: *const std::ffi::c_char, owner: u32, group: u32) -> i32;
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
pub(crate) fn jet_std_fs_scope_read(scope: &JetFileScope, path: &str, ) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
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

pub(crate) fn jet_std_fs_rename(from: &str, to: &str) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    jet_fs_rename(from, to)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, from, error))
}

pub(crate) fn jet_std_fs_read_link_path(path: &str) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    std::fs::read_link(path)
        .map(|target| target.to_string_lossy().to_string())
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))
}

pub(crate) fn jet_std_fs_hard_link_path(from: &str, to: &str, ) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    jet_std_fs_hard_link_raw(from, to)
}

pub(crate) fn jet_std_fs_hard_link_raw(from: &str, to: &str) -> Result<(), jet_std::IOError> {
    std::fs::hard_link(from, to)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, to, error))
}

fn jet_std_fs_temp_path(prefix: &str) -> String {
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

pub(crate) fn jet_std_fs_temp_dir_path(prefix: &str) -> Result<String, jet_std::IOError> {
    let path = jet_std_fs_temp_path(prefix);
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    jet_std_fs_temp_dir_path_raw(path)
}

pub(crate) fn jet_std_fs_temp_dir_path_raw(path: String) -> Result<String, jet_std::IOError> {
    std::fs::create_dir(&path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &path, error))?;
    Ok(path)
}

pub(crate) fn jet_std_fs_temp_file_path(prefix: &str) -> Result<String, jet_std::IOError> {
    let path = jet_std_fs_temp_path(prefix);
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    jet_std_fs_temp_file_path_raw(path)
}

pub(crate) fn jet_std_fs_temp_file_path_raw(path: String) -> Result<String, jet_std::IOError> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &path, error))?;
    Ok(path)
}


pub(crate) fn jet_std_fs_temp_dir(prefix: &str) -> Result<JetTempDirOwner, jet_std::IOError> {
    let path = jet_std_fs_temp_dir_path(prefix)?;
    jet_std_fs_temp_dir_owner_raw(path)
}

fn jet_std_fs_temp_dir_owner_raw(path: String) -> Result<JetTempDirOwner, jet_std::IOError> {
    Ok(JetTempDirOwner {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}

pub(crate) fn jet_std_fs_temp_dir_raw(path: String) -> Result<JetTempDirOwner, jet_std::IOError> {
    jet_std_fs_temp_dir_owner_raw(jet_std_fs_temp_dir_path_raw(path)?)
}

pub(crate) fn jet_std_fs_temp_file(prefix: &str) -> Result<JetTempFileOwner, jet_std::IOError> {
    let path = jet_std_fs_temp_file_path(prefix)?;
    jet_std_fs_temp_file_owner_raw(path)
}

fn jet_std_fs_temp_file_owner_raw(path: String) -> Result<JetTempFileOwner, jet_std::IOError> {
    Ok(JetTempFileOwner {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}

pub(crate) fn jet_std_fs_temp_file_raw(path: String) -> Result<JetTempFileOwner, jet_std::IOError> {
    jet_std_fs_temp_file_owner_raw(jet_std_fs_temp_file_path_raw(path)?)
}


pub(crate) fn jet_std_fs_fsync(path: &str) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some(path.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    jet_std_fs_fsync_raw(path)
}

pub(crate) fn jet_std_fs_fsync_raw(path: &str) -> Result<(), jet_std::IOError> {
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Flush, path, e))
}

pub(crate) fn jet_std_fs_glob(pattern: &str) -> Result<Vec<String>, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(pattern.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    jet_std_fs_glob_raw(pattern)
}

pub(crate) fn jet_std_fs_glob_raw(pattern: &str) -> Result<Vec<String>, jet_std::IOError> {
    jet_fs_glob(pattern)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, pattern, error))
}

pub(crate) fn jet_std_fs_canonicalize(path: &str) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    jet_std_fs_canonicalize_raw(path)
}

pub(crate) fn jet_std_fs_canonicalize_raw(path: &str) -> Result<String, jet_std::IOError> {
    jet_fs_canonicalize(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Resolve, path, error))
}
pub(crate) fn jet_fs_remove_path(path: &str) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    jet_fs_remove_entry(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
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

pub(crate) fn jet_fs_stat(path: &str) -> Result<JetFsStat, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    jet_fs_stat_raw(path)
}

pub(crate) fn jet_fs_stat_raw(path: &str) -> Result<JetFsStat, jet_std::IOError> {
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
pub(crate) fn jet_std_fs_stat_mode_raw(path: &str) -> Result<i64, jet_std::IOError> {
    Ok(jet_fs_stat_raw(path)?.mode)
}

pub(crate) fn jet_std_fs_is_fifo(path: &str) -> Result<bool, jet_std::IOError> {
    Ok((jet_fs_stat(path)?.mode & 0o170000) == 0o010000)
}

pub(crate) fn jet_std_fs_is_socket(path: &str) -> Result<bool, jet_std::IOError> {
    Ok((jet_fs_stat(path)?.mode & 0o170000) == 0o140000)
}


pub(crate) fn jet_std_fs_chown(path: &str, owner: i64,
group: i64,) -> Result<(), jet_std::IOError> {
    jet_std_fs_chown_raw(path, owner, group)
}

pub(crate) fn jet_std_fs_chown_raw(path: &str, owner: i64, group: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        use std::ffi::CString;

        let owner = if owner == -1 {
            u32::MAX
        } else {
            u32::try_from(owner).map_err(|_| {
                jet_std::IOError::other(
                    jet_std::IOOperation::Write,
                    Some(path.to_owned()),
                    "owner must be -1 or a non-negative u32",
                )
            })?
        };
        let group = if group == -1 {
            u32::MAX
        } else {
            u32::try_from(group).map_err(|_| {
                jet_std::IOError::other(
                    jet_std::IOOperation::Write,
                    Some(path.to_owned()),
                    "group must be -1 or a non-negative u32",
                )
            })?
        };
        let c_path = CString::new(path.as_bytes()).map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some(path.to_owned()),
                "path contains an interior NUL",
            )
        })?;
        if unsafe { chown(c_path.as_ptr(), owner, group) } == 0 {
            Ok(())
        } else {
            Err(jet_std::io_error_at(
                jet_std::IOOperation::Write,
                path,
                std::io::Error::last_os_error(),
            ))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, owner, group);
        Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            None,
            "chown is unavailable on this platform",
        ))
    }
}
// JET_VETTED_UNSAFE_END: jet_fs_chown


pub(crate) fn jet_std_fs_set_mode(path: &str, mode: i64) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    jet_std_fs_set_mode_raw(path, mode)
}

pub(crate) fn jet_std_fs_set_mode_raw(path: &str, mode: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)]
    {
        if mode < 0 || mode > i64::from(u32::MAX) {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some(path.to_owned()),
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
                Some(path.to_owned()),
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
pub(crate) fn jet_std_fs_symlink(from: &str, to: &str) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    #[cfg(windows)]
    let directory = jet_std_fs_symlink_directory_raw(from)?;
    #[cfg(not(windows))]
    let directory = false;
    jet_std_fs_symlink_raw(from, to, directory)
}

pub(crate) fn jet_std_fs_symlink_directory_raw(from: &str) -> Result<bool, jet_std::IOError> {
    std::fs::metadata(from)
        .map(|meta| meta.is_dir())
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, from, error))
}

pub(crate) fn jet_std_fs_symlink_raw(from: &str, to: &str, directory: bool) -> Result<(), jet_std::IOError> {
    #[cfg(not(windows))]
    let _ = directory;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to)
            .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
    }
    #[cfg(windows)]
    {
        if directory {
            std::os::windows::fs::symlink_dir(from, to)
                .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
        } else {
            std::os::windows::fs::symlink_file(from, to)
                .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = from;
        Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.to_owned()),
            "symbolic links are unavailable on this platform",
        ))
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
pub(crate) fn jet_std_fs_read_at(path: &str, offset: i64,
len: i64,) -> Result<Vec<u8>, jet_std::IOError> {
    use std::io::{Read, Seek, SeekFrom};
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
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
pub(crate) fn jet_std_fs_read_bytes(path: &str) -> Result<Vec<u8>, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    std::fs::read(path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))
}

/// D-FOUND-VIEW1: open one shared read-only mapping. The adapter owns only
/// fault classification; native lifetime and path identity stay in JetStd.
pub(crate) fn jet_std_fs_map(path: &str) -> Result<jet_std::JetMappedFile, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
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

// Temporary existing-authority projection and no-follow OS leaf. Root choice,
// lexical component policy, checkpoints, retry-last-error, and UTF-8 are Jet.
// Replaced by Jet runtime L7; dies at cutover (CoreFiles1-2 leaf ledger).
pub(crate) fn jet_files_scope_roots_raw(scope: &JetFileScope) -> Vec<(Vec<u8>, bool)> {
    scope.roots().iter().map(|root| (root.path().as_os_str().as_encoded_bytes().to_vec(), root.allow_absolute())).collect()
}

pub(crate) fn jet_files_scope_read_at_raw(scope: &JetFileScope, index: i64, relative: &str, requested: &str) -> Result<Vec<u8>, jet_std::IOError> {
    let root = usize::try_from(index).ok().and_then(|index| scope.roots().get(index))
        .ok_or_else(|| jet_std::IOError::PermissionDenied(jet_std::IOContext::new(
            jet_std::IOOperation::Read, Some(requested.to_owned()), None,
            Some("path is outside the granted FS.Read root".to_string()))))?;
    jet_foundation::SHA256::read_file_nofollow_at_root(
        root.path(), std::path::Path::new(relative), jet_foundation::SHA256::MAX_TREE_FILE_BYTES,
    ).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, requested, error))
}
