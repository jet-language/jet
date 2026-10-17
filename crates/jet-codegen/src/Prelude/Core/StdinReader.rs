// Shared streaming stdin reader for AOT, JIT, and interpreter hosts.
// The owner keeps std::io::Stdin alive across pulls so unread bytes are never
// discarded when a loop stops after a sentinel.  The surrounding host supplies
// `jet_std`, `jet_fault_should_fail`, and its canonical IOError carrier.

pub(crate) struct JetStdinReader {
    inner: std::io::Stdin,
}

pub(crate) fn jet_std_io_stdin() -> JetStdinReader {
    JetStdinReader {
        inner: std::io::stdin(),
    }
}

pub(crate) fn jet_std_io_read_line<R: std::io::BufRead>(
    reader: &mut R,
) -> Result<Option<String>, jet_std::IOError> {
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(error) => Err(jet_std::io_error_at(
            jet_std::IOOperation::Read,
            "stdin",
            error,
        )),
    }
}

pub(crate) fn jet_std_io_stdin_read_line(
    reader: &mut JetStdinReader,
) -> Result<Option<String>, jet_std::IOError> {
    let mut lock = reader.inner.lock();
    jet_std_io_read_line(&mut lock)
}


// Source StdinHandle owns the checkpoint and trimming. std::Stdin's process
// buffer remains shared with input/take/read-until; this never closes fd 0.
// Replaced by RtL7 stdio layer; Rust seed/verification dies at cutover.
pub(crate) fn jet_stdin_line_raw() -> Result<Option<String>, jet_std::IOError> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    let mut line = String::new();
    match lock.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(line)),
        Err(error) => Err(jet_std::io_error_at(jet_std::IOOperation::Read, "stdin", error)),
    }
}
