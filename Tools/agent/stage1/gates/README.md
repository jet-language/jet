# Stage-one HARD performance gates

Each gate is executable and uses only its sibling helpers plus installed Node,
Python, perf/systemd, and the existing stage-one compiler protocol. No gate edits
Compiler/ or Core/. Growth and static failures are hard failures. Standalone
profiling reports its actual status; Main explicitly made profile collection
non-blocking for candidate builds and deferred it until OPEN when a hold exists.

## Call contracts

### `growth-gate.sh <ladder-run-dir>`

Reads the original `rungs.tsv`, `<rung>/result.env`, `<rung>/trace.json`, and (when
present) `ladder.env`. It fits least-squares `ln(metric)` against `ln(unit_bytes)`
for the ladder's canonical hierarchical phases, exclusive `(rest)`, startup+exit,
wall time, end RSS and cumulative peak RSS. The helper imports `readTrace`,
`phases` and `fitExponent` from ../ladder.mjs; its normal CLI remains unchanged.
Repeated completed spans in one row are summed; RSS uses category-end maxima.
The threshold is **strictly k > 1.3**, using unrounded trace values, including
short phases (no noise exemption). Exactly zero time at every distinct rung is
reported CONSTANT rather than fitted.

At least two distinct rung sizes and fitted RSS evidence are required. A phase
without two positive elapsed observations, missing requested rung, failed receipt
status, malformed trace or unfinished span prevents a pass. Completed phases of
failed runs are still printed for investigation; unfinished categories are excluded
from fitting. Failed/partial runs never establish successful scaling acceptance.

- **0:** complete available evidence, every fitted time/RSS exponent <= 1.3.
- **1:** any superlinear fit; prints offending phase, metric, k and sample count,
  even when there are additional availability failures.
- **2:** unavailable/incomplete evidence or incorrect invocation, without a
  demonstrated superlinear fit. A single-rung run cannot pass.

Existing cand24f/cand25f L1 and L5 were recorded in separate directories. Tests
combine copies of their original receipts/traces and retain source SHA256s in
`evidence-origin.json`; no traces or completion statuses are rewritten.

### `superlinear-static.mjs <tree> [paths...]`

Default paths are `Compiler` and `Core`. Optional paths are tree-relative Jet
files/directories, not a separate glob language. Recursively scans `.jet` files
and tokenizes comments, quoted/interpolated strings and balanced delimiters.
Rules cover nested variable-sized loops (including same-bound loops), linear
find/contains/index_of/remove and scan operations inside loops, growing String/list
`+` or `+=` and prefix interpolation, join-in-loop, copy-push, explicit `~`/clone
and by-value aggregate bindings/field expressions/parameters, and helper scans
propagated through the function call graph.

This is a conservative structural **review-required** checker, not a Jet
parser/typechecker or an asymptotic proof. Disjoint child traversals, fixed bounds,
indexed methods, small scalar structs and generated read-only borrowing can be
false positives. Field/alias/type inference is syntactic; opaque/native code and
generated Rust require separate dynamic growth and profiling evidence. Do not
interpret an empty hit list as proof of whole-compiler linearity.

Environment:

- `STATIC_ALLOWLIST`: JSON file; defaults to `<tree>/.superlinear-allowlist.json`
  when present. Array of `{ "key": "Compiler/path.jet:123:rule", "justification":
  "Specific bound, disjoint ownership or verified generated borrow evidence" }`.
  Keys are exact file:line:rule identities. Every entry needs a nonempty
  justification; duplicate/unknown/stale entries fail. There are no blanket globs,
  automatic allowances or production allowlist shipped by this gate.
- `STATIC_PROFILE`: phase-counts.json from the latest relevant L5 profile. Rank
  by maximum phase-inclusive sample count of each function; print phase and share.
  Jet `_u` symbol escaping is decoded for source-function matching. Unprofiled
  does not mean cold; partial profiles do not establish throughput acceptance.
- `STATIC_REPORT`: write the ranked Markdown hit list to this exact output path.

