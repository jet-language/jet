# Terminal control API: what Core has and what other libraries offer

Date: 2026-10-30. Card #6315. Dev base abab9666d8, probes run with the
prebuilt dev compiler on o0, o1 and o2 (`$JET_SCRATCH_ROOT/lanes/SurfaceDesign/probes/m6315`,
`m6315b`, `m6315r`).

The owner asked for terminal "commands" that read like
`execute(term.clear_buffer, term.enable_raw_mode, term.enable_alternate_screen, term.hide_cursor)`,
broad and deep enough that no outside library is needed.

## What Core has today

`core.term` (Core/Term/Term.jet) covers line I/O and a little more:

- stdin/stdout/stderr handles with `is_tty` (Term.jet:43, 64, 84),
  `input`, `input_secret`, `readline`, `read_until` (117-155);
- `terminal_width` and `terminal_height` read COLUMNS/LINES, then TIOCGWINSZ
  on fd 0, then fall back to 80x24 (204-218; runtime RouteTerm.jet:199-205);
- `style` and `style_force` wrap text in SGR codes (220-233);
- `read_key` returns one `Key` (236), with the enum at 90-104: no Home, End,
  PageUp, PageDown, Insert, and no Shift or Alt;
- `progress` rewrites one stderr line (245).

`core.tui` (Core/Tui/Tui.jet) holds pure models. It has `TUIEvent` (22), color
profiles and colors (32-43), and styles (217-258). `TUICapabilities` covers
color, unicode, mouse, resize, clipboard and size (58-67); line 82 detects them
from the environment. It also has `display_width` (260), layout, list and table
(287-350).
Nothing in it writes to the terminal.

The runtime has one terminal-mode primitive pair:
`jet_term_mode_enter(raw)` and `jet_term_mode_leave` (RouteTerm.jet:252,
282). The saved mode is a stack in thread-local storage, so a signal
handler or another thread cannot restore it. The only compiler caller is
`input_secret` (Term.jet:126).

## Ratified but not built

- D-TUI-ANSI1 (A): checked `ANSI{"[bold red]..."}` text and helpers such
  as `tui.clearLine()` for cursor, clearing, alternate screen and mouse.
  There is no `ANSI` type and no helper anywhere in Core or the runtime.
- D-TERM-INCREMENTAL1 (A): `term.keys() -> Receiver<Key>`,
  `term.KeyDecoder` and `term.set_escape_wait`, plus the final `Key` enum
  with EscapeEnter, Home and End. Core still has only `read_key`.
- D-CORE-TERMSTDIN1 (A): `is_terminal`. Core still says `is_tty`.
- The `#Live` block is specified to enter no-echo, unbuffered input with a
  restoring guard (spec.md:3527-3529). Sema passes the body through as a
  plain block (Statements.jet:144) and nothing calls the mode primitive.
  Probe `m6315r/during.jet` runs a `#Live` body blocked in `read_key` on a
  pseudo-terminal: `stty -a` shows `icanon` and `echo` still on.

## Pseudo-terminals already exist

D-PROCESS-SESSION1 (A) and D-PROCESS-SESSION2 (D) (syntax-decisions.md:
4541-4553) give `ProcessSpec.terminal(TerminalPolicy{size, mode})`,
`ProcessChild.terminal: TerminalSession?` and `TerminalSession.resize`.
The runtime opens the pair with posix_openpt, TIOCGPTPEER, TIOCSWINSZ and
a cfmakeraw-equivalent (HostSpawn.jet:195-244).

- Probe `m6315/t01` runs `stty size; test -t 0; test -t 1` under a 100x30
  terminal and prints `30 100`, `tty-in`, `tty-out` on every tier.
- Probe `t03` writes `hello` to the child's stdin and reads its lines back
  (`hello`, `got:hello`, `24 80`).
- Probe `t02` shows reading the `child.terminal` field is an internal
  compiler error ("ir.build unlowered: field of ProcessChild"), so resize
  cannot be reached yet. D-CORE-PROCBYTES1's `chunks` is also missing.

These gaps belong to the process-stream card, not to the terminal-command
surface. The 41 test rows that use `pty:` steps are refused by the test
kit itself (Tools/testkit/Steps.jet:564-570).

## Capability map

Each row lists what Jet has, then how the reference libraries spell it.
Prior art: crossterm 0.28 (Rust), termion 4 (Rust), ncurses 6.5,
notcurses 3, tcell 2 with Bubble Tea 1 (Go), blessed 1.20 and
prompt_toolkit 3 (Python).

