---
title: CLI and diagnostics accessibility audit 2026-08-13
---
# CLI and diagnostics accessibility audit — 2026-08-13

**Scope:** human diagnostics, machine diagnostics, `jet ?`, `jet --help`, and
terminal stream order.

**Method:** static read of the current renderer, CLI paths, and checked-in
goldens. This pass does not claim a screen-reader run. The owner instruction
skipped builds, tests, and devtools.

## Result

The ordinary no-color diagnostic frame has a usable linear order:
`Error`, location, source line, caret line, `Why`, and `Fix`. `NO_COLOR` and
`--color=never` remove ANSI styling. JSON reports expose stable fields for an
agent, including the file, line, column, span, and `fix_edits`.

The audit does not prove a complete screen-reader path. `jet ?` redraws a raw
terminal frame and can enter an alternate screen. Non-PTY output can reorder
stdout and stderr between execution tiers. Machine paths and machine fixes can
also block an unattended repair loop. Those gaps have live owner cards in the
table below.

## Findings

The table is the machine-readable handoff. `pass` means no actionable gap was
found in this static pass. Every other state names one existing live Tower card.

| id | surface | screen_reader | state | card |
| --- | --- | --- | --- | --- |
| A11Y-001 | human diagnostic frame | reads as one ordered text block with `Why` and `Fix` labels | pass | — |
| A11Y-002 | `NO_COLOR` and `--color=never` | removes ANSI control bytes from the readable path | pass | — |
| A11Y-003 | source span and caret box | repeated `^` marks have no spoken range or semantic label | not-proven | #1807 |
| A11Y-004 | interactive `jet ?` on a TTY | raw redraw, cursor movement, and alternate-screen control can break linear reading | gap | #1858 |
| A11Y-005 | `jet --help` command inventory | stale advice sends a reader into a dead command loop | gap | #1901 |
| A11Y-006 | non-PTY stdout and stderr | a reader can receive arrival order instead of semantic report order | gap | #1931 |
| A11Y-007 | `jet.report/v1` file path | an unresolved path prevents a reader or agent from opening the source | gap | #1873 |
| A11Y-008 | `fix_edits` | a human fix can exist while the machine edit is empty | gap | #1877 |
| A11Y-009 | LSP structured report | matching Why, Fix, explanation, and related-location flow is not proven against terminal output | not-proven | #1807 |

## Evidence and stance

`crates/jet-foundation/src/Diagnostics.rs` renders the human frame in a fixed
sequence. Its caret width uses display width, so wide characters do not shift
the visible underline. The same module emits JSON Lines and an explicit clean
result object.

`crates/jet-foundation/src/Terminal.rs` makes `NO_COLOR` presence win over
automatic TTY color. Explicit `--color=never` also disables color. This is a
good default for a screen reader and a pipe.

`crates/jet-cli/src/Help/Interactive.rs` uses raw input, cursor movement, and
the alternate screen for the TTY help view. Its non-TTY branch prints a static
categorized view. The static branch is the accessible baseline; the TTY branch
needs a recorded linear transcript and keyboard-plus-reader check.

`Source/main.rs` and `crates/jet-cli/src/CLI.rs` expose `--json`, `--color`,
`--quiet`, and `--a11y`, but not every CLI failure path is proven to use one
structured report frame. The audit treats this as a carded gap, not as proof
that every direct message is inaccessible.

## Acceptance rule

Close a row only with evidence from the named card. A screen-reader check must
cover a TTY and a pipe, `NO_COLOR`, a diagnostic with a wide-character span,
`jet ?`, `jet --help`, JSON output, and interleaved stdout and stderr. The check
must state what the reader receives and preserve a plain-text transcript.

No new card was created in this read-only pass. The table uses only live cards:
#1807, #1858, #1873, #1877, #1901, and #1931.
