// Streaming file operations shared by AOT, JIT, and the interpreter.
//
// The surrounding engine supplies `JetFileReader`, `JetFileWriter`, `jet_std`,
// `jet_fault_should_fail`, and the raw `jet_fs_*` kernels. This fragment owns
// the operation policy and line trimming once, so a default-tier deopt calls
// the same functions as generated AOT code.

pub(crate) fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let file = jet_fs_open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, path, error))?;
    Ok(JetFileReader {
        inner: std::io::BufReader::new(file),
        path: path.clone(),
    })
}

pub(crate) fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let file = std::fs::File::create(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(file),
        path: path.clone(),
    })
}

pub(crate) fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, path, error))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(file),
        path: path.clone(),
    })
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