| Capability | Jet today | Prior art |
|---|---|---|
| Raw mode | internal only (RouteTerm.jet:252) | crossterm `enable_raw_mode()`; termion `into_raw_mode()` guard; ncurses `raw()`; blessed `with term.raw()` |
| Alternate screen | none | crossterm `EnterAlternateScreen` command; termion `AlternateScreen` guard; ncurses `initscr`; Bubble Tea `WithAltScreen` |
| Cursor move, hide, shape, save | none | crossterm `MoveTo`, `Hide`, `SetCursorStyle`, `SavePosition`; ncurses `move`, `curs_set` |
| Cursor position query | none | crossterm `cursor::position()` (DSR 6) |
| Clear screen, line, scrollback | none | crossterm `Clear(ClearType::All/Purge/CurrentLine/...)`; ncurses `clear`, `clrtoeol` |
| Scroll and scroll regions | none | crossterm `ScrollUp`, `ScrollDown`; ncurses `setscrreg`, `scrl` |
| Colors and styles | `style` strings (Term.jet:220); color models (Tui.jet:39) | crossterm `SetForegroundColor`, `SetAttribute`; ncurses color pairs; blessed `term.red` |
| Size | `terminal_width/height` (Term.jet:204) | crossterm `size()`; tcell `Size()` |
| Resize events | model only (Tui.jet:24) | crossterm `Event::Resize`; ncurses `KEY_RESIZE`; tcell `EventResize` |
| Keys | `read_key` (Term.jet:236) | crossterm `KeyEvent` with modifiers; tcell `EventKey` |
| Mouse | capability flag only (Tui.jet:62) | crossterm `EnableMouseCapture` (SGR 1006); ncurses `mousemask` |
| Bracketed paste | none | crossterm `EnableBracketedPaste`, `Event::Paste`; prompt_toolkit paste handling |
| Focus events | model only (Tui.jet:27) | crossterm `EnableFocusChange`, `Event::FocusGained` |
| Synchronized output (mode 2026) | none | crossterm `BeginSynchronizedUpdate`; notcurses uses it per frame |
| Hyperlinks (OSC 8) | none | notcurses `ncplane_putstr` with links; Rich and blessed `term.link` |
| Clipboard (OSC 52) | capability flag only (Tui.jet:64) | crossterm `osc52` via `SetClipboard` (crossterm 0.28 feature) |
| Title | none | crossterm `SetTitle` |
| Images (kitty, sixel, iTerm2) | none | notcurses `ncvisual_blit`; viuer and ratatui-image in Rust |
| Batch, flush once | none | crossterm `queue!` then `flush()`; `execute!` writes and flushes once per call |
| Restore on exit | none (see #Live above) | termion restores on Drop only; ratatui `init`/`restore` add a panic hook; ncurses and notcurses install SIGINT/SIGTERM/SIGQUIT handlers that restore; Bubble Tea restores on panic and on its quit |
| Suspend (Ctrl-Z) | none | ncurses and notcurses restore on SIGTSTP and re-enter on SIGCONT |
| Windows console | none in the native runtime | crossterm turns on ENABLE_VIRTUAL_TERMINAL_PROCESSING, else falls back to WinAPI calls; tcell has a Windows console screen |
| Child on a pseudo-terminal | ratified and spawning works (see above) | portable-pty (Rust), creack/pty (Go), Python `pty.spawn` |

## What the prior art teaches

- A command list written once beats a function per action. crossterm's
  `execute!` and `queue!` take any number of command values and write one
  buffer, so a frame costs one write. A function per action (termion,
  blessed) makes the caller manage flushing.
- Raw mode is not an escape code. It is a terminal setting changed by a
  system call, and its undo is the saved setting. Libraries that model it as
  a command (crossterm) still store the saved setting globally.
- Restore by scope guard alone misses signals and aborts. termion's Drop and
  Python context managers do not run on SIGTERM. ncurses and notcurses
  install signal handlers that write precomputed restore bytes, which is
  safe inside a handler. ratatui added a panic hook for the same reason.
- No process can run code on SIGKILL. A parent that launched the program can
  restore the saved settings when it sees the child die.
- Modern terminals answer a few queries (DSR cursor position, device
  attributes, kitty keyboard flags). D-TERM-INCREMENTAL1 already has Jet
  read one such reply through the shared stdin buffer.

## Language facts that shape the spelling

- A list of different command values through one variadic parameter works
  today when the commands are one enum. `execute(.ClearBuffer,
  .EnableRawMode, .MoveTo(col: 3, row: 4))` and `execute(...frame)` print the
  same result on o0, o1 and o2 (probe `m6315b/t05`).
- A module-level lowercase constant such as `clear_buffer` is E0357
  (constants are ALL_CAPS), so the owner's exact spelling
  `term.clear_buffer` cannot be a constant (probe `m6315/t04`). The enum
  case `.ClearBuffer` keeps the same words.
- `os.atexit` handlers and scope cleanup run at a runtime stop
  (spec.md:1023-1037), and `process.on_signal` arms SIGINT, SIGHUP and
  SIGTERM (Core/Process/Process.jet:177). Neither restores a terminal today.
