// D-IO-TERM1=A: terminal policy shared by AOT, resident JIT, and the
// interpreter. Engines marshal its decisions, frames, and errors; terminal
// detection, stream flushing, prompts, and progress line endings live here.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JetTermSecretError {
    NonTerminal,
    Echo,
    Flush(String),
    Read(String),
}

impl JetTermSecretError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NonTerminal => "secret input needs a terminal".to_string(),
            Self::Echo => "could not disable terminal echo".to_string(),
            Self::Flush(message) | Self::Read(message) => message.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetTermSecretErrorKind {
    InvalidInput,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetTermSecretErrorOperation {
    Read,
    Flush,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JetTermSecretErrorProjection {
    pub(crate) kind: JetTermSecretErrorKind,
    pub(crate) operation: JetTermSecretErrorOperation,
    pub(crate) resource: &'static str,
}

pub(crate) fn jet_term_secret_error_projection(
    error: &JetTermSecretError,
) -> JetTermSecretErrorProjection {
    match error {
        JetTermSecretError::NonTerminal => JetTermSecretErrorProjection {
            kind: JetTermSecretErrorKind::InvalidInput,
            operation: JetTermSecretErrorOperation::Read,
            resource: "stdin",
        },
        JetTermSecretError::Echo => JetTermSecretErrorProjection {
            kind: JetTermSecretErrorKind::Other,
            operation: JetTermSecretErrorOperation::Read,
            resource: "stdin",
        },
        JetTermSecretError::Flush(_) => JetTermSecretErrorProjection {
            kind: JetTermSecretErrorKind::Other,
            operation: JetTermSecretErrorOperation::Flush,
            resource: "stdout",
        },
        JetTermSecretError::Read(_) => JetTermSecretErrorProjection {
            kind: JetTermSecretErrorKind::Other,
            operation: JetTermSecretErrorOperation::Read,
            resource: "stdin",
        },
    }
}

pub(crate) fn jet_term_stdin_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

pub(crate) fn jet_term_stdout_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

pub(crate) fn jet_term_stderr_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

pub(crate) fn jet_term_write_stdout(text: &str, flush: bool) -> std::io::Result<()> {
    jet_term_write_stdout_bytes(text.as_bytes(), flush)
}

pub(crate) fn jet_term_write_stdout_bytes(bytes: &[u8], flush: bool) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    out.write_all(bytes)?;
    if flush || jet_term_stdout_is_terminal() {
        out.flush()?;
    }
    Ok(())
}

pub(crate) fn jet_term_write_stderr(text: &str, flush: bool) -> std::io::Result<()> {
    jet_term_write_stderr_bytes(text.as_bytes(), flush)
}

pub(crate) fn jet_term_write_stderr_bytes(bytes: &[u8], flush: bool) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::io::stderr().lock();
    out.write_all(bytes)?;
    if flush || jet_term_stderr_is_terminal() {
        out.flush()?;
    }
    Ok(())
}

pub(crate) fn jet_term_width(get: impl Fn(&str) -> Option<String>) -> i64 {
    jet_term_positive_env_int(get("COLUMNS"))
        .or_else(|| jet_term_size_from_stty().map(|(width, _)| width))
        .unwrap_or(80)
}

pub(crate) fn jet_term_height(get: impl Fn(&str) -> Option<String>) -> i64 {
    jet_term_positive_env_int(get("LINES"))
        .or_else(|| jet_term_size_from_stty().map(|(_, height)| height))
        .unwrap_or(24)
}

fn jet_term_positive_env_int(value: Option<String>) -> Option<i64> {
    value?.parse::<i64>().ok().filter(|value| *value > 0)
}

fn jet_term_size_from_stty() -> Option<(i64, i64)> {
    let output = std::process::Command::new("stty")
        .arg("size")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let rows = parts.next()?.parse::<i64>().ok()?;
    let columns = parts.next()?.parse::<i64>().ok()?;
    (rows > 0 && columns > 0).then_some((columns, rows))
}

pub(crate) fn jet_term_style_enabled(
    no_color: bool,
    term_is_dumb: bool,
    stdout_is_terminal: bool,
) -> bool {
    !no_color && !term_is_dumb && stdout_is_terminal
}

pub(crate) fn jet_term_style(style: &str, text: &str, enabled: bool) -> String {
    if enabled {
        jet_term_style_force(style, text)
    } else {
        text.to_string()
    }
}

pub(crate) fn jet_term_style_force(style: &str, text: &str) -> String {
    let code = match style {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    };
    code.map_or_else(|| text.to_string(), |code| format!("\x1b[{code}m{text}\x1b[0m"))
}

pub(crate) fn jet_term_secret_preflight(
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> Result<(), JetTermSecretError> {
    (stdin_is_terminal && stdout_is_terminal)
        .then_some(())
        .ok_or(JetTermSecretError::NonTerminal)
}

pub(crate) fn jet_term_input_secret(
    prompt: &str,
    mut write: impl FnMut(&str) -> Result<(), String>,
    mut read: impl FnMut() -> Result<String, String>,
) -> Result<String, JetTermSecretError> {
    jet_term_secret_preflight(
        jet_term_stdin_is_terminal(),
        jet_term_stdout_is_terminal(),
    )?;
    write(prompt).map_err(JetTermSecretError::Flush)?;
    if !jet_term_mode_enter(false) {
        let _ = write("\n");
        return Err(JetTermSecretError::Echo);
    }
    let guard = JetTermModeGuard;
    let secret = read().map_err(JetTermSecretError::Read);
    drop(guard);
    let _ = write("\n");
    let mut secret = secret?;
    jet_term_trim_line(&mut secret);
    Ok(secret)
}

struct JetTermModeGuard;

impl Drop for JetTermModeGuard {
    fn drop(&mut self) {
        jet_term_mode_leave();
    }
}

pub(crate) fn jet_term_confirm_prompt(prompt: &str) -> String {
    format!("{prompt} [y/N] ")
}

pub(crate) fn jet_term_confirm_with_io<E>(
    prompt: &str,
    mut write: impl FnMut(&str) -> Result<(), E>,
    mut read: impl FnMut() -> Result<String, E>,
) -> bool {
    let prompt = jet_term_confirm_prompt(prompt);
    let answer = write(&prompt)
        .and_then(|_| read())
        .unwrap_or_default();
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    )
}

