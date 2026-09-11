# Earned-preference experience campaign

Campaign parent: `#2386`  
Rerun child: `#2393`  
Hostile closeout child: `#2394`  
Run identity: `2393-r1`

This document is the campaign control record. It freezes the pass bar, the
matched-run protocol, and the disposition ledger. It is not a scorecard and it
does not turn missing evidence into a zero or a pass. The frozen Jetpack canary
at `dogfood/jetpack/` is evidence only; this campaign does not edit it or
resume parity work. Parity resumption remains owner-gated by `#2327`.

## Fixed pass bar

The campaign passes only when every condition below is true:

- Ten fresh participants complete both arms over the four matched tasks.
- Every expected raw receipt is present, digest-verified, and marked measured,
  or an explicit stop receipt is retained and the campaign fails. A missing
  value is `not measured`, never zero, pass, parity, or not applicable.
- Every participant gives Jet a median of `5/5` in reading, writing, reasoning,
  creating, modifying, diagnostics, and tooling/docs.
- No underlying Jet category rating is below `4/5`.
- Both arms are project-green for every task: clean check, exact default output,
  and exact optimized/native output.
- The full ten-participant Jet campaign median is `5` in all seven categories.
- After both arms finish, every participant chooses Jet in the neutral blind
  preference question. Rust or no preference fails, and a missing answer fails.
- Every actionable finding and every concrete negative testimony statement has
  shipped evidence, an owning card or ballot, an existing owner, or an explicit
  recorded decline. An unresolved row fails the campaign.

A mean, source-size ratio, retrospective answer, single successful task, or
omitted receipt cannot satisfy this bar.

## Preregistered protocol

Freeze the task contracts, fixture bytes, expected outputs, tool versions, agent
configuration, repair limit, arm order, score rules, and manifest before the
first implementation session. Record SHA-256 digests for exact task, fixture,
prompt, source, and receipt bytes.

### Agent sourcing

Use ten fresh participants, `A01` through `A10`: five OpenAI-family sessions and
five Anthropic-family sessions. Pin model/version, context budget, tool policy,
temperature, seed, timeout, and repair limit within each family. Each
participant receives:

1. One cold implementation session for Jet.
2. One cold implementation session for Rust.
3. One comparison session that receives only the two finished artifacts, the
   task contracts, observed outputs, and the neutral question.

Implementation sessions receive no campaign report, testimony table, Tower
history, owner preference, mechanism-card status, or source from
`dogfood/jetpack/`. A participant with campaign history is excluded and
replaced before any score is recorded. The language cannot remain hidden after
an arm starts writing, so blinding hides the expected winner, campaign history,
score target, and arm order.

### Matched task contracts

Both arms receive the same behavior, input bytes, argv, exit contract, and
expected output. Only language name, source filename, and compiler command
differ. Agents must compute results from input; hardcoded fixture output fails.
The complete frozen contracts and fixtures are also retained in
`docs/audits/fresh-agent-5-of-5-rerun-2026-08-30.md`.

#### T1 — string-heavy configuration parsing

Read `records.cfg` from the first argument. Parse section headers, quoted
`key = value` fields, and one-level `inherits`. Print each resolved section in
source order and fields in first-appearance order as `section.key=value`.
Reject a duplicate key in one section with non-zero exit and no stdout, naming
the duplicate field and section in stderr.

Success fixture:

```text
[base]
name = "Ada"
role = "admin"
[dev]
inherits = "base"
role = "reviewer"
```

Expected stdout:

```text
base.name=Ada
base.role=admin
dev.name=Ada
dev.role=reviewer
```

#### T2 — typed CLI dispatch

Implement `list`/`ls`, `inspect`, and `remove`/`rm`; accept `--json`,
`--format text|json`, and `--dry-run`. Dispatch once to a typed command value.
Unknown commands and missing values exit non-zero with no stdout. The fixed
vectors are:

```text
tool ls --json alpha
tool inspect alpha --format text
tool rm alpha --dry-run
tool unknown alpha
```

The first three outputs are:

```text
list name=alpha format=json dry_run=false
inspect name=alpha format=text dry_run=false
remove name=alpha format=text dry_run=true
```

The final vector must fail with no stdout.

#### T3 — store and journal state

Read `journal.log` from the first argument. Replay `put|key|value` and
`delete|key|` records in order. Print final state by first insertion order as
`key=value`; duplicate `put` updates without moving the key, and delete removes
it. Unknown record kinds or malformed fields exit non-zero with no stdout.

