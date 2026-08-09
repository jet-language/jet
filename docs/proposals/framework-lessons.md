# Framework transplant lessons

This proposal records settled law and shipped boundaries for D-BINPAT1,
D-STM1, D-AUTH1, D-SYNC1, D-VALIDATE1, D-DBPOLICY1, D-ENVHOOK1,
D-OBSERVE-LIVE1, D-LIVEQUERY1, D-SCHEDULE1, and D-LINTPOLICY1. Ratified
decisions are law. This document records each unshipped ratified behavior as
an implementation boundary; it does not keep that behavior as an option.

## Placement lessons

| lesson | placement |
|---|---|
| One semantic mechanism must serve every execution tier. | Core behavior stays in the shared Prelude. AOT, JIT, and comptime only marshal values into it. |
| A source string can be a compatibility boundary, but not the state model. | Live footprints, auth records, row policies, and sync receipts use typed runtime state. Display strings are emitted only at fixed observability boundaries. |
| Safe denial is part of the feature. | Poisoned locks, bad identifiers, invalid policy tables, expired auth state, malformed snapshots, and non-canonical sync values return bounded failure instead of widening access. |
| Transaction scope must be explicit. | Shared edits buffer on the emitted `#Transact` guard. No ambient thread-local transaction stack owns production state. |
| Tooling readers must parse the producer schema. | `jet inspect live` validates the closed snapshot objects through the foundation JSON parser before rendering facts. |
| Ratified syntax needs consumers, not only parser checks. | Every consumer reads the checked `#Every` value from the shared time rail. |

## Settled law and implementation boundary

- D-BINPAT1 and D-ENVHOOK1 remain shipped in their existing parser and tooling
  paths. D-LINTPOLICY1 keeps the default warning path and removes a denied
  finding from that path before emitting its single E1293 policy failure.
- D-STM1, as amended by D-CONC-STM1, runs each transaction body exactly once.
  The commit takes touched Shared locks in fixed order, applies buffered edits,
  and waits on contention instead of retrying.
- D-AUTH1 now uses typed process-global records, checked lifetimes, single-use
  OAuth and magic-link state, and cryptographic entropy for opaque tokens.
  Magic links require a registered user with a syntactically valid delivery
  identity, and consume rechecks that identity. Durable DB-backed app routes,
  provider network calls, and actual email delivery are unshipped.
- D-SYNC1 defines typed CRDT carriers, deterministic merge, and
  `app.sync(doc, over: session)`. The shipped child slice proves the CRDT
  algebra and typed canonical session receipts. The session registry is
  process-local. Vector-clock access, remote authenticated reconnect, transport,
  and merge protocol are unshipped.
- D-DBPOLICY1 defines typed per-row rules that apply below app code on every
  query, mutation, and live-query path. The shipped boundary accepts only the
  compiled `true` and `owner == user` forms and rejects other expressions.
  The general policy closure compiler and generated per-path proof or filter
  are unshipped.
- D-OBSERVE-LIVE1 renders only typed, bounded task, channel, effect, and
  resource facts. Payloads and process memory remain outside the schema.
- D-LIVEQUERY1 defines effect-qualified queries, read-footprint tracking,
  write-set invalidation, query rerun, and `Signal<T>` delivery over
  `core.ws`. The shipped boundary stores normalized footprints in a bounded
  registry, reports invalid or evicted handles, and marks matching records
  dirty. The typed query runner and rerun callback, `Signal<T>` binding,
  `core.ws` transport, and remote reconnect are unshipped.
- D-SCHEDULE1, as amended by D-CONC-SCHED1, defines one typed schedule value
  on the D-TYPE2-TIME1 rail. A job is a task the runtime starts. The shipped
  boundary checks the legacy `ns`/`us`/`ms`/`s`/`min` table and lets `jet dev`
  read the checked result. The typed `Duration` and wall-clock values,
  including `2h` and `1d`, service and jetos consumers, and one Job/task
  vocabulary in docs, diagnostics, and the CLI are unshipped.
- D-VALIDATE1 has in-body accumulation and `Type.validate`. Derived struct
  decode now runs the same validator after shape decoding. Hand codecs still
  opt in explicitly. D-VALIDATE-DECODE1 settles the sole
  `Result<T, [FieldError]>` Decode contract across generated and hand codecs.
  `Validate.over` is unshipped.

## Verification checkpoint

Child cards #1157 and #1160 record focused implementation evidence for their
shipped boundaries. This docs-only reconciliation ran no test, build, or
formatter command. The unshipped ratified behaviors above remain named
boundaries; this file makes no implementation claim for them.
