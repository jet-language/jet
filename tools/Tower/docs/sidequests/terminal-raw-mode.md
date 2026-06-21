# Plan: Terminal raw-mode + single-key input (D-TERM1)

**Status: plan — awaiting owner decision D-TERM1.**

Unblocks: **Kofi** (terminal puzzle game — the one persona verdict that is
*blocked*, not merely friction).

---

## Goal

`core.io` is line-based: `input` blocks on a full line, there is no cursor
movement, no color, no single-keystroke polling. A playable terminal game needs
raw mode (read a key without Enter), cursor positioning, and color. The
user-facing goal: a `core.term` (or `jet.term`) surface that puts the terminal in
raw mode, reads one key (incl. arrows), moves the cursor, and writes color —
restoring cooked mode automatically on scope exit.

Verified: no terminal/raw-mode code exists (`grep raw.?mode|terminal Source/` →
nothing). `core.io` is `args/input/read_all_input/eprint` only.

## Pipeline touch points

- **stdlib** (new `core.term` or `jet.term` ring package): raw-mode enter/exit,
  key read, cursor/color escapes. Likely needs an external dep to bootstrap
  (termios/crossterm-style) → **I6 owner approval** required, like regex (c79).
  Native std-only termios FFI is the I6-clean alternative.
- **sema**: register the new module + its methods.
- **codegen** (`Prelude/Std.rs`): helpers over termios (`tcgetattr`/`tcsetattr`)
  or an external crate.
- RAII: raw mode must restore on scope exit — leans on the ratified scope-guard
  (D-DEFER1 / `core.scope.guard`).

## Invariants in play

- **I6** zero external crates in the *compiler*; a stdlib bootstrap dep needs owner
  approval and a native-replacement plan before Epoch 3 ends. Decide native
  termios vs bootstrap crate (this is part of D-TERM1).
- **I1** raw mode is an expert-ish capability but games are a beginner persona —
  the restore-on-exit guard must be automatic so a beginner can't leave the
  terminal wedged.
- **I5** ships an interactive example (or a scripted/golden-able subset).

## Open questions (need owner decision — D-TERM1)

1. **Scope** — minimal v1 = raw mode + single-key read + cursor move + 16 colors,
   or a fuller TUI (alt-screen, mouse, truecolor, resize events)? Game persona
   needs the minimal four; pick the v1 line.
2. **Surface** — a `core.term` module of functions, a `Terminal` handle value
   with methods, or a `raw_mode { … }` scoped block that guarantees restore.
3. **Key model** — return a `Key` enum (`Char(c)`, `Arrow(dir)`, `Enter`, `Esc`,
   `Ctrl(c)`) vs raw bytes. An enum is far more beginner-usable.
4. **Implementation source (I6)** — native termios FFI (std-only, I6-clean) vs
   bootstrap a crossterm-style crate then native-ize (needs I6 waiver).
5. **Color surface** — escape-code helpers vs a styled-string value
   (`"hi".red().bold()`); does it share anything with `jet.log` formatting?

## Test plan

1. `examples/features/term_keys.jet` — read keys in raw mode, echo a `Key` enum
   name per press, quit on `q`. Interactive; provide a scripted-input golden
   variant if the harness can feed keystrokes.
2. Restore test: after the example exits (incl. via panic), the terminal is back
   in cooked mode (guard fired).
3. Color/cursor: write a positioned colored cell, assert the emitted escape
   sequence (unit test on the helper).
