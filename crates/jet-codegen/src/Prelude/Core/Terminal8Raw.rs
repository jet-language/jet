// OWNER #32 bootstrap host adapters. Terminal policy is Core/term/term.jet.
// Replaced by Jet runtime layer L7; every adapter dies at cutover.
// Included in the same Prelude closure as Term.rs and IoLineStream.rs.
fn jet_terminal8_fault(name: &String) -> bool { jet_fault_should_fail(name) }
fn jet_terminal8_write_stdout(text: &String, flush: bool) -> Option<String> {
    jet_term_write_stdout(text, flush).err().map(|error| error.to_string())
}
fn jet_terminal8_write_stderr(text: &String, flush: bool) -> Option<String> {
    jet_term_write_stderr(text, flush).err().map(|error| error.to_string())
}
fn jet_terminal8_line() -> (bool, String, Option<String>) {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(n) => (n == 0, line, None),
        Err(error) => (false, line, Some(error.to_string())),
    }
}
fn jet_terminal8_read_all() -> (String, Option<String>) {
    use std::io::Read;
    let mut text = String::new();
    let cause = std::io::stdin().read_to_string(&mut text).err().map(|error| error.to_string());
    (text, cause)
}
fn jet_terminal8_take(n: i64) -> (Vec<u8>, Option<String>) {
    use std::io::Read;
    // Source has already rejected negative sizes before probing IO.Read.
    let mut bytes = vec![0u8; n as usize];
    match std::io::stdin().lock().read(&mut bytes) {
        Ok(read) => { bytes.truncate(read); (bytes, None) }
        Err(error) => (bytes, Some(error.to_string())),
    }
}
fn jet_terminal8_read_until(delimiter: &String) -> (String, Option<String>) {
    use std::io::Read;
    // The held stdin lock, delimiter consumption, and UTF-8 read operation are
    // the existing IoLineStream.rs host operation, without policy/fault probes.
    let needle = delimiter.as_bytes();
    let mut stdin = std::io::stdin().lock();
    let mut bytes = Vec::new();
    let mut window = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stdin.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                bytes.push(byte[0]);
                window.push(byte[0]);
                if window.len() > needle.len() { window.remove(0); }
                if window.as_slice() == needle {
                    bytes.truncate(bytes.len() - needle.len());
                    break;
                }
            }
            Err(error) => return (String::new(), Some(error.to_string())),
        }
    }
    match String::from_utf8(bytes) {
        Ok(text) => (text, None),
        Err(error) => (String::new(), Some(error.to_string())),
    }
}
fn jet_terminal8_env(name: &String) -> Option<String> { jet_std_env_get(name) }
fn jet_terminal8_env_has(name: &String) -> bool { jet_env_value_raw(name).is_some() }
fn jet_terminal8_stty() -> Option<(i64, i64)> { jet_term_size_from_stty() }
fn jet_terminal8_color_mode() -> i64 { JET_TERM_COLOR_MODE.with(|mode| i64::from(mode.get())) }
fn jet_terminal8_key_bytes() -> (Vec<u8>, i64) {
    use std::io::Read;
    let platform = if cfg!(unix) { 1 } else if cfg!(windows) { 2 } else { 0 };
    if platform == 0 { return (Vec::new(), 0); }
    let mut bytes = [0u8; 6];
    let read = std::io::stdin().lock().read(&mut bytes).unwrap_or(0);
    (bytes[..read].to_vec(), platform)
}
