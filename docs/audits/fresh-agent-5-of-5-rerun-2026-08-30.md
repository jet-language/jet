CHECK OK

# Fresh-agent 5/5 rerun

Date: 2026-08-30  
Card: `#2393`  
Status: `BLOCKED`  
Verdict: `FAIL — the fixed 5/5 bar is not met`

The protocol is recorded below. The run did not start because the mechanism
slate remains open. No raw score, timing sample, diagnostic count, or preference
answer exists. This report does not turn missing data into a pass.

## Fixed pass bar

The campaign passes only when all conditions hold:

- Each participating agent gives Jet a median of `5/5` in reading, writing,
  reasoning, creating, modifying, diagnostics, and tooling/docs.
- No raw Jet category rating is below `4/5`.
- After both arms, every participating agent chooses Jet in the neutral
  preference question. Rust or no preference fails the gate.
- Every concrete negative testimony statement has shipped evidence or an
  owner-recorded decline in the ledger.

The campaign does not pass on a mean, a source-size ratio, a retrospective
answer, a single successful task, or a missing value.

## Preregistered protocol

Run ID: `2393-r1`. Freeze the task contracts, fixtures, expected outputs, tool
versions, agent configuration, repair limit, arm order rule, and score rules
before the first implementation session. Record SHA-256 digests for the exact
task and fixture bytes in the run manifest.

### Agent sourcing

Use ten fresh participants: five sessions from the OpenAI adapter and five from
the Anthropic adapter. Pin the model, model version, context budget, tool
policy, temperature, seed, timeout, and repair limit within each family. Give
each participant:

1. One cold implementation session for Jet.
2. One cold implementation session for Rust.
3. One comparison session that receives only the two finished artifacts,
   their task contracts, and the neutral question.

The implementation sessions receive no campaign report, testimony table,
Tower history, owner preference, mechanism-card status, or source from
`dogfood/jetpack/`. They receive only the task contract, the named language
reference, and the normal compiler tools. A participant that has campaign
history is excluded and replaced before any score is recorded.

The language itself cannot stay hidden after an agent starts writing code. The
blinding therefore hides the expected winner, campaign history, score target,
and arm order. The comparison session calls the results `Arm A` and `Arm B`.

### Matched task contracts

Each task is a bounded real-tool slice from the stressed domains in the
original report. Both arms receive the same behavior, input bytes, argv, exit
contract, and expected output. Only the language name, source filename, and
compiler command differ. Agents must compute results from input; hardcoded
fixture output fails the task.

#### T1 — string-heavy configuration parsing

Prompt: write a complete program that reads `records.cfg` from its first
argument. Parse section headers, quoted `key = value` fields, and one-level
`inherits`. Print each resolved section in source order, with fields in first
appearance order, as `section.key=value`. Reject a duplicate key in one section
with a non-zero exit and no stdout.

Fixture:

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

Failure fixture:

```text
[base]
name = "Ada"
name = "Grace"
```

The failure arm must exit non-zero, write no stdout, and identify the duplicate
field and section in stderr.

#### T2 — typed CLI dispatch

Prompt: write a complete command-line program with `list`/`ls`, `inspect`, and
`remove`/`rm` commands. Accept `--json`, `--format text|json`, and
`--dry-run`. Dispatch once to a typed command value. Print only the selected
operation and its options. Unknown commands and missing values must exit
non-zero with no stdout.

The runner invokes the same argv vectors in both arms. `tool` names the
executable; the program receives the arguments after that name.

```text
tool ls --json alpha
tool inspect alpha --format text
tool rm alpha --dry-run
tool unknown alpha
```

The first three outputs are, in order:

```text
list name=alpha format=json dry_run=false
inspect name=alpha format=text dry_run=false
remove name=alpha format=text dry_run=true
```

The last invocation must fail with no stdout. The contract tests aliases,
ordered option handling, one dispatch point, and a root-cause error path.

#### T3 — store and journal state

