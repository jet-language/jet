---
title: CLI and diagnostics accessibility audit 2026-08-14
---
# CLI and diagnostics accessibility audit — 2026-08-14

## Result

Jet has a readable plain diagnostic frame, one machine report shape, and one
color policy. Checked-in terminal fixtures prove the normal diagnostic order,
`NO_COLOR`, explicit color modes, copyable fixes, JSON fields, and the static
`jet ?` floor.

The audit does not close six product gaps. TTY `jet ?` still redraws a raw
screen, help inventory still needs one live source check, non-PTY stream order
is not one merged report, machine paths and edits still have open cards, and
the full LSP-to-terminal parity path is not proven. Each gap points to an
existing live Tower card. No new output mode or second renderer is proposed.

## Method and evidence boundary

This pass reads the checked-in terminal transcripts, PTY fixture states,
diagnostic JSON, LSP fixture, source renderer, and linked specifications. It
does not claim a new screen-reader session or new measurements. The owner
instruction forbids tests, builds, linters, formatters, and devtools, so the
existing executable checks were not run here.

The primary evidence is:

- `tests/cli/check_human.txt` — plain E0102 diagnostic transcript.
- `tests/cli/check_json.txt` and `tests/cli/json_test.txt` — `jet.report/v1`
  reports with location and `fix_edits`.
- `tests/help_pty.rs` — PTY color, redraw, focus, keyboard, alternate-screen,
  shell-prefill, resize, and viewport states.
- `tests/cli/question_mark_help.txt` — static, box-drawn `jet ?` output.
- `tests/cli/man.txt` and `tests/cli/man_full.txt` — generated and captured
  command inventories.
- `examples/features/expected/io/terminal_parity.out` and
  `examples/features/expected/io/terminal_parity.stderr.out` — separate
  stdout and stderr captures from the non-PTY terminal fixture.
- `tests/lsp/10_editor_reports.json` — structured editor report and code-action
  expectations.
- `crates/jet-foundation/src/Diagnostics.rs`,
  `crates/jet-foundation/src/Terminal.rs`, and
  `crates/jet-cli/src/Help/Interactive.rs` — current implementation path.

## Findings

`pass` means the checked-in fixture proves the named surface. `gap` means the
fixture exposes an accessibility or agent-loop risk. `not-proven` means the
surface has useful structure but the required end-to-end proof is still open.

| id | surface | screen_reader | state | card |
| --- | --- | --- | --- | --- |
| A11Y-001 | human diagnostic order | reads `Error`, location, source, caret, `Why`, `Fix`, and the final action line in order | pass | — |
| A11Y-002 | color and `NO_COLOR` | plain mode removes ANSI styling; `--color=never` and `NO_COLOR` win over forced color | pass | — |
| A11Y-003 | source span and caret box | the caret is visible text, but its spoken range and wide-character semantics are not proven in a reader session | not-proven | #1807 |
| A11Y-004 | interactive `jet ?` on a TTY | raw redraw, cursor movement, and alternate-screen controls are not a linear spoken transcript | gap | #1858 |
| A11Y-005 | `jet --help` command inventory | competing captured inventories can send a reader or agent to a stale route | gap | #1901 |
| A11Y-006 | non-PTY stdout and stderr | separate stream captures do not prove one semantic order after a reader merges them | gap | #1931 |
| A11Y-007 | `jet.report/v1` file path | an unresolved path blocks opening the source named by the report | gap | #1873 |
| A11Y-008 | `fix_edits` | the human Fix can be readable while the machine edit is absent or points at the wrong file | gap | #1877 |
| A11Y-009 | LSP structured report | the fixture carries What, Why, Fix, explanation link, schema, and edit, but full terminal parity remains open | not-proven | #1807 |
| A11Y-010 | human copy/paste recovery | `run \`jet explain E0102\`` is a plain final line; `Fix` names the replacement `print` | pass | — |
| A11Y-011 | CLI JSON parity | E0102 and E0111 JSON retain the same code, What, Why, Fix, location, and typed edit as the human path | pass | — |

Every non-pass row uses an existing live card. No additional finding is
introduced outside the table.

## Fixture transcripts and states

### Human diagnostic frame

`tests/cli/check_human.txt` contains this complete plain sequence:

```text
Error [E0102]: nothing named `pirnt` exists here
  --> BAD.jet:2:5
    |
  2 |     pirnt("hi");
    |     ^^^^^
 Why: only functions that have been defined (or built in, like `print` / `input`) can be called
 Fix: did you mean `print`?

1 problem found
run `jet explain E0102` to learn more
```