Success fixture and output:

```text
put|alpha|1
put|beta|2
delete|alpha|
put|gamma|3
```

```text
beta=2
gamma=3
```

The failure fixture `put|beta|2` followed by `rename|beta|4` must reject the
unknown record and write no stdout.

#### T4 — deterministic wire output

Read `report.tsv`, parse the named `name`, `count`, and `note` fields, build a
typed report, and write exactly one canonical JSON object with key order
`count`, `name`, `note`. Escape control characters and quotes and write exactly
one trailing newline. The fixture is:

```text
name\tAda
count\t2
note\tline "one"
```

Expected bytes, including the final newline:

```text
{"count":2,"name":"Ada","note":"line \"one\""}
```

### Order and arm blinding

Derive task and arm order from the first bit of
`SHA-256("2393-r1:" + participant_id + ":" + task_id)` and record the
assignment before each session. Do not show the desired preference or campaign
bar to an agent. The comparison session receives randomized opaque labels and
asks:

> For the tested work, which arm would you choose for the next task of this
> kind? Choose Arm A, Arm B, or no preference. Give one reason.

The comparison sees both final sources, observed outputs, and the task contract
only after both arms reach a stop state. It receives no aggregate score or other
participant's answer. The mapping, exact answer, mapped language, and reason
are sealed in the comparison receipt.

### Repair and stop rules

Start each arm from a clean, disk-backed scratch directory and an empty
arm-specific build cache. Allow at most three correction passes after the
initial source; the initial source is pass zero. After a failed semantic check,
return exact compiler/checker output and do not disclose expected output before
a clean check. After a clean check, run the fixed default and optimized/native
commands and preserve exact output. Stop at project-green, the correction
limit, or an unrecoverable tool failure, whichever comes first.

Project-green means clean check, exact default output, and exact optimized/native
output. Jet uses its normal check, default run, and AOT/build path. Rust uses
`cargo check`, debug run, and release/native run. A standalone wrapper is a
recorded protocol deviation, not silently equivalent evidence. Record exact
commands, tool versions, exit statuses, stdout, stderr, and stop reason.

### Matched measurements

Apply the matched-input and `not measured` laws in
`dogfood/jetpack/METRICS.md` without editing that frozen file:

- Keep task bytes, argv, environment, expected result, source boundary, and
  tool-version identity equal across arms.
- Use fresh disk-backed caches for cold measurements; where a warmup applies,
  use one warmup and five samples.
- Restore measurement-only source edits byte-for-byte and verify the digest.
- Record physical source lines and Unicode-whitespace token counts for each
  final arm. These are source metrics, not model-token counts.
- Capture wall time, time to first stdout, peak RSS where supplied, correction
  passes, compiler-check time, and default/AOT run time.
- Record every diagnostic encounter, its seeded cause, whether the first
  diagnostic names it, and whether the agent resolved it.
- Preserve stdout, stderr, exit status, final source, command lines, environment
  identity, and raw timer output. A missing sample remains `not measured`.
- Do not use network, package downloads, or a shared mutable store. Keep Jet and
  Rust artifacts in separate roots under `~/.cache/jet-test-scratch`; delete
  scratch only after sealing the receipt.

### Raw scorecard receipt

Preserve one JSON receipt per participant, task, and arm at:

```text
docs/audits/raw/2393-r1/<participant>/<task>/<arm>.json
```

Each receipt includes frozen task/source/prompt digests, participant family,
opaque arm assignment, timestamps, final source, commands, raw stdout/stderr,
exit statuses, correction passes, diagnostics, wall-time samples, source
counts, and all seven category ratings. The comparison receipt includes the
exact preference answer and reason. A manifest lists all 80 expected receipts
and each SHA-256 digest. Missing or untrusted receipts fail; they are not
omitted from the denominator.

## Score rules

After each arm reaches its stop state, ask the same neutral question:

> Rate this arm for this task from 1 to 5. Use only the work you observed.
> Explain the lowest rating that applies.

The seven categories are reading, writing, reasoning, creating, modifying,
diagnostics, and tooling/docs. Anchors are:

| Score | Meaning |
| ---: | --- |
| 1 | Could not use the result or understand the work. |
| 2 | Needed substantial repair or outside explanation. |
| 3 | Worked, but common steps or concepts were hard to follow. |
| 4 | Worked with small friction; the main path was clear. |
| 5 | Worked with no material friction; the main path was immediately clear. |

