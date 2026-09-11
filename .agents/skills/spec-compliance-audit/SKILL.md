---
name: spec-compliance-audit
description: >-
  Audit the codebase against ratified syntax and spec. Measure shipped vs gap.
  Do not reopen syntax.
---

# Spec Compliance Audit

Locate the relevant ratified section by searching
`docs/spec/syntax-decisions.md` for the requested feature or decision. Follow
only linked or task-triggered spec sections; do not preload unrelated specs.
Compare the selected ratified law to parser, sema, tests, and examples. Status
keys: `shipped`, `partial`, `gap`, `gated`, `declined`, `stale-doc`. Cite paths.
Do not invent or reopen syntax.

Before running, read `.agents/skills/_shared/audit-dispositions.md`. It owns
shared publication, workflow-boundary, and disposition mechanics; this method
still owns comparison against ratified law, live probes, status keys, and
stopping conditions.


## The standing lens (partial)

Apply the **probe the running binary** and **honesty rules** sections of
`.agents/skills/_shared/standing-lens.md`. Skip the four questions, the five
quantities, and the micro sweep: this skill measures shipped against ratified,
and a competitive or design frame would distort that measurement.

Probing is not optional here — it is the whole method. A `shipped` status
earned from a spec paragraph, a code path that looks right, or a passing name in
a test list is not earned. Run the surface and read the real output before
writing `shipped`.

Two failures this skill exists to catch, both of which read as `shipped` from a
distance:

- A registered surface that cannot fire — a diagnostic code with no
  implementation, a documented field emitted as a constant, a flag parsed and
  ignored.
- A surface that fires for the demo case and nothing else. Record it as
  `partial` with the covered case named, never as `shipped`.

## Output

This is a report-only method. Write one markdown report under `docs/audits/`
through the project-approved non-serve CLI. Do not create Tower work or
implementation edits unless the owner explicitly asks. Read
`.agents/skills/_shared/audit-dispositions.md` before the run and use it for
publication rules and the required finding-disposition table. Report completion
does not change a `gap`, `partial`, or `gated` status into implementation
completion.