Prompt: write a complete program that reads `journal.log` from its first
argument. Replay `put|key|value` and `delete|key|` records in order. Print the
final state by first insertion order as `key=value`. A duplicate `put` updates
the value without moving the key. A delete removes the key. Unknown record
kinds or malformed fields must exit non-zero and write no stdout.

Fixture:

```text
put|alpha|1
put|beta|2
delete|alpha|
put|gamma|3
```

Expected stdout:

```text
beta=2
gamma=3
```

Failure fixture:

```text
put|beta|2
rename|beta|4
```

The failure arm must reject the unknown `rename` record and write no stdout.

#### T4 — deterministic wire output

Prompt: write a complete program that reads `report.tsv` from its first
argument. Parse the three named fields, build a typed report value, and write
one canonical JSON object. Keys must use the order `count`, `name`, `note`.
Escape JSON control characters and quotes. Write exactly one trailing newline.

Fixture: each `\t` below is one U+0009 tab byte.

```text
name\tAda
count\t2
note\tline "one"
```

Expected bytes, shown as text:

```text
{"count":2,"name":"Ada","note":"line \"one\""}
```

The runner compares bytes, including key order, escaping, and the final newline.
The task measures typed report construction and exact wire behavior without
using Jetpack parity code.

### Order and arm blinding

Assign stable participant IDs `A01` through `A10`, alternating the two model
families. Derive the task and arm order from the first bit of
`SHA-256("2393-r1:" + participant_id + ":" + task_id)`. Record the assignment
before the session starts. Do not show the desired preference or the campaign
bar to an agent. Ask the comparison question with randomized labels and option
order:

> For the tested work, which arm would you choose for the next task of this
> kind? Choose Arm A, Arm B, or no preference. Give one reason.

The comparison session receives both final sources, their observed outputs,
and the task contract only after both arms reach their stop state. It receives
no aggregate score or other agent answer.

### Repair and stop rules

Start each arm from a clean, disk-backed scratch directory and an empty
arm-specific build cache. Give the agent the same maximum of three correction
passes after the initial source. Count the initial source as pass zero.

After a failed semantic check, return the exact compiler or checker output. Do
not return expected-output details before the source reaches a clean check.
After a clean check, run the fixed default and optimized/native commands. Give
the same pass/fail result and exact observed output to both arms when repair is
needed. Stop at project-green, the correction limit, or an unrecoverable tool
failure, whichever comes first.

Project-green means a clean check, exact expected output on the default tier,
and exact expected output on the optimized/native tier. Jet uses its normal
`check`, default run, and AOT/build path. Rust uses `cargo check`, debug run,
and release/native run. The runner records the exact commands and tool
versions.

### Matched measurements

Apply the applicable laws in
[`dogfood/jetpack/METRICS.md`](../../dogfood/jetpack/METRICS.md), especially
lines 7-14 and 34-51:

- Keep task bytes, argv, environment, expected result, source boundary, and
  tool-version identity equal across arms.
- Use fresh disk-backed caches for cold measurements. Use one warmup and five
  samples for executable latency measurements where a warmup applies.
- Restore any measurement-only source edit byte-for-byte and check its digest.
- Record physical source lines and Unicode-whitespace token counts for each
  final arm. These are source metrics, not model-token counts.
- Capture wall time, time to first stdout, peak RSS where the timer supplies it,
  correction-pass count, compiler-check time, and default/AOT run time.
- Record every diagnostic encounter, the seeded cause, whether the first
  diagnostic names it, and whether the agent resolved it.
- Keep stdout, stderr, exit status, final source, command lines, environment
  identity, and raw timer output. A missing sample is `not measured`, never
  zero, pass, parity, or not applicable.

No network, package download, or shared mutable store is allowed. The runner
must keep Jet and Rust artifacts in separate owned scratch roots and delete
scratch after the receipt is sealed.

### Raw scorecard receipt

Preserve one JSON receipt per participant, task, and arm at:

```text
docs/audits/raw/2393-r1/<participant>/<task>/<arm>.json
```