For each participant and arm, take the median of the four task ratings in each
category. Take the campaign median over all ten participant medians. The Jet
side must have `5` for every participant/category median, every underlying Jet
rating must be at least `4`, and every category campaign median must be `5`.
Every preference receipt must choose Jet. Any lower median, raw rating below
`4`, Rust choice, no-preference choice, missing row, untrusted digest, or
unresolved testimony statement fails the campaign.

## Verification tooling

`scripts/agent/dogfood-campaign.mjs` is a read-only verifier. It never starts
agents, executes Jet or Rust programs, edits receipts, writes Tower state, or
changes the ledger. It validates the sealed manifest and receipt digests,
checks receipt identity and explicit incomplete states, checks blind mappings,
computes the participant and campaign medians, checks this protocol and all
finding rows, and requires dated rerun and hostile-closeout verdict artifacts.

Run the human report:

```text
scripts/agent/jet-env node scripts/agent/dogfood-campaign.mjs check \
  --manifest docs/audits/raw/2393-r1/manifest.json \
  --protocol docs/proposals/dogfood-jet-experience-5-of-5.md \
  --ledger docs/proposals/dogfood-jet-experience-5-of-5.md \
  --rerun-report docs/audits/fresh-agent-5-of-5-rerun-2026-08-31.md \
  --closeout-report <dated-2394-closeout-report>
```

Add `--json` for deterministic canonical JSON. `score` is an alias for
`check`. Exit status is `0` only for a complete pass, `1` for valid evidence
whose fixed gate is not met, and `2` for a usage or setup error. A closeout
report is required for a pass; the tool does not infer hostile closure from a
missing file.

## Current evidence

The 2026-08-31 execution is recorded at
`docs/audits/fresh-agent-5-of-5-rerun-2026-08-31.md`, with sealed receipts listed
by `docs/audits/raw/2393-r1/manifest.json`:

| Evidence | Observed result | State |
| --- | --- | --- |
| Expected raw receipts | 80 | fixed denominator |
| Measured raw receipts | 72 | recorded |
| Explicit unmeasured receipts | 8, all A04 after exit 137 at the 600-second stop limit | fail |
| Jet project-green receipts | 27/36 measured tasks; 27/40 against the fixed denominator | fail |
| Rust project-green receipts | 36/36 measured tasks; 36/40 against the fixed denominator | descriptive only |
| Full ten-participant medians | not measured because A04 has no score row | fail |
| Blind preference | 9/9 measured choices were Rust; A04 had no answer | fail |
| Dated rerun verdict | `FAIL — fixed 5/5 gate not met` | fail |
| Hostile closeout | no passing `#2394` report supplied | open |

The r1 deviations are retained, not normalized away: A01 had a permitted
adapter fallback and task-order deviation; Rust check mode used a standalone
`rustc` wrapper because the fixture had no Cargo project; A04 timed out and was
not replaced or retried; and A08/A10 rating retries preserve their failed
invocations.

The exact r1 blockers are retained as evidence, not normalized away: A01 T1
hit the Scheduler fact-registry ICE on the PATH-corrected native retry;
A02, A06, A07, and A08 hit Jet T1 native/AOT internal compiler failures; A03
T4 default execution hit E0956 (`JSON lenient decode coercion audit effects`);
and A04 Jet timed out at 600 seconds and exited 137 before its Rust arm ran.
Advisory Jet diagnostics remain encounters, while the seeded duplicate,
unknown, and rename failures were named and resolved in the measured command
set. These are the exact residuals that r2 must recheck.

The r1 report records these residual compiler, diagnostic, and experience
observations for r2; it does not convert them into passes. This verifier
deliberately does not run the compiler and therefore cannot claim a green build.

## Finding disposition ledger

Every actionable finding from
`docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md` has exactly one row
below. A disposition names the owning card, ballot, existing owner, or explicit
decline. The hostile closeout must report zero uncarded findings before this
campaign can pass.

