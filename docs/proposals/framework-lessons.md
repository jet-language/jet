# Framework transplant lessons

This proposal records the implementation lessons for D-BINPAT1, D-STM1,
D-AUTH1, D-SYNC1, D-VALIDATE1, D-DBPOLICY1, D-ENVHOOK1, D-OBSERVE-LIVE1,
D-LIVEQUERY1, D-SCHEDULE1, and D-LINTPOLICY1. It is a truth ledger, not a
claim that the open transport and validation boundaries are complete.

## Placement lessons

| lesson | placement |
|---|---|
| One semantic mechanism must serve every execution tier. | Core behavior stays in the shared Prelude. AOT, JIT, and comptime only marshal values into it. |
| A source string can be a compatibility boundary, but not the state model. | Live footprints, auth records, row policies, and sync receipts use typed runtime state. Display strings are emitted only at fixed observability boundaries. |
| Safe denial is part of the feature. | Poisoned locks, bad identifiers, invalid policy tables, expired auth state, malformed snapshots, and non-canonical sync values return bounded failure instead of widening access. |
| Transaction scope must be explicit. | Shared edits buffer on the emitted `#Transact` guard. No ambient thread-local transaction stack owns production state. |
| Tooling readers must parse the producer schema. | `jet inspect live` validates the closed snapshot objects through the foundation JSON parser before rendering facts. |
| Ratified syntax needs consumers, not only parser checks. | `#Every` has one checked `EveryArg::resolve` law, but service-runtime and jetos timer adapters still need to consume its typed result. |

## Current implementation boundary

- D-BINPAT1, D-ENVHOOK1, and D-LINTPOLICY1 remain shipped in their existing
  parser, tooling, and policy paths.
- D-STM1 now has explicit guard-owned buffering and canonical lock-fold commit.
  Optimistic retry-on-conflict is not claimed.
- D-AUTH1 now uses typed process-global records, checked lifetimes, single-use
  OAuth and magic-link state, and cryptographic entropy for opaque tokens.
- D-SYNC1 keeps typed CRDT carriers and adds typed canonical session documents
  with idempotent duplicate receipts. Remote authenticated reconnect remains
  open.
- D-DBPOLICY1 keeps the closed v1 policy language and rejects invalid table
  identifiers. General policy closures are not claimed.
- D-OBSERVE-LIVE1 renders only typed, bounded task, channel, effect, and
  resource facts. Payloads and process memory remain outside the schema.
- D-LIVEQUERY1 marks matching subscriptions dirty from typed footprints. A
  query graph, rerun callback, and browser `core.ws` transport remain open.
- D-SCHEDULE1 shares parser and sema resolution, but its service-runtime and
  jetos timer consumers remain open.
- D-VALIDATE1 has in-body accumulation and `Type.validate`. Automatic decode
  validation and `Validate.over` await ballot `D-VALIDATE-DECODE1`.

## Verification checkpoint

This implementation checkpoint intentionally has no test, build, or formatter
run. The two closeout cards remain open until targeted proof and independent
review are recorded.
