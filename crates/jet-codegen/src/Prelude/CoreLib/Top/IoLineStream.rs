// #1480: line/byte stdin primitives, split out of
// FSIoEnvOsTesting.rs so the JIT host (`crates/jet-jit/src/IO.rs`) can
// `include!` this exact source instead of re-encoding the logic in Rust a
// second time (I9 — semantics live only in Prelude/**; JIT hosts marshal
// args/results and call the same Prelude function).
fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IOError> {
    if let Some(p) = prompt {
        jet_term_write_stdout(p, true)
            .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Flush, None, e))?;
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
    let mut buf = vec![0u8; n as usize];
    let read = std::io::stdin()
        .lock()
        .read(&mut buf)
        .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), e))?;
    buf.truncate(read);
    Ok(buf)
}