| Finding | Category | Disposition |
| --- | --- | --- |
| F01 | docs/examples | Owned by `#2391`: golden-backed idiom suites are the canonical examples. |
| F02 | semantic | Owned by `#2388` for contract teaching and `#2391` for the failure suite; implicit failure law is `#2172` / D-FAILURE-FOUNDATION1. |
| F03 | semantic | Resolved in law by `#2172` / D-FAILURE-FOUNDATION1; residual diagnostic quality owned by `#2388`. |
| F04 | diagnostics | Owned by `#2388`: registered visibility diagnostic names the declaration to change. |
| F05 | diagnostics | Owned by `#2388`: E2404 names callee, span, effective contract, and two graded repairs. |
| F06 | diagnostics | Owned by `#2392`: caused-by links and root-first ordering at the projection seam. |
| F07 | tooling/check | Owned by `#2389`; check depth follows D-CHECKSCOPE1. |
| F08 | diagnostics | Owned by `#2387`: ownership diagnostics name callee, move site, and a graded `~` edit. |
| F09 | docs/examples | Owned by `#2391` wire-output suite with brace/interpolation pitfalls in-file. |
| F10 | tooling/check | Resolved by closed `#2352`; check-time entry-resolution proof owned by `#2389`. |
| F11 | tooling/check | Owned by `#2389`: file checks resolve owning graph or teach missing context. |
| F12 | stdlib/Core | Owned by `#2390`; public package model location follows D-PACKAGE-MODEL1. |
| F13 | stdlib/Core | No new mechanism under I8; teaching owned by `#2391`, default-run Codable parity by `#1310`. |
| F14 | build-perf | Existing owners `#666`, `#1023`, `#1026`, and `#2346`; warm-canary criterion extended on `#1026`; numeric baseline remains not measured. |
| F15 | lint/idiom | Owned by `#2395`: detector, graded rewrite, LSP action, and quiet cases. |
| F16 | lint/idiom | Evidence for `#2395`; canonical replacement is in the `#2391` dispatch suite. |
| F17 | lint/idiom | Canonical form is `#2173` / D-RESULT-DECON2; teaching owned by `#2391`, nesting evidence feeds `#2395`. |
| F18 | lint/idiom | Evidence for `#2395`: alias grouping with `|`. |
| F19 | lint/idiom | Owned by `#2391` finite-state suite: typed parser state over booleans. |
| F20 | lint/idiom | Evidence for `#2395`; early-guard quiet case is a named criterion. |
| F21 | lint/idiom | Evidence for `#2395`: selector-key alias table. |
| F22 | lint/idiom | Evidence for `#2395`: dispatch once, then validate. |
| F23 | lint/idiom | Evidence for `#2395`: provider dispatch table. |
| F24 | docs/examples | Owned by `#2391` finite-state suite; enum, variant group, tag, and typestate replace string state. |
| F25 | docs/examples | Absorbed by `#2391`; suite structure demonstrates parse/facts/render separation without editing the frozen canary. |
| F26 | docs/examples | Owned by `#2391` wire-output suite: byte-exact canonical writer and Codable round trip. |
| F27 | docs/examples | Owned by `#2388` for visibility/contract teaching and `#2391` for the failure suite; laws are `#2172`, `#1712`, and `#2326`. |
| F28 | process | Product half owned by `#2389`; continuous-check process is a named `#2393` protocol requirement. |
| F29 | bug | Existing owner `#1310`; listed as a rerun blocker on `#2393`. |
| F30 | bug | Resolved by closed `#2252`; registry conformance corpus is `#2285` / `#2286`. |
| F31 | bug | Resolved by closed `#2350`. |
| F32 | bug | Resolved by closed `#2352`. |
| F33 | bug | Resolved by closed `#2354`; canonical form is `#2173` / D-RESULT-DECON2. |
| F34 | bug | Resolved by closed `#2368`; formatter defect is identified in the dogfood report. |
| F35 | bug | Resolved by closed `#2369`. |
| F36 | bug | Resolved by closed `#2370`. |
| F37 | bug | Resolved by closed `#2371`; cold/warm timing remains not measured and incremental work is F14. |
| F38 | bug | Resolved in shipped Rust jetpack as `#2355`; no Jet-side card is claimed. |
| F39 | process | Standing sovereign-package constraint; explicit non-goal on `#2389` and `#2395`, no new card. |
| F40 | process | Owner-gated by `#2327`; no card edits `dogfood/jetpack/**`. |
| F41 | tooling/check | Root defects resolved by closed `#2350` and `#2066`; rerun re-proves the workflow and reopens owners on failure. |
| F42 | process | Owned by campaign parent `#2386`; measured as correction-pass count in the `#2393` protocol. |
| F43 | rerun-protocol | Owned by `#2393`: every task has a matched Rust arm from cold context. |
| F44 | rerun-protocol | Owned by `#2393`: wall time per arm is a named measure. |
| F45 | rerun-protocol | Owned by `#2393`: rust-analyzer baseline is required or the gap is explicit. |
| F46 | rerun-protocol | Explicitly declined for this bounded campaign; long-term maintenance needs a longitudinal study. |
| F47 | rerun-protocol | Owner-gated by `#2327`; full provider/package-universe parity is paused Jetpack replacement work. |
| F48 | rerun-protocol | Owned by `#2393`: blind preference follows both completed arms. |
| F49 | rerun-protocol | Owned by `#2393`: repeatability and missing-data rules follow the matched-input and `not measured` laws. |
| F50 | diagnostics/io | Owned by `#2417`; raw witness is `raw/2393-r1/A03/T4/jet.json`, current fixture is `tests/ui/ioerror_display_migration.jet`. |
| F51 | diagnostics/idiom | Closed by `#2416`; receipt, fixture, behavior proof, and hostile-ledger proof are recorded in the raw evidence. |
| F52 | bug/compiler | Closed by `#2415`; stale Scheduler ICE witness, fresh replay receipt, and durable duplicate-registration regression are recorded. |
| F53 | bug/hostile-input | Closed by `#2419`; fresh matched Jet/Rust receipts and durable malformed-input fixtures cover the full T4 failure matrix. |

