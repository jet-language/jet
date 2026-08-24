# Development from first principles: one act, one receipt — and the inner loop re-founded

First-principles audit of development in general — the whole lifecycle at every echelon, friction overall, not only writing and reading code. Date: 2026-08-19. Evidence: five research lanes (live probe of 80 CLI commands, lifecycle code map, decision sweep, silhouette sweep, 96-row industry pain corpus), lane files under `target-fpa/L1..L5`. Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Executive summary

**The finding.** Development has two famous prices: you wait, and you get lied to. The audit measured both on the live binary. Waiting: `jet run` answers warm in 0.02 s, but `jet build`, `jet test`, `jet prove`, and `jet budget check` each cost ~33 s on a hello-world project **with zero changes** — the warm rerun pays the full cold price, every time (L1 §2.2). Lying: the parity ledger declared 0 gaps while observation found 51; three goldens were blessed against wrong output; a shipped DAP debugger sits behind an open "implement" card while a proposal claims 57 Canvas rows that were never driven once; agent fleets closed ~25 real defects the board never recorded (L4 §3, L5 rows 36, 39, 90). Every echelon pays one of these prices, and the big surprise is that they are the same defect: **a development act leaves no durable record bound to what it read.** A verb that cannot remember what it proved must recompute it (the waiting), and a project that cannot render what was proved must hand-write its own status (the lying).

**The one idea.** *Every development act — check, run, test, build, prove, publish, deploy — is an action over a locked input closure that leaves a receipt. Every claim about the project is rendered from receipts, or it is prose. Unchanged inputs never pay twice. Every receipt names its witness.*

**The "ohhh".** Jet already ratified every organ of this machine — in five separate places, each stopping at its own border. D-PROVE-SEM1 says "every summary is derived from evidence, never independently counted" — for one verb. D-BUILDCACHE1=A says the action cache is automatic — for `fn build` actions, and it is unbuilt, which is why warm `jet build` costs 33 s. D-ECO-RECEIPT2=A says one connected record spans inputs → action → output digest → proof → generation — for store realizations, fragmented across five cards. The Hangar is a content-addressed store with signing and trust domains — for packages. `jet inspect digest` renders `llms.text` and CI byte-compares it — the one project ledger that cannot lie, because it is rendered. Five local laws, one global law nobody stated. This proposal states it.

**What it buys, concretely.** Warm `jet test` drops from 33 s to reading a store — and the second run of the full golden suite (45–90 min today, ~476 identical Prelude compiles) becomes mostly cache hits, which is the top-ranked fix in the test-performance plan landing as law instead of a one-off optimization. "Done" stops being a claim: a test result, a golden, a proof, a budget baseline, an API snapshot become one receipt shape, so the coverage ledger, the feature manifest, and a Tower card's evidence render from the store and cannot drift. Review, CI, and team caching stop being missing products: a review is a meaning-diff plus a receipt-diff; CI is a witness that replays cold and countersigns; a teammate's green is your instant green, under trust policy you set. False green dies structurally: an agent cannot claim what the store does not hold.

**What the ballots ask.** Thirteen direction-level choices in two waves. Wave 1, the evidence machine: adopt the law (D-DEVR-LAW1); never pay twice on every verb (D-DEVR-TWICE1); claims leave receipts and ledgers render (D-DEVR-CLAIM1); one project truth surface (D-DEVR-STATUS1); review as a product (D-DEVR-REVIEW1); the witness model for team, CI, and registry (D-DEVR-WITNESS1); production runs leave receipts locally (D-DEVR-PROD1). Wave 2, the inner loop (Part II): the cone law for verdict latency (D-DEVR-CONE1); debugging as queries over a recorded run (D-DEVR-CAUSE1); `#Todo` as a typed goal with `jet fill` (D-DEVR-HOLE1); discovery by contract (D-DEVR-FIND1); refactors that carry their meaning (D-DEVR-SEMID1); speculative acts with rollback (D-DEVR-TRY1). Each stands alone.

**What does not change.** The inner loop law (D-RUN-LAW1: verbs pick the observer; claims pick inputs and evidence grade) is untouched — receipts are what observers write down. The evidence words stay the ratified four: proved, passed, observed, met. I9 tier parity, the one report frame, the fact law, the corpus law, D-TELEMETRY1 (nothing ever phones home), and every frozen wall stay. Zero new mechanisms: this proposal generalizes three ratified ones (action cache, receipts, evidence ledger) and deletes their hand-maintained imitations.

## The problem, briefly

Two terminal transcripts, both from the live probe (L1), carry the whole finding.

The waiting price — the same project, nothing changed between lines:

```text
$ time jet run run.jet        # 0.02 s   warm, interpreter tier
$ time jet build              # 34.3 s   warm, zero changes since last build
$ time jet test math.jet      # 33.2 s   warm rerun, same test, same result
$ time jet budget check       # 34.1 s   on hello-world, "0 budgets passed"
$ time jet prove run.jet      # 33.1 s   burned BEFORE noticing rustc is missing
```

The lying price — what the project says about itself versus what is true:

| Claim surface | Said | Was true | Evidence |
|---|---|---|---|
| `jit_coverage_audit` ledger | 0 gaps | 51 observed failures, +80 set drift each way | e3 handoff, card #1663 |
| Three golden files | blessed expected output | blessed against wrong emitted output | fixed 8acbb3454 |
| Tower card #12 (DAP debugger) | open, "implement" | DAP server shipped in `crates/jet-debug` | L4 §2 |
| Canvas epoch post-mortem | 57 rows "shipped" | zero verified by driving the UI | epoch-6 README |
| Agent fleet burndown | cards "done" from source reading | ~25 real defects fixed that the board never recorded | orchestration law, L5 row 90 |
| `jet remove json` (dep never added) | `removed 'json'` · exit 0 | nothing existed to remove | L1 §2.8 |

