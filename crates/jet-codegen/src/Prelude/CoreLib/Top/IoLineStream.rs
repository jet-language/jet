// #1480: line/byte stdin primitives, split out of
// FSIoEnvOsTesting.rs so the JIT host (`crates/jet-jit/src/IO.rs`) can
// `include!` this exact source instead of re-encoding the logic in Rust a
// second time (I9 — semantics live only in Prelude/**; JIT hosts marshal
// args/results and call the same Prelude function).
fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IOError> {
    if let Some(p) = prompt {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
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
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
            .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Flush, None, e))?;
    }
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), e))?;
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    Ok(s)
}

// #1480: free-function spellings the Core surface ledger scores against peers.
fn jet_std_io_readline() -> Result<String, jet_std::IOError> {
    jet_std_io_input(None)
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