## Testimony closure ledger

The original testimony table is at
`docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md:289-304`.
Preference qualifiers are retained as `T-PREF`; they are not production claims.

| ID | Testimony | Finding(s) | Owner or evidence | State at r1 gate |
| --- | --- | --- | --- | --- |
| T01 | Hand-written state machines and brace/interpolation confusion; teach canonical table-driven parsing. | F01, F09, F19, F24 | `#2391` finite-state, dispatch, and wire suites | open |
| T02 | Large scanner/renderer caused repeated ownership edits; add typed profile parsing and deterministic output. | F12, F13, F25, F26 | `#2390`, `#2391` | open |
| T03 | Project-root imports and incomplete module checks; check from any source file with project context. | F07, F10, F11 | `#2389` | open |
| T04 | Direct fallible-call matching propagated instead of binding; make Result matching reliable. | F33 | closed `#2354`; rerun confirmation remains open | partial |
| T05 | Entry resolution needed adapters and several checks; follow the checked source graph. | F10, F31, F32 | closed `#2352`; project-check proof `#2389` | partial |
| T06 | Reusing consuming Core values required many `~` copies; improve move diagnostics. | F08 | `#2387` | open |
| T07 | Cross-module AOT and test reachability needed correction; make modular AOT discovery reliable. | F31, F41 | `#2350`, `#2066` | partial |
| T08 | Error-domain mismatches appeared at call sites; give fix-its naming the source helper. | F03, F05 | `#2388` | open |
| T09 | View materialization and literal-brace syntax caused repair work; improve ownership guidance. | F08, F09 | `#2387`, `#2391` | open |
| T10 | Helper error domains and public error visibility were hard to learn; give first-class guidance. | F03, F04, F05 | `#2388` | open |
| T-PREF | Participants supplied production-negative qualifiers rather than a production preference. | F48 | `#2393` blind comparison; 9/9 measured choices were Rust | measured; fail |

F46 is the explicit longitudinal-study decline. F47 remains owner-gated by
`#2327`; neither is silently dropped or replaced by the smaller matched task
set.

## Gate assessment

| Criterion | State | Evidence |
| ---: | --- | --- |
| 1 | done | The 53 finding rows above each name an owner, closed evidence, ballot, standing constraint, or explicit decline. The verifier checks all IDs and rejects duplicates or empty dispositions. |
| 2 | open | Child rerun and hostile closeout evidence do not satisfy the fixed 5/5 bar: r1 has 72 measured receipts, eight A04 stop receipts, 27/36 measured Jet green tasks, and 9/9 measured Rust preferences. |

Criterion 2 stays open until the fixed ten-participant rerun passes and the
hostile closeout reports zero uncarded findings. No missing or failed receipt is
silently converted into a score.

## Dependencies and owner gates

The mechanism cards `#2395`, `#2387`, `#2388`, `#2391`, and `#2392`, the
existing owner `#1310`, and the r1 repair cards `#2415`, `#2416`, `#2417`, and
`#2419` are not replaced by this campaign. Their evidence and ownership remain
in the F50-F53 rows; a later rerun may reopen an owner only for a new failure.
`#2393` must rerun after its current blockers settle, preserving the
ten-participant denominator and all raw receipts. `#2394` then performs hostile
closeout. The only explicit owner choices in this record are D-CHECKSCOPE1 on
`#2389`, D-PACKAGE-MODEL1 on `#2390`, and the existing parity gate D-2327 /
`#2327`.

