// Filesystem write and binary terminal operations shared by AOT and the
// interpreter. The surrounding engine supplies `jet_std`,
// `jet_fault_should_fail`, and the raw path kernel.

pub(crate) fn jet_std_fs_write_bytes(path: &str, bytes: &Vec<u8>,) -> Result<(), jet_std::IOError> {
    if let Some(error) = jet_std::jet_std_files_writer_refusal(path) {
        return Err(error);
    }
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::write(path, bytes)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_std_fs_write_at(path: &str, offset: i64,
bytes: &Vec<u8>,) -> Result<(), jet_std::IOError> {
    if let Some(error) = jet_std::jet_std_files_writer_refusal(path) {
        return Err(error);
    }
    use std::io::{Seek, SeekFrom, Write};
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.to_owned()),
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

pub(crate) fn jet_std_fs_write_atomic(path: &str, bytes: &Vec<u8>,) -> Result<(), jet_std::IOError> {
    if let Some(error) = jet_std::jet_std_files_writer_refusal(path) {
        return Err(error);
    }
    use std::io::Write;

    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.to_owned()),
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
            Some(path.to_owned()),
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


// Temporary host leaves for source whole-file policy. These perform only the
// existing Rust I/O operations and typed OS-error projection; no fault rail,
// clamping, UTF-8 policy, mapped-writer refusal, or filesystem composition.
// Replaced by Jet runtime layer L7, dies at cutover (CoreFiles1-2 ledger).
pub(crate) fn jet_files_read_bytes_os(path: &str) -> Result<Vec<u8>, jet_std::IOError> {
    std::fs::read(path).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))
}

pub(crate) fn jet_files_write_bytes_os(path: &str, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
    std::fs::write(path, bytes).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_files_append_bytes_os(path: &str, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    file.write_all(bytes).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_files_create_dirs_os(path: &str) -> Result<(), jet_std::IOError> {
    std::fs::create_dir_all(path).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_files_read_at_os(path: &str, offset: i64, count: i64) -> Result<Vec<u8>, jet_std::IOError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))?;
    let mut bytes = vec![0u8; count as usize];
    let count = file.read(&mut bytes)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))?;
    bytes.truncate(count);
    Ok(bytes)
}

pub(crate) fn jet_files_write_at_os(path: &str, offset: i64, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new().create(true).write(true).open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    file.write_all(bytes).map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))
}

pub(crate) fn jet_files_read_link_os(path: &str) -> Result<String, jet_std::IOError> {
    std::fs::read_link(path).map(|target| target.to_string_lossy().into_owned())
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))
}
