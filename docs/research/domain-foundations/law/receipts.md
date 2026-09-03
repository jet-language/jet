# receipts

## Ratified

- **D-DEVR-TWICE1=A** — “`check`, `build`, `test`, `prove`, and `budget check` consult one local receipt store”; identity uses “input bytes and invocation context,” never filesystem timestamps, and one codec holds opaque result payloads. — `Source/ReceiptStore.rs:1-7`
- **D-DEVR-STATUS1=A** — status is “one read-only project truth surface”; it consumes authenticated receipts, “never runs a producer command,” keeps stale evidence visible, and makes no claim when evidence is absent. — `Source/CmdStatus.rs:1-4`
- **D-CLAIM1=C** — “One evidence report”; `jet test` reaches generated evidence, while `jet prove` remains the ratified umbrella and `--lens solver` adds solver evidence. — `tower` (`D-CLAIM1` outcome C)
- **D-DEVR-CLAIM1=A** — “All claim kinds, one receipt, rendered ledgers”: test, golden, budget, API, and schema claims become receipt kinds; parity and feature ledgers are rendered views, not hand-written truth. — `tower` (`D-DEVR-CLAIM1` outcome A)
- **D-VERDICT-LOOP1=D** — every row keeps the ratified report line; typed `no_fix_reason` is mutually exclusive with `fix_edits`, coverage ratchets per plane, and `--json` uses one stable status object on pass and fail. — `tower` (`D-VERDICT-LOOP1` outcome D)
- **D-PROVE-REPLAY1=A / D-PROVE-SEM1=A / D-JPROOF1=A / D-JREPLAY1=A / D-PROVE-SOLVER1=A / D-PROVE-LENS1=A** — `jet prove TARGET` is the “single progressive proof/replay command”; it owns target and producer order, evidence policy, exits, complete JSON, typed `.jetproof`/`.jetproof-replay`, opt-in solver proof, and presentation facets. — `docs/spec/syntax-decisions.md:5462-5466`; detailed contracts `docs/spec/proof-replay-decisions.md:15-37,64-78,116-142,166-182,223-242`
- **D-JPROOF1=A** — `.jetproof` embeds the complete semantic `ProofReport` under `proofReport`; metadata is a sibling and cannot replace or alter it. The version-1 envelope has fixed required members and canonical hashes. — `docs/spec/proof-replay-decisions.md:64-76`
- **D-JREPLAY1=A** — capture is closed to the paired `jet prove TARGET --capture[=ARTIFACT]` and `--capture-sensitive` forms; safe mode records Time only, sensitive mode is the explicit consent path for Rand/IO/Net, and replay requires exact identity. — `docs/spec/proof-replay-decisions.md:116-142`
- **D-PROVE-LENS1=A** — only `solver` enables a producer; other lenses are presentation-only, `--json` always emits the complete ProofReport, and facets are the fixed enum `all, refinements, effects, taint, contracts, tests, budgets, replay, solver`. — `docs/spec/proof-replay-decisions.md:223-241`
- **D-ARTIFACT-EXT1=A** — every tool artifact uses the closed `.jet<kind>` family (`.jetmap`, `.jetnb`, `.jetproof`, `.jettrace`, `.jetreplay`, `.jetproof-replay`); new kinds require a ballot. — `docs/spec/syntax-decisions.md:4034-4035`
- **D-RUN-RECORD1=A** — `--record=NAME` is available on `run`, `dev`, and `test`, writes `.jetproof-replay`, and `jet debug --replay=NAME` consumes it through the ratified adapters. — `docs/spec/syntax-decisions.md:7920-7925`
- **D-PERFSESSION1=D** — `jet perf` has one `.jettrace` truth; `run`/`test` preserve the exact base-command driver, and `attach/view/compare/export` share artifact verification. — `Source/CmdPerf.rs:1-9`; `docs/spec/syntax-decisions.md:6023-6028`
- **D-FREESTAND-ARTIFACT1=C** — source closure remains semantic authority; a linked artifact is only an “optional cache projection with the full identity key,” and a miss recompiles the same source closure. — `tower` (`D-FREESTAND-ARTIFACT1` outcome C)