## 2393-r2 rerun addendum (2026-09-05)

The r2 operational protocol, matched-task neutrality matrix, fail-closed
outcome rules, and explicit manifest/receipt/comparison schema are recorded in
[`docs/audits/fresh-agent-5-of-5-rerun-2026-09-05.md`](../audits/fresh-agent-5-of-5-rerun-2026-09-05.md).
The formal JSON schema is
[`docs/audits/raw/2393-r2/manifest.schema.json`](../audits/raw/2393-r2/manifest.schema.json).

The exact preregistered task and fixture record is
[`docs/audits/raw/2393-r2/fixtures.json`](../audits/raw/2393-r2/fixtures.json);
it contains inputs and expected outcomes only, not participant evidence.
These are an addendum to this control record, not a replacement for the frozen
task contracts, measurement laws, or r1 evidence.

The r2 manifest must carry `jet.strict-performance-policy.v1` with
`per_cell_and_metric` comparison, Rust win `<1`/parity `<=1.05` as measurement
noise only/loss `>1.05`, every non-Rust win `<1`/loss `>=1`, and fail-closed
states `missing`, `wrong`, `unavailable`, `uncovered`, `mismatched`, and
`inconclusive`; the JSON schema encodes these fields.

Required coverage remains the foundations (numerics, text, files, concurrency,
networking, build time, and run time) plus real workloads in web, games,
CLI/scripts, data analysis, backend services, AI/ML, GUI, and embedded; a niche
is required only after a first-party battery exists. No average, substitute
workload or tier, omitted peer, semantic trade, or historical receipt rewrite
weakens the gate.

The r2 identity is `2393-r2`. It keeps the ten participants, four real tasks,
both language arms, exact fixtures, expected bytes, and fixed pass bar. The
fixture record also seals the seven malformed T4 cases from #2419, with byte
digests and required diagnostics; each applicable tier must execute all seven.
Each task uses isolated task-arm sessions so the pre-registered arm order is honored
for each task. The r1 witnesses remain mandatory checks: A01's Scheduler
fact-registry ICE, A02/A06/A07/A08 T1 native/AOT ICEs, A03 T4 `E0956`, A04's
600-second/exit-137 stop, and A10's seven-case malformed T4 matrix. A live
witness stops the run; it is not designed out, retried away, or converted into
a passing row.

Witnesses are prerequisites, not inclusion filters. If a witness now passes,
the same participant/task/arm remains in the r2 set and denominator; only its
preflight witness status changes to `resolved`. The fixture bytes, argv,
semantic expected output, required tiers, and task identity do not change. If
it fails, the same row remains with its explicit failure or stop receipt and
owner. The runner must never drop a task because the bug it exposed was fixed
or alter an expected outcome to make a failure pass. The dated r2 report links
the exact command-and-observable unblock checklist.

### R2 ledger entry

| Evidence | R2 result | State |
| --- | --- | --- |
| Expected arm-task receipts | 80 | fixed denominator |
| Measured arm-task receipts | not measured; no r2 manifest exists | blocked |
| Explicit stop receipts | not measured; no participant session started | blocked |
| Jet/Rust project-green tasks | not measured | blocked |
| Seven-category Jet medians and raw-score floor | not measured | blocked |
| Blind preference | not measured; no comparison session started | blocked |
| Testimony closure | no new r2 closure evidence; all original rows remain mapped | open |
| Dated r2 verdict | `BLOCKED — execution did not start` | fail closed |

The 2026-09-05 preflight attempted
`scripts/agent/jet-env cargo build --release -p jet` and failed in
`TimeMonotonic.rs` with `E0599` (`Option<fn() -> Option<i64>>` has no
`flatten`). `scripts/agent/jet-env jet --version` also failed because the local
debug binary is absent and printed `fix: cargo build`. The historical cold-agent
preflight records missing OpenAI and Anthropic adapter configuration; the
current direct adapter check reports `ADAPTER CHECK OK: OpenAI+Anthropic`, but
no participant session started. These observations block the r2 run; they are
not participant results.

No r2 source, scorecard, receipt, comparison, or preference is claimed. The
testimony closure table above remains the complete row-by-row disposition:
F46 is explicitly declined for this bounded campaign and F47 remains gated by
`#2327`; every other row retains its named owner or evidence. A future run must
append its measured scores, blind answers, and residual findings here without
rewriting this blocked entry.
