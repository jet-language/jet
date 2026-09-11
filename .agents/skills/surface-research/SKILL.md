---
name: surface-research
description: >-
  Research Jet surface gaps by mining other languages for ideas.
---

# Surface Research

Mine peer languages for ideas that address Jet surface gaps, issues, and
weaknesses. Prefer primary sources. Record idea, Jet use, and failure to avoid.
This is research, not ratification.


Before running, read `.agents/skills/_shared/audit-dispositions.md`. It owns
shared publication, workflow-boundary, and disposition mechanics; this method
still owns source selection, micro-sweep mining, peer comparison, and
non-ratifying recommendations.

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

The micro sweep is the search grid here. Mine each category deliberately —
syntax, ergonomics, surfaces, APIs and types and methods, defaults, naming,
error text, UX and DX, tooling and CLI shape, ceremony versus control — rather
than collecting whatever the sources happened to discuss.

## Mine for the win, not only the patch

An idea that closes a Jet gap brings Jet level. An idea that Jet can take
further than its origin is worth more. For every idea recorded, add:

- **Ceiling** — how far the origin took it, and what stopped it going further.
  A feature stuck behind an unstable flag for nine years, or a convention the
  origin never checked, is an opening.
- **Jet's version** — what the same idea becomes under Jet's invariants. One
  mechanism, safe default, expert opt-in, and a machine-checkable fact where the
  origin left a convention.
- **What they must change to match it.** If the answer is "nothing, they could
  ship it next release", say so. That idea is a patch, not a win.

Record the failure to avoid alongside each idea, as now. An idea with no known
failure mode has not been researched, only admired.

## Output

This is a report-only, non-ratifying method. Write one markdown report under
`docs/research/` through the project-approved non-serve CLI. Do not create
Tower work, ballots, or implementation edits unless the owner explicitly asks.
Read `.agents/skills/_shared/audit-dispositions.md` before the run and use it
for publication rules and the required finding-disposition table. Report
completion does not ratify or implement a proposed idea.