Both tables have one root. The act that knew the truth — the compile, the test run, the parity check, the UI drive — kept no record bound to its inputs. So the next verb recomputes (price one), and the next claim hand-writes (price two).

### Card #2111 criterion 3: warm-cost measurement

The following run measures one unchanged invocation with receipts disabled, then
one unchanged invocation after a receipt was written. Each verb used a fresh
copy of `examples/features/tooling/test_target`. The four cases used one pinned
copy of the Jet binary and one `jet-env` shell. This kept the tool and shell
identity fixed between samples.

| Verb | Before: bypass, warm | After: receipt hit | Status | Receipt evidence |
|---|---:|---:|---:|---|
| `build` | 731 ms | 40,093 ms | 0 | `ok: build current (receipt 0cf4d3498d49)` |
| `test` | 740 ms | 39,926 ms | 0 | `ok: test current (receipt 66adb04bd912)` |
| `prove` | 48,684 ms | 37,101 ms | 0 | `ok: prove current (receipt 42df9f084fb1)` |
| `budget check` | 75,720 ms | 38,930 ms | 0 | `ok: budget check current (receipt e3a6324e9ff4)` |

Command shape for each row:

```text
JET_RECEIPT_BYPASS=1 <jet> <verb> # warm producer path
JET_RECEIPT_BYPASS=1 <jet> <verb> # before: measured no-receipt run
<jet> <verb>                      # prime the receipt
<jet> <verb>                      # after: measured receipt-hit run
```

The after rows prove that the producer did not run. They do not yet show a
cheap warm command. Receipt lookup hashes the large compiler tool identity on
each process start, so lookup cost remains about 37–40 seconds on this
checkout. The report records the before and after price; it leaves that lookup
cost visible for the next performance fix.

Jet already builds receipts. It builds them seven different ways, one per corner, none shared. Each row below is the same underlying thing — evidence of an act, keyed by inputs — wearing a different coat:

