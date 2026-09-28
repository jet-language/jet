# Fresh-agent 5/5 rerun r2 protocol and preflight verdict — #2393

**Run ID:** `2393-r2`  
**Protocol freeze date:** 2026-09-05  
**Status:** **BLOCKED — execution did not start**  
**Verdict:** **NOT MEASURED. No earned-preference claim is made.**

This is the dated r2 operational addendum for Tower card `#2393`. It does not
replace the frozen campaign control record or the sealed `2393-r1` evidence. The
control record remains [`../proposals/dogfood-jet-experience-5-of-5.md`](../proposals/dogfood-jet-experience-5-of-5.md);
its r1 task contracts, pass bar, measurement laws, and finding ledger remain the
source of truth. The r2 manifest/receipt/comparison schema is
[`raw/2393-r2/manifest.schema.json`](raw/2393-r2/manifest.schema.json).

The exact r2 task/fixture record is
[`raw/2393-r2/fixtures.json`](raw/2393-r2/fixtures.json). It carries only
pre-registered inputs and expected outcomes copied from the sealed r1
contracts and the durable seven-case T4 hostile-input matrix from `#2419`; it
contains no participant or run result.

No r2 participant, source, scorecard, preference answer, receipt, or measurement
is present. The schema file is only a shape definition. It is not a receipt
manifest and does not manufacture a denominator row.

## Why r2 is not allowed to design around r1

The sealed r1 run measured 72 of 80 arm-task receipts, 27/40 Jet
project-green tasks, 36/40 Rust project-green tasks, and nine measured blind
choices, all for Rust. A04 stopped at the 600-second limit with exit 137 before
its Rust arm or comparison. Those are failures or missing measurements, not
permission to remove a participant, task, arm, or metric.

The r2 run must execute the same ten-participant, four-task set. In particular,
the following r1 witnesses remain mandatory preflight and run checks:

