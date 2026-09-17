---
name: spec-compliance-audit
description: >-
  Compare a declared Jet feature with ratified law without reopening syntax. Use
  when checking whether a feature is shipped, partial, gated, or a gap.
---

# Spec compliance audit

Locate the relevant ratified section by searching `docs/spec/syntax-decisions.md` for the requested feature or decision. Follow only linked or task-triggered sections; do not preload unrelated specs. Compare that law with the parser, sema, tests, examples, and running behavior. Use only these status keys: `shipped`, `partial`, `gap`, `gated`, `declined`, `stale-doc`. Cite paths. Do not invent or reopen syntax.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns comparison against ratified law, live probes, status keys, and finite closeout.

## Evidence

Apply only the standing-lens probe and honesty sections relevant to the selected ratified sections. Skip unrelated questions, quantities, micro-sweep, or competitive work. A spec paragraph, code path, or test name is not proof of executable behavior. Run the real surface through `scripts/agent/jet-env` and read its output, exit code, and emitted paths before assigning `shipped`.

Keep these failures visible:

- A registered surface that cannot fire, such as a diagnostic with no implementation, a documented field emitted as a constant, or a parsed-but-ignored flag.
- A surface that works for the demo case and nothing else. Mark it `partial` and name the covered case.

## Completion and output

Stop when every task-triggered ratified section has a status plus live evidence or an honest `unknown`, every required path and output is recorded, and the report's disposition marker is complete. `unknown` must name the missing probe or source; it never becomes `shipped` by distance. Report completion does not change a `gap`, `partial`, or `gated` status into implementation completion.

This is a report-only method. Write one report under `docs/audits/` through the project-approved non-serve CLI, cite existing Tower records read-only, and create no cards, decisions, ballots, or implementation edits unless the owner explicitly changes the boundary. Do not reopen syntax during that authorized change.
