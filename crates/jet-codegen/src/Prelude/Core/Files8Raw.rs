// OWNER #32: the existing filesystem host operations, without source policy.
// Replaced by Jet runtime layer L7; every function dies at cutover.
fn jet_files8_platform() -> i64 { if cfg!(unix) { 1 } else if cfg!(windows) { 2 } else { 0 } }
fn jet_files8_map(path: &String) -> Result<jet_std::JetMappedFile, jet_std::IOError> {
    jet_std::jet_std_files_map(path)
}
fn jet_files8_remove(path: &String) -> Result<(), jet_std::IOError> {
    jet_fs_remove_entry(path).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}
fn jet_files8_is_dir(path: &String) -> bool { std::path::Path::new(path).is_dir() }
fn jet_files8_remove_all(path: &String, directory: bool) -> Result<(), jet_std::IOError> {
    let result = if directory { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    result.map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}
fn jet_files8_copy(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    std::fs::copy(from, to).map(|_| ()).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, from, error))
}
fn jet_files8_fsync(path: &String) -> Result<(), jet_std::IOError> {
    std::fs::OpenOptions::new().read(true).open(path).and_then(|file| file.sync_all())
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Flush, path, error))
}
fn jet_files8_stat_mode(path: &String) -> Result<i64, jet_std::IOError> {
    std::fs::symlink_metadata(path).map(|metadata| mode_of(&metadata))
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))
}
fn jet_files8_set_mode(path: &String, mode: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode as u32))
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
    }
    #[cfg(not(unix))] {
        let mut permissions = std::fs::metadata(path)
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?.permissions();
        permissions.set_readonly(mode == 1);
        std::fs::set_permissions(path, permissions)
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
    }
}
fn jet_files8_chown(path: &String, owner: i64, group: i64) -> Result<(), jet_std::IOError> {
    #[cfg(unix)] {
        let path_c = std::ffi::CString::new(path.as_bytes()).map_err(|_| jet_std::IOError::other(
            jet_std::IOOperation::Write, Some(path.clone()), "path contains an interior NUL"))?;
        // SAFETY: CString owns a terminated path through the existing chown ABI;
        // source validated each ID as -1 or u32 before entering the host leaf.
        if unsafe { chown(path_c.as_ptr(), owner as u32, group as u32) } == 0 { Ok(()) }
        else { Err(jet_std::io_error_at(jet_std::IOOperation::Write, path, std::io::Error::last_os_error())) }
    }
    #[cfg(not(unix))] {
        let _ = (path, owner, group);
        Err(jet_std::IOError::other(jet_std::IOOperation::Write, None, "chown is unavailable on this platform"))
    }
}
fn jet_files8_symlink_directory(path: &String) -> Result<bool, jet_std::IOError> {
    std::fs::metadata(path).map(|metadata| metadata.is_dir())
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))
}
fn jet_files8_symlink(from: &String, to: &String, directory: bool) -> Result<(), jet_std::IOError> {
    #[cfg(unix)] {
        let _ = directory;
        std::os::unix::fs::symlink(from, to).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, to, error))
    }
    #[cfg(windows)] {
        let result = if directory { std::os::windows::fs::symlink_dir(from, to) }
            else { std::os::windows::fs::symlink_file(from, to) };
        result.map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, to, error))
    }
    #[cfg(not(any(unix, windows)))] {
        let _ = (from, directory);
        Err(jet_std::IOError::other(jet_std::IOOperation::Write, Some(to.clone()), "symbolic links are unavailable on this platform"))
    }
}
fn jet_files8_temp_file_at(path: String) -> Result<JetTempFileOwner, jet_std::IOError> {
    std::fs::OpenOptions::new().create_new(true).write(true).open(&path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &path, error))?;
    Ok(JetTempFileOwner { path, cleanup: std::rc::Rc::new(()) })
}
