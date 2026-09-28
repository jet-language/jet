# prim-receipts probe

## What I built

I built a small typed regression-fit program with slope, intercept, residuals, tolerance, maximum residual, and pass/fail evidence. It emits the evidence as JSON, records two named runs, replays a pure evidence run, and asks `why`/`when` questions against the recorded acts. I also ran a time-bearing variant to test the normal replay authority.

Files authored:

- `pkg/package.jet`
- `pkg/src/run.jet` (baseline fit)
- `pkg/src/shifted.jet` (different fit)
- `pkg/src/evidence.jet` (pure replay fixture)
- `pkg/src/evidence_shifted.jet` (second pure replay fixture)
- `pkg/src/replay.jet` (ambient Time replay fixture)
- `pkg/src/isolate.jet` (debug-boundary isolation fixture)

## What worked

- `jet check src/run.jet` and `jet check src/shifted.jet` -> exit 0; no diagnostics.
- `jet run src/run.jet` -> typed JSON with `slope:1.8999999999999997`, `residuals:[0.0,-0.10000000000000009,0.20000000000000018,8.881784197001252e-16]`, `tolerance:0.25`, `passed:true`.
- `jet run src/shifted.jet` -> typed JSON with `slope:2.0666666666666664`, `max_residual:0.5333333333333332`, `passed:false`.
- `jet run src/replay.jet --record=clock2 -- foo` -> exit 0, writes `.jet/replays/clock2.jetproof-replay`; `stat` reports 1168 bytes and mode 600.
- `jet run src/evidence.jet --record=evidence` -> exit 0, writes `.jet/replays/evidence.jetproof-replay`; pure act snapshots retain the complete `RegressionEvidence` value.
- `printf 'why passed == true\\nwhen max_residual\\ncontinue\\n' | jet debug src/evidence.jet --replay=evidence` -> `passed = true because act 6 ...`; `max_residual = 0.2 at act 5 ...`; program finished.
- `jet prove src/evidence.jet --json` -> exit 0 and a canonical `ProofReport`; its evidence contains only the compiler `front_end` item (`facet:"all"`), not the user `RegressionEvidence` fields.
- `jet perf run src/run.jet` and `jet perf run src/shifted.jet` -> separate `.jettrace` files; `jet perf compare` reports wall-time deltas.

## Gaps

### prim-receipts-G1 — impossible

**Primitive:** An extensible typed facet payload on the canonical `jet-receipt-v2` receipt with shared query and diff access.

**Evidence:** `Source/ReceiptStore.rs:47-54` fixes `Receipt` to `claim/status/stdout/stderr/digest`; `:301-328` accepts only those fields. `jet status src/evidence.jet --json` returns only `action/claim/reason/receipt/state`. `jet perf compare .jet/replays/evidence.jetproof-replay .jet/replays/evidence-shifted.jetproof-replay` returns `Error [E2102]: Base trace: trace path must end with .jettrace`. The typed record is usable in Jet and in replay acts, but it cannot be published as a shared receipt facet or compared by the receipt tooling.

**Areas:** data, backend, embedded, ai-ml. **Severity:** blocks.

### prim-receipts-G2 — defect

**Primitive:** A replay adapter that executes a successfully captured ambient Time read in the source debugger.

**Evidence:** `jet run src/replay.jet --record=clock2 -- foo` prints `captured:1788388744998`, `capture: finalized outcome=exit status=0`, and `artifact: .jet/replays/clock2.jetproof-replay`. `jet debug src/replay.jet --replay=clock2` opens `Time; dev-tir-v1`, stops at `time.now()`, then returns `[E0956] static time.now isn't supported by the current evaluator yet`.

**Areas:** data, backend, embedded, ai-ml. **Severity:** blocks.

### prim-receipts-G3 — defect

**Primitive:** Replay queries that render user-defined evidence with its Jet source type name rather than an internal generated name.

**Evidence:** `printf 'when evidence\\ncontinue\\n' | jet debug src/evidence.jet --replay=evidence` returns `evidence = __jet_RegressionEvidence { __jet_slope: 1.9, ... }`. `crates/jet-debug/src/lib.rs:3-6` promises Jet-source values and says generated Rust is never surfaced.

**Areas:** data, backend, embedded, ai-ml. **Severity:** hurts.

## Friction

- Named replay capture requires a project-relative target. Running `jet run` from the repository root with an absolute scratch path fails `Error [E3629]: Entry identity must be inside the current project`; capture must run with the scratch package as cwd.
- The launcher emits a scratch-cwd cleanup-script path warning before each command; Jet commands still complete.
- Safe `jet prove --capture=.jet/replays/proof.jetproof-replay` rejects ordinary `print` output as raw IO with `Error [E3627]`; safe capture is Time-only. The normal `jet run --record` path does record a successful IO-producing run.
- There is no `jet inspect receipt` or `jet receipts` query surface in `jet help inspect`/`jet help`; `jet status` is a narrow claim summary.

## Research mined

- `~/.cache/jet-luna/dx2/mlops/report.md:103-106,113-124,155-156`: W&B/MLflow/DVC all put named runs, typed config/metrics, metadata, and run diff/search beside results; Jet's typed data and `.jettrace` are present, but run records and run diff are marked real gaps.
- `~/.cache/jet-luna/dx2/observability-platforms/report.md:77-91,120-125`: Jet `.jettrace`, `jet perf compare`, and payload-free observation work; named replay and `why`/`when` are ratified but still need receipt-backed completion, and no extension seam is documented.
- `~/.cache/jet-luna/dx2/proof-formal/report.md:84-94`: typed ProofReport, authority-bound replay, and `.jetproof` are the intended shared evidence shape; this current binary's `jet prove --json` smoke now passes, but it still has no user-domain facet path.
- Deleted `~/.cache/jet-luna/dx2/ballots/D-M-RECEIPTS1.json:7-9,28-31`: the historical “one typed receipt” decision explicitly identified versioned domain facets as the missing attachment mechanism. It is archived context, not shipped API evidence.

## Defects

- G2 is a successful `--record` followed by a replay failure at the one captured `core.time.now` operation.
- G3 violates the debugger's own source-level value promise by exposing `__jet_RegressionEvidence` and field names.

## Battery notes

Not applicable: this is a primitive probe, not one of the eight critical-area probes.

## Verdict

- Typed domain evidence is buildable as an ordinary Jet record and can be serialized or retained in pure replay acts.
- The core receipt codec has no user-level typed facet attachment, query, or cross-run diff capability.
- `jet perf compare` compares timing traces only, not `.jetproof-replay` evidence.
- Ambient Time capture can succeed but cannot currently be executed by source-level replay.
- A shared extensible receipt facet primitive is required for data, backend, embedded, and AI/ML authors.