Exit **0**: no unallowlisted findings; **1**: at least one finding; **2**: bad
arguments, source/allowlist/profile unavailable or malformed. Each finding and
allowance is printed with its exact identity and rationale.

### `phase-profile.sh <jetc0> <rung-project>`

Project must contain `package.jet` and `src/compiler.jet`. Acquires
`~/.cache/jet-dev/jetc0-run.lock`, honors EndToEndQA/frontend-hold, copies the rung
into a private project/store, and runs jetc0 through:

`systemd-run --user --slice=jetwork.slice --scope -p MemoryMax=20G -p MemorySwapMax=0 timeout -k 10 1200 perf record -k CLOCK_MONOTONIC -m 64 -F 199 -g -- jetc0`

Environment controls:

- `PHASE_PROFILE_OUT`: evidence parent directory; creates a unique `run-<ns>`
  child on each invocation. Default is `<jetc0-dir>/phase-profiles/<rung-name>`.
- `PHASE_PROFILE_MEM_GIB`: 1..20 (default 20), requires cap + 2 GiB available.
- `PHASE_PROFILE_SECONDS`: 1..1200 (default 1200); no unbounded override.
- `PERF`: exact perf executable (default is the pinned Linux 7.0.11 store path).
- `PHASE_PROFILE_REPLAY`: **test-only** existing evidence directory containing
  clock, phases.trace, perf.script and done.json. Replays attribution without
  launching a compiler. Explicitly labelled REPLAY ONLY, retains original compiler
  status and cannot turn failed historical compilation into success.

Outputs: perf.data, perf.script, clock, phases.trace, compile.log, receipt,
done.json (command/caps/status), phase-counts.json and top-20.txt, all alongside
the candidate. Phase attribution reuses PerfAudit-1's paired wall/monotonic-clock
`profile-phases.py`, extended to all traced categories, top-20 leaf/native-inclusive
and Jet-inclusive symbols, and unioned repeated spans (no double-counting samples
within a category). Unclosed phases retain samples through the recording's last
sample and are labelled OPEN/partial; serialized end bounds remain finite.
Failed/capped compiles retain partial evidence and fail.
Attribution sweeps timestamp-ordered phase boundaries with active-category
refcounts instead of rescanning all spans for each sample.

Exit **0**: recorded compilation rc=0, complete receipt, and phase-attributed
samples; **1**: recorded/replayed compiler failure or incomplete receipt;
**2**: unavailable perf/systemd/evidence, closed proof window, unsafe resource cap
or no attributable samples. Replay rc=0 only reports the existing recording's
status, never a new compiler proof.

## Integration

`selfcheck.sh` and `fast-cand.sh` run the full ladder and invoke growth-gate.sh
as a failing step. Ladder/growth nonzero prevents candidate-build success.
Profiling status is separately retained but **does not fail a candidate build**.
When Main's frontend-hold is present, the wrappers log `profile deferred`, set
profile_rc=0 for that skipped step, and do not start another compiler. This zero
is only the orchestration-step status, **not** a passing profile receipt. A real
L5 profile runs after OPEN. Without the hold, collection uses L5 immediately.
Selfcheck writes OUTDIR/phase-profile; candidate writes KEEP/phase-profile.
Private rung generation uses OUTDIR/rungs to avoid shared-rung mutation.
SpeedHost owns fast-cand4.sh and has mirrored the hold/non-blocking behavior
there; PerfGates did not edit that script.

The static gate is independently callable; the live-tree audit deliberately fails
on unreviewed findings rather than manufacturing an allowlist.

## Exercised contract/evidence tests

`python3 ~/.cache/jet-dev/stage1/gates/test-gates.py [evidence-out] [live-tree]`

