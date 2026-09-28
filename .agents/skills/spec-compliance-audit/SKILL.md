---
name: spec-compliance-audit
description: >-
  Compare a declared Jet feature with ratified law without reopening syntax. Use
  when checking whether a feature is shipped, partial, gated, or a gap.
---

# Spec compliance audit

Locate the relevant ratified section by searching `Docs/spec/syntax-decisions.md` for the requested feature or decision. Follow only linked or task-triggered sections; do not preload unrelated specs. Compare that law with the parser, sema, tests, examples, and running behavior. Use only these status keys: `shipped`, `partial`, `gap`, `gated`, `declined`, `stale-doc`. Cite paths. Do not invent or reopen syntax.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns comparison against ratified law, live probes, status keys, and finite closeout.

## Evidence

Apply only the standing-lens probe and honesty sections relevant to the selected ratified sections. Skip unrelated questions, quantities, micro-sweep, or competitive work. A spec paragraph, code path, or test name is not proof of executable behavior. Run the real surface through `Tools/agent/jet-env` and read its output, exit code, and emitted paths before assigning `shipped`.

## Repeat and counterexample method

When the request is a conformance rerun, pin the exact source and binary
identities, governing decision, target, configuration, fixture/oracle revision,
and finite probe scope before a row runs. Re-derive the denominator from the
canonical registry and match observations by semantic behavior, input, and
execution mode. Source coordinates and mutable line hashes are useful locators,
but they do not establish cross-revision identity.

For the bounded map-extrema rerun retained by
[`mine-for-jet`](../../Docs/audits/mine-for-jet-2026-09-10.md#f2-production-comptime-map-extrema-return-the-wrong-numeric-answer),
freeze these two cases across the applicable paths:

- `{ "first": 2, "second": 10 }` requires `min = 2` and `max = 10`.
- `{ "first": -1, "second": 2, "third": 10 }` requires `min = -1` and
  `max = 10`.

The named matrix is the retained production comptime-library path, public AOT,
the default `jet run`, the interpreter, and Web. Record the exact command or
API before each row runs. An absent interface, unsupported host, failed build,
or explicit gate is a missing or unmeasured row, never an implied pass. The
historical F2 result is evidence for the recorded comptime path; it is not a
fresh result for every public spelling or execution mode.

Use an independently derived expected relation and consume the returned value.
For non-value behavior, consume the diagnostic, exit code, effect, state
transition, or emitted artifact named by the contract. A synthetic bundle in
which two modes both report `min = 10` and `max = 2` must be rejected as a
shared wrong answer. A matching valid bundle must pass that narrow answer
check, but it can support only the paths it actually measures. Keep the
controller's expected dispositions separate from the record labels so a
reviewer derives correctness from the approved rule.

Compare a rerun only with comparable observations. Report fixed, recurring,
regressed, newly exposed, retired by authority, added, removed, and
incomparable coverage separately. A changed case is changed coverage, not a
fix for the original case. An unrun mode stays unmeasured. Add no further
campaign: stop after the two declared cases and the named matrix have
dispositions, including the adjacent `[-1, 2, 10]` case.

This method keeps the existing `shipped`/`partial`/`gap`/`gated`/`declined`/
`stale-doc` vocabulary. Missing evidence is recorded in the finding and can
leave a report complete, but it cannot support `shipped` or a broad clean
verdict. The report remains report-only: it does not approve a release, replace
`verify`, add a runner or schema, or reopen syntax.

Keep these failures visible:

- A registered surface that cannot fire, such as a diagnostic with no implementation, a documented field emitted as a constant, or a parsed-but-ignored flag.
- A surface that works for the demo case and nothing else. Mark it `partial` and name the covered case.

## Completion and output

Stop when every task-triggered ratified section has a status plus live evidence or an honest `unknown`, every required path and output is recorded, and the report's disposition marker is complete. `unknown` must name the missing probe or source; it never becomes `shipped` by distance. Report completion does not change a `gap`, `partial`, or `gated` status into implementation completion.

This is a report-only method. Write one report under `Docs/audits/` through the project-approved non-serve CLI, cite existing Tower records read-only, and create no cards, decisions, ballots, or implementation edits unless the owner explicitly changes the boundary. Do not reopen syntax during that authorized change.