pub(crate) fn jet_term_choose_menu(prompt: &str, items: &[String]) -> String {
    let mut menu = format!("{prompt}\n");
    for (index, item) in items.iter().enumerate() {
        menu.push_str(&format!("  {}) {item}\n", index + 1));
    }
    menu
}

pub(crate) fn jet_term_choose_invalid(item_count: usize) -> String {
    format!("Enter a number from 1 to {item_count}.\n")
}

pub(crate) fn jet_term_choose_with_io<E>(
    prompt: &str,
    items: &[String],
    mut write: impl FnMut(&str) -> Result<(), E>,
    mut read: impl FnMut() -> Result<String, E>,
    empty_error: impl FnOnce() -> E,
) -> Result<String, E> {
    if items.is_empty() {
        return Err(empty_error());
    }
    write(&jet_term_choose_menu(prompt, items))?;
    loop {
        let answer = write("> ").and_then(|_| read())?;
        if let Ok(index) = answer.trim().parse::<usize>() {
            if let Some(item) = index.checked_sub(1).and_then(|index| items.get(index)) {
                return Ok(item.clone());
            }
        }
        write(&jet_term_choose_invalid(items.len()))?;
    }
}

pub(crate) fn jet_term_choose_empty_error() -> &'static str {
    "choose needs at least one item"
}

pub(crate) fn jet_term_trim_line(line: &mut String) {
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
}

pub(crate) fn jet_term_progress_frame(is_terminal: bool, text: &str) -> String {
    if is_terminal {
        format!("\r{text}")
    } else {
        format!("{text}\n")
    }
}

pub(crate) fn jet_term_progress_finish(is_terminal: bool) -> &'static str {
    if is_terminal { "\n" } else { "" }
}

#[cfg(unix)]
mod jet_term_mode {
    const TCSANOW: i32 = 0;
    const ECHO: u32 = 0o0000010;
    const ICANON: u32 = 0o0000002;
    const VMIN: usize = 6;
    const VTIME: usize = 5;

    #[repr(C)]
    struct Termios {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        #[cfg(target_os = "linux")]
        c_line: u8,
        c_cc: [u8; 32],
        #[cfg(target_os = "linux")]
        c_ispeed: u32,
        #[cfg(target_os = "linux")]
        c_ospeed: u32,
        #[cfg(not(target_os = "linux"))]
        _pad: [u8; 12],
    }

    unsafe extern "C" {
        fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        fn tcsetattr(fd: i32, optional_actions: i32, termios: *const Termios) -> i32;
    }

    std::thread_local! {
        static SAVED: std::cell::RefCell<Vec<Termios>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(super) fn enter(raw: bool) -> bool {
        unsafe {
            let mut mode = std::mem::zeroed::<Termios>();
            if tcgetattr(0, &mut mode) != 0 {
                return false;
            }
            let saved_mode = std::mem::transmute_copy(&mode);
            mode.c_lflag &= !ECHO;
            if raw {
                mode.c_lflag &= !ICANON;
                mode.c_cc[VMIN] = 1;
                mode.c_cc[VTIME] = 0;
            }
            if tcsetattr(0, TCSANOW, &mode) != 0 {
                return false;
            }
            SAVED.with(|saved| saved.borrow_mut().push(saved_mode));
            true
        }
    }

    pub(super) fn leave() {
        unsafe {
            SAVED.with(|saved| {
                if let Some(mode) = saved.borrow_mut().pop() {
                    tcsetattr(0, TCSANOW, &mode);
                }
            });
        }
    }
}

#[cfg(windows)]
mod jet_term_mode {
    unsafe extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn GetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, dwMode: u32) -> i32;
    }

    const STD_INPUT_HANDLE: u32 = 0xFFFFFFF6u32;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_LINE_INPUT: u32 = 0x0002;

    std::thread_local! {
        static SAVED: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(super) fn enter(raw: bool) -> bool {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) == 0 {
                return false;
            }
            let mut next = mode & !ENABLE_ECHO_INPUT;
            if raw {
                next &= !ENABLE_LINE_INPUT;
            }
            if SetConsoleMode(handle, next) == 0 {
                return false;
            }
            SAVED.with(|saved| saved.borrow_mut().push(mode));
            true
        }
    }

    pub(super) fn leave() {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            SAVED.with(|saved| {
                if let Some(mode) = saved.borrow_mut().pop() {
                    SetConsoleMode(handle, mode);
                }
            });
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod jet_term_mode {
    pub(super) fn enter(_raw: bool) -> bool {
        false
    }

    pub(super) fn leave() {}
}

pub(crate) fn jet_term_mode_enter(raw: bool) -> bool {
    jet_term_mode::enter(raw)
}

pub(crate) fn jet_term_mode_leave() {
    jet_term_mode::leave();
}
