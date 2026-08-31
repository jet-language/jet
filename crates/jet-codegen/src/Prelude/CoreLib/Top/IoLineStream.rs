// #1480: line/byte stdin primitives, split out of
// FSIoEnvOsTesting.rs so the JIT host (`crates/jet-jit/src/IO.rs`) can
// `include!` this exact source instead of re-encoding the logic in Rust a
// second time (I9 — semantics live only in Prelude/**; JIT hosts marshal
// args/results and call the same Prelude function).
// D-STDIN1=A: streaming line-by-line stdin.
pub(crate) struct JetStdinReader {
    pub(crate) inner: std::io::BufReader<std::io::Stdin>,
}

pub(crate) fn jet_std_io_stdin() -> JetStdinReader {
    JetStdinReader {
        inner: std::io::BufReader::new(std::io::stdin()),
    }
}

pub(crate) fn jet_std_io_stdin_read_line(
    r: &mut JetStdinReader,
) -> Result<Option<String>, jet_std::IOError> {
    use std::io::BufRead;
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            e,
        )),
    }
}

fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IOError> {
    if let Some(p) = prompt {
        if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
            .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Flush, None, e))?;
    }
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    // #2009: the shared reader owns the line read and the trailing-newline
    // trim. `io.input` answers with text, so a closed stream reads as an empty
    // line here; the prompt kernels take the sharper `JetTermRead` fact.
    let line = jet_term_read_stdin_line().map_err(|e| {
        jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), e)
    })?;
    Ok(jet_term_read_text(line))
}

// #1480: free-function spellings the Core surface ledger scores against peers.
fn jet_std_io_readline() -> Result<String, jet_std::IOError> {
    jet_std_io_input(None)
}

fn jet_std_io_read_all_input() -> Result<String, jet_std::IOError> {
    use std::io::Read;
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Read,
                Some("stdin".to_string()),
                error,
            )
        })?;
    Ok(input)
}

fn jet_std_io_read_until(delim: &String) -> Result<String, jet_std::IOError> {
    use std::io::Read;
    if delim.is_empty() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty delimiter"),
        ));
    }
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let needle = delim.as_bytes();
    let mut stdin = std::io::stdin().lock();
    let mut out = Vec::new();
    let mut window = Vec::new();
    let mut buf = [0u8; 1];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                out.push(buf[0]);
                window.push(buf[0]);
                if window.len() > needle.len() {
                    window.remove(0);
                }
                if window.as_slice() == needle {
                    out.truncate(out.len() - needle.len());
                    break;
                }
            }
            Err(e) => {
                return Err(jet_std::IOError::other(
                    jet_std::IOOperation::Read,
                    Some("stdin".to_string()),
                    e,
                ))
            }
        }
    }
    String::from_utf8(out).map_err(|e| {
        jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        )
    })
}

fn jet_std_io_take(n: i64) -> Result<Vec<u8>, jet_std::IOError> {
    use std::io::Read;
    if n < 0 {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "negative take"),
        ));
    }
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut buf = vec![0u8; n as usize];
    let read = std::io::stdin()
        .lock()
        .read(&mut buf)
        .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), e))?;
    buf.truncate(read);
    Ok(buf)
}
