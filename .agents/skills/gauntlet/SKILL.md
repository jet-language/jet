---
name: gauntlet
description: >-
  Run or build Jet's paired real-workload competitive corpus. Use when the owner
  names a gauntlet run, scoreboard, matrix, entry, or harness update.
---

# Gauntlet

The gauntlet is Jet's evidence-based competitive corpus. It compares paired,
real programs and records coverage, wins, parity, losses, failures, and advisory
ergonomics. A report is evidence, not delivery.

## Choose the route

Use the mode stated by the request. Do not ask again when the request says
**Run** or **Build/update**.

- **Run**: execute the existing corpus and render the scoreboard. Read
  [`references/run.md`](references/run.md), then
  [`references/shared.md`](references/shared.md).
- **Build/update**: define or evolve the matrix, entries, or harness. Read
  [`references/build.md`](references/build.md), then
  [`references/shared.md`](references/shared.md).
- If the mode is not stated and both routes are plausible, ask one focused mode
  question. Do not invent a route from context.

If `gauntlet/matrix.json` is missing, Run cannot execute. Keep the stated mode,
report the missing matrix, and route the next owner decision to Build/update.
Propose the matrix or its diff first. Do not seed or land a matrix, entry, or
policy change until the owner approves that proposal. A missing matrix is not
permission to silently narrow the corpus.

## Boundaries

Before either route, read `.agents/skills/_shared/audit-dispositions.md`. It
owns shared publication and workflow boundaries. The gauntlet still owns its
strict peer comparator, same-run paired workload rules, coverage and output
failure rules, advisory-versus-gating rules, and loss/card ratchet.

Compare shipped artifacts on a level playing field. Do not silently omit a
peer, cell, metric, invalid result, or uncovered cell. Do not turn a report into
implementation work unless the applicable gauntlet rule authorizes it. The
route is complete only when its referenced close conditions and evidence are
satisfied.
