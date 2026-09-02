// Filesystem write and binary terminal operations shared by AOT and the
// interpreter. The surrounding engine supplies `jet_std`,
// `jet_fault_should_fail`, and the raw path kernel.

pub(crate) fn jet_std_fs_write_bytes(
    path: &String,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::write(path, bytes)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_std_fs_write_at(
    path: &String,
    offset: i64,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IOError> {
    use std::io::{Seek, SeekFrom, Write};
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    file.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    file.write_all(bytes)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_std_fs_write_atomic(
    path: &String,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IOError> {
    use std::io::Write;

    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let target = std::path::Path::new(path);
    let parent = target.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut temp_path = None;
    let mut temp_file = None;
    for sequence in 0..128u64 {
        let candidate = parent.join(format!(
            ".jet_tmp_{}_{}",
            std::process::id(),
            sequence
        ));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(jet_std::io_error_at(
                    jet_std::IOOperation::Write,
                    path,
                    error,
                ))
            }
        }
    }
    let Some(temp_path) = temp_path else {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "could not reserve an atomic-write temporary file",
        ));
    };
    let mut temp_file = temp_file.expect("atomic temporary file must exist");
    let existing_permissions = std::fs::metadata(target).ok().map(|m| m.permissions());
    if let Err(error) = temp_file.write_all(bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(jet_std::io_error_at(
            jet_std::IOOperation::Write,
            &temp_path.to_string_lossy(),
            error,
        ));
    }
    if let Some(permissions) = existing_permissions {
        if let Err(error) = temp_file.set_permissions(permissions) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(jet_std::io_error_at(
                jet_std::IOOperation::Write,
                &temp_path.to_string_lossy(),
                error,
            ));
        }
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(jet_std::io_error_at(
            jet_std::IOOperation::Flush,
            &temp_path.to_string_lossy(),
            error,
        ));
    }
    drop(temp_file);
    if let Err(error) = std::fs::rename(&temp_path, target) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(jet_std::io_error_at(jet_std::IOOperation::Write, path, error));
    }
    #[cfg(unix)]
    if let Err(error) = std::fs::File::open(parent).and_then(|dir| dir.sync_all()) {
        return Err(jet_std::io_error_at(jet_std::IOOperation::Flush, path, error));
    }
    Ok(())
}

pub(crate) fn jet_std_io_binwrite(
    path: &String,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IOError> {
    jet_std_fs_write_bytes(path, bytes)
}

pub(crate) fn jet_std_io_buffered() -> JetStdinReader {
    jet_std_io_stdin()
}
