# Implementation plans

Plans describe sequencing and proof for work not yet represented by shipped
behavior. `docs/spec/` remains authoritative for language behavior and design;
Tower owns live status, claims, decisions, and blockers.

## Where plans live

- [`epoch-3/`](epoch-3/) — compiler and language program plans.
- [`epoch-4/`](epoch-4/) — jetpack package-manager and environment substrate.
- [`epoch-5/`](epoch-5/) and [`epoch-6/`](epoch-6/) — later ratified arcs.
- [`epoch-7/`](epoch-7/) — jetos and Studio plans, including frozen work.
- [`../sidequests/`](../sidequests/) — reviewed cross-epoch work. Delete a
  sidequest plan after its behavior moves into the spec, examples, and tests.

The live queue is not duplicated here. Start it using the canonical command in
[`AGENTS.md`](../../AGENTS.md).

## Implementing-agent protocol

Follow the repository root [`AGENTS.md`](../../AGENTS.md). It is the canonical
read order, syntax-gate protocol, test-first workflow, completion standard,
verification command, delegation policy, and invariant list. A plan adds only
card-specific sequencing, ratified decision IDs, affected seams, and executable
exit criteria; it must not weaken or copy a stale variant of that protocol.

Examples live under `examples/features/<topic>/` with matching output under
`examples/features/expected/<topic>/`. There is no reserved numbered-example
table.
