// Streaming file operations shared by AOT, JIT, and the interpreter.
//
// The surrounding engine supplies `JetFileReader`, `JetFileWriter`, `jet_std`,
// `jet_fault_should_fail`, and the raw `jet_fs_*` kernels. This fragment owns
// the operation policy and line trimming once, so a default-tier deopt calls
// the same functions as generated AOT code. Each handle keeps its stream
// behind its own lock, so every operation takes the handle by `&`.

pub(crate) fn jet_std_files_open(path: &str) -> Result<JetFileReader, jet_std::IOError> {
    jet_std_files_open_reader(-1, path, 0)
}

pub(crate) fn jet_std_files_create(path: &str) -> Result<JetFileWriter, jet_std::IOError> {
    jet_std_files_open_writer(-1, path, false, 0, -1, 0)
}

pub(crate) fn jet_std_files_append(path: &str) -> Result<JetFileWriter, jet_std::IOError> {
    jet_std_files_open_writer(-1, path, true, 0, -1, 0)
}

/// D-CORE-FILEOPEN1 / D-CORE-FILEDIR1: open a reader at `name` relative to a
/// held directory key (negative: a path). `links`: 0 follow, 1 refuse.
pub(crate) fn jet_std_files_open_reader(handle: i64, name: &str, links: i64,) -> Result<JetFileReader, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(name.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    let file = jet_std::jet_fs_open_at(handle, name, 0, 0, -1, links)?;
    // A held-directory member must be a regular file; a path keeps the
    // historical "open whatever the path names" rule.
    if handle >= 0 {
        let meta = file
            .metadata()
            .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, name, error))?;
        if !meta.is_file() {
            return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Read,
                Some(name.to_owned()),
                None,
                Some("not a regular file; use open_directory for a directory".to_string()),
            )));
        }
    }
    Ok(JetFileReader {
        inner: std::sync::Mutex::new(std::io::BufReader::new(file)),
        path: name.to_owned(),
    })
}

/// `core.files.jet_std_files_open_reader_nonblocking`: compiler-private raw leaf (one host step,
/// no policy). Opens the path for reading without waiting on a FIFO or device
/// peer; the caller stats the opened reader and reads it with `read(max)`.
pub(crate) fn jet_std_files_open_reader_nonblocking(
    path: &str,
) -> Result<JetFileReader, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.to_owned()),
            "fault injected: FS.Read",
        ));
    }
    let file = jet_std::jet_fs_open_nonblocking(path)?;
    Ok(JetFileReader {
        inner: std::sync::Mutex::new(std::io::BufReader::new(file)),
        path: path.to_owned(),
    })
}

/// D-CORE-FILEOPEN1 / D-CORE-FILEDIR1: open a writer. `append` keeps existing
/// bytes; otherwise the file is truncated. `if_exists`: 0 replace, 1 refuse.
/// `mode` < 0 keeps the default creation mode. `links`: 0 follow, 1 refuse.
pub(crate) fn jet_std_files_open_writer(handle: i64, name: &str, append: bool,
if_exists: i64,
mode: i64,
links: i64,) -> Result<JetFileWriter, jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(name.to_owned()),
            "fault injected: FS.Write",
        ));
    }
    if handle < 0 {
        if let Some(error) = jet_std::jet_std_files_writer_refusal(name) {
            return Err(error);
        }
    }
    let access = if append { 2 } else { 1 };
    let file = jet_std::jet_fs_open_at(handle, name, access, if_exists, mode, links)?;
    Ok(JetFileWriter {
        inner: std::sync::Mutex::new(std::io::BufWriter::new(file)),
        path: name.to_owned(),
    })
}

pub(crate) fn jet_std_file_reader_path(reader: &JetFileReader) -> String {
    reader.path.clone()
}

pub(crate) fn jet_std_file_reader_read_line(
    reader: &JetFileReader,
) -> Result<Option<String>, jet_std::IOError> {
    use std::io::BufRead;

    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(reader.path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let mut line = String::new();
    let read = reader
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .read_line(&mut line);
    match read {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(error) => Err(jet_std::io_error_at(
            jet_std::IOOperation::Read,
            &reader.path,
            error,
        )),
    }
}

pub(crate) fn jet_std_file_writer_write_line(writer: &JetFileWriter, line: &str, ) -> Result<(), jet_std::IOError> {
    use std::io::Write;

    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(writer.path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let mut stream = writer.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|error| {
            jet_std::io_error_at(jet_std::IOOperation::Write, &writer.path, error)
        })
}

pub(crate) fn jet_std_file_writer_flush(
    writer: &JetFileWriter,
) -> Result<(), jet_std::IOError> {
    use std::io::Write;

    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(writer.path.clone()),
            "fault injected: FS.Write",
        ));
    }
    writer
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .flush()
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Flush, &writer.path, error))
}

