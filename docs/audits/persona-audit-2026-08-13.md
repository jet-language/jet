---
title: Persona audit 2026-08-13: first-session delight baseline
---
# Persona audit — 2026-08-13

**Card:** #1924

**Status:** first-session lens installed; runtime baseline not measured.

**Method:** four fresh personas, from beginner to expert, in distinct domains.
This report records the core loops and the checks that the next executable run
must fill. The owner instruction skipped builds, tests, and devtools, so this
report does not invent elapsed times or pixels.

## Personas

### P1 Mara — first-time programmer · small command-line tool

Core loop: read the first example, write one `fn run`, run `jet check`, fix the
first diagnostic, then run the file.

- **Pull:** `print`, `input`, plain diagnostics, and `NO_COLOR` give her a
  readable starting path.
- **Push:** first-session time is unknown. A reader-friendly diagnostic does not
  prove that the first command starts fast.
- **Verdict:** usable-with-friction. The first-session timing is not proven.

### P2 Devon — TypeScript developer · subcommand CLI

Core loop: declare commands, ask for help, run a valid subcommand, inspect JSON
when an input fails, and repeat until the tool works.

- **Pull:** one CLI surface, `--help`, `--json`, and named `Why` and `Fix`
  fields make the loop discoverable.
- **Push:** stale help, unresolved machine paths, and missing machine edits can
  add repair turns. Cards #1901, #1873, and #1877 own those gaps.
- **Verdict:** usable-with-friction.

### P3 Inez — graphics beginner · first window

Core loop: create a window, draw one pixel, change its color, and rerun after
each edit.

- **Pull:** the lens names the joy event precisely: a visible first-party
  window, then a visible first pixel.
- **Push:** the windowed backend is not available in this pass. There is no
  honest duration or frame receipt to record.
- **Verdict:** blocked. The first-window loop cannot finish until the backend
  exists and a real run records both checks.

### P4 Luna — unattended coding agent · small diagnostic migration

Core loop: read repository context, edit one file, run the checker, read the
structured verdict, apply one repair, repeat, and stop when clean.

- **Pull:** JSON Lines, source spans, `Why`, `Fix`, and stable report fields can
  make the loop mechanical.
- **Push:** this pass did not run the checker. The agent cannot claim verdict
  fidelity or latency from a static read. Machine path and edit gaps remain
  carded by #1873 and #1877.
- **Verdict:** blocked for this audit run. The unattended loop needs a real
  checker run before it can receive a ship-ready or usable-with-friction
  verdict.

No verbatim persona reaction was collected. Execution was skipped by instruction;
the next run must record one short reaction per persona.

## First-session delight lens

Record both rows for every persona. `not-applicable` is an honest result while
the windowed backend is absent. It is not a zero.

| persona | time-to-first-window | first-pixel | state |
| --- | --- | --- | --- |
| Mara | not-applicable; no windowed backend in this pass | not-applicable; no window exists | blocked |
| Devon | not-applicable; CLI session has no window target | not-applicable; no window exists | blocked |
| Inez | not-applicable; windowed backend is not shipped | not-applicable; no frame receipt exists | blocked |
| Luna | not-applicable; checker session has no window target | not-applicable; no frame receipt exists | blocked |

Required evidence for the next run:

1. Start the clock before the first project command.
2. Record the first usable window time, backend, size, and input.
3. Record the first visible pixel time and a frame receipt.
4. Repeat after one edit so the first-session result is not a one-off startup
   artifact.

## Agent-optimality read

- **Verdict fidelity:** typed diagnostics and structured report fields are
  shipped; machine-fix and path coverage still have open cards.
- **Verdict latency:** no timing claim in this pass. The next run must record
  edit-to-verdict time.
- **Verdict actionability:** `Why`, `Fix`, spans, and JSON fields help; missing
  `fix_edits` and unresolved paths reduce actionability.
- **Context economy:** JSON Lines and one report schema reduce parsing work;
  raw interactive redraws add noise for a screen reader or agent.
- **Repair determinism:** one report should yield one obvious next edit. The
  machine-fix card tests this property.

## Four questions

1. **How does Jet win?** The shipped safety model and one typed diagnostic path
   can give beginners and agents the same meaning. The first-window advantage
   is not shipped or measured.
2. **What does Jet avoid?** It avoids treating a missing timing value as zero.
   It must also avoid claiming screen-reader or machine-repair coverage from a
   source-only read.
3. **What does this say about AI development?** An agent needs a fast, stable,
   structured verdict. Human prose without a usable edit is not a closed loop.
4. **What surfaces must Jet cover?** Covered: `jet check`, human diagnostics,
   `--json`, `NO_COLOR`, and report fields. Worth checking: `jet ?`, `--help`,
   PTY order, and wide-character spans. Missing: measured first-window and
   first-pixel receipts.

## Micro sweep

| area | current read |
| --- | --- |
| Syntax | `fn run` and CLI declarations are visible in the first loop. |
| Ergonomics | A first command can reach a diagnostic, but timing is unmeasured. |
| Surfaces | Human, JSON, help, and interactive terminal paths differ. |
| APIs, types, and methods | `--json`, `--color`, and report fields are named; first-window API is absent. |
| Defaults | Safe plain output is available through TTY and `NO_COLOR` policy. |
| Naming | `time-to-first-window` and `first-pixel` name two different events. |
| Error text and diagnostics | `Why` and `Fix` are readable; machine edits and paths need proof. |
| UX and DX | The edit-check-repair loop is defined; first-session joy is not measured. |
| Tooling and CLI shape | Help and JSON exist; interactive redraw needs an accessibility check. |
| Ceremony versus control | Beginners get a plain path; experts need structured report control. |

## Strongest unverified assumption

Jet may have a good first-session feel once a window backend lands, but source
shape and report design cannot prove the time to the first window or pixel.
