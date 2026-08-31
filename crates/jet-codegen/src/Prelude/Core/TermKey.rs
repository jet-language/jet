// ── D-TERM1 (ratified 2026-06-22): terminal direct-input primitives ───────────
// The ONE terminal key kernel. AOT embeds this source into the generated
// program; the canonical TIR evaluator (`Codegen/TIR/eval/mod.rs`
// The in-process TIR evaluator and resident Cranelift host call the one
// `jet-codegen::terminal_runtime` instance. AOT embeds this source into its
// generated program. Every execution tier therefore decodes the same bytes
// into the same `Key` and enters/restores the same terminal mode (I9). An
// engine marshals arguments and results; it never re-encodes the decode table
// or raw-mode entry policy.
//
// `live { … }` blocks in Jet source emit:
//   jet_term_enter();
//   let _live_guard = jet_scope_guard(|| { jet_term_leave(); });
//   <body>
//
// `term.read_key()` emits `jet_term_read_key()`.
//
// The raw-mode kernel itself (`jet_term_mode_enter` / `jet_term_mode_leave`)
// lives in `Prelude/Term.rs` beside the rest of the terminal device surface;
// each in-process owner includes both files in one module before this source.
//
// I6: zero external crates. Platform-specific setup uses inline `extern "C"` /
// `extern "system"` declarations — standard Rust FFI, not the `libc` crate.
// ──────────────────────────────────────────────────────────────────────────────

/// The key-event type returned by `term.read_key()` (D-TERM1).
///
/// `impl JetShow for JetKey` stays in `Prelude/Core/RuntimeControl.rs`: the
/// `JetShow` trait is an AOT-only rendering seam, and the in-process engines
/// project a key into their own value carrier instead.
#[derive(Clone, Debug, PartialEq)]
pub enum JetKey {
    /// A printable character.
    Char(char),
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Delete (forward delete).
    Delete,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Function key F1–F12.
    F(i64),
    /// Ctrl + a printable character (e.g. Ctrl-C = Char('\x03')).
    Ctrl(char),
    /// Anything else (bytes we could not parse into a known sequence).
    Unknown,
}

#[cfg(unix)]
mod jet_term_unix {
    use std::io::Read;

    pub fn read_key() -> super::JetKey {
        use super::JetKey;
        let mut buf = [0u8; 6];
        let stdin = std::io::stdin();
        let n = stdin.lock().read(&mut buf).unwrap_or(0);
        if n == 0 {
            return JetKey::Unknown;
        }
        match &buf[..n] {
            [0x0d] | [0x0a] => JetKey::Enter,
            [0x1b] if n == 1 => JetKey::Escape,
            [0x7f] | [0x08] => JetKey::Backspace,
            [0x09] => JetKey::Tab,
            // CSI sequences: ESC [ …
            [0x1b, 0x5b, rest @ ..] => parse_csi(rest),
            // Ctrl + letter: bytes 0x01–0x1a (A–Z).
            [b] if *b >= 1 && *b <= 26 => JetKey::Ctrl((b'a' - 1 + *b) as char),
            [b] if *b < 0x80 => JetKey::Char(*b as char),
            // Multi-byte UTF-8 character.
            bytes => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    if let Some(c) = s.chars().next() {
                        return JetKey::Char(c);
                    }
                }
                JetKey::Unknown
            }
        }
    }

    fn parse_csi(rest: &[u8]) -> super::JetKey {
        use super::JetKey;
        match rest {
            [0x41] => JetKey::Up,
            [0x42] => JetKey::Down,
            [0x43] => JetKey::Right,
            [0x44] => JetKey::Left,
            [0x33, 0x7e] => JetKey::Delete,
            // F1–F4: ESC O P/Q/R/S (VT100) — handled as CSI variant here.
            // F1–F12 numeric: ESC [ 1 1 ~ through ESC [ 2 4 ~
            bytes => {
                // Try numeric Pn ~ form: digits followed by ~.
                if let Some((&0x7e, digits)) = bytes.split_last() {
                    if let Ok(s) = std::str::from_utf8(digits) {
                        if let Ok(n) = s.parse::<i64>() {
                            let fkey = match n {
                                11 => 1,
                                12 => 2,
                                13 => 3,
                                14 => 4,
                                15 => 5,
                                17 => 6,
                                18 => 7,
                                19 => 8,
                                20 => 9,
                                21 => 10,
                                23 => 11,
                                24 => 12,
                                _ => return JetKey::Unknown,
                            };
                            return JetKey::F(fkey);
                        }
                    }
                }
                JetKey::Unknown
            }
        }
    }
}

#[cfg(windows)]
mod jet_term_windows {
    use std::io::Read;

    pub fn read_key() -> super::JetKey {
        use super::JetKey;
        let mut buf = [0u8; 6];
        let n = std::io::stdin().lock().read(&mut buf).unwrap_or(0);
        if n == 0 {
            return JetKey::Unknown;
        }
        match &buf[..n] {
            [0x0d] | [0x0a] => JetKey::Enter,
            [0x1b] => JetKey::Escape,
            [0x7f] | [0x08] => JetKey::Backspace,
            [0x09] => JetKey::Tab,
            [0x1b, 0x5b, rest @ ..] => match rest {
                [0x41] => JetKey::Up,
                [0x42] => JetKey::Down,
                [0x43] => JetKey::Right,
                [0x44] => JetKey::Left,
                _ => JetKey::Unknown,
            },
            [b] if *b >= 1 && *b <= 26 => JetKey::Ctrl((b'a' - 1 + *b) as char),
            [b] if *b < 0x80 => JetKey::Char(*b as char),
            bytes => {
                if let Ok(s) = std::str::from_utf8(bytes) {
                    if let Some(c) = s.chars().next() {
                        return JetKey::Char(c);
                    }
                }
                JetKey::Unknown
            }
        }
    }
}

// ── Platform-dispatched entry points ────────────────────────────────────────

/// Enter un-buffered, no-echo terminal input mode.
/// Called at the top of every `live { … }` block.
pub fn jet_term_enter() {
    let _ = jet_term_mode_enter(true);
}

/// Disable terminal echo but keep canonical line editing for secret input.
pub fn jet_term_enter_secret() -> bool {
    jet_term_mode_enter(false)
}

/// Restore the terminal to the state captured by the most recent `jet_term_enter`.
/// Called by the scope guard that `live { … }` installs.
pub fn jet_term_leave() {
    jet_term_mode_leave();
}

/// Read one key event from stdin (blocking).
/// Used by `term.read_key()`.
pub fn jet_term_read_key() -> JetKey {
    #[cfg(unix)]
    return jet_term_unix::read_key();
    #[cfg(windows)]
    return jet_term_windows::read_key();
    #[cfg(not(any(unix, windows)))]
    return JetKey::Unknown;
}