pub(crate) fn jet_std_file_writer_path(writer: &JetFileWriter) -> String {
    writer.path.clone()
}

/// The largest count one `FileReader.read` call asks the system for; a larger
/// `max` reads at most this many bytes per call.
const JET_FILE_READ_CHUNK_LIMIT: usize = 1 << 24;

/// #3593: read up to `max` bytes from the reader's current position. Bytes a
/// previous `read_line` buffered come first. An empty result is end of file.
/// `max` is the checked exact Int; a count beyond the native word saturates.
pub(crate) fn jet_std_file_reader_read(
    reader: &JetFileReader,
    max: jet_foundation::Numeric::JetInt,
) -> Result<Vec<u8>, jet_std::IOError> {
    use std::io::Read;

    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(reader.path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let max = max.to_i64().unwrap_or(if max < jet_foundation::Numeric::JetInt::from_i64(0) {
        i64::MIN
    } else {
        i64::MAX
    });
    if max <= 0 {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Read,
            Some(reader.path.clone()),
            None,
            Some(format!("read needs a positive byte count, got {max}")),
        )));
    }
    let want = usize::try_from(max).unwrap_or(usize::MAX).min(JET_FILE_READ_CHUNK_LIMIT);
    let mut bytes = vec![0u8; want];
    let mut stream = reader.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        match stream.read(&mut bytes) {
            Ok(count) => {
                bytes.truncate(count);
                return Ok(bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(jet_std::io_error_at(jet_std::IOOperation::Read, &reader.path, error))
            }
        }
    }
}

/// #3593 / D-CORE-FILESTAT1: metadata of the open file (fstat), as the row
/// Core's `Stat` constructor reads.
pub(crate) fn jet_std_file_reader_stat(reader: &JetFileReader) -> Result<Vec<i64>, jet_std::IOError> {
    let stream = reader.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    jet_std::jet_fs_stat_file(stream.get_ref(), &reader.path)
}

/// #3593: write every byte of `bytes` (buffered; `flush`/`fsync` push them out).
pub(crate) fn jet_std_file_writer_write_all(
    writer: &JetFileWriter,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IOError> {
    use std::io::Write;

    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(writer.path.clone()),
            "fault injected: FS.Write",
        ));
    }
    writer
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .write_all(bytes)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &writer.path, error))
}

/// #3593: flush buffered bytes, then make the file's data and metadata durable
/// (fsync).
pub(crate) fn jet_std_file_writer_fsync(writer: &JetFileWriter) -> Result<(), jet_std::IOError> {
    use std::io::Write;

    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(writer.path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let mut stream = writer.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    stream
        .flush()
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Flush, &writer.path, error))?;
    stream
        .get_ref()
        .sync_all()
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Flush, &writer.path, error))
}

/// #3593 / D-CORE-FILESTAT1: metadata of the open file (fstat). Bytes still
/// buffered by `write_all`/`write_line` are not counted until a flush.
pub(crate) fn jet_std_file_writer_stat(writer: &JetFileWriter) -> Result<Vec<i64>, jet_std::IOError> {
    let stream = writer.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    jet_std::jet_fs_stat_file(stream.get_ref(), &writer.path)
}

/// #3593: set the open file's permission bits (fchmod). Windows keeps only the
/// read-only flag: it is set when no write bit is present.
pub(crate) fn jet_std_file_writer_set_mode(writer: &JetFileWriter, mode: i64) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(writer.path.clone()),
            "fault injected: FS.Write",
        ));
    }
    if !(0..=0o7777).contains(&mode) {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Write,
            Some(writer.path.clone()),
            None,
            Some(format!("mode {mode:#o} is outside 0o0..0o7777")),
        )));
    }
    let stream = writer.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = stream.get_ref();
    let mut permissions = file
        .metadata()
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &writer.path, error))?
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(mode as u32);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(mode & 0o222 == 0);
    file.set_permissions(permissions)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, &writer.path, error))
}

pub(crate) fn jet_files_link_reader_at_raw(source: &JetFileReader, into: i64, name: &str, ) -> (bool, bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    let stream = source.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    jet_std::jet_fs_link_fd_at_raw(stream.get_ref(), into, name)
}

// Temporary OS buffered-read leaf for source FileReader.read_line policy.
// Existing BufReader state is reused; no fault, UTF8 or terminator policy.
// Replaced by Jet runtime L7, dies at cutover (CoreFiles1-2 ledger).
pub(crate) fn jet_file_reader_line_bytes_raw(reader: &JetFileReader) -> Result<Vec<u8>, jet_std::IOError> {
    let mut bytes = Vec::new();
    let mut stream = reader.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    std::io::BufRead::read_until(&mut *stream, b'\n', &mut bytes)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, &reader.path, error))?;
    Ok(bytes)
}

