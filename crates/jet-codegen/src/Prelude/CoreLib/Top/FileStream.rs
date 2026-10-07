// Streaming file operations shared by AOT, JIT, and the interpreter.
//
// The surrounding engine supplies `JetFileReader`, `JetFileWriter`, `jet_std`,
// `jet_fault_should_fail`, and the raw `jet_fs_*` kernels. This fragment owns
// the operation policy and line trimming once, so a default-tier deopt calls
// the same functions as generated AOT code.

pub(crate) fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IOError> {
    jet_std_files_open_reader(-1, path, 0)
}

pub(crate) fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IOError> {
    jet_std_files_open_writer(-1, path, false, 0, -1, 0)
}

pub(crate) fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IOError> {
    jet_std_files_open_writer(-1, path, true, 0, -1, 0)
}

/// D-CORE-FILEOPEN1 / D-CORE-FILEDIR1: open a reader at `name` relative to a
/// held directory key (negative: a path). `links`: 0 follow, 1 refuse.
pub(crate) fn jet_std_files_open_reader(
    handle: i64,
    name: &String,
    links: i64,
) -> Result<JetFileReader, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(name.clone()),
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
                Some(name.clone()),
                None,
                Some("not a regular file; use open_directory for a directory".to_string()),
            )));
        }
    }
    Ok(JetFileReader {
        inner: std::io::BufReader::new(file),
        path: name.clone(),
    })
}

/// D-CORE-FILEOPEN1 / D-CORE-FILEDIR1: open a writer. `append` keeps existing
/// bytes; otherwise the file is truncated. `if_exists`: 0 replace, 1 refuse.
/// `mode` < 0 keeps the default creation mode. `links`: 0 follow, 1 refuse.
pub(crate) fn jet_std_files_open_writer(
    handle: i64,
    name: &String,
    append: bool,
    if_exists: i64,
    mode: i64,
    links: i64,
) -> Result<JetFileWriter, jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(name.clone()),
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
        inner: std::io::BufWriter::new(file),
        path: name.clone(),
    })
}

pub(crate) fn jet_std_file_reader_path(reader: &JetFileReader) -> String {
    reader.path.clone()
}

pub(crate) fn jet_std_file_reader_read_line(
    reader: &mut JetFileReader,
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
    match reader.inner.read_line(&mut line) {
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

pub(crate) fn jet_std_file_writer_write_line(
    writer: &mut JetFileWriter,
    line: &String,
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
        .write_all(line.as_bytes())
        .and_then(|_| writer.inner.write_all(b"\n"))
        .map_err(|error| {
            jet_std::io_error_at(jet_std::IOOperation::Write, &writer.path, error)
        })
}

pub(crate) fn jet_std_file_writer_flush(
    writer: &mut JetFileWriter,
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
        .flush()
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Flush, &writer.path, error))
}

pub(crate) fn jet_std_file_writer_path(writer: &JetFileWriter) -> String {
    writer.path.clone()
}
