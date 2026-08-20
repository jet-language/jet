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
| Safe denial is part of the feature. | Poisoned locks, bad identifiers, invalid policy tables, expired auth state, malformed snapshots, and non-canonical sync values return bounded failure instead of widening access. Denial must cover every read of a denied carrier, not only its display: `SyncMap.get` answering from an invalid carrier was a hole, and `SyncCounter.value` reporting `0` for one still is. |
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
  `app.sync(doc, over: session)`. The shared Prelude now turns malformed or
  overflowing carriers into an absorbing invalid state instead of truncating
  replica data. Typed carrier operations retain their identity metadata. The
  fixed `app.sync` String seam merges canonical map/list/counter displays,
  preserves duplicate idempotence, and publishes the latest receipt through
  the bounded local live transport for replay on reconnect. Vector-clock
  access, auth-scoped routing, remote reconnect, and a network merge protocol
  are unshipped.
- D-DBPOLICY1 defines typed per-row rules that apply below app code on every
  query, mutation, and live-query path. The shipped boundary accepts only the
  compiled `true` and `owner == user` forms and rejects other expressions. That
  closed language now lives in one Prelude fragment
  (`CoreLib/JetStd/RowPolicy.rs`) that AOT, the Cranelift host, the ambient
  interpreter, and comptime all include, and each tier stores the COMPILED
  policy. It previously existed twice and the two copies disagreed: the wire
  validator accepted a leading-digit table name that the typed constructor
  rejected, so a policy legal on one tier was illegal on another.
  The general policy closure compiler, the generated per-path proof or filter,
  and the ratified audit-output listing of active policies are unshipped.
- D-OBSERVE-LIVE1 renders only typed, bounded task, channel, effect, and
  resource facts. Payloads and process memory remain outside the schema. The
  producer is reachable only from the AOT generated runtime (`Prelude/EnvInit.rs`
  calls it; no JIT, ambient, or comptime bridge exists), so `jet run --observe`
  on the default resident tier publishes nothing — an I9 gap, not a boundary.
  Two named pieces of the ratified text are also absent and stay owed rather
  than being written down as narrower law: GC statistics are not in the producer
  or reader schema, and Canvas projects only the `event_observations` sequence
  instead of the same task/channel/effect/resource facts.
- D-LIVEQUERY1 defines effect-qualified queries, read-footprint tracking,
  write-set invalidation, query rerun, and `Signal<T>` delivery over
  `core.ws`. The bounded shared registry stores normalized footprints, a typed
  rerunner, and the canonical signal sink. Matching invalidations rerun outside
  the lock, reject stale generations, and publish through the existing
  serialized WebSocket writer. The general app-query graph, browser protocol,
  and remote authenticated reconnect remain unshipped. A bounded local
  transport replay exists for the latest event on each live/sync topic.
- D-SCHEDULE1, as amended by D-CONC-SCHED1, defines one typed schedule value
  on the D-TYPE2-TIME1 rail. A scheduled job is the lifecycle unit the runtime
  starts; `task` remains the separate structured-concurrency construct. The
  canonical Time resolver now accepts `2h` and `1d` alongside the existing
  units, and `jet dev` reads the same checked result. Service and jetos
  consumers remain unshipped.
- D-VALIDATE1 has in-body accumulation and `Type.validate`. Derived struct
  decode runs the same validator after shape decoding. A hand `impl T.Decode`
  does NOT: the codec bridge emits the user body only
  (`Codegen/TIR/emit/functions.rs:691-735`), while the automatic call exists
  solely in the generated decoder (`Sema/Registration/Serde.rs:1029-1041`).
  The ratified text says `decode<T>()` runs the block automatically, so
  "hand codecs opt in explicitly" is a current limitation owed as work, not a
  lawful exception. D-VALIDATE-DECODE1 settles the sole
  `Result<T, [FieldError]>` Decode contract across generated and hand codecs.
  `Validate.over` is unshipped.

## Verification checkpoint

Child cards #1157 and #1160 record focused implementation evidence for their
shipped boundaries. The unshipped ratified behaviors above remain named
boundaries; this file does not claim those broader slices.

No pass that wrote this file ran a test, build, formatter, linter, or devtool
command, so every tier statement here is either source reading or a row read out
of `tests/jit_corpus_gate.txt`. Read those rows only for what they license: a
`resident_jit` row proves the optimized AOT oracle and the resident Cranelift run
agree, and nothing about the interpreter when the TIR evaluator refused the
program (`classify_corpus_stem` waives an E2201/E0956 refusal without changing
the class, `tests/dev_parts/support.rs:4094-4099`). Card `#2101` owns that gate
defect. Until a claim here is backed by an explicit three-leg run — `jet run`,
`jet run --release`, and `jet run --interpret` compared byte for byte — it is a
source claim, not a tier claim.
