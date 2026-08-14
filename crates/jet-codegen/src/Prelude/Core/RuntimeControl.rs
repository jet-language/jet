// ── D-DEFER1 option B: core.scope.guard ──────────────────────────────────────
// A ScopeGuard stores a zero-argument closure and runs it in Drop — on every
// exit path (normal fall-through, early `return`, `?` propagation).
// LIFO ordering is guaranteed by Rust's reverse-declaration drop order.
// Generic over F: avoids boxing and allows non-'static captures. Purely safe.
struct JetScopeGuard<F: FnOnce()> {
    f: Option<F>,
}
fn jet_scope_guard<F: FnOnce()>(f: F) -> JetScopeGuard<F> {
    JetScopeGuard { f: Some(f) }
}
impl<F: FnOnce()> Drop for JetScopeGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.f.take() {
            f();
        }
    }
}
// ── D-TXN1–D-TXN4 + D-TXN-ROLLBACK (2026-06-24/25): #Transact transaction blocks
// A `#Transact(tx) { … }` block lowers to:
//   { let mut tx = jet_transaction(); <body>; tx.commit(); }
//
// Three Drop-backed hook stacks, each LIFO (mirroring scope-guard drop order):
//   • commit hooks   (`tx.on_commit(() => …)`, D-TXN3) — run only if `commit()` ran.
//   • rollback hooks (`tx.on_rollback(() => …)`, D-TXN-ROLLBACK layer 3) — run only
//     if `commit()` did NOT run (a `?`-failure / early return undoes the block).
//   • auto-snapshots (D-TXN-ROLLBACK layer 1) — restore closures captured at the
//     point of mutation; run only if `commit()` did NOT run, restoring the pre-state.
//
// A `?`-failure (or any early return) inside the block skips `commit()`, so on Drop:
//   committed   → commit hooks fire LIFO; rollback hooks + snapshots are dropped un-run.
//   uncommitted → snapshots restore (LIFO) then rollback hooks fire (LIFO); commit
//                 hooks are dropped un-run.
// Purely safe std Rust; no runtime effect machinery (I3).
struct JetTransaction {
    hooks: Vec<Box<dyn FnOnce()>>,
    undo: Vec<Box<dyn FnOnce()>>,
    committed: bool,
}
fn jet_transaction() -> JetTransaction {
    JetTransaction {
        hooks: Vec::new(),
        undo: Vec::new(),
        committed: false,
    }
}
impl JetTransaction {
    fn on_commit(&mut self, f: Box<dyn FnOnce()>) {
        self.hooks.push(f);
    }
    fn on_rollback(&mut self, f: Box<dyn FnOnce()>) {
        self.undo.push(f);
    }
    fn commit(&mut self) {
        self.committed = true;
    }
}
impl Drop for JetTransaction {
    fn drop(&mut self) {
        if self.committed {
            // Clean commit: run commit hooks LIFO; undo stack is dropped un-run.
            while let Some(f) = self.hooks.pop() {
                f();
            }
        } else {
            // Rollback path (`?`-failure / early return): restore auto-snapshots
            // and run explicit rollback hooks, both LIFO; commit hooks drop un-run.
            // `undo` holds both kinds interleaved in registration order, so a single
            // LIFO drain mirrors the source order they were established in.
            while let Some(f) = self.undo.pop() {
                f();
            }
        }
    }
}
// D-TXN-ROLLBACK layer 1 (auto-snapshot): the snapshot/restore mechanism lives in a
// vetted prelude module, mirroring `jet_mem`. `jet_txn_snapshot` clones the
// pre-mutation state of a place and registers a Drop-backed restore on the
// transaction's undo stack; on a `?`-failure the guard's Drop writes the clone back.
// The raw-pointer writeback is sound because the transaction guard is declared after
// the place and dropped before it (LIFO scope teardown), so the place is always live
// when restore runs. The compiler picks WHICH places to snapshot (I3); this module is
// just the dumb runtime. Stripped from the golden memory-safety check like `jet_mem`.
mod jet_txn {
    use super::JetTransaction;
    /// Snapshot `*place` (a `Clone` of its pre-mutation state) and register a restore
    /// closure on `tx`'s undo stack. Restores on rollback; dropped un-run on commit.
    pub(crate) fn snapshot<T: Clone + 'static>(tx: &mut JetTransaction, place: &mut T) {
        let saved = place.clone();
        let raw: *mut T = place;
        tx.on_rollback(Box::new(move || {
            // `raw` points at a local that outlives the transaction guard; the
            // guard's Drop (the caller) runs before that local is dropped.
            let slot: &mut T = unsafe { &mut *raw };
            *slot = saved;
        }));
    }
    /// D-TXN-ROLLBACK layer 2: snapshot a value via its `Rollback` impl instead of
    /// a full `Clone`. The caller captures the snap by calling `place.snapshot()` and
    /// passes it together with the type-erased `restore` function pointer. Sound for
    /// the same reason as `snapshot`: the place outlives the transaction guard (LIFO).
    pub(crate) fn snapshot_custom<T: 'static, S: 'static>(
        tx: &mut JetTransaction,
        place: &mut T,
        snap: S,
        restore: fn(&mut T, S),
    ) {
        let raw: *mut T = place;
        tx.on_rollback(Box::new(move || {
            let slot: &mut T = unsafe { &mut *raw };
            restore(slot, snap);
        }));
    }
}
// ── D-STM1=A (ratified 2026-07-12, card #506): the Shared plane of #Transact ──
// `#Transact(tx) { from.edit(…); to.edit(…) }` on `Shared<T>` handles lowers to:
//   { let mut tx = jet_transaction();
//     let mut __jet_stm = jet_stm::begin();
//     from.edit_txn(&mut __jet_stm, move |b| …);
//     to.edit_txn(&mut __jet_stm, move |b| …);
//     __jet_stm.commit(); tx.commit(); }
//
// Each `edit_txn` defers its payload closure onto the explicit transaction
// guard. The shared Prelude owns participant identity, lock ordering, commit,
// and rollback; this generated module only preserves the emitter's existing
// `jet_stm::begin()` spelling.
mod jet_stm {
    pub(crate) type Guard = crate::JetSharedTransaction;