## Shipped

- `ReceiptStore` uses magic `jet-receipt-v2`, fixed `ReceiptClaim{verb,key,inputs}` and `Receipt{claim,status,stdout,stderr,digest}`, validates current input digests, rejects malformed/stale objects as misses, and publishes immutable redacted objects. — `Source/ReceiptStore.rs:19-54,274-328`
- `jet status [target] --json` reads receipts and renders `proven`, `unproven`, `stale`, or no-receipt state; current failures take precedence over current success. — `Source/CmdStatus.rs:425-447,488-521`
- `jet run/dev/test --record=NAME` and `jet debug --replay=NAME` are exercised by the primitive probe; pure typed `RegressionEvidence` survives replay acts, and source-level `when`/`why` queries work for pure values. — `~/.cache/jet-luna/dx3/prim-receipts/probe.md:17-26`
- `jet prove --json` emits a canonical `ProofReport`; the probe observed compiler front-end evidence with `facet:"all"`, not user-domain evidence facets. — `~/.cache/jet-luna/dx3/prim-receipts/probe.md:23-25`
- `jet perf run` writes `.jettrace`; `jet perf compare` verifies both traces, checks toolchain/hardware identity unless overridden, prints schema/IDs/deltas, and can use a pinned baseline. — `Source/CmdPerf.rs:2079-2193`
- The standard artifact extension and proof/replay behavior are also summarized in the spec inventory. — `docs/spec/spec.md:306-312`; `docs/spec/observability.md:29-51`

## Undecided

- Should `jet-receipt-v2` gain an extensible, typed facet attachment that remains content-addressed and supports shared query and cross-run diff without creating a second receipt store?
- Should receipt tooling expose a stable `jet inspect receipts`/`jet inspect receipt` query and diff surface, beyond `jet status`'s claim summary and `.jettrace`-only `jet perf compare`?
- Should the replay adapter execute captured ambient `Time` reads in the source debugger, rather than stopping at E0956?
- Should source-level replay render user-defined values with Jet source type and field names rather than generated `__jet_*` names?
- Should typed domain evidence become a first-class receipt facet while preserving the complete ProofReport and fixed artifact-extension family?
- Should reverse stepping ship after the ratified jump-and-fixture gate, or remain forward-only until that gate is implemented? (The selected D-TIMETRAVEL2 law makes this a delivery gate, not a new receipt schema.)

## Conflicts

- The receipt codec is intentionally fixed to `claim/status/stdout/stderr/digest`; a ballot that silently adds facets to the current v2 object conflicts with `Source/ReceiptStore.rs:47-54` and must define an explicit extension law.
- `D-ARTIFACT-EXT1` closes artifact suffixes. A new `.jetreceipt` or alternate replay suffix is not an implementation detail; it requires a new ballot.
- `D-PROVE-LENS1` forbids a second machine schema or lens-filtered JSON. A domain-evidence proposal must extend the existing report/receipt path, not replace `ProofReport` or invent a facet-specific JSON envelope.
- `D-DEVR-STATUS1` makes status a read-only consumer. “Fixing” missing receipt evidence by rerunning a producer from `jet status` would violate the ruling.
- `D-PERFSESSION1` and the current implementation compare `.jettrace`, not `.jetproof-replay`; the probe's attempted replay comparison fails because the path is not `.jettrace`. — `~/.cache/jet-luna/dx3/prim-receipts/probe.md:30-35`
- The primitive probe records G2/G3 as defects, not missing policy: capture succeeds, but ambient-Time source replay stops at E0956 and generated names leak through the debugger. — `~/.cache/jet-luna/dx3/prim-receipts/probe.md:38-52`
- `D-TIMETRAVEL2=B` keeps forward-only behavior until jump and the two adapter fixture facts land; illustrative `back` output is not shipped behavior. — `tower` (`D-TIMETRAVEL2` outcome B)
