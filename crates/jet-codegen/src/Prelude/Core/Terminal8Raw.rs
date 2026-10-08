// OWNER #32 bootstrap host adapters. Terminal policy is Core/term/term.jet.
// Replaced by Jet runtime layer L7; every adapter dies at cutover.
// Included in the same Prelude closure as Term.rs and IoLineStream.rs.
fn jet_terminal8_fault(name: &str) -> bool {
    jet_fault_should_fail(name)
}
fn jet_terminal8_write_stdout(text: &str, flush: bool) -> Option<String> {
    jet_term_write_stdout(text, flush).err().map(|error| error.to_string())
}
fn jet_terminal8_write_stderr(text: &str, flush: bool) -> Option<String> {
    jet_term_write_stderr(text, flush).err().map(|error| error.to_string())
}
fn jet_terminal8_line() -> (bool, String, Option<String>) {
    match jet_term_read_stdin_line_raw() {
        Ok((eof, line)) => (eof, line, None),
        Err(error) => (false, String::new(), Some(error.to_string())),
    }
}
fn jet_terminal8_read_all() -> (String, Option<String>) {
    match jet_std_io_read_all_raw() {
        Ok(text) => (text, None),
        Err(error) => (String::new(), Some(error.to_string())),
    }
}
fn jet_terminal8_read_until(delimiter: &str) -> (String, Option<String>) {
    match jet_std_io_read_until_raw(delimiter) {
        Ok(text) => (text, None),
        Err(error) => (String::new(), Some(error.to_string())),
    }
}
fn jet_terminal8_take(n: i64) -> (Vec<u8>, Option<String>) {
    match jet_std_io_take_raw(n) {
        Ok(bytes) => (bytes, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    }
}
fn jet_terminal8_env(name: &str) -> Option<String> {
    jet_std_env_get(name)
}
fn jet_terminal8_env_has(name: &str) -> bool {
    jet_env_value_raw(name).is_some()
}
fn jet_terminal8_stty() -> Option<(i64, i64)> { jet_term_size_from_stty() }
fn jet_terminal8_color_mode() -> i64 { jet_term_color_mode_raw() }
fn jet_terminal8_key_bytes() -> (Vec<u8>, i64) {
    // One six-byte read, shared with the existing Unix/Windows key kernels.
    let platform = if cfg!(unix) { 1 } else if cfg!(windows) { 2 } else { 0 };
    if platform == 0 { return (Vec::new(), 0); }
    let (bytes, read) = jet_term_read_key_bytes_raw();
    (bytes[..read].to_vec(), platform)
}