This uses small synthetic structural controls and replays existing evidence only;
it launches no Jet compiler. Logs and receipts are saved under
`~/.cache/jet-dev/scratch/PerfGates/gate-tests/`. Tests cover linear time/RSS,
time-only/RSS-only failure, missing rungs, exclusive-rest/startup superlinearity,
all static rules including aggregate field copies and non-assignment concatenation,
justified/unjustified allowlists, a numeric-loop negative control, cand24f/cand25f
single-rung and combined evidence, cand9, real L5 and capped-L1 replay (including
open hot-phase attribution), and the ladder's direct CLI through the migrated
cache namespace. Optional live-tree writes the ranked
audit. See receipts.json for exercised commands/statuses, not a claim
that these historically failed compiles passed.
The final run passed all **18 contract/evidence cases** with live-tree enabled.

Observed existing growth failures: cand24f L1/L5 decode k=2.797389 and sema.failure
k=1.672347; cand25f decode k=2.806885, sema.failure k=1.668717, comptime k=1.740311.
The original cand25f comptime run was contended (Main-confirmed); this is a hard
rejection of that measurement, **not** a demonstrated code regression. The quiet
L5 profile replay attributes decode SHA256::compress 89.71% leaf and line_col
5.62% leaf. Profiles and receipts remain partial compilation evidence (rc101).

A separate **actual recording-path test** ran cand25f on existing L1 sources with
MemoryMax=8G, MemorySwapMax=0 and timeout=30 seconds after L5Probe permitted
serialized admission. It ended compiler rc=124 / gate rc=1, perf-script rc=0,
and retained 468 parse samples with top-20 symbols. This exercises recording,
caps, attribution and failed-run retention; it is not a successful compilation.
Evidence is alongside cand25f in
`~/.cache/jet-dev/cand25f/phase-profile-gate-test/run-1791077897315783862/`
(resolved cache namespace: `~/.cache/jet-dev/cand25f/...`).
Attribution is also replay-tested from that actual capped recording, including
the unclosed comptime phase, without another compiler launch.

The live audit is `~/.cache/jet-dev/perf/STATIC-SUPERLINEAR.md`, ranked with
PerfAudit-1/regression-runs/c25f-L5/phase-counts.json. Its hit count represents
review obligations, not a count of independently proven quadratic bugs.
That run scanned 422 files and reported 18,093 unallowlisted obligations (rc1).
The capped-recording replay retained 2,959 comptime samples marked OPEN/partial.

## Scratch fix packets and proof admission

`~/.cache/jet-dev/scratch/PerfGates/patch-manifest.json` records the immutable
cand25f source baseline, prepared source hashes, and two targeted patches:

1. `01-trait-membership.patch`: one U64 membership owner for declared/queued MIR
   traits, preserving needed-row order and canonical-name refusal checks.
2. `02-ownership-exact-joins.patch`: exact loan/view identities replace repeated
   growing root/carrier bucket scans. Length-framed canonical Strings, tagged typed
   projections and exactly the former comparison fields are emitted as chunks,
   with one join per identity. First base rows and duplicate base rows are preserved;
   invalidation metadata updates the same first equal row; malformed non-reflexive
   places never deduplicate. Move-overlap/share-site scans are intentionally unchanged.

No live Compiler/Core edits. Rebase only targeted hunks, never overwrite a newer
provider file with the frozen scratch copy. These are structural fixes, not
measured speedups. After Main reopened the proof window, the guarded combined
assembled isocheck completed **rc=0, errors=0**, checking both changed files
against frozen cand25f inputs. `isocheck-receipt.json`, `isocheck-admission.json`
and `isocheck.log` record the priority lock, fresh >=20 GiB admission and result.
`run-isocheck.py` checks RAM/disk/hold *inside* the priority lock before executing
the real isocheck script. Its unguarded equivalent target command is:

```sh
flock -o ~/.cache/jet-dev/iso-priority.lock env \
  ISOCHECK_BASE=$HOME/.cache/jet-dev/scratch/PerfGates/tree \
  ~/.cache/jet-dev/isocheck-priority.sh PerfGates-ExactIndexes
```

Require at least 20 GiB available RAM (and enough disk) before admission. The
assembled check establishes source typechecking, not runtime equivalence or a
measured speedup. Main must still exercise loan/view equality, first-row
invalidation/order, trait-name refusals and successful geometric growth before
landing/performance acceptance.
