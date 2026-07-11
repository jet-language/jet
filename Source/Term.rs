//! Raw-mode terminal control — shared by the interactive hybrid REPL
//! (D-FE-REPL1=D) and the `jet ?` hybrid help app (D-FE-HELP1=D). One
//! mechanism (I8): both raw-mode TTY apps in this compiler go through this
//! module rather than each shelling out to `stty` on their own.
//!
//! I6: zero external crates — no `termios`/`crossterm`/`libc` crate. Raw mode
//! is toggled by shelling out to the `stty` binary (present on every Unix the
//! dev shell targets); key decoding is plain byte parsing over `std::io`.
//! When stdin/stdout aren't both a TTY, or `stty` isn't available, `enable()`
//! returns `None` and the caller falls back to its non-interactive floor
//! (REPL: `Source/REPL/mod.rs::run_cooked`; help: the static/query palette).

use std::io::{self, IsTerminal, Read};
use std::process::{Command, Stdio};

/// RAII guard: puts the controlling terminal into raw mode and restores the
/// saved `stty -g` state on drop — including on panic (this workspace does
/// not set `panic = "abort"`, so unwinding still runs `Drop`). Must never be
/// held across `std::process::exit` (which skips destructors); callers that
/// need to exit the process do so only after this guard has already dropped.
pub struct RawGuard {
    saved: String,
    restored: bool,
}

impl RawGuard {
    /// `min 0 time 1`: each read blocks up to 100ms for the *first* byte of a
    /// key, then returns immediately once bytes start arriving. This is what
    /// lets `KeyReader` tell a bare Escape keypress (timeout, no continuation
    /// byte) apart from the start of an arrow-key escape sequence, without
    /// ever hanging the loop waiting on a byte that will never come.
    const RAW_ARGS: &'static [&'static str] = &["-icanon", "-echo", "-isig", "min", "0", "time", "1"];

    /// `None` when stdin/stdout aren't both a real terminal, or `stty` isn't
    /// available/failed — the caller should use the cooked fallback.
    pub fn enable() -> Option<RawGuard> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return None;
        }
        // `Command::output()` defaults stdin to `Stdio::null()` — `stty`
        // reads/sets the terminal attached to ITS OWN stdin, so without an
        // explicit inherit here `stty -g` always fails with ENOTTY
        // ("Inappropriate ioctl for device"), even from a real terminal.
        let saved_out = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()
            .ok()?;
        if !saved_out.status.success() {
            return None;
        }
        let saved = String::from_utf8_lossy(&saved_out.stdout).trim().to_string();
        if saved.is_empty() {
            return None;
        }
        let ok = Command::new("stty")
            .args(Self::RAW_ARGS)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            let _ = Command::new("stty").arg(&saved).status();
            return None;
        }
        Some(RawGuard { saved, restored: false })
    }

    /// Restore saved terminal state before an orderly process exit. `Drop`
    /// cannot run after `process::exit`, so consequence-gated REPL exit calls
    /// this explicitly first.
    pub fn restore_now(&mut self) {
        if !self.restored {
            let _ = Command::new("stty").arg(&self.saved).status();
            self.restored = true;
        }
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        self.restore_now();
    }
}

/// One decoded input event.
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// F1 (SS3 form, `ESC O P` — the common raw/xterm encoding).
    F1,
    CtrlB,
    CtrlP,
    CtrlF,
    CtrlR,
    CtrlC,
    /// End of input (Ctrl-D on an empty line, or the stream closed).
    Eof,
    /// A byte/sequence we didn't recognize — swallowed by callers.
    Unknown,
    /// `min 0 time 1` timeout with nothing typed yet — not a real key, just
    /// "keep waiting" (lets the interactive loop redraw a blinking ghost,
    /// poll for resize, etc. without ever busy-spinning the CPU).
    Idle,
}

/// Byte-at-a-time raw stdin reader with ANSI escape-sequence decoding.
pub struct KeyReader<R: Read> {
    stdin: R,
}

impl<R: Read> KeyReader<R> {
    pub fn new(stdin: R) -> Self {
        KeyReader { stdin }
    }

    fn read_byte(&mut self) -> Option<u8> {
        let mut b = [0u8; 1];
        match self.stdin.read(&mut b) {
            Ok(0) => None,
            Ok(_) => Some(b[0]),
            Err(_) => None,
        }
    }

