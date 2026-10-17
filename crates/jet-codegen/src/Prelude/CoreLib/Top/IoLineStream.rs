// #1480: line/byte stdin primitives, split out of
// FSIoEnvOsTesting.rs so the JIT host (`crates/jet-jit/src/IO.rs`) can
// `include!` this exact source instead of re-encoding the logic in Rust a
// second time (I9 — semantics live only in Prelude/**; JIT hosts marshal
// args/results and call the same Prelude function).
fn jet_std_io_input(prompt: Option<&str>) -> Result<String, jet_std::IOError> {
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
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    jet_std_io_read_all_raw().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), error)
    })
}

fn jet_std_io_read_until(delim: &str) -> Result<String, jet_std::IOError> {
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
    jet_std_io_read_until_raw(delim).map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), error)
    })
}

fn jet_std_io_read_all_raw() -> std::io::Result<String> {
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn jet_std_io_read_until_raw(delim: &str) -> std::io::Result<String> {
    use std::io::Read;
    let needle = delim.as_bytes();
    let mut stdin = std::io::stdin().lock();
    let mut out = Vec::new();
    let mut window = Vec::new();
    let mut buf = [0u8; 1];
    loop {
        match stdin.read(&mut buf)? {
            0 => break,
            _ => {
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
        }
    }
    String::from_utf8(out).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn jet_std_io_take(n: i64) -> Result<Vec<u8>, jet_std::IOError> {
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
    jet_std_io_take_raw(n).map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), error)
    })
}

fn jet_std_io_take_raw(n: i64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = vec![0u8; n as usize];
    let read = std::io::stdin().lock().read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}