| r1 witness | r2 must do | If it recurs |
| --- | --- | --- |
| A01 T1 Jet Scheduler fact-registry ICE on the PATH-corrected native retry | Run the same T1 source through Jet check, default, and optimized/native paths; preserve the exact diagnostic and command evidence. | Stop the affected arm and reopen the compiler owner recorded for the Scheduler witness (`#2415`); do not substitute an easier T1. |
| A02, A06, A07, and A08 T1 Jet native/AOT internal compiler failures | Exercise the same T1 success fixture and native/AOT path for every fresh participant; do not classify a default-only success as project-green. | Preserve each receipt and reopen `#2415` for each task/participant finding. |
| A03 T4 Jet default `E0956` (`JSON lenient decode coercion audit effects`) | Run the same quoted-note TSV through the fixed default command before native evidence. | Preserve the error and reopen the IO/evaluator owner recorded for the witness (`#2417`). |
| A04 Jet timeout at 600 s, exit 137, before Rust | Apply the same per-session wall limit and retain an explicit stop receipt if reached. | Do not replace, retry, drop A04, or run a comparison without both arms; the run fails closed. |
| A10 T4 malformed-row matrix (the #2419 hostile closeout witness) | Run all seven sealed malformed inputs on every applicable tier and retain each failure receipt. | Preserve every case and its diagnostic; a skipped case or unavailable tier is an explicit stop and reopens `#2419`; do not shrink the matrix. |

A preflight that reports any live witness stops before participant scoring. A
preflight that clears them does not itself count as a participant result.

## Fixed pass bar and unfavourable path

The r2 result can come out against Jet. The campaign passes only if all ten
participants complete both arms over all four tasks; every expected receipt is
present, digest-verified, and measured; both arms are project-green on every
task; every Jet raw category score is at least 4; every participant's Jet
category median and every campaign Jet category median is 5; every comparison
is measured and chooses Jet; and every finding/testimony row is resolved,
owned, or explicitly declined.

Any one of these is a fail, not a reason to change the denominator: a missing or
untrusted receipt, a stop receipt, a Jet or Rust project-green miss, a Jet raw
rating below 4, a participant/category median below 5, a campaign median below
5, Rust or no-preference in a blind comparison, an unresolved testimony row, or
an unsealed digest. On a fail, preserve the exact task, transcript/session
reference, receipt, and finding, then reopen the owning mechanism card. The
runner must not average away a cell, discard an inconvenient participant, retry
only a failing arm, or change a fixture.

## R2 preregistered protocol

Before any participant session, seal the following files and digests in the r2
manifest:

- this protocol addendum;
- the control-record task contracts and exact fixture bytes;
- the expected outputs, failure outputs, and argv vectors;
- the assignment and blind-mapping metadata;
- the pinned Jet, Rust, OMP, Node, and runner identities; and
- the participant/arm/task receipt paths.

The r2 run ID is `2393-r2`. The r1 task contracts and fixture bytes are reused
byte-for-byte. Only the run identity and the resulting assignment/mapping
digests differ. A source must compute its result from its input; hardcoding the
fixture output is not a project-green result.

### Agent sourcing and isolation

Use exactly ten fresh participants, `A01` through `A10`: five OpenAI-family
sessions and five Anthropic-family sessions. Pin model/version, context budget,
tool policy, temperature, seed, timeout, and correction limit within each
family. Exclude and replace a participant with campaign history before any
score is recorded. If either adapter family cannot supply its pinned fresh
sessions, stop before the run and record the unavailability; do not fill the
row with an owner, a synthetic participant, or an old transcript.

For each participant, create separate disk-backed roots under
`~/.cache/jet-test-scratch/2393-r2/` for Jet and Rust. Start each task-arm
session from an empty arm-specific build cache. The r2 operational refinement
uses one cold implementation session per task and arm (eight isolated sessions
per participant). This does not change the four-task set; it ensures the
pre-registered arm order is actually honoured for each task, fixing r1 A01's
one-session-per-arm deviation.

Implementation sessions receive no campaign report, testimony table, Tower
history, owner preference, mechanism-card status, or source from
`dogfood/jetpack/`. They receive only the frozen task contract, the named
language reference, and normal compiler tools. The comparison session receives
no aggregate score, other participant answer, desired winner, or score target.

### Matched real-task and neutrality matrix

Each row is a real, bounded tool task drawn from the parser, CLI, state, and wire
workloads that exposed the original experience findings. Both languages receive
the same behavior, fixture bytes, argv, exit contract, and expected bytes. Only
the language, source filename, and compiler invocation differ.

| Task | Matched real work in both arms | Controls that prevent steering; real unfavourable result |
| --- | --- | --- |
| **T1 — string-heavy configuration parsing** | Read the same `records.cfg` from argv[1]. Parse section headers, quoted `key = value` fields, and one-level `inherits`; print resolved sections in source order and fields in first-appearance order as `section.key=value`. Run the same duplicate-key fixture. A duplicate must exit non-zero, print no stdout, and name the field and section in stderr. | Freeze the fixture, argv, output bytes, and duplicate failure before either source. Keep Jet check/default/native and Rust check/debug/release commands. Do not disclose expected output before a clean check. Any wrong ordering, hardcoded output, default/native ICE, or diagnostic with no project-green receipt is a fail and remains in the denominator. |
| **T2 — typed CLI dispatch** | Run the same vectors: `tool ls --json alpha`, `tool inspect alpha --format text`, `tool rm alpha --dry-run`, and `tool unknown alpha`. The first three must produce the frozen list/inspect/remove lines; the last must fail with no stdout. Both arms must dispatch once to a typed command value. | Pre-register all four argv vectors and outputs. Give both arms the same option order and no language-specific hint beyond the compiler command. Blind comparison sees only opaque labels after both arms stop. Unknown-command failure, missing-value failure, or a source that only handles the happy path is recorded as a failed task. |
| **T3 — store and journal state** | Read the same `journal.log`, replay `put|key|value` and `delete|key|` in order, retain first insertion order, update duplicate puts without moving keys, and remove deleted keys. The success output is the frozen `beta=2` / `gamma=3` state. The same `rename|beta|4` fixture must fail with no stdout. | Seal journal bytes, argv, output, and unknown-record failure before implementation. Do not permit a participant to choose a different store model or omit the malformed input. A wrong ordering, accepted `rename`, or missing error evidence fails the task for that arm. |
| **T4 — deterministic wire output** | Read the same tab-delimited `report.tsv` containing `name`, `count`, and a quoted `note`; build a typed report and emit exactly `{"count":2,"name":"Ada","note":"line \"one\""}` plus one trailing newline. Compare bytes, key order, escaping, and newline in both default and optimized/native paths. Run all seven sealed malformed fixtures (`truncated`, `empty-key`, `extra-column`, `unknown-key`, `duplicate-key`, `invalid-count`, and `missing-required-key`) on every applicable tier. | Freeze the literal tab and quote bytes, the seven malformed fixture bytes, and byte-level comparator. Ask for no aggregate score or preference before both arms stop. E0956, a key-order/escaping mismatch, missing newline, malformed-row acceptance/skipping, or native/default failure remains a failed receipt; no easier note or alternate serializer is permitted. |

The same neutral rating question follows each stopped arm/task:

> Rate this arm for this task from 1 to 5. Use only the work you observed.
> Explain the lowest rating that applies.

The seven recorded categories are `reading`, `writing`, `reasoning`,
`creating`, `modifying`, `diagnostics`, and `tooling_docs`. The anchors remain
1 = unusable, 2 = substantial repair, 3 = worked with hard-to-follow common
steps, 4 = worked with small friction, and 5 = no material friction.

### Assignment and blind comparison

For each participant/task, derive the first assignment bit from:

```text
SHA-256("2393-r2:" + participant_id + ":" + task_id)
```

A bit of 0 means `Jet, Rust`; a bit of 1 means `Rust, Jet`. Record the digest and
order in `.tmp/2393-r2/assignment.json` before that task's first arm starts.
Because each task has isolated arm sessions, the recorded order cannot silently
collapse into one participant-wide order.

For participant comparison mapping, derive:

```text
mapping_digest = SHA-256("2393-r2:comparison:" + participant_id)
```

Use bit 0 to assign distinct opaque labels `Arm A` and `Arm B` to Jet and Rust,
bit 1 as the recorded secondary mapping bit, and rotate the three-option order
`[Arm A, Arm B, no preference]` by byte 2 modulo 3. Persist the mapping before
the comparison prompt. The comparison receives both final sources, observed
outputs, and the task contract only after both arms reach a stop state. It asks:

> For the tested work, which arm would you choose for the next task of this
> kind? Choose Arm A, Arm B, or no preference. Give one reason.

The exact answer, mapping, language choice, and reason are sealed. Rust and
no-preference are genuine losing outcomes; a missing comparison is also a fail.

### Repair, timeout, and stop rules

Each arm starts from clean source and disk-backed cache state. The initial source
is pass zero; permit at most three correction passes. After a failed semantic
check, return the exact checker output and do not disclose expected output before
clean check. After clean check, run the fixed default and optimized/native
commands and preserve exact output. Stop at project-green, the correction
limit, a 600-second per-session wall limit, or an unrecoverable tool failure,
whichever comes first.

Project-green means clean check, exact default output, and exact optimized/native
output. Jet uses its normal check, default run, and AOT/build path. Rust uses
`cargo check`, debug run, and release/native run. A standalone wrapper is a
recorded protocol deviation, never silently equivalent evidence. Record exact
commands, tool versions, exit status, stdout, stderr, stop reason, wall time,
first-output time, peak RSS where supplied, and all diagnostics. A timeout or
unrecoverable tool failure produces an explicit stop receipt and stops the
campaign; it does not trigger replacement, retry, or denominator repair.

### Matched measurements

Apply the laws in [`dogfood/jetpack/METRICS.md`](../../dogfood/jetpack/METRICS.md)
without editing that frozen evidence file:

- keep task bytes, argv, environment, expected result, source boundary, and tool
  identity equal across arms;
- use fresh disk-backed caches for cold measurements and one warmup plus five
  samples where a warm measurement applies;
- restore measurement-only source edits byte-for-byte and verify the source
  digest;
- record physical source lines and Unicode-whitespace token counts, not model
  token counts;
- capture wall time, first stdout, peak RSS where available, correction passes,
  check time, and default/AOT time;
- record every diagnostic encounter, seeded cause, first-diagnostic actionability,
  and whether the agent resolved it; and
- preserve raw stdout/stderr, command lines, environment identity, timer output,
  source, and receipt bytes. A missing sample is `not measured`, never zero or
  pass.

No network, package download, or shared mutable store is allowed. Seal a receipt
before deleting its scratch root. Do not edit `dogfood/jetpack/**`.

## Explicit r2 evidence schema

The formal schema at [`raw/2393-r2/manifest.schema.json`](raw/2393-r2/manifest.schema.json)
defines the three JSON kinds below. The existing read-only verifier remains the
only campaign verifier; this schema adds no second runner or scorer.

### Manifest

The sealed manifest is `docs/audits/raw/2393-r2/manifest.json` and has:

- `schema_version: "2393-r2-manifest-v1"`, `card: 2393`, and `run_id: "2393-r2"`;
- `protocol_path` and `protocol_sha256` for this report;
- `task_contract_path` and `task_contract_sha256` for the control record;
- `fixture_manifest_path` and `fixture_manifest_sha256` for the exact fixture
  bytes and expected outputs;
- `assignment_path`/`assignment_sha256` and
  `comparison_mapping_path`/`comparison_mapping_sha256`;
- the required `strict_performance_policy` object, with the ratified policy
  identity, per-cell/per-metric comparators, and six fail-closed statuses;
- a pinned `toolchain` object containing Jet, rustc, OMP, Node, runner commit,
  capture time, and its digest;
- `expected_receipt_count: 80` and exactly one unique entry for every
  `A01`–`A10` × `T1`–`T4` × `Jet`/`Rust`; and
- exactly ten `comparison_receipts`, one per participant.

Each entry records participant, task, arm, the relative receipt path,
lowercase SHA-256, `status`, `complete`, and `project_green`. A measured entry
has `status: "measured"` and `complete: true`; a stop entry has
`status: "not_measured"` or `"unrecoverable_tool_failure"`, `complete: false`,
and `project_green: false`. The verifier additionally enforces all 80 unique
keys, receipt identity, digest equality, and explicit incomplete state.

### Arm receipt

Each measured or stop receipt is at
`docs/audits/raw/2393-r2/<participant>/<task>/<jet|rust>.json` and uses
`schema_version: "2393-r2-receipt-v1"`. It records card/run/participant/family/
task/arm identity; opaque arm label; assignment and blind-mapping evidence;
protocol and task/fixture/prompt digests; transport and session identity; source
path, exact final source and source metrics; status and stop reason; every
command with raw I/O/timers; task outcome; correction passes; diagnostics and
diagnostic policy; rating prompt/wording, raw response and session reference;
and all seven normalized ratings plus the reason when measured.

A measured receipt must contain a non-null source, task outcome with a boolean
`project_green`, integer ratings 1–5, and correction passes 0–3. A stop receipt
must retain a non-empty stop reason and explicit null source/outcome/rating;
there is no fabricated score.

### Blind comparison receipt

Each participant has
`docs/audits/raw/2393-r2/<participant>/comparison.json` with
`schema_version: "2393-r2-comparison-v1"`, card/run/participant identity, status,
blind mapping, prompt path/digest, preference, mapped choice, reason, raw
response, and comparison session identity. A measured comparison requires
both distinct labels, the exact opaque answer, its mapped language, and a
non-empty reason. A stop comparison has a non-empty stop reason and null mapping,
answer, and response. The verifier requires all eight measured task-arm receipts
before accepting a measured comparison.

There is intentionally no `2393-r2/manifest.json`, receipt, or comparison file
yet: the preflight below stopped before participant work. The schema is landed
so a future run cannot invent a different denominator or omit a failed arm.

## Preflight evidence on 2026-09-05

The release compiler prerequisite was attempted before any participant session:

```text
scripts/agent/jet-env cargo build --release -p jet
```

It failed with the first compiler error in the current tree:

```text
error[E0599]: no method named `flatten` found for enum `Option<fn() -> Option<i64>>` in the current scope
  --> crates/jet-foundation/src/../../jet-codegen/src/Prelude/Core/TimeMonotonic.rs:22:60
error: could not compile `jet-foundation` (lib) due to 1 previous error
```

The local development binary was also absent:

```text
scripts/agent/jet-env jet --version
```

Final output line:

```text
fix: cargo build
```

That command exited 1. The prior cold-agent preflight remains separate evidence:
`fresh-agent-5-of-5-rerun-2026-08-30.md:311-314` records no OpenAI or Anthropic
adapter configuration and zero rows. It cannot supply r2 participants or
scores.

The release compiler is still the blocking prerequisite. The current adapter
configuration check is recorded in the checkable gate below; it reports that
the OpenAI and Anthropic profiles are configured, but no participant session
was started.

Because the compiler build failed, `jet check`, `jet run`, `jet build`, the Rust
arm, participant scoring, and blind comparison were not run. No claim is made
that any r1 blocker is fixed. In particular, the r1 Scheduler ICE, four
native/AOT ICEs, E0956, malformed T4 rows, and A04 timeout remain explicit r2
prerequisites above.

## Checkable r2 start gate

The following checks are prerequisites, not participant evidence. A future
runner must execute them in order and record the command exit code plus the
listed observable. Every row must pass before the first r2 prompt is sent.
Any non-zero exit, missing file, timeout, ICE, changed stdout, changed
diagnostic, unavailable tier, or missing adapter keeps r2 **BLOCKED**.

### Gate 1 — release binary

```text
scripts/agent/jet-env cargo build --release -p jet
scripts/agent/jet-env target/release/jet --version
```

Required observable: the build exits `0`, `target/release/jet` exists, and
`target/release/jet --version` exits `0` with a version line. The current
preflight fails the first command with `E0599` in
`crates/jet-codegen/src/Prelude/Core/TimeMonotonic.rs:22`; therefore no witness
probe below has been run in r2.

### Gate 2 — adapter availability

```text
scripts/agent/jet-env node -e 'const fs=require("node:fs"); const os=require("node:os"); const path=require("node:path"); const file=path.join(os.homedir(),".omp","agent","config.yml"); if(!fs.existsSync(file)){ console.error("ADAPTER CHECK FAIL: missing "+file); process.exit(1); } const text=fs.readFileSync(file,"utf8"); const missing=["openai-codex/gpt-5.6-luna","anthropic/"].filter((marker)=>!text.includes(marker)); if(missing.length){ console.error("ADAPTER CHECK FAIL: missing "+missing.join(",")); process.exit(1); } console.log("ADAPTER CHECK OK: OpenAI+Anthropic");'
```

Required observable: exactly `ADAPTER CHECK OK: OpenAI+Anthropic`. This check
was run on 2026-09-05 and passed. It proves only that both pinned provider
profiles are configured; it does not create a participant or prove a fresh
session.

### Gate 3 — r1 witness re-probes

Restore only the archived r1 witness source bytes and the already sealed r2
fixture bytes into the fixed scratch root. This is preflight materialization,
not a participant receipt and not a task-set change:

```text
scripts/agent/jet-env node -e 'const fs=require("node:fs"); const os=require("node:os"); const path=require("node:path"); const root=path.join(os.homedir(),"jet-test-scratch","2393-r2","preflight"); const fixture=JSON.parse(fs.readFileSync("docs/audits/raw/2393-r2/fixtures.json","utf8")); const task=(name)=>fixture.tasks.find((item)=>item.task===name); const rows=[["A02","T1","docs/audits/raw/2393-r1/A02/T1/jet.json"],["A06","T1","docs/audits/raw/2393-r1/A06/T1/jet.json"],["A07","T1","docs/audits/raw/2393-r1/A07/T1/jet.json"],["A08","T1","docs/audits/raw/2393-r1/A08/T1/jet.json"],["A03","T4","docs/audits/raw/2393-r1/A03/T4/jet.json"]]; for(const [agent,name,receiptPath] of rows){const receipt=JSON.parse(fs.readFileSync(receiptPath,"utf8")); if(typeof receipt.source!=="string") throw new Error("missing source "+agent); const dir=path.join(root,agent,name); fs.mkdirSync(dir,{recursive:true}); fs.writeFileSync(path.join(dir,name+".jet"),receipt.source); for(const value of Object.values(task(name).files||{})) fs.writeFileSync(path.join(dir,value.path),value.content); } console.log("R1 WITNESS SOURCES RESTORED");'
```

Required observable: exactly `R1 WITNESS SOURCES RESTORED`; any missing
embedded source aborts the gate. The A04 receipt has no source and is
intentionally not synthesized.

The five witness groups then have these exact probes and expected current
outcomes:

| r1 witness group | Exact probe | Required observable |
| --- | --- | --- |
| A01 Scheduler fact-registry ICE | `scripts/agent/jet-env cargo test --test marker_declarations repeated_scheduler_fact_registration_is_stable_and_conflicts_are_diagnostic -- --exact` | Final test line `test repeated_scheduler_fact_registration_is_stable_and_conflicts_are_diagnostic ... ok`; no ICE or panic. |
| A02/A06/A07/A08 unclassified T1 generated-Rust/native-AOT failures | `for p in A02 A06 A07 A08; do scripts/agent/jet-env target/release/jet check "$HOME/.cache/jet-test-scratch/2393-r2/preflight/$p/T1/T1.jet" && scripts/agent/jet-env target/release/jet run "$HOME/.cache/jet-test-scratch/2393-r2/preflight/$p/T1/T1.jet" -- records.cfg && scripts/agent/jet-env target/release/jet run --release "$HOME/.cache/jet-test-scratch/2393-r2/preflight/$p/T1/T1.jet" -- records.cfg && scripts/agent/jet-env target/release/jet build "$HOME/.cache/jet-test-scratch/2393-r2/preflight/$p/T1/T1.jet" || exit; done` | Every iteration exits `0`, emits the sealed T1 success stdout, and has no internal compiler failure; native/AOT `run --release` and `build` must both complete. |
| A03 T4 `E0956` | `scripts/agent/jet-env target/release/jet check "$HOME/.cache/jet-test-scratch/2393-r2/preflight/A03/T4/T4.jet" && scripts/agent/jet-env target/release/jet run "$HOME/.cache/jet-test-scratch/2393-r2/preflight/A03/T4/T4.jet" -- report.tsv && scripts/agent/jet-env target/release/jet run --release "$HOME/.cache/jet-test-scratch/2393-r2/preflight/A03/T4/T4.jet" -- report.tsv && scripts/agent/jet-env target/release/jet build "$HOME/.cache/jet-test-scratch/2393-r2/preflight/A03/T4/T4.jet"` | All commands exit `0`; default and optimized output is exactly `{"count":2,"name":"Ada","note":"line \"one\""}` plus one newline; no `E0956`. |
| A04 600-second timeout / exit 137 before Rust | `scripts/agent/jet-env cargo test --test agent_executor_closeout agent_executor_limits_and_secret_receipts_match_all_tiers -- --exact` **and** exactly one family-selected fresh-session command: `scripts/agent/jet-env omp --model openai-codex/gpt-5.6-luna:max --max-time 600 -p @$HOME/.cache/jet-test-scratch/2393-r2/preflight/A04-openai.prompt` or `scripts/agent/jet-env omp --model anthropic/claude-opus-5:high --max-time 600 -p @$HOME/.cache/jet-test-scratch/2393-r2/preflight/A04-anthropic.prompt` | The canary ends with `test agent_executor_limits_and_secret_receipts_match_all_tiers ... ok`; the fresh A04 receipt must be `measured` or an explicit `stopped` receipt with `timed_out=true`, `elapsed_ms>=600000`, and `exit_code=137`. There is no archived A04 source, so the session probe cannot be replaced by a replay. |
| A10 seven malformed T4 rows | `scripts/agent/jet-env cargo test --test fresh_agent_t4 fresh_agent_t4_rejects_malformed_rows_on_all_applicable_tiers -- --exact` | Final test line `test fresh_agent_t4_rejects_malformed_rows_on_all_applicable_tiers ... ok`; all seven named cases run on every applicable tier, with non-zero exit, no stdout, and the sealed stderr marker. |

The A04 family command is selected by the sealed assignment; the prompt file
contains the unchanged A04 task contract and its digest. It is not permission
to improvise a model or prompt. Until that A04 session emits its receipt, the
A04 row is not passed.

### Fixed task-set retention

A witness is an observed prerequisite, not an inclusion filter. If a witness
now passes, its same participant/task/arm remains in the r2 set and denominator;
only the witness status is updated to `resolved` in the preflight record. The
task's fixture bytes, argv, semantic expected output, and required tiers do not
change. If it fails, the same row remains with its explicit failure or stop
receipt and owner. The runner must never drop a task because the bug it exposed
was fixed, and must never alter an expected outcome to turn a failure into a
pass.


## Tooling check against sealed r1

The reusable verifier was run; it reads evidence and never starts agents,
executes task programs, edits receipts, or writes Tower:

```text
scripts/agent/jet-env node scripts/agent/dogfood-campaign.mjs check \
  --manifest docs/audits/raw/2393-r1/manifest.json \
  --protocol docs/proposals/dogfood-jet-experience-5-of-5.md \
  --ledger docs/proposals/dogfood-jet-experience-5-of-5.md \
  --rerun-report docs/audits/fresh-agent-5-of-5-rerun-2026-08-31.md
```

The command's final status line was:

```text
DOGFOOD CAMPAIGN FAIL
```

That is the expected fail-closed result for sealed r1: 72/80 measured receipts,
8 explicit stops, 27/40 Jet project-green tasks, 9/9 measured Rust preferences,
and no supplied hostile closeout. The verifier was not pointed at an invented
r2 manifest.

## Schema syntax check

The landed schema was parsed with Node before any r2 evidence could be
generated:

```text
scripts/agent/jet-env node -e 'JSON.parse(require("fs").readFileSync("docs/audits/raw/2393-r2/manifest.schema.json", "utf8")); console.log("SCHEMA JSON OK")'
```

Final status line:

```text
SCHEMA JSON OK
```

The check validates JSON syntax only; it does not invent or bless a manifest,
receipt, participant, score, or preference.

The manifest shape also requires the ratified strict-performance policy
(`jet.strict-performance-policy.v1`, `AGENTS.md#strict-performance-gate`) and
encodes its per-cell/per-metric comparators: Rust win `<1`, parity `<=1.05`
only as measurement noise, loss `>1.05`; every non-Rust win `<1`, with
`>=1` a loss. The six fail-closed states are `missing`, `wrong`, `unavailable`,
`uncovered`, `mismatched`, and `inconclusive`.

The required coverage remains the foundations (numerics, text, files,
concurrency, networking, build time, and run time) plus real workloads in web,
games, CLI/scripts, data analysis, backend services, AI/ML, GUI, and embedded;
a niche is required only after a first-party battery exists. No average,
substitute workload or tier, omitted peer, semantic trade, or historical receipt
rewrite can weaken this gate.

The policy fields were checked directly:

```text
scripts/agent/jet-env node -e 'const s=require("./docs/audits/raw/2393-r2/manifest.schema.json"); if(s.$defs.strictPerformancePolicy.properties.comparators.properties.rust.properties.parity.const!=="<=1.05"||s.$defs.strictPerformancePolicy.properties.comparators.properties.non_rust.properties.win.const!=="<1") process.exit(1); console.log("SCHEMA JSON OK strict policy encoded")'
```

Final status line:

```text
SCHEMA JSON OK strict policy encoded
```

The preregistered fixture record was parsed and its four task identities were
checked:

```text
scripts/agent/jet-env node -e 'const f=require("./docs/audits/raw/2393-r2/fixtures.json"); if(f.card!==2393||f.run_id!=="2393-r2"||f.tasks.length!==4) process.exit(1); console.log("FIXTURE JSON OK tasks="+f.tasks.length)'
```

Final status line:

```text
FIXTURE JSON OK tasks=4
```

The fixture content digests were recomputed before any run:

```text
scripts/agent/jet-env node -e 'const crypto=require("node:crypto"); const fs=require("node:fs"); const f=JSON.parse(fs.readFileSync("docs/audits/raw/2393-r2/fixtures.json","utf8")); const bad=[]; for(const t of f.tasks){ for(const value of Object.values(t.files||{})){ const got=crypto.createHash("sha256").update(value.content,"utf8").digest("hex"); if(got!==value.sha256) bad.push(t.task+":"+value.path); } for(const value of t.failure_cases||[]){ const got=crypto.createHash("sha256").update(value.content,"utf8").digest("hex"); if(got!==value.sha256) bad.push(t.task+":"+value.path); } } if(bad.length){ console.error("FIXTURE DIGEST MISMATCH "+bad.join(",")); process.exit(1); } console.log("FIXTURE DIGESTS OK");'
```

Final status line:

```text
FIXTURE DIGESTS OK
```

## Criterion disposition

| Criterion | r2 state | Evidence and what still requires an actual run |
| ---: | --- | --- |
| 1 | **MET for protocol; run gate open** | The control record plus this dated addendum name the agents, four unchanged real tasks, matched inputs, per-task neutrality locks, blinding, scoring, stop rules, fail path, and all three r2 JSON shapes. The schema is landed. The run itself still requires a sealed pre-run manifest. |
| 2 | **BLOCKED / OPEN** | No r2 compiler or participant session started. Eighty measured arm receipts, matched outputs, METRICS-law samples, and raw per-agent scorecards require a working compiler and fresh agents. |
| 3 | **BLOCKED / OPEN** | No r2 category score or blind comparison exists. The pass requires every Jet median 5, no Jet raw score below 4, and every blind answer Jet; any other answer fails and must reopen its owner. |
| 4 | **OPEN, no silent drop** | The control record still lists every F01–F53 and T01–T10/T-PREF testimony row with an owner, evidence, or explicit decline, but its r1 gate retains open/partial obligations. No r2 run occurred to supply new closure evidence. |
| 5 | **PARTIAL / OPEN** | This dated report and the r2 schema are landed. The r2 score, preference, raw receipts, and residual findings are `not measured`; the control record's r2 ledger section links here and records that state. |

## Handoff for the next permitted run

1. Resolve or explicitly record the release-build blocker and verify a local
   compiler before starting agents.
2. Recheck every named r1 witness on its original task and tier. Do not edit
   the task set to make the preflight green.
3. Confirm five fresh OpenAI and five fresh Anthropic sessions, then seal the
   assignment, fixture, protocol, and toolchain digests.
4. Run the eight isolated task-arm sessions per participant under the fixed
   three-pass/600-second stop rules. Preserve every measured or explicit stop
   receipt and comparison receipt.
5. Run the existing verifier with the r2 manifest and this report. A fail must
   preserve the finding and reopen its owning card; only the complete fixed bar
   can support an earned-preference claim.
