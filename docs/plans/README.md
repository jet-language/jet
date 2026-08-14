# Implementation plans

Plans describe sequencing and proof for work not yet represented by shipped
behavior. `docs/spec/` remains authoritative for language behavior and design;
Tower owns live status, claims, decisions, and blockers.

## Where plans live

- [`docs-cleanup-sweep.md`](docs-cleanup-sweep.md) — **accepted** cleanup policy
  for Docs UI content (archive vs delete), executed through Tower card #1848.
- [`compiler-speed.md`](compiler-speed.md) — two-lens law and self-hosted speed bets.
- [`epoch-3/`](epoch-3/) — compiler and language program plans.
- [`epoch-4/`](epoch-4/) — jetpack package-manager and environment substrate.
- [`epoch-5/`](epoch-5/) and [`epoch-6/`](epoch-6/) — later ratified arcs.
- [`epoch-7/`](epoch-7/) — jetos and Studio plans, including frozen work.
- [`../sidequests/`](../sidequests/) — reviewed cross-epoch work. In the Tower
  Docs tab these files list under **Plans**. Delete or archive a sidequest plan
  after its behavior moves into the spec, examples, and tests.

The live queue is not duplicated here. Start it using the canonical command in
[`AGENTS.md`](../../AGENTS.md).

## Docs tab sections (Tower)

Scratchpad, then Spec (collapsed by default), Proposals, Plans (includes
sidequests), Research, Audits, References. `docs/archive/` and `docs/ballots/`
do not appear. Use **Archive** to retire a file into `docs/archive/`; use
**Delete** to remove it. Spec files cannot be archived from the UI.

## Implementing-agent protocol

Follow the repository root [`AGENTS.md`](../../AGENTS.md). It is the canonical
read order, syntax-gate protocol, test-first workflow, completion standard,
verification command, delegation policy, and invariant list. A plan adds only
card-specific sequencing, ratified decision IDs, affected seams, and executable
exit criteria; it must not weaken or copy a stale variant of that protocol.

Examples live under `examples/features/<topic>/` with matching output under
`examples/features/expected/<topic>/`. There is no reserved numbered-example
table.
