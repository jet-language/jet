# Jet terminal interaction principles

Durable reference for every Jet-facing terminal surface. Applies to REPL,
help, dev, live inspect, test, build, run, diagnostics, explain, inspect,
debug, notebook terminal clients, and any future TUI.

## 1. Responsiveness

Show first paint quickly. Long work emits semantic progress events. TTY output
may render a live view; pipes receive ordered newline records; JSON receives the
same events in structured form. Never hide a running state behind a batch flush.
Cancellation must stop the active operation or state clearly why it cannot.

Measure edit-to-visible, first-paint, and time-to-final-result separately. Do
not infer responsiveness from source code or a green test.

## 2. Keyboard model

One key has one meaning across interactive Jet surfaces. Required controls:

- `Esc`: close the current transient view or cancel the current choice;
- `Ctrl-C`: interrupt active work or cancel the current input;
- `Ctrl-D`: EOF at an empty prompt;
- `Enter`: accept the current choice;
- `q`: close a pager or live detail view when no text input owns the key.

Every interactive surface prints its controls at first use. Prompt text must
name the next valid action. Unknown keys do not mutate input. Raw mode restores
the terminal on every orderly and error exit.

## 3. Progressive disclosure

Human output starts with the smallest useful summary, then offers detail. Long
lists, diagnostics, provenance, timing, and test output need a visible count and
a deterministic drill-down action. JSON never drops detail to imitate the human
summary.

The REPL fold marker, pin rail, bindings pane, and completion menu are the
current local examples. Future surfaces should reuse the same idea: summary
first, detail on demand, stable labels, no hidden state.

## 4. Color and `NO_COLOR`

Terminal color is semantic, not decorative. All color decisions use the shared
`ColorChoice`/`Theme` policy in `crates/jet-foundation/src/Terminal.rs`.

Resolution order:

1. explicit `--color=always|never`;
2. `NO_COLOR` disables, otherwise `FORCE_COLOR` enables;
3. `auto` enables only for a real terminal stream.

`NO_COLOR=1` output contains no ANSI control bytes. Piped and redirected output
is plain unless the user explicitly requests forced color. Raw escape sequences
belong only in the terminal owner; callers emit semantic text or render roles.

## 5. Resize and narrow terminals

Interactive views query terminal width through one shared helper. A resize
causes redraw from semantic state, not incremental patching of stale cells. If
resize support is unavailable, output degrades to wrapped, line-safe records and
never truncates a diagnostic or corrupts the prompt.

Every new TUI surface gets a `COLUMNS=60` check. Width-sensitive output must
also work when stdout is a pipe and no terminal size exists.

## 6. Raw mode and I6

Raw mode is a small std-only adapter. `stty` shell-out is allowed at the one
terminal boundary; line-editing crates are not. The adapter owns save, enable,
interrupt mode, restore, and failure fallback. Callers never duplicate `stty`
arguments or terminal restoration.

When stdin/stdout are not TTYs, use a cooked or one-shot floor. Never emit cursor
movement, screen clears, progress rewrites, or interactive prompts into a pipe.

## 7. Renderer ownership

Semantic producers provide events, rows, diagnostics, or report records. One
terminal renderer chooses TTY, pipe, JSON, color, width, and quiet behavior.
Do not add a second renderer for a command family. Human and machine forms must
share the same facts.

Diagnostics already establish the desired shape with `render_all_colored`,
`render_all_linked`, and `render_all_json`. Progress and live views should reach
the same one-home model.

## 8. Capture contract

Before calling a terminal surface complete, capture:

- real TTY, normal color;
- real TTY with `NO_COLOR=1`;
- pipe, normal environment;
- pipe with `NO_COLOR=1`;
- `COLUMNS=60` TTY;
- interactive key script covering accept, cancel, EOF, interrupt, and resize.

Store raw transcripts. Trimmed excerpts in an audit must name the exact command,
environment, terminal mode, and key input. Source evidence is not a runtime
capture.

## 9. Source checklist for future cards

Before editing a terminal surface, cite this document and answer:

1. Which semantic events does it emit?
2. Which shared renderer owns TTY, pipe, JSON, color, width, and quiet output?
3. What is first paint and what is cancellation behavior?
4. Which keys are discoverable, and what does `Esc` do?
5. What is summary and what is drill-down?
6. What happens at `COLUMNS=60`, on resize, and without a TTY?
7. Where does raw mode restore on every exit path?

The interaction contract is maintained here; implementation checks belong in
the TUI test fixtures.