    /// Read exactly one key event. Returns `Key::Idle` on a `min 0 time 1`
    /// timeout (no byte arrived) rather than blocking forever — callers loop
    /// on `Idle` themselves so a bare Escape press can be told apart from the
    /// start of an escape sequence (see `read_escape`).
    pub fn read_key(&mut self) -> Key {
        let Some(b0) = self.read_byte() else {
            return Key::Idle;
        };
        match b0 {
            0x02 => Key::CtrlB,
            0x03 => Key::CtrlC,
            0x04 => Key::Eof,
            0x06 => Key::CtrlF,
            0x09 => Key::Tab,
            0x0d | 0x0a => Key::Enter,
            0x10 => Key::CtrlP,
            0x12 => Key::CtrlR,
            0x7f | 0x08 => Key::Backspace,
            0x1b => self.read_escape(),
            b if b < 0x20 => Key::Unknown,
            b if b < 0x80 => Key::Char(b as char),
            b => self.read_utf8_char(b),
        }
    }

    /// Called right after consuming the leading `0x1b`. A timeout here (no
    /// second byte within 100ms) means the user pressed a bare Escape.
    fn read_escape(&mut self) -> Key {
        let Some(b1) = self.read_byte() else {
            return Key::Escape;
        };
        if b1 != b'[' && b1 != b'O' {
            return Key::Unknown;
        }
        let Some(b2) = self.read_byte() else {
            return Key::Unknown;
        };
        match b2 {
            b'A' => Key::Up,
            b'B' => Key::Down,
            b'C' => Key::Right,
            b'D' => Key::Left,
            b'H' => Key::Home,
            b'F' => Key::End,
            b'P' if b1 == b'O' => Key::F1,
            b'1' | b'7' => {
                self.read_byte(); // trailing `~`
                Key::Home
            }
            b'4' | b'8' => {
                self.read_byte();
                Key::End
            }
            b'3' => {
                self.read_byte();
                Key::Delete
            }
            _ => Key::Unknown,
        }
    }

    fn read_utf8_char(&mut self, b0: u8) -> Key {
        let extra = if b0 >= 0xF0 {
            3
        } else if b0 >= 0xE0 {
            2
        } else {
            1
        };
        let mut buf = vec![b0];
        for _ in 0..extra {
            match self.read_byte() {
                Some(b) => buf.push(b),
                None => break,
            }
        }
        match std::str::from_utf8(&buf) {
            Ok(s) => s.chars().next().map(Key::Char).unwrap_or(Key::Unknown),
            Err(_) => Key::Unknown,
        }
    }
}

/// Terminal column width, via `stty size` (falls back to 80 — the same
/// fallback the codegen prelude's `core.io.terminal_width()` uses).
pub fn terminal_width() -> usize {
    Command::new("stty")
        .arg("size")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let mut parts = s.split_whitespace();
            parts.next()?; // rows
            parts.next()?.parse::<usize>().ok() // cols
        })
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn decodes_plain_char_and_enter() {
        let mut r = KeyReader::new(Cursor::new(b"a\r".to_vec()));
        assert_eq!(r.read_key(), Key::Char('a'));
        assert_eq!(r.read_key(), Key::Enter);
    }

    #[test]
    fn decodes_control_keys() {
        let mut r = KeyReader::new(Cursor::new(vec![0x02, 0x10, 0x06, 0x12]));
        assert_eq!(r.read_key(), Key::CtrlB);
        assert_eq!(r.read_key(), Key::CtrlP);
        assert_eq!(r.read_key(), Key::CtrlF);
        assert_eq!(r.read_key(), Key::CtrlR);
    }

    #[test]
    fn decodes_arrow_escape_sequences() {
        let mut r = KeyReader::new(Cursor::new(b"\x1b[A\x1b[B\x1b[C\x1b[D".to_vec()));
        assert_eq!(r.read_key(), Key::Up);
        assert_eq!(r.read_key(), Key::Down);
        assert_eq!(r.read_key(), Key::Right);
        assert_eq!(r.read_key(), Key::Left);
    }

    #[test]
    fn decodes_f1_ss3_escape() {
        let mut r = KeyReader::new(Cursor::new(b"\x1bOP".to_vec()));
        assert_eq!(r.read_key(), Key::F1);
    }

    #[test]
    fn bare_escape_with_no_continuation_is_escape_not_a_hang() {
        let mut r = KeyReader::new(Cursor::new(vec![0x1b]));
        assert_eq!(r.read_key(), Key::Escape);
    }

    #[test]
    fn decodes_multibyte_utf8_char() {
        let mut r = KeyReader::new(Cursor::new("é".as_bytes().to_vec()));
        assert_eq!(r.read_key(), Key::Char('é'));
    }

    #[test]
    fn empty_stream_is_idle_then_eof_is_distinct() {
        let mut r = KeyReader::new(Cursor::new(Vec::<u8>::new()));
        // A Cursor over an empty Vec reports Ok(0) immediately, same as a
        // closed pipe — both surface as Idle here (`read_byte` can't tell a
        // true EOF from a 0-byte read in the `min 0 time 1` framing); a real
        // Ctrl-D (0x04) is decoded explicitly as `Key::Eof` above.
        assert_eq!(r.read_key(), Key::Idle);
    }
}