Each receipt must include the frozen task and source digests, participant
family, opaque arm assignment, prompt and response timestamps, source, all
commands, raw stdout/stderr, exit statuses, correction passes, diagnostic
records, wall-time samples, source counts, and seven category ratings. The
comparison receipt must include the exact preference answer and reason. A
manifest must list every expected receipt and its SHA-256 digest. Missing
receipts fail the campaign; they are not omitted from the median.

## Score rules

After each arm reaches its stop state, the participant rates that arm for each
task. Use the same neutral prompt:

> Rate this arm for this task from 1 to 5. Use only the work you observed.
> Explain the lowest rating that applies.

Use these anchors for all seven categories:

| Score | Meaning |
| ---: | --- |
| 1 | Could not use the result or could not understand the work. |
| 2 | Needed substantial repair or outside explanation. |
| 3 | Worked, but common steps or concepts were hard to follow. |
| 4 | Worked with small friction; the main path was clear. |
| 5 | Worked with no material friction; the main path was immediately clear. |

The seven categories are reading, writing, reasoning, creating, modifying,
diagnostics, and tooling/docs. The receipt records the category wording and
the participant's reason without coaching or post-hoc editing.

For each participant and arm, take the median of the four task ratings in each
category. The campaign median is the median of those ten participant medians.
The Jet side must have `5` for every participant/category median and every
underlying Jet rating must be at least `4`. The campaign median must be `5` in
all seven categories. Every preference receipt must choose Jet. Any lower
median, raw rating below `4`, Rust choice, no-preference choice, missing row, or
unresolved testimony statement fails the campaign.

## Run evidence

| Evidence | State | Meaning |
| --- | --- | --- |
| Protocol | `recorded` | This document freezes the method before a run. |
| Fresh participants | `not measured` | No external agent sessions started. |
| Matched Jet/Rust tasks | `not measured` | No task arm ran. |
| Raw per-agent scorecards | `none` | No fabricated rows. |
| Category medians | `not measured` | No pass claim. |
| Wall time and diagnostics | `not measured` | No sample exists. |
| Blind preference | `not measured` | Both arms did not finish. |
| Testimony ledger | `mapped; open` | See the proposal ledger and residual owners below. |

The existing cold-agent preflight is not this run. Its scoreboard at
[`cold-agent-jet-scoreboard.json`](cold-agent-jet-scoreboard.json) is
`status: blocked`, has zero rows, and names missing OpenAI and Anthropic adapter
configuration. It cannot supply campaign scores.

## Testimony and residual findings

The complete row-by-row mapping is in
[`docs/proposals/dogfood-jet-experience-5-of-5.md`](../proposals/dogfood-jet-experience-5-of-5.md),
section `#2393 rerun record`. Closed implementation fixes have named evidence
there. Open experience obligations remain on `#2391`, `#2387`, `#2388`,
`#2389`, `#2390`, and `#2393`; `#1310` remains a rerun blocker.

F46 is explicitly declined for this bounded campaign because it needs a
longitudinal study. F47 remains owner-gated by `#2327` and is not silently
replaced with this smaller task set.

## Gate assessment

| Criterion | State | Evidence |
| ---: | --- | --- |
| 1 | `done` | This report: protocol, task contracts, sourcing, blinding, controls, and scoring rules. |
| 2 | `open` | No matched run; no raw receipts. `cold-agent-jet-scoreboard.json` has zero rows. |
| 3 | `open` | No category medians or preference answers; fixed bar is not met. |
| 4 | `open` | All testimony rows are mapped, but unresolved owners remain and no fresh closure evidence exists. |
| 5 | `open` | Dated report and ledger section exist; scores, preference, and residual closure are not measured. |

## Conclusion

The campaign cannot claim earned Jet preference on 2026-08-30. The protocol is
ready for a later run after the mechanism slate and `#1310` clear. The next run
must preserve every raw receipt and must reopen the owning card for any score,
diagnostic, or testimony failure.
