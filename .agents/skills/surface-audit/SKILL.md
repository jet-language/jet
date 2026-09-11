---
name: surface-audit
description: >-
  Audit Jet language shape, uniformity, and consistency. Find outliers and gaps
  in syntax and structure; say what to do next.
---

# Surface Audit

Find outliers and gaps in language shape, uniformity, and consistency. Compose
output quality from `docs/spec/philosophy.md` and
`docs/spec/syntax-decisions.md`. End with concrete next actions (ballot
titles or card ids only — do not create cards unless asked).

Search live specs, examples, stdlib, and CLI surfaces. Prefer
`scripts/agent/jet-env` and `rg` over memory.

Before running, read `.agents/skills/_shared/audit-dispositions.md`. It owns
shared publication, workflow-boundary, and disposition mechanics; this method
still owns the surface question, evidence, micro sweep, and stopping rule.

## The standing lens


Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

## Method: the micro sweep is this skill

"Shape, uniformity, and consistency" is measured category by category, not by
impression. Walk every category in the shared lens's micro sweep — syntax,
ergonomics, surfaces, APIs and types and methods, defaults, naming, error text,
UX and DX, tooling and CLI shape, ceremony versus control — and report each one
even when it is clean. A category with no finding is a result worth printing; a
category you skipped is a hole in the audit.

For each outlier, say which of the four it is:

- **Inconsistent** — the same idea spelled two ways. Name both and pick one.
- **Absent** — a shape the language implies but does not offer.
- **Ceremonial** — required text that buys the reader nothing.
- **Asymmetric** — the beginner road and the expert road disagree about the
  same concept.

Judge each finding against verdict actionability and repair determinism as well
as human readability. A surface with one obvious spelling is cheaper for an
agent to drive, which is the machine-facing half of I8.

## Output

This is a report-only method. Write one markdown report under `docs/audits/`
through the project-approved non-serve CLI. Do not create Tower work or
implementation edits unless the owner explicitly asks. Read
`.agents/skills/_shared/audit-dispositions.md` before the run and use it for
publication rules and the required finding-disposition table. Keep this
method's concrete next actions as ballot titles or card IDs; do not create them
in this run.
