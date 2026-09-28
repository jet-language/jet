# Jet terminal interaction principles

This is the interaction contract for Jet-facing terminal surfaces: help, REPL,
dev/live inspect, test, build, run, diagnostics, explain, inspect, debug,
notebook clients, and future TUIs. It is for CLI and TUI authors who need one
policy for input, output, color, width, cancellation, and terminal ownership.
The executable seams are [`Terminal.rs`](../../../crates/jet-foundation/src/Terminal.rs),
[`Outcome.rs`](../../../crates/jet-foundation/src/Outcome.rs),
[`OutputProfile.rs`](../../../crates/jet-cli/src/OutputProfile.rs), and the
interactive help implementation under
[`jet-cli/src/Help`](../../../crates/jet-cli/src/Help). TUI fixtures and
surface-specific checks belong beside their implementation; this document
states the shared behavior they must exercise.

## Responsiveness and cancellation

Show a first paint quickly. Long work emits semantic progress events. A TTY may
render a live view, while a pipe receives ordered newline records and JSON
receives the same events in structured form. Do not hide a running state behind
a batch flush.

Cancellation stops the active operation or states clearly why it cannot. Measure
edit-to-visible, first-paint, and time-to-final-result separately; source shape
or a green test does not prove responsiveness.

## Keyboard model

One key has one meaning across interactive Jet surfaces. The shared controls
are:

- `Esc` closes the current transient view or cancels the current choice.
- `Enter` accepts the focused choice or submits the current command.
- `Ctrl-C` requests cancellation without silently changing the input.
- `Ctrl-D` requests end-of-input where the surface supports it.
- arrow keys navigate lists and panes; `↑↓ · → into · ⏎ command · Alt+⏎ example · Esc back`
  is the help-browser control vocabulary.
- terminal resize redraws the current semantic state.

An interactive surface prints its available controls at first use, and prompt
text names the next valid action. Unknown keys do not mutate input. Raw mode
restores the terminal on orderly exits, cancellation, and error paths.

## Progressive disclosure

Human output starts with the smallest useful summary, then offers detail. Long
lists, diagnostics, provenance, timing, and test output expose a visible count
and a deterministic drill-down action. JSON does not drop detail to imitate the
human summary.

The REPL fold marker, pin rail, bindings pane, and completion menu are local
examples of the same rule: summary first, detail on demand, stable labels, and
no hidden state. A new surface should use the same progressive-disclosure
shape rather than inventing a second navigation model.

## Color policy

Terminal color is semantic, not decorative. All color decisions use the shared
`ColorChoice` and `Theme` policy in
[`Terminal.rs`](../../../crates/jet-foundation/src/Terminal.rs). The CLI accepts
`--color=auto`, `--color=always`, and `--color=never`.

Resolution order is:

1. an explicit color choice wins;
2. when `NO_COLOR` is present, disable color;
3. when `FORCE_COLOR` is present, enable color for a color-capable output;
4. otherwise use whether the output stream is a TTY.

`NO_COLOR` is a presence rule, not a requirement that its value be `1`.
Piped and redirected output is plain unless the user explicitly requests forced
color. Callers emit semantic text or renderer roles; only the terminal owner
writes raw ANSI escape sequences. Shared theme roles keep diagnostics, prompts,
selection, links, and progress consistent across commands.

## Width and resize

Interactive views query terminal width through the shared width helper. A resize
redraws from semantic state rather than incrementally patching stale cells. If
resize support is unavailable, output degrades to wrapped, line-safe records;
it never truncates a diagnostic or corrupts the prompt.

Every new TUI surface receives a `COLUMNS=60` check. Width-sensitive output also
works when stdout is a pipe and no terminal size exists. `COLUMNS` is a test
input, not a substitute for querying the actual terminal during interaction.

## Raw mode and non-TTY behavior

Raw mode is a small standard-library adapter. A single `stty` shell-out is
allowed at the terminal boundary; line-editing crates are not. The adapter owns
saving settings, enabling raw and interrupt modes, restoring settings, and
failure fallback. Callers do not duplicate `stty` arguments or terminal
restoration.

When stdin or stdout is not a TTY, use a cooked or one-shot floor. Never emit
cursor movement, screen clears, progress rewrites, or interactive prompts into a
pipe. The non-TTY floor must still expose the same semantic facts and diagnostics
as the human view.

## Renderer ownership

Semantic producers provide events, rows, diagnostics, or report records. One
terminal renderer chooses TTY versus pipe, JSON, color, width, and quiet
behavior. Do not add a second renderer for a command family; human and machine
forms must share the same facts.

Diagnostics establish the shared shape with `render_all_colored`,
`render_all_linked`, and `render_all_json`. Progress and live views should feed
the same one-home renderer rather than formatting private copies of a report.

## Capture contract

Before treating a terminal surface as complete, capture these modes:

- a real TTY with normal color;
- a real TTY with `--color=never`;
- a pipe with default color policy;
- a pipe with `NO_COLOR` present;
- a narrow terminal (`COLUMNS=60`);
- resize during an active view;
- cancellation during an active operation;
- an invalid command or diagnostic path;
- EOF and `Ctrl-C` where the surface accepts keyboard input.

Store raw transcripts. A trimmed audit excerpt names the exact command,
environment, terminal mode, and key input. Source evidence is not a runtime
capture.

## Checklist for new surfaces

Before editing a terminal surface, cite this document and answer:

1. Which semantic events does it emit?
2. What is the first useful human summary and its drill-down action?
3. Which keys accept, cancel, navigate, or request EOF?
4. Which renderer owns TTY, pipe, JSON, color, width, and quiet behavior?
5. What happens when the terminal is narrow, resized, piped, or unavailable?
6. How are raw-mode settings restored on every exit path?
7. Which capture covers normal output, color suppression, cancellation, and
   diagnostics?

The interaction contract is maintained here; implementation checks belong in the
TUI fixtures and tests for each surface.