The sequence has headings and no color dependency. A reader can move from the
problem to the reason and the next action without inferring a hidden field.
The caret still has no spoken range label. That is A11Y-003, not a pass claim.

`Diagnostics.rs` owns this order for both plain and colored output. Its color
path wraps the same text, so color does not change the words a pipe receives.

### Color and `NO_COLOR`

`Terminal.rs` resolves choices in this order: explicit `always` or `never`,
then `NO_COLOR`, then `FORCE_COLOR`, then TTY state. The PTY fixture checks the
real renderer states:

- `--color=always` contains the accent or selection ANSI style.
- `--color=never` contains neither interactive style.
- `NO_COLOR` with automatic color contains neither interactive style.
- an empty `NO_COLOR` value still disables automatic color.

This covers the screen-reader-safe plain path and the expert override. It does
not claim that a screen reader interprets ANSI styling well; plain output is
the accessible baseline.

### `jet ?`, focus, and keyboard

Focus and keyboard apply only to the TTY branch. The non-TTY fixture is a
single static frame:

```text
┌─ jet ? — command palette ────────────────────────────────────────────┐
│  type to search · ↑↓ · ⏎ command · Alt+⏎ example · ⇥ detail · F1     │
├──────────────────────────────────────────────────────────────────────┤
│> ▸ Build & Run                                                       │
│  ▸ Projects & Environments                                           │
│  ▸ Packages                                                          │
│  ▸ Jetos                                                             │
│  ▸ Development Server                                                │
│  ▸ Reference                                                         │
│  ▸ Error Codes                                                       │
└──────────────────────────────────────────────────────────────────────┘
```

`tests/help_pty.rs` covers these TTY states without claiming reader output:

- Enter expands a category and reports `selected: jet run`.
- Alt-Enter reports `selected: jet run examples/features/basics/hello.jet`.
- F1 enters `\x1b[?1049h`; Escape restores `\x1b[?1049l`.
- Up/down keeps selection inside the viewport at normal, zero-size, and
  shrinking terminal sizes.
- shell-prefill captures one command on stdout while the palette stays on the
  terminal stream.

The state coverage proves keyboard behavior. The raw redraw and alternate
screen make the TTY path unsuitable as a claimed linear screen-reader
transcript. A11Y-004 remains open on #1858.

### Help inventory

`tests/cli/man.txt` is a generated, grouped inventory. `tests/cli/man_full.txt`
is a captured full manual with a different command grouping and older bare
routes such as `lsp`, `store`, and `install`. This is enough evidence to keep
the inventory finding open. It is not enough to claim which live `jet --help`
path produced each capture without running the command. #1901 owns that
resolution.

### Copy/paste and JSON parity

The human fixture ends with a command that can be copied as text:

```text
run `jet explain E0102` to learn more
```

Its repair names the exact replacement:

```text
Fix: did you mean `print`?
```

The matching machine report keeps the same report meaning and adds typed
coordinates and the edit:

```json
{"schema":"jet.report/v1","moment":"compile","severity":"error","code":"E0102","what":"nothing named `pirnt` exists here","why":"only functions that have been defined (or built in, like `print` / `input`) can be called","fix":"did you mean `print`?","detail":null,"file":"BAD.jet","line":2,"col":5,"span":{"start":15,"end":20},"fix_edits":[{"file":"BAD.jet","span":{"start":15,"end":20},"new_text":"print"}],"cause":[],"clears":0}
```

`tests/cli/json_test.txt` gives the same parity for E0111 and a `:=` edit.
The one-report law is D-REPORT-MACHINE1=A. The editor fixture follows
D-REPORT-EDITOR1=A with `message`, `codeDescription`, structured `data`, Why,
Fix, and `new_text`. The editor-to-terminal execution-tier proof remains open
on #1807.

Machine edit construction is not treated as a screen-reader pass. The open
#1877 card owns the cases where a readable Fix and a usable `fix_edits` entry
can diverge. #1873 owns source-path resolution before any reader or agent can
open the file.

### Stream order

The terminal parity fixture writes these expected stdout lines:

```text
Continue? [y/N] confirm: false
Choose a target:
  1) staging
  2) production
> Enter a number from 1 to 2.
> Enter a number from 1 to 2.
> choice: production
secret: non-tty
stdout stream
raw
stdout tty: false
stderr tty: false
size: 100x40
plain
15
progress
```

Its expected stderr capture is:

```text
stderr stream
```

The fixture proves the stream facts and the plain text values. Separate files
do not prove the order a screen reader or pipe consumer receives after stdout
and stderr are merged. A11Y-006 remains open on #1931.

