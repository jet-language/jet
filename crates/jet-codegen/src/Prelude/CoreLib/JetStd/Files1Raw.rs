// Temporary OS leaves for CoreFiles1-2 source atomic-write policy.
// Included beside FileHandles.rs in jet_std, reusing its existing held table.
// Reservation, byte I/O, OS permission copying, fsync, and namespace changes
// only. Retry count, naming, checkpoints, cleanup, and ordering are Jet source.
// Replaced by Jet runtime L7; dies at cutover (leaves.d/CoreFiles1-2.tsv).
pub(crate) fn jet_files_atomic_reserve_raw(target: &String, candidate: &String) -> Result<i64, IOError> {
    let file = std::fs::OpenOptions::new().create_new(true).write(true).open(candidate)
        .map_err(|error| io_error_at(IOOperation::Write, target, error))?;
    Ok(jet_fs_hold(file, candidate.clone()))
}

pub(crate) fn jet_files_atomic_finish_raw(handle: i64, bytes: &Vec<u8>, target: &String, candidate: &String) -> Result<(), IOError> {
    use std::io::Write;
    // The old atomic writer captured metadata before the first write. Keep
    // that same snapshot; a missing/unreadable target has no permissions to copy.
    let permissions = std::fs::metadata(target).ok().map(|meta| meta.permissions());
    let mut held = jet_fs_held().lock().unwrap_or_else(|poison| poison.into_inner());
    let entry = held.get_mut(&handle).ok_or_else(|| IOError::Closed(jet_fs_context(
        IOOperation::Write, candidate, None, "the atomic temporary file is already closed")))?;
    entry.file.write_all(bytes).map_err(|error| io_error_at(IOOperation::Write, candidate, error))?;
    if let Some(permissions) = permissions {
        entry.file.set_permissions(permissions).map_err(|error| io_error_at(IOOperation::Write, candidate, error))?;
    }
    entry.file.sync_all().map_err(|error| io_error_at(IOOperation::Flush, candidate, error))
}

pub(crate) fn jet_files_atomic_remove_raw(path: &String) -> Result<(), IOError> {
    std::fs::remove_file(path).map_err(|error| io_error_at(IOOperation::Write, path, error))
}

pub(crate) fn jet_files_atomic_rename_raw(candidate: &String, target: &String) -> Result<(), IOError> {
    std::fs::rename(candidate, target).map_err(|error| io_error_at(IOOperation::Write, target, error))
}

pub(crate) fn jet_files_atomic_sync_parent_raw(parent: &String, target: &String) -> Result<(), IOError> {
    #[cfg(unix)]
    std::fs::File::open(parent).and_then(|directory| directory.sync_all())
        .map_err(|error| io_error_at(IOOperation::Flush, target, error))?;
    #[cfg(not(unix))]
    let _ = (parent, target);
    Ok(())
}

pub(crate) fn jet_files_atomic_pid_raw() -> i64 { i64::from(std::process::id()) }
pub(crate) fn jet_files_path_windows_raw() -> bool { cfg!(windows) }