    pub(crate) fn begin() -> Guard {
        crate::jet_shared_transaction_begin()
    }
}
trait __jet_Serialize {
    fn to_json(&self) -> String;
}

// ── D-TERM1 (ratified 2026-06-22): terminal direct-input primitives ───────────
// `live { … }` blocks in Jet source emit:
//   jet_term_enter();
//   let _live_guard = jet_scope_guard(|| { jet_term_leave(); });
//   <body>
//
// `term.read_key()` emits `jet_term_read_key()`.
//
// I6: zero external crates. Platform-specific setup uses inline `extern "C"` /
// `extern "system"` declarations — standard Rust FFI, not the `libc` crate.
// ──────────────────────────────────────────────────────────────────────────────

/// The key-event type returned by `term.read_key()` (D-TERM1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JetKey {
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

impl JetShow for JetKey {
    fn jet_show(&self) -> String {
        match self {
            JetKey::Char(c) => format!("Char({})", c),
            JetKey::Enter => "Enter".to_string(),
            JetKey::Escape => "Escape".to_string(),
            JetKey::Backspace => "Backspace".to_string(),
            JetKey::Tab => "Tab".to_string(),
            JetKey::Delete => "Delete".to_string(),
            JetKey::Up => "Up".to_string(),
            JetKey::Down => "Down".to_string(),
            JetKey::Left => "Left".to_string(),
            JetKey::Right => "Right".to_string(),
            JetKey::F(n) => format!("F({})", n),
            JetKey::Ctrl(c) => format!("Ctrl({})", c),
            JetKey::Unknown => "Unknown".to_string(),
        }
    }
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
fn jet_term_enter() {
    let _ = jet_term_mode_enter(true);
}

/// Disable terminal echo but keep canonical line editing for secret input.
fn jet_term_enter_secret() -> bool {
    jet_term_mode_enter(false)
}

/// Restore the terminal to the state captured by the most recent `jet_term_enter`.
/// Called by the scope guard that `live { … }` installs.
fn jet_term_leave() {
    jet_term_mode_leave();
}

/// Read one key event from stdin (blocking).
/// Used by `term.read_key()`.
fn jet_term_read_key() -> JetKey {
    #[cfg(unix)]
    return jet_term_unix::read_key();
    #[cfg(windows)]
    return jet_term_windows::read_key();
    #[cfg(not(any(unix, windows)))]
    return JetKey::Unknown;
}