## Mission lens

### 1. Level playing field

Jet wins where one typed report supplies readable What/Why/Fix text, source
location, and machine edit data to terminal, JSON, and editor surfaces. This
is shipped in the checked-in renderer and fixtures. The winning claim stops at
the unproven LSP execution-tier and path/edit cases.

### 2. What Jet must avoid

| Mistake | Evidence | Jet exposure |
| --- | --- | --- |
| Use color as meaning | `NO_COLOR` and `--color=never` fixtures | low on plain paths; TTY help still needs a reader transcript |
| Make a redraw look like a document | raw cursor and alternate-screen PTY states | open; #1858 |
| Make a reader infer a source range from carets | plain diagnostic has `^^^^^` without a range label | open; #1807 |
| Make machine repair depend on English | human Fix and `fix_edits` are separate fields by design | implementation exists; #1877 keeps the proof gap visible |
| Make a path depend on ambient Git state | path card tests this boundary but remains open | open; #1873 |

Jet is structurally protected against a second diagnostic prose parser: the
typed row and `Diagnostics.rs` own the report, and `fix_edits` is a separate
field. That protection is a design asset, not proof that every card is done.

### 3. AI-driven development

| Quantity | Verdict | Evidence |
| --- | --- | --- |
| Verdict fidelity | aligned for the checked human/JSON rows | E0102 and E0111 carry one code and one What/Why/Fix set |
| Verdict latency | unknown for the live TTY and merged-stream paths | no new run allowed; #1858 and #1931 remain open |
| Verdict actionability | aligned for the checked `jet explain` line and typed JSON edits | `check_human.txt`, `check_json.txt`, `json_test.txt` |
| Context economy | aligned in plain diagnostic order; at risk in raw redraw output | linear diagnostic frame versus cursor-control transcript |
| Repair determinism | partly aligned | one renderer and typed edits exist; path/edit proof remains open on #1873/#1877 |

### 4. Concrete surface coverage

Covered with fixture proof: `Diagnostics::render`, `Diagnostics::to_json`,
`ColorChoice::resolve`, `NO_COLOR`, `--color=never`, static `jet ?`, human
What/Why/Fix order, E0102/E0111 JSON parity, and the existing TTY key states.

Worth checking: spoken caret range, live `jet --help` inventory, merged
stdout/stderr order, source path resolution, machine edit application, and
full LSP-to-terminal parity. These surfaces map to A11Y-003 through A11Y-009
and their live cards.

Missing proof: a real screen-reader transcript for a TTY diagnostic and
interactive help flow. This audit does not invent one.

## Micro sweep

| Category | Result | Evidence or linked finding |
| --- | --- | --- |
| Syntax | clean in this scope | no new CLI or diagnostic spelling proposed |
| Ergonomics | pass for the plain diagnostic; TTY help remains costly to read | A11Y-001, A11Y-004 |
| Surfaces | one static help floor and one raw TTY branch | `question_mark_help.txt`, #1858 |
| APIs, types, and methods | typed report and `fix_edits` fields exist; path/edit application is open | A11Y-007, A11Y-008 |
| Defaults | automatic color follows TTY and `NO_COLOR` disables it | `Terminal.rs`, `help_pty.rs` |
| Naming | `Why`, `Fix`, `jet.report/v1`, and `fix_edits` are explicit | diagnostic and JSON fixtures |
| Error text and diagnostics | ordered What/Why/Fix frame is readable; caret speech is not proven | A11Y-001, A11Y-003 |
| UX and DX | copyable explain command exists; redraw and stream order remain open | A11Y-004, A11Y-006, A11Y-010 |
| Tooling and CLI shape | JSON parity is present; help inventory needs one source | A11Y-005, A11Y-011 |
| Ceremony versus control | plain output is automatic; explicit color and PTY control remain available | A11Y-002, A11Y-004 |

## Criteria mapping

1. This document covers screen-reader flow for human diagnostics, JSON,
   editor output, static `jet ?`, TTY `jet ?`, help, copy/paste, and stream
   order. It marks unproven paths instead of claiming a reader session.
2. Every actionable finding in the table names one of the existing live cards
   #1807, #1858, #1873, #1877, #1901, or #1931. Pass rows use `—`.
3. The findings table has the exact required columns: `id`, `surface`,
   `screen_reader`, `state`, and `card`. Live `card show --json` reads on
   2026-08-14 returned every referenced card. The required
   `truthfulness` test was not run because the owner instruction forbids tests.

No Tower or plugin data changed. No code gap was marked pass by this report.
