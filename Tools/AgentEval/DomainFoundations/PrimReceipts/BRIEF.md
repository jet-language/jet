# Probe prim-receipts — Receipts and evidence as an extensible shared type

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: data, backend, embedded, ai-ml.

## Build this

Learn the shipped receipt (jet-receipt-v2, `jet prove`, `--record`, .measure, Examples/Tooling/Performance). Then, as a library author, add a domain field group (a regression fit's residuals and tolerances) to a receipt, query it, replay it, and diff two runs. Record what a library can attach, what only core can, and what a user must type.

## Answer these

1. Can a library extend the shared receipt without core changes?
2. Is replay/diff available to library code or only to the CLI?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/mlops/`, `$JET_STATE_ROOT/dx2/proof-formal/`, `$JET_STATE_ROOT/dx2/observability-platforms/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/prim-receipts/probe.md`, `gaps.json`, and the code under `$JET_STATE_ROOT/dx3/prim-receipts/pkg/`. Gap ids start with `prim-receipts-G`.