| # | The act | Its record today | Home | Defect |
|---|---|---|---|---|
| 1 | `jet prove` | `.jetproof` — typed, versioned, input-SHA'd, "summaries derived from evidence, never counted" | `docs/spec/proof-replay-decisions.md:31-37` | The model row. Exists for one verb only |
| 2 | `jet test` | **nothing** — results reach the terminal and die; the durable `JETTEST2` record is written under `jet prove`, and `jet test --json` is unbuilt | L1 probe; toolchain proposal row 7 | The most frequent act leaves the least evidence |
| 3 | Golden examples | `examples/features/expected/**` files, hand-blessed | repo convention | No input closure recorded — stale blessing is undetectable (3 found) |
| 4 | API surface at publish | `.jet/cache/api/<name>.api` snapshot, diffed by E1218/E2601 | `Source/CmdSupply.rs:305-366` | Bespoke format; unreadable outside publish |
| 5 | Perf baselines | `jet budget update` pins + `tools/perf/baseline.json` + shell scripts | `tools/perf/ci-perf-check.sh` | Two homes; CI half lives outside the toolchain |
| 6 | Store realization | connected receipt: inputs → action → digest → proof → generation | D-ECO-RECEIPT2=A, sd:4197 | Ratified, schema unpicked, "fragmented across #420, #422, #424, #425, #431" (spec's own words) |
| 7 | Build actions | automatic action cache, keys named down to env and tool digest | D-BUILDCACHE1=A, sd:4376 | Ratified, unbuilt — hence the 33 s warm tax |
| 8 | Tier parity | `tests/jit_gaps.txt` hand ledger, watched by the `jit_coverage_audit` observer | tests/ | The ledger declared 0 gaps while the observer found 51; the observer now ratchets both directions (#1998), but the ledger is still hand-written |
| 9 | Feature claims | `docs/spec/feature-claims.json` | docs/spec/ | Hand-maintained beside the code it describes |
| 10 | LLM surface digest | `llms.text` rendered by `jet inspect digest`, CI byte-compares | `Source/CmdInspect.rs:12-99` | None — the second model row: a ledger that cannot lie |
| 11 | Card evidence | prose rows on Tower cards | `.tower/` | Destroyed by a git reset once; drifts both directions (#12 vs shipped DAP) |
| 12 | Schema history | `.jet/cache/schema/` snapshots, E0910 gate | `Source/CmdSchema.rs` | Solid but private to one verb |
| 13 | Run traces | `.jettrace`, `--gc-trace`, `.jetproof-replay` | D-PERFSESSION1, D-JREPLAY1 | Each a separate artifact family; none reachable from `jet status`-style questions |
| 14 | Support bundle | `jet report` — version, target, policy only | `Source/CmdReport.rs:30-45` | Carries no evidence at all; "thin for real support cases" (L2) |

Glossary, one line each. **Act** — one invocation of a lifecycle verb doing real work. **Closure** — the exact inputs the act read: sources, deps, toolchain, settings, environment (the lock already knows how to say this). **Receipt** — the durable record: closure digest, act, outcome, evidence rows, witness, time. **Witness** — who ran the act: you, a teammate, CI, the registry. **Grade** — the ratified evidence words: proved, passed, observed, met.

## The proposal

### The law and the grid

**Every act is an action over a locked closure and leaves a receipt. Every claim about the project renders from receipts, or it is prose. Unchanged inputs never pay twice. Every receipt names its witness.**

The ratified rules are already theorems of it. D-PROVE-SEM1 ("summaries derive from evidence") is the law inside one verb. D-BUILDCACHE1 ("cache is automatic") is the never-pay-twice clause for build actions. D-ECO-RECEIPT2 (inputs → action → digest → proof → generation) is the receipt chain for realizations. D-ONCE-LAW1 ("every truth has one home; second copies fail the build") is the render clause for the corpus — this law extends it from what the repo *says* to what the project *has proved*. The package trust rules — signing, read-time verification, and revocation from D-JPK-CACHEAUTH1, trust domains from D-JPK-REMOTE1 — are the witness clause for packages.

Every lifecycle question becomes one cell of a small grid — which act, over which closure, earning which grade, witnessed by whom:

| Question today | As a grid cell |
|---|---|
| "Do the tests pass?" | test act · this closure · passed · me |
| "Is the build warm?" | build act · this closure · met · me (receipt exists → nothing to do) |
| "Did CI check this?" | same acts · same closure · same grades · CI witness countersigned |
| "Is this PR safe?" | receipt diff between two closures |
| "Is the parity ledger honest?" | rendered from tier-tagged test receipts — it cannot be anything else |
| "Is this card done?" | its exit criteria cite receipt ids |
| "Did prod break?" | run act · deployed closure · observed · the machine it ran on |

### Element 1 — never pay twice (the speed law)

What to look at: the same commands as the problem section, after the law. No new words, no flags — the beginner types nothing and the tax disappears.

```text
$ jet build
built: build/run · authority: none · effects: IO          # 34 s — cold, honest work

$ jet build                                                 # proposed warm behavior
ok: build/run current (closure a1b2c3, receipt 9f41)        # <0.1 s — the receipt IS the answer

$ vim src/parser.jet                                        # touch one file
$ jet test                                                  # proposed warm behavior
2 claims re-checked (parser.jet reached them) · 12 hold from receipts · 1.4 s
```

This is D-BUILDCACHE1 promoted from "actions inside `fn build`" to **every verb**: build, test, prove, budget, bench measurement, doc rendering, image layers. The key is the ratified one, in full: inputs, outputs, argv, env, caps, tool digest, target, policy, toolchain, compiler version, and generated source hashes. The test-performance plan's top fixes — content-addressed Prelude/Core rlibs, run-scoped AOT artifact reuse, affected-only golden runs — stop being optimizations and become the law's first enforcement cases: a warm identical compile is a bug, not a cost.

The expert's three exits, in-line — every automatic reuse owes these:

```text
$ jet explain receipt 9f41            # SEE: why was this reused; full closure, witness, chain
                                      #      (extends the ratified `jet inspect explain-build`)
$ jet test --fresh                    # REPLACE: refuse all receipts, recompute everything    (proposed flag)
settings: .{ receipts: .LocalOnly }   # REFUSE, project-wide: never share; `.Off` never records  (proposed setting, typed per D-CONF-KEY1)
```

### Element 2 — claims leave receipts, ledgers render (the honesty law)

What to look at: the most frequent act finally leaves evidence, and every hand-written status table dies.

```text
$ jet test
double doubles: pass · parse rejects empty: pass · 14 passed
wrote receipt 4c22 (closure a1b2c3, grade: passed, tiers: run/aot)   # proposed line
```

Three consequences, each deleting a lying surface from the problem table:

**Goldens gain closures.** A golden file's receipt records which compiler, which example bytes, which tier produced the blessed output. Blessing against stale output becomes impossible to do silently — the receipt's closure will not match the tree. The three wrongly-blessed goldens (8acbb3454) are this defect class, retired.

**The parity ledger renders.** The hand-written `tests/jit_gaps.txt` ledger is replaced by a rendered view over tier-tagged test receipts, exactly as `llms.text` is rendered from typed registries and byte-compared in CI today. The `jit_coverage_audit` observer — the check that caught the 0-versus-51 lie and now ratchets both directions (#1998) — is kept and becomes the render guard. A parity claim with no receipt renders as `not-recorded`, never as `0 gaps`. Card #1663 (reconcile the ledger) lands here instead of hand-fixing it once more.

**Card evidence cites receipts.** A Tower exit criterion says `receipt 4c22`, not "tests pass". The orchestrator reads the store instead of re-auditing the worker; the worker cannot claim what the store does not hold. This is the product answer to fleet false-green (L5 row 90) — process law today, structure tomorrow.

The receipt is one shape for every act. Its base is the chain D-ECO-RECEIPT2 ratified verbatim — exact inputs, planned actions, produced output digests, activation proof, parent generation — and this proposal extends that record with two fields of its own: evidence rows (the `.jetproof` shape, four grades unchanged) and the witness. That extension is a named amendment carried by D-DEVR-LAW1. `.jetproof`, the API snapshot, the budget baseline, and the schema snapshot become receipt payloads, not parallel formats. No new `.jet<kind>` artifact is minted — receipts are extension-less immutable Hangar objects (D-ECO-RECEIPTSTORE1=A), existing artifacts keep their ratified extensions, and any future receipt file format goes through D-ARTIFACT-EXT1's ballot gate like everything else.

### Element 3 — one truth surface

What to look at: today the question "what is true about my project?" costs six verbs and 2 minutes of AOT; proposed, it is one read of the store.

```text
$ jet status                                                          # proposed verb
closure  a1b2c3 · 2 files changed since receipt 9f41
check    ok        current
claims   14 hold   current                       (receipt 4c22)
proofs   2 proved  STALE — parser.jet changed    (jet prove to refresh)
budgets  3 met     current                       (receipt 77d0)
publish  0 breaks  current against v1.2 API      (receipt 5e19)
```

Beginner rung: `jet status` with no arguments, plain words, one screen. It never runs anything — it reads receipts and says what is current, stale, or never-recorded. Expert rung: `--json` emits one versioned status object per row — status data in the ratified sense, not a `jet.report/v1` report, since D-REPORT-LAW1 keeps status lines out of the report frame. `jet explain` on any row walks the receipt chain. Agent rung: this table *is* the loop state — instead of re-running suites to find out what its edit broke, an agent reads which claims went stale, fixes, and re-runs only those.

The five agent quantities, priced against this surface:

| Quantity | Today | Under the law |
|---|---|---|
| Verdict fidelity | ledgers can claim 0 gaps at 51 | a grade with no receipt renders `not-recorded` — lying requires forging a store object |
| Verdict latency | warm test 33 s; full suite 45–90 min | affected claims only; the rest is a store read |
| Verdict actionability | scrollback archaeology | `status` names the stale claims and the input that staled them |
| Context economy | agents re-run suites to learn state | one table, one JSON line per claim |
| Repair determinism | "something in CI is red" | one stale claim → one receipt → one closure diff → one fix target |

### Element 4 — review is a receipt diff

What to look at: code review has no owner anywhere in Jet (L4 hole #1). It does not need a new engine — every ingredient ships today: structural diff (`jet diff --structural`), blast radius (`jet inspect impact`), the gate ledger (`jet inspect gates`), and now receipts. Review is their join:

```text
$ jet review origin/main                                              # proposed verb
meaning    2 fns changed, 1 signature changed (blast: 7 callers)      # from semindex diff
authority  no new gates · no new effects · no new deps                # from gate-ledger diff
claims     +2 new · 1 held-at-base now stale · 0 deleted              # from receipt diff
evidence   13 hold at both ends · 2 await this machine's witness
verdict    nothing unproved got weaker · run `jet test` for the 2 new claims
```

A human reviewer reads what changed in meaning and what changed in proof, instead of re-deriving both from a line diff. An agent reviewer gets the same as JSON. GitHub-style line review stays whatever the team likes — this is the part no forge can do, because no forge holds the semantic graph or the evidence store. Where Jet loses today, named honestly: GitHub owns review network effects and inline-comment ergonomics, Bazel-scale monorepos have battle-tested remote caching, and Go builds fast enough that caching barely matters — Jet's counter is that none of them can join meaning, authority, and evidence in one verdict, because none of them owns all three.

### Element 5 — witnesses (team, CI, registry: one sharing model)

What to look at: CI today is a consumer Jet serves with exit codes and `--json`; pipelines, runners, and "is my teammate's green my green?" are unowned (L4 hole #2). Under the law, all of them are one question: **whose receipts do you accept?**

```text
# a receipt is signed by its witness — you, alice@laptop, ci@github, registry
$ jet test
14 hold from receipts (11 yours · 3 countersigned by ci@team)  · 0.1 s

# CI is not a pipeline DSL. CI is a cold witness:
$ jet verify --cold                    # proposed: replay every act from the lock, no reuse,
wrote 31 receipts, countersigned       #           then countersign what it reproduced
```

Receipts ride the sharing rails that already exist: the Hangar, the signed-cache boundary, host-owned cache bindings, signing and read-time verification and revocation per D-JPK-CACHEAUTH1, and trust domains per D-JPK-REMOTE1. A teammate's or CI's receipt reaches you exactly like a cached artifact does, and it is *accepted* only per policy. The three exits again: see (`jet explain receipt` shows the witness chain), replace (`--fresh` recomputes locally, `verify --cold` is institutionalized distrust), refuse (`receipts: .LocalOnly` — never accept a foreign witness; the default for a fresh project, per D-TELEMETRY1's spirit: nothing shared unless you bind a cache).

This dissolves the pipeline question rather than answering it: there is no `jet ci` and no YAML, because "CI" is `jet verify --cold` run by any machine you choose to trust, and "CD" is the already-ratified `jet deploy` consuming realization receipts (D-ECO-RECEIPT2) whose chain now starts at the test receipts instead of at the store door. The broken-trunk pain (master unbuildable twice, L5 row 49) gets its merge-queue answer for free: a merge target that requires a countersigned receipt for the merged closure *is* the not-rocket-science rule, enforced by the store instead of a bot.

One naming note, owned by the ballot: `jet verify` is a new flat verb, so D-DEVR-WITNESS1 carries the same D-CLI-SURFACE1 amendment STATUS1 and REVIEW1 carry, and it must settle the word against the ratified store spelling `jet hangar verify` (D-CLI-STORE2=A) — same word, store scope — the way `jet check` and `jet os check` already coexist.

### Element 6 — production runs leave receipts

What to look at: today `jet report` writes version-and-policy, `jet inspect live` needs a pre-armed process, and a production crash leaves whatever the operator scraped from journald. The run that crashed is an act like any other — it owes a receipt.

```text
$ jet run --release service.jet        # in prod, crash:
wrote crash receipt 8a3f (.jet/reports/8a3f): report frame, closure, replay capture   # proposed
$ jet report --attach 8a3f             # proposed: the support bundle carries evidence
$ jet debug --replay=8a3f              # proposed: receipt-id addressing extends the ratified --replay=NAME rail (D-RUN-RECORD1)
```

Scope, honestly: this is the on-ramp, not an observability platform. Local-only stays law (D-TELEMETRY1=A — receipts never leave the machine unless the operator ships the bundle). A fleet that forbids any on-host artifact refuses recording itself with the same project switch (`receipts: .Off`). Metrics exporters, distributed tracing, and OTel remain explicitly future territory; what this element fixes is that the crash you are asked to debug arrives with its closure and its replay instead of a prose ticket.

## Part II — the inner loop, re-founded

Wave 1 re-founded the process plane: what happens around the code. The owner's challenge — is that all? — is answered by mapping every plane of development to the law that owns it. Six planes of the inner loop had no owner. Part II founds them. Nothing here is a new mechanism: each element is a missing consequence of machinery Jet already ratified, and each one names its rails.

| Plane of development | Owning law | State |
|---|---|---|
| Meaning, types, knowledge | D-TYPE2-* (carriers and knowledge) | ratified |
| Failure | D-FAIL-* (one report, three routes) | ratified |
| Memory | memory v5 slate (D-MEM-*) | ratified |
| Concurrency | D-CONC-* (work is a value) | ratified |
| Authority and safety | D-AUTHORITY-*, D-FACT-* (one fact law) | ratified |
| Build and config facts | D-CONF-* (one plane) | ratified |
| Names and modules | D-NAME-* (one tree) | ratified |
| Trust and format boundaries | D-BOUND-* (one crossing) | ratified |
| Everything Jet says | D-REPORT-* (one report, four surfaces) | ratified |
| Compile-time programming | D-META-* (one compile-time program) | ratified |
| The run and the claim | D-RUN-LAW1, D-CLAIM-* | ratified |
| Corpus truth | D-ONCE-* (say it once) | ratified |
| Process evidence: speed, honesty, sharing | Part I (D-DEVR-LAW1 … PROD1) | this slate |
| Verdict latency under edits | D-DEVR-CONE1 | this slate, Part II |
| Debugging causality | D-DEVR-CAUSE1 | this slate, Part II |
| Writing toward a goal | D-DEVR-HOLE1 | this slate, Part II |
| Finding what exists | D-DEVR-FIND1 | this slate, Part II |
| Refactoring identity | D-DEVR-SEMID1 | this slate, Part II |
| Safe speculation | D-DEVR-TRY1 | this slate, Part II |
| Observability exporters, live-DB migrations, hosted registry operations, time-travel engine | named future territory | deliberate (E6 schedule for time travel kept) |

### Element 7 — the cone law: verdict latency scales with the edit, not the program

Part I's receipts erase the cost when nothing changed. The cone law bounds the cost when something did: **the work to re-verdict after an edit is proportional to the edit's blast radius, never to program size.** The blast-radius engine already exists (`jet inspect impact`); the query crate already computes incrementally for the editor; the rlib card (c09otnjg) is the law's first artifact on the AOT side. What is missing is the law itself — so today every CLI verb re-derives the world, and only the LSP is incremental.

```text
# today: one-line edit, whole-program price
$ vim src/parser.jet && jet check      # re-runs the whole front end
$ jet run --release                    # regenerates and recompiles one monolithic program

# proposed: the price is the cone
$ vim src/parser.jet && jet check      # re-checks parse_line's cone: 3 fns, 2 files
ok: 3 definitions re-checked · rest current · 0.02 s
```

Honesty about evidence: today's warm numbers are measured at toy scale (0.05 s hello-world `jet check`), where whole-program cost is invisible. The law is what keeps that number flat at 100 kLOC instead of letting it grow linearly — the failure every large codebase in every language lives with today, and the reason IDEs and compilers give different answers. The law is budget-enforced (typed latency budgets per verb and rung, riding D-PERFBUDGET-COMPILE1 and card #677), so a violation is a red budget, not a slow afternoon. Expert exits: `jet explain` names why a re-verdict exceeded the cone (a comptime dependency, a changed signature); budgets are project-tunable; there is nothing to refuse — a guarantee, not a behavior.

### Element 8 — every "why" is a query over a recorded run

Debugging is reconstructing causality backward from a symptom, but every mainstream tool samples forward: breakpoints, prints, re-runs. Jet already ratified the recording rail (D-JREPLAY1, shipped under `jet prove`; Part I's PROD1 feeds it from crashes). The missing law: **any recorded run answers questions.**

```text
$ jet debug --replay=8a3f              # the crash receipt from element 6
> why total == 0                        # proposed: backward slice
total = 0   because sum(items) ran over []        service.jet:41
items = []  because parse(csv) dropped 14 rows    service.jet:22   (decode refused: E0910)
> when order.state                      # proposed: write timeline for one place
order.state = .Open      service.jet:12
order.state = .Stale     service.jet:87   ← last write before the crash
```

The beginner story: `jet debug` is unchanged, and the crash that used to be a prose ticket arrives replayable. The expert story: `why` and `when` over any recorded run — local, test, or production. The schedule story, honestly: `when` is a query over recorded local snapshots. It does not add reverse-step execution or a standing per-variable history engine. Those remain inside D-TIMETRAVEL1=C and D-RUN-RECORD1=A's deferral. Option A pulls the query surface over existing recordings forward. Recording is opt-in (a flag, a crash, or a claim run); queries are read-only over a capture; a project that never records never carries one. Peers: rr and Pernosco prove record-and-query works and stay niche because they fight the platform from outside; a language that owns every tier can make determinism a property, not a heroic capture.

### Element 9 — the goal is a value: `#Todo` goals and `jet fill`

Jet already has the goal: `#Todo` compiles and stops at runtime. But the checker — which knows the expected type, the required effects, and every fact in scope at that position — says nothing about it. The upgrade: **`#Todo` in expression position is a typed goal, and the toolchain talks about it.**

```text
fn parse_row(line: String) Row ? ParseError -> {
    #Todo                               # shipped today: compiles, stops if reached (E3011)
}

$ jet check                             # proposed: the goal card
goal: parse_row's body                  needs Row ? ParseError
  in scope: line: String · ParseError.{...} · Row.{...}
  required effects: none (pure)
$ jet fill run.jet:2                    # proposed verb (or a `jet fix` mode — the ballot decides): ranked candidates
  1. Row{ cells: line.split(",") }                     (uses: split — passes check)
  2. line.take_pattern("{a}, {b}") … build Row          (uses: D-PARSESTR1 pattern)
```

Beginners keep writing programs that run before they are finished — the stop-on-reach behavior is unchanged, so no rung gains ceremony. Experts and agents get the highest-value property in the five quantities: repair determinism — one goal, one typed target, ranked candidates that already pass sema. The walls hold by construction: `fill` proposes ordinary term-level code that then enters ordinary checking (never AST injection, never a macro — D-METAMUTATE1 untouched; comptime still never creates a type). Exits: goal cards are advisory rows that never change `jet check`'s exit code, they summarize past a handful ("…and 17 more goals"), and a project switch silences them entirely. Precedent: GHC's typed holes and Idris's holes are the most-loved feature of their ecosystems and never left them, because no mainstream toolchain owns both the checker and the fill surface. Naming note: the word remains reserved for interpolation slots (D-PARSESTR1, D-META-CODE1); `#Todo` uses the distinct vocabulary "goal" in E3011/E2902, D-TOOL2, and tool output.

### Element 10 — discovery by contract: `jet find`

The costliest beginner hour is "what is this called"; the costliest agent tokens are directory listings. Names are the wrong key — the developer knows the shape of what they need, not its name.

```text
$ jet find "String -> Path"                       # proposed: search by signature (unification, not text)
core.files.Path.from(s: String) -> Path ? PathError
$ jet find --effect FS.Read "read a csv"          # proposed: search by effect + words
core.data.csv.read(path) -[FS.Read]> Table ? CsvError
$ jet find --example '"a,b" -> ["a","b"]'         # proposed: run pure candidates at comptime
String.split(sep) -> [String]                      (1 match on the example)
```

One naming settlement, owned by the ballot: `find` joins `search` in the discovery word-space — `search`/`info` are ratified discovery verbs for **packages** — so D-DEVR-FIND1 fixes the split the way WITNESS1 fixed `verify`: `find` asks code by shape and contract, `search` asks the package catalog by name. `jet search` is the flat package-catalog route.

Signature search is Hoogle — loved for two decades, still confined to Haskell because nobody else's toolchain holds full typed signatures. Effect search and example search are Jet-only: effects are typed facts here, and comptime can safely run pure candidates against the example. Rails: semindex (definitions and signatures), the digest (the same rows the LLM surface renders), the comptime evaluator. Exits: it is a read-only query; there is nothing to refuse; `--json` for agents.

### Element 11 — refactors carry their meaning

A rename touches 47 lines, so the diff shows 47 edits, the merge conflicts with any concurrent change, blame loses the definition's history, and the reviewer burns attention on churn (pain rows 48, 52, 54). The tools that executed the rename — `jet fix`, codemod plans, LSP rename — knew exactly what it meant, and threw that knowledge away. The law: **a toolchain-executed refactor records its semantic op beside the text change, and diff, merge, review, and blame consume ops plus stable definition IDs.** Structural merge already keys on stable IDs; codemod plans are already replayable; this element joins them.

```text
$ jet review origin/main                # proposed rendering, riding element 4
meaning    renamed parse_row → parse_line (47 sites)   ← one line, not 47
           1 fn body changed (blast: 2 callers)
claims     all 14 hold at both ends
```

A concurrent edit to the renamed function's body merges clean, because the rename is an op on a definition ID and the edit is a change to its body — different subjects, no textual collision. Exits: hand edits stay plain text forever; ops are additive metadata; every surface keeps a `--text` view.

### Element 12 — try before you touch: speculative acts

Agents (and careful humans) need the answer to "would this break?" without dirtying the tree. Today that costs apply → run → revert, with the tree exposed in between and nothing remembered afterward.

```text
$ jet try fix-plan.json                 # proposed: apply in an isolated overlay,
2 claims re-checked · 1 breaks:         #   re-run only the affected claims,
  parse rejects empty: FAIL (E3001 at parser.jet:12)
rolled back · wrote receipt 6b09        #   answer, roll back, remember
$ jet try fix-plan.json --keep          # the verdict was clean → land it
```

Rails: overlay-aware semindex open (shipped), codemod apply/undo (shipped), affected-claim selection (element 1), receipts (Part I). This is the transaction primitive the agent fix-loop card (`c0kbzrub`, "Prove the agent fix loop converges") needs under it, and the runtime rollback-regions draft deliberately does not cover — that draft is about program state at run time; this is about tree state at development time. Exits: `try` never touches the tree by default; `--keep` is the only write; a `try` receipt is marked speculative so no ledger counts it as project truth.

## The final vision

One day, three echelons, one store. Every command line below that mentions receipts, status, review, verify, countersigning, crash receipts, goals, find, why, or try is proposed; the rest is shipped behavior.

**Solo beginner** — types nothing new, ever:

```text
$ jet new game && cd game
$ jet run          # 0.02 s
$ jet test         # runs, writes receipts
$ jet test         # 0.08 s — "14 hold (nothing changed)"
$ jet find "String -> Path"    # what is it called? — answered by shape, not name
$ jet check        # a #Todo left in the code prints its goal card, and the program still runs
$ jet build        # 34 s cold, honest
$ jet build        # 0.1 s — current
```

**Team + CI** — the same verbs, plus policy:

```text
$ git pull && jet status
closure d4e5f6 · claims: 11 hold (3 countersigned ci@team) · proofs current
$ jet review origin/main          # before merging a teammate's branch
meaning   renamed parse_row → parse_line (47 sites) · 1 fn body changed
authority no change · claims all hold at both ends
$ jet verify --cold               # what the CI machine runs; nothing else exists
```

**Agent fleet** — the loop is the store:

```text
$ jet status --json               # 1 line per claim: current/stale/not-recorded
$ jet try fix-plan.json           # would this break? — verdict + receipt, tree untouched
$ (edit) && jet test              # only the 2 stale claims re-run
$ jet debug --replay=8a3f         # a failing claim's run, then: why total == 0
$ tower card close #N --evidence receipt:4c22   # closure cites the store; false green impossible
```

The shape of the end state — one store, every act, every witness:

```text
project/
  .jet/
    lock                      # the closure: what everything was computed FROM
    receipts/                 # what was computed, by whom, at which grade   (proposed home)
      9f41  build   met      me         closure a1b2c3
      4c22  test    passed   me         tiers: run,aot
      77d0  budget  met      me
      5e19  publish met      me         api vs v1.2
      31xx  verify  passed   ci@team    countersigned
      8a3f  run     observed prod-host  crash + replay
  ~/.jet (Hangar)             # content-addressed bytes + immutable receipt objects
                              # shared via cache bindings, per trust policy

renders (never hand-written):
  jet status        ← receipts               tier-parity ledger  ← test receipts
  jet review        ← receipts × semindex    feature manifest    ← receipts
  card evidence     ← receipt ids            llms.text           ← registries (already law)
```

Every element marked its proposed spellings in place; the final-vision transcripts are proposed end-state behavior as a whole, per the note above. Everything else on this page is shipped or ratified today.

## What this unlocks

- **Ten-line script author**: the 550–970× `--release` cliff (L5 row 2) becomes a one-time cost; warm is always instant. Nothing to learn.
- **Game dev**: asset-heavy rebuilds become receipt hits; `jet dev` reload keeps its 52 ms loop and gains "which claims went stale" on save.
- **Data/ML**: notebook cells re-run only what their closure change reaches; a result cell can cite the receipt that produced it.
- **Safety-critical**: the qualification bundle *is* the receipt chain — inputs, toolchain digest, proofs, witnesses, countersignatures — the Ferrocene-shaped product (L5 row 6) rendered instead of assembled by hand.
- **500-dev monorepo**: affected-only testing, remote receipt sharing, and the merge-queue rule land on the ratified cache/trust rails instead of importing Bazel culture.
- **Agent fleet**: the echelon Jet's own repo proves daily — verification too slow to run per change → false green → reconciliation audits — loses its root cause. The board renders from the store; the fleet's verdict latency is a store read.
- **The compiler's own CI**: the 45–90 min golden wall and the ~958 duplicate oracle builds (test-performance plan) become the law's first regression tests.

## What stays

- **D-RUN-LAW1 and the claim grid** — untouched; receipts are the durable output of the observers it named.
- **The four evidence words** (proved, passed, observed, met) — the grade axis, unchanged.
- **I9 and D-ONCE-TIER1** — strengthened: parity becomes a rendered fact per receipt, never a declared ledger.
- **D-TELEMETRY1=A** — receipts are local; sharing is an explicit cache binding, never a default.
- **`.jetproof` and D-JPROOF1** — kept as the proof receipt's serialization; nothing reopens its schema.
- **Goldens as the I5 mechanism** — kept; they gain closures, not replacements.
- **All frozen walls** — no top type, no HKT, no macros, comptime never creates types, facts never dispatch, borrow checker stays a prover. Nothing here touches the language.
- **Kept on merit, not inertia**: `jet inspect digest`'s render-and-byte-compare pattern is the proof this model works; it becomes the norm, not a special case.

## Decisions for the owner

| Ballot | Question | Direction options (worked in ballot text) | Touches ratified law |
|---|---|---|---|
| D-DEVR-LAW1 | Adopt the law: act → receipt over locked closure; claims render from receipts | full law (all four clauses) / evidence law without the speed clause / cache law without the evidence clause / decline | extends D-ONCE-LAW1, D-PROVE-SEM1, D-ECO-RECEIPT2 fields to project state (named amendments) |
| D-DEVR-TWICE1 | Never pay twice: every verb consults and writes the receipt store | all verbs / build+test first / decline | generalizes D-BUILDCACHE1=A beyond `fn build` (amendment) |
| D-DEVR-CLAIM1 | Test/golden/budget/API results are receipts; parity + feature ledgers render | all claim kinds with rendered ledgers / test receipts only / render-by-rerun with no store | extends D-ECO-RECEIPTSTORE1=A; retires the jit_gaps hand ledger (#1663 lands here) |
| D-DEVR-STATUS1 | One truth surface | new verb `jet status` / `jet project status` / `jet prove --status` / decline | amends D-CLI-SURFACE1=B (closed verb family) if a verb is added |
| D-DEVR-REVIEW1 | Review product | new verb `jet review <ref>` / `jet diff --review` / decline | amends D-CLI-SURFACE1=B if a verb is added |
| D-DEVR-WITNESS1 | Witness + countersign model; CI = cold witness (`jet verify --cold`); team share via cache bindings | full witness model / local witnesses only / decline | amends D-CLI-SURFACE1=B (new `verify` verb; settles the word against `jet hangar verify`); rides D-JPK-CACHEAUTH1=D signing and D-JPK-REMOTE1=D trust domains |
| D-DEVR-PROD1 | Crash/observed-run receipts + evidence-carrying `jet report` | adopt / crash receipts only / decline | feeds D-JREPLAY1=A; keeps D-TELEMETRY1=A verbatim |
| D-DEVR-CONE1 | The cone law: re-verdict cost scales with the edit's blast radius, budget-enforced | adopt for every verb with typed latency budgets / budgets on check+dev only / decline | rides D-PERFBUDGET-COMPILE1=C (#677); per-verb latency rows are new entries in its closed budget grammar (named extension) |
| D-DEVR-CAUSE1 | Debugging is queries (`why`, `when`) over a recorded run | adopt the substrate law now (named amendment) / crash-replay queries only / decline | amends D-TIMETRAVEL1=C and D-RUN-RECORD1=A's per-variable-history deferral for query-over-recording only; reverse-step and the always-on history engine stay Epoch 6; rides D-JREPLAY1=A |
| D-DEVR-HOLE1 | `#Todo` is a typed goal; `jet fill` proposes checked candidates | goal card + fill / goal card only / decline | `#Todo` runtime meaning unchanged; `fill` spelling amends D-CLI-SURFACE1=B or rides `jet fix` (options); goal vocabulary keeps interpolation "hole" distinct from `#Todo`; walls untouched |
| D-DEVR-FIND1 | Discovery by contract: signature, effect, and example search | full find verb / signature-only / under `jet inspect` / decline | amends D-CLI-SURFACE1=B if a flat verb is added; settles `find` (code by shape) vs `search` (packages by name); read-only |
| D-DEVR-SEMID1 | Toolchain refactors record semantic ops; diff/merge/review/blame consume ops + stable IDs | record and consume / record only / decline | rides D-SEMINDEX1, D-CODEMOD1, structural-merge stable IDs; hand edits stay plain text |
| D-DEVR-TRY1 | Speculative acts: apply in overlay, verdict affected claims, roll back, remember | full try verb / flag on `jet inspect codemod` (agent-only JSON) / decline | amends D-CLI-SURFACE1=B if a verb is added; try receipts marked speculative |

Each ballot stands alone; any subset composes. Adopting none still leaves the defect cards below worth fixing.

## Implementation shape

**Phase A — the substrate, no surface change.** One receipt schema (the ratified D-ECO-RECEIPT2 chain plus the proposal's evidence-rows and witness fields), stored content-addressed in `.jet/` and the Hangar as extension-less store objects — no new `.jet<kind>`, so D-ARTIFACT-EXT1's closed family is untouched. Wire the existing artifacts as receipt payloads: `.jetproof`, API snapshots, budget baselines, schema snapshots, `.jettrace`. `jet test` writes receipts. All tests green, nothing rendered yet.

**Phase B — land ratified-but-unbuilt work on the substrate, so it is built once.** D-BUILDCACHE1's automatic action cache (kills the 33 s warm tax); the receipts cards (#655, #1019–#1020) connect their chain to it; the test-performance plan's rlib/action caching becomes receipt reuse; `jet test` gains a versioned status `--json` that emits the receipt (status data, beside — not inside — the `jet.report/v1` report stream); the parity ledger renders from tier-tagged receipts (#1663); the dev-loop slate remainder (D-CLAIM-BENCH1's `--measure`, D-RUN-RECORD1's `--record`) writes receipts from day one instead of growing private files.

**Phase C — the balloted surfaces.** Wave 1: `jet status`, `jet review`, witness countersigning and `jet verify --cold`, crash receipts, evidence-carrying `jet report`. Wave 2: the goal card and `jet fill`, `jet find`, `why`/`when` queries under `jet debug --replay`, semantic-op recording in `jet fix`/codemod and its consumption in diff/merge/review, `jet try`. Each is a coherent greenfield migration that deletes its replaced form (hand ledgers, bespoke snapshot formats, the thin report bundle) in the same change. The cone law (CONE1) has no surface of its own: it lands as typed latency budgets plus the incremental re-verdict work they force, starting from the suite-speed cards.

## Adjacent defects (cards, not ballots)

The live probe surfaced beginner-lethal defects independent of any ballot: `jet add --path` ICEs at exit 101 *and corrupts package.jet*; `jet remove` claims success for a dep that never existed; `jet update jet` rejects the pin `jet new` itself wrote; `jet help <cmd>` prints the global screen; project-mode `jet test` discovers only the entry file (the Zig trap the spec forbids); E0601's fix text teaches syntax E0930 rejects and a vocabulary D-CLAIM-WORD1 retired; `jet eval` swallows print output; `jet project parts` prints nothing; `shared-store status` attempts an install and demands sudo; nine surfaces leak the word "jetpack" into `jet`'s own voice. These are minted as defect cards on the audit card, with L1 transcripts as evidence. Separately, the 45–90 minute golden wall has three implementation cards that need no ballot and can start now (content-addressed Prelude/Core rlibs; a run-scoped oracle cache for the dev batteries; profile reclassification, shared FFI dep artifacts, and weighted CI shards — the ranked plan of the 2026-08-08 test-performance audit).

---

**Strongest unverified assumption.** That receipt granularity can be made fine enough (per claim, per action) without the closure-hashing itself becoming the new wait — Bazel pays real overhead for exactly this bookkeeping. The mitigations are ratified (the lock already digests the closure; the action-cache key is already specified down to env and tool digest), but no measurement exists yet, and D-DEVR-TWICE1's ballot text must carry a latency budget for the cache probe itself (target: a warm no-op verb answers within the interpreter tier's 0.05 s bar, enforced by D-PERFBUDGET-COMPILE1's machinery).
