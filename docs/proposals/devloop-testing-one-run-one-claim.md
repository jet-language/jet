# One run, one claim — the dev loop and testing as a single machine

Combined first-principles audit: the dev loop (`jet run` / `dev` / `watch` / REPL / `jet debug`, JIT-as-dev-tier, deferred time travel) and testing/benchmarks (assertions, `#Test`, `#Bench`, Check Outputs, budgets). Date: 2026-08-07.

## Executive summary

Jet already ships a strong dev loop and a strong test kit. The audit found that they are the same machine wearing seven coats. Every dev verb — run, dev, test, bench, debug, repl, record — starts the same program on the same tiers and differs only in what watches the run. Every check — `require`, `#Test`, property, doctest, golden, budget, contract — states the same kind of fact and differs only in who supplies the inputs and how strong the evidence is. Nobody named that, so each coat grew its own seams: two assertion vocabularies ratified at once, a bench marker that is a test marker by the code's own comment, an unfinished bench-budget migration, five meanings of "check", two hand-copied watch loops, and a test harness that throws away the file, the line, and the caret exactly when the user runs `jet test`.

The one idea: **Jet has one run and one claim. Verbs pick the observer of the run. Claims pick the inputs and the evidence grade. Nothing else may vary.**

Why now. The landing zone is already ratified and unbuilt: D-REPORT-TEST1=A gives test failures the one report frame, D-FAIL-BREACH1=A binds `require` into the one stop family, D-CORE-PRELUDE1=A adds `assert`/`assert_eq`, D-PERFBUDGET-BENCHMIGRATE1 orders the bench prototypes deleted, D-ENTRY-SCRIPT1=B makes bare code runnable, and D-BENCH-PARITY1=B makes `jet bench` accept what `jet test` accepts. If those cards build on today's seams, they build seven coats again. If they build on one substrate, the whole area collapses into one small grid.

What the ballots ask: one word for assertions (D-CLAIM-WORD1), benchmarks as measured tests (D-CLAIM-BENCH1), watch as a modifier on every runnable verb (D-RUN-WATCH1), one resident session with in-session keys (D-RUN-SESSION1), and the recording observer as the Epoch 6 time-travel on-ramp (D-RUN-RECORD1). D-RUN-LAW1 adopts the model itself. Each ballot stands alone.

What does not change: I9 tier parity, `#Test` as the only test syntax (D-TESTKIT1=A), `jet dev` as the one dev loop (D-CLI-DEVSERVE1=A), silent deopt with `--trace-tiers` (D-LENS-RUN2=A), the nine Output kinds, the `jet prove` evidence words, and the Epoch 6 deferral of per-variable time travel (D-TIMETRAVEL1=C).

## The problem, briefly

One underlying thing, many coats. Each row is a place where the same job grew a second (or fifth) form. File:line is the live code.

| # | The job | Coat A | Coat B (and more) | The defect |
|---|---------|--------|-------------------|------------|
| 1 | State a runtime fact | `require` / `require_eq` (S43, `Syntax/math_layout.rs:610`) | `assert` / `assert_eq` ratified into the prelude (D-CORE-PRELUDE1=A, unbuilt) | Two ratified vocabularies for one job; no ruling retires either |
| 2 | Report a failed fact | `jet_panic_rich(file, line, fn, caret, locals)` in normal code | `return Err(format!("left: {}, right: {}"))` under `jet test` (`Codegen/TIR/emit/helpers.rs:429-437`) | The test path drops file, line, caret, and locals — the poorer report goes to the person debugging |
| 3 | Declare a runnable check | `#Test("name") { … }` | Output `.Check.{ entry: verify }` — both become `JetTestSlot` rows in the same generated main (`Codegen/mod.rs:2580,2587`) | Two spellings, one harness, nobody says so |
| 4 | Time a region | `#Bench("name") { … }` | `#Test("name") { … }` — "identical structure to `TestDef`" (`AST/items.rs:814`) | Same node shape, separate marker, no members (`.setup` in `#Bench` is E0614) |
| 5 | Enforce a perf limit | `#Bench` timing output | typed `Budget.{…}` rows; game budgets a separate path | Spec orders one path ("no second enforcement path may survive", `performance-budget-decisions.md:37`); the `bench_budget` prototype is already gone from code, but the ratified migration (D-PERFBUDGET-BENCHMIGRATE1=B, paired GAMEMIGRATE1) is unfinished |
| 6 | Say "check" | `jet check` (typecheck) | `.Check` Output, `jet fmt --check`, `jet os check`, `jet split env --check` | Five meanings; on a parse error `jet fmt --check` exits 2 where `jet check` exits 1 |
| 7 | Collect the tests | Spec: "`jet test` auto-collects every `#Test` in the package" (S43) | Code scans the entry module only (`Source/lib.rs:1126,1140`; `Codegen/mod.rs:3025`) | Tests in imported modules silently do not run — the Zig trap Jet's own spec forbids |
| 8 | Watch files and re-run | `Source/CmdDevTools.rs:64-117` (native) | `Source/CmdCompile.rs:2652-2680` (web) — same engine, loop copy-pasted | Two choreographies to keep in sync |
| 9 | Benchmark honestly | Spec: "`jet bench` owns the optimized benchmark profile" (`spec.md:2235`) | Code passes `BuildProfile::Default` (`CmdDevTools.rs:2476,2584,2794`) | Numbers come from the wrong tier and nothing says so |
| 10 | Compare against a stored value | `testing.golden(path, actual) -> Bool` (`FSIoEnvOsTesting.rs:964`) | `expect(value).snapshot()` with paths, update flow, missing-file message | The golden path folds "missing file", "unreadable", and "differs" into one `false` |
| 11 | Run a package's tests | `jet test` | `jetpack test` — its body ends in `cmd_build` and runs no test (`trust_env_build.rs:918`) | A verb that lies |
| 12 | Record and replay a run | `.jetproof-replay` (D-JREPLAY1=A, capture + `jet prove --replay`) | `.jetreplay` game recordings — disjoint by decree | Both exist; neither is reachable from `jet run`, `jet dev`, or `jet debug` |

Glossary in one line each. **Observer**: the thing that watches a run — nothing (plain run), a file watcher, the claim harness, a measurer, a stepper, a prompt, a recorder. **Claim**: one stated fact about the program that can earn evidence. **Evidence grade**: the ratified `jet prove` words — proved, passed, observed, met.

## The proposal

### The law and the two grids

The law: **one program, one meaning, one claim; a verb may choose the observer of a run, and a claim may choose its inputs and its evidence grade — nothing else may vary.**

Ratified law already states most of it. I9 says every tier preserves one meaning. D-DEVMODE1 says dev output is byte-identical to release. D-FAIL-BREACH1 says every stop is one report on every tier. D-ONECORE1 says one interpreter, speed tiers on top. This proposal names the remaining two axes and makes the surface show them.

The run grid — every dev verb is one row of "same run, different observer":

| Verb | Observer | Exists today as |
|------|----------|-----------------|
| `jet run` | none | shipped |
| `jet dev` | file watcher + resident session | shipped |
| `jet test` | claim harness | shipped |
| `jet bench` | measurer | shipped, wrong profile |
| `jet debug` | stepper | shipped |
| `jet repl` / notebook | prompt | shipped |
| `--record` | recorder | ratified capture format, reachable only under `jet prove` |

The claim grid — every check is one cell of "who supplies inputs × what evidence it earns":

| Inputs from | Construct today | Evidence word (ratified) |
|-------------|-----------------|--------------------------|
| the caller, at runtime | `require(...)` in code; `#Pre`/`#Post` contracts (D-FAIL-TIER1) | observed |
| the author, as literals | `#Test("name") { … }`; `.Check` Output entry | passed / failed |
| the docs | `/// ```jet` doctest, `// => VALUE` | passed |
| a generator | `#Test fn prop(xs: [Int])` property tests; `jet fuzz` corpus | passed (sampled) |
| a stored artifact | `expect(...).snapshot()`, `testing.golden` | passed |
| a measurement | `#Bench` region + `Budget.{…}` limit | met |
| the prover | range/refinement facts; erased contracts | proved |

Everything on both grids already exists. The proposal deletes the seams between the cells, not any cell.

### One claim word — ballot D-CLAIM-WORD1

Today the same fact is spelled `require` in code and tests, and a second ratified family (`assert`/`assert_eq`, D-CORE-PRELUDE1=A, unbuilt) is about to join it. One fact, one word. The ballot picks the word; the loser is retired before it ships or by migration.

Before (two vocabularies, both law):

```jet
fn withdraw(amount: Int) {
    require(amount > 0, "amount must be positive")   // S43 spelling
}

#Test("withdraw rejects zero") {
    assert_eq(balance_after(0), Err)                 // D-CORE-PRELUDE1 spelling, unbuilt
}
```

After (one word everywhere — shown with option B, `assert`):

```jet
fn withdraw(amount: Int) {
    assert(amount > 0, "amount must be positive")
}

#Test("withdraw rejects zero") {
    assert_eq(balance_after(0), Err)
}
```

Options: **B** — `assert`/`assert_eq` everywhere (recommended); amend S43, D-PRELUDE-LAW1=A, and the D-FAIL-BREACH1 wording; migrate every example. **A** — `require`/`require_eq` everywhere; amend D-CORE-PRELUDE1=A to drop `assert`/`assert_eq` before they are built. **C** — a fresh single family named for the model, `claim`/`claim_eq`; amends the same set as B. **D** — keep both words with one meaning (the status quo the two ratifications drift toward). Whatever wins keeps the ratified rich structural diff and the one stop family (registered E30xx codes, `.expect_fail`). Worked examples elsewhere in this document spell `require` — today's shipped word — and migrate wholesale if B or C wins.

### Every failed claim gets the one report — ratified, this builds it

D-REPORT-TEST1=A and D-FAIL-BREACH1=A are law: assertion and golden failures render the one report frame from the same renderer production stops use. Today's harness does the opposite — it strips the frame. This is the single highest-leverage fix in the area, and it needs no ballot.

Before (today, verbatim shape):

```
double returns twice the input: FAIL
  left: 6, right: 7
```

After (proposed rendering of the ratified frame):

```
double returns twice the input: FAIL (0.2 ms)

Stop [E3001]: expected 6, got 7
  --> app.jet:13:5
   |
13 |     require_eq(double(3), 7)
   |     ^^^^^^^^^^^^^^^^^^^^^^^^
   |
2 passed, 1 failed
run `jet explain E3001` for what/why/fix
```

The frame word, code family, and shape follow the ratified sample in D-REPORT-TEST1=A (`Stop [E3001]`, the D-FAIL-BREACH1 stop family), not a new code.

The same substrate gives `jet test --json` (D-REPORT-MACHINE1=A owns the shape), per-claim durations, and a golden helper that says *which* of "missing / unreadable / differs" happened instead of returning a bare `false`.

### Benchmarks are measured tests — ballot D-CLAIM-BENCH1

The code already says it: a `#Bench` body "type-checks exactly like a `#Test` body". The model says why: a benchmark is a claim whose inputs are a measurement and whose limit is a `Budget`. Unifying them deletes the marker seam, gives benches the full member vocabulary for free, and completes the ratified order to delete the bench-budget prototypes (D-PERFBUDGET-BENCHMIGRATE1).

Before (today — no members allowed, E0614):

```jet
#Bench("parse") {
    parse(load_fixture())        // setup cost pollutes the measurement
}
```

After (proposed — one marker family; members come free):

```jet
use core.testing as testing

#Bench("parse") {
    .setup { input :: testing.fixture("big.json") }
    parse(input)
}
```

Enforcement is only ever a `Budget` row (ratified direction: D-PERFBUDGET-BENCHMIGRATE1=B and its paired game-budget decision, D-PERFBUDGET-GAMEMIGRATE1). `jet bench` reaches test parity (D-BENCH-PARITY1=B: directories, projects, `--filter`) and runs the optimized profile the spec already promises, labeling the tier in every line and artifact:

```
parse   142.1 ns/iter (±3.4)   7,036,000 ops/sec   [aot, optimized]
```

Options: **A** — keep the `#Bench` spelling as the measured-test marker (recommended; the word earns its place). **B** — delete `#Bench`; spell it `#Test` with a `.measure` member. **C** — status quo (separate node, no members). All options adopt the profile fix and tier label.

### One collection, one selection — defect fixes, no ballot

Ratified text already says "`jet test` auto-collects every `#Test` in the package". The code scans the entry module only. That is a defect card, not a decision. With collection fixed, one listing shows every claim and where it came from — the audit ledger for the discovery magic:

```
$ jet test --list                                        (proposed)
app.jet:12          #Test "double returns twice the input"    unit
app.jet:20          #Test fn reverse_twice_is_identity        property (~200 cases)
lib/parse.jet:8     /// doctest                               doc
package.jet         check: .Check.{ entry: verify }           check
```

Same family of fixes: `jetpack test` defers to `jet test` instead of silently building, `jet fmt --check` exits like every other check, and the harness exit rides `ExitCodes` instead of a bare `exit(1)`.

### Watch is a modifier — ballot D-RUN-WATCH1

The strongest cross-language lesson in the research: watch composes as a flag and fragments as a verb. Jet is halfway there (`jet run --watch`, `jet dev`). The ballot extends `--watch` to every runnable verb, riding the one `WatchSession` engine, with affected-only selection from the watch graph — and an honest line, because a partial green must never read as a full green.

```
$ jet test app.jet --watch                               (proposed)
3 passed  (0.4 s)
— lib/parse.jet changed —
2 claims re-run (affected); full suite: jet test
2 passed
```

Options: **A** — `--watch` on run/test/bench/check; `jet dev` stays the resident session verb (recommended; respects D-CLI-DEVSERVE1=A). **B** — fold watching entirely into `jet dev` lenses (`jet dev --test`); plain verbs never watch. **C** — status quo (watch on run/dev only). Internal to every option: the duplicated web watch loop dies; one choreography serves native and web.

### One session, in-session verbs — ballot D-RUN-SESSION1

`jet dev` already holds the resident program, the heap, `#Persist`, and the swap baseline. The proposal makes the session the meeting point instead of a dead end: in-session keystrokes for the loop verbs (the Flutter lesson — the loop lives inside the process), and the prompt and stepper attach to the same session instead of owning parallel engines. The ratified script-mode law (D-ENTRY-SCRIPT1=B: a REPL line and a notebook cell are bare code under one entry law) is the substrate.

```
$ jet dev app.jet                                        (proposed keys)
watching app.jet … (Ctrl-C to stop)
✓ ran in 38 ms
  r re-run   R restart fresh   t tests   f failed claims only   q quit
```

`t` runs the package's claims inside the warm session — no process spawn, no cold compile. `f` re-runs only the failures, the most underrated verb in test tooling. The magic stays announced: every swap prints `[hot-swap]`, every fallback prints `[restart]` with the reason — that contract already ships and does not change.

Options: **A** — adopt the session model: keys in `jet dev`, repl/debug attach, canvas projects the same session (recommended). **B** — keys only (r/R/t/f/q), tools stay separate processes. **C** — status quo.

### The recorder — ballot D-RUN-RECORD1, the Epoch 6 on-ramp

Time travel is deferred to Epoch 6 (D-TIMETRAVEL1=C) behind two named prerequisites: the D-REPLAY1 runtime replay harness (shipped — `#Replayable`, E0725) and a mature `jet debug` (carded, #12). The model shows what the on-ramp is: recording is just one more observer, and Jet's capability-gated effects make it cheap — log the capability chokepoint, not syscalls. The artifact is the ratified `.jetproof-replay`; no second format, and the ratified capture preflight stays: programs that reach FFI, tasks, FS, Env, or Exec still refuse capture (E3625), and sensitive sources still need the interactive consent phrase (E3627). The on-ramp's first users are the runs the law already accepts — tests, property shrinks, and replayable programs.

```
$ jet test app.jet --record=flaky              (proposed; new producer flag on a user verb)
$ jet debug app.jet --replay=flaky             (proposed; deterministic, via the ratified replay adapters)
(jet) continue
paused at app.jet:41 — the recorded run stops here
```

This builds no per-variable history and no reverse-step — those stay deferred exactly as ratified. It puts the recorder on the verbs users hold, so Epoch 6 starts from a shipped artifact instead of a design. Two D-JREPLAY1 clauses are amended by option A and named in the ballot: "no option … changes `jet run`" (producer flags land on run/dev/test) and "consumption stays exactly `jet prove <target> --replay <artifact>`" (`jet debug --replay=` becomes a second consumer). The closed `--capture=ARTIFACT` grammar is kept, not amended — the artifact name always follows `=`.

Options: **A** — `--record=` on run/dev/test + `jet debug --replay=` now (recommended). **B** — capture stays only under `jet prove`; add `jet debug --replay=` only. **C** — everything waits for Epoch 6.

### The verb space itself

A tooling audit owes the same sweep a syntax audit owes: what is claimed, reserved, or squatting. `jet serve` sits in the command registry, completions, and the man page as a verb whose only action is to refuse ("Use `jet dev --swap` instead") — it should leave the registry and live only as a typo teaching error, keeping the word reserved. The observer flags become one named family — `--watch`, `--record`, `--trace-tiers`, `--json` — documented together as "observers" so the next verb inherits them instead of reinventing them. The in-session keystroke space (`r R t f q`) is claimed here; future lenses (`b` bench, `p` profile) extend it by ballot, not by drift.

### The ladder — beginner magic to expert control

Rule: no upper rung changes what the lowest rung does.

**Rung 0 — type nothing.** A file runs. Script mode (ratified, unbuilt) means the first program is one line. A failed claim shows the same full report everywhere.

```jet
print("hello")
```

```
$ jet hello.jet
hello
```

**Rung 1 — one watched loop.** `jet dev file.jet`. Saves re-run. Type-stable edits hot-swap and say so. Broken saves keep the last good program running.

**Rung 2 — the first claim.** A `#Test` next to the code; `jet test` finds every claim in the package. No registration, no manifest, no separate file.

```jet
fn double(n: Int) => Int { n * 2 }

#Test("double returns twice the input") {
    require_eq(double(3), 6)
}
```

**Rung 3 — inputs get stronger.** Property tests from a typed parameter. Doctests from the docs. Snapshots with a review flow. A measured claim with a budget. All the same construct, climbing the input axis.

**Rung 4 — full control.** `--filter`, `--shuffle=seed`, `--serial`, `--coverage` (shipped); `--json` (ratified shape, unbuilt); `-p member` and `--affected` (ratified D-JPK-SELECTOR1=C, unbuilt). `--trace-tiers` shows every tier decision. `--record=`/`--replay=` capture a run (proposed). `jet prove` grades all evidence in one ledger. Profiles are explicit; budgets gate CI.

Every magic default keeps its three exits:

| Magic | See it | Spell it | Refuse it |
|-------|--------|----------|-----------|
| dev auto-detect (resident vs re-run) | the `[hot-swap]`/`[restart]` lines name the choice and reason | `--swap` or `--restart` pins the mode; `--interpret` pins the tier | pinning a mode is the refusal of auto-detect; `--watch=off` refuses the loop itself |
| claim discovery | `jet test --list` (proposed) | `--filter=`, `-p member` | no refusal exists today; proposed: a package `settings` switch that excludes doctest or path-scoped collection |
| build/run caching | `jet explain-build` (D-BUILDQUERY1=A) | pinned profiles | `--no-cache` (D-BUILDCACHE1=A) |
| tier selection | `--trace-tiers` | `jet build` for pure AOT | no exit by ratified choice — deopt is silent (D-LENS-RUN2=A); `jet build` sidesteps the tier choice entirely |
| affected-only watch re-run | the "N claims re-run (affected)" line | plain `jet test` for the full suite | proposed: `--watch --all` re-runs the full suite on every change |

## The final vision

One beginner file, the whole loop, nothing typed that is not the program:

```jet
// feed.jet — proposed end state (script mode ratified, unbuilt)
fn price(qty: Int) => Int { qty * 3 }

print(price(14))

#Test("price scales") {
    require_eq(price(2), 6)
}

#Bench("price hot path") {
    .setup { q :: 1000 }
    price(q)
}
```

```
$ jet feed.jet            # runs it            $ jet test feed.jet      # claims, full frames
42                                             price scales: pass (0.1 ms)
                                               1 passed
$ jet dev feed.jet        # the session
watching feed.jet …                            $ jet bench feed.jet
✓ ran in 31 ms                                 price hot path  2.9 ns/iter  [aot, optimized]
  r re-run  t tests  f failed  q quit
```

The expert extreme, same machine, no new mechanism:

```
$ jet test . -p core --affected --shuffle=7 --json > claims.json   # -p/--affected: D-JPK-SELECTOR1=C, unbuilt
$ jet test app.jet --record=flaky         # proposed producer flag
$ jet debug app.jet --replay=flaky        # proposed; deterministic, ratified replay adapters
$ jet prove app.jet --lens tests          # every claim, graded: proved / passed / observed / met
$ jet bench . --filter=parse --json       # proposed parity: optimized tier, labeled, budget-gated in CI
```

The end-state shape of the whole area:

```
one program (I9: one meaning on every tier)
│
├── the run ────────── observers ──────────────────────────────
│     jet run          none
│     jet dev          watcher + resident session (keys: r R t f q)
│     jet test         claim harness         ┐
│     jet bench        measurer              │ one generated harness,
│     jet debug        stepper               │ one report frame,
│     jet repl / nb    prompt (script law)   │ one collection
│     --record         recorder (.jetproof-replay)
│
└── the claim ──────── inputs × evidence ──────────────────────
      require(x)       caller      → observed
      #Test            author      → passed
      /// doctest      docs        → passed
      #Test fn (p: T)  generated   → passed (sampled)
      snapshot/golden  stored      → passed
      #Bench + Budget  measured    → met
      refinements      prover      → proved
      … all graded in one ledger: jet prove
```

## What this unlocks

**Teaching.** The first hour becomes: write a line, `jet dev`, add a claim, press `t`. One mental model from hello-world to CI.

**Servers and games.** The session keys plus hot-swap make the resident loop first-class; `--record` on a crashing service turns "cannot reproduce" into an artifact you step through.

**Critical software.** The claim grid ends at `proved`; the same `require` a beginner writes is the fact an expert later discharges statically. Nothing is rewritten to climb the ladder.

**Performance work.** Honest benches (right profile, labeled tier, deterministic budget gates) end the "numbers from the wrong tier" class of error permanently.

**Epoch 6.** Time travel lands on a shipped recorder and a mature debugger, exactly as its deferral demanded — instead of starting from zero.

## What stays

- **I9 and the parity ledgers.** The 12 `run_gaps` rows remain live violations to burn down, not accepted debt. This proposal adds no carve-out.
- **Silent deopt (D-LENS-RUN2=A).** Ratified on the merits: beginners never see tier noise; experts have `--trace-tiers`.
- **`#Test` as the only test syntax (D-TESTKIT1=A).** The unification strengthens it: even benches become its family.
- **`jet dev` as the one loop (D-CLI-DEVSERVE1=A)** and the announced swap/restart contract.
- **The nine Output kinds and plural Checks under `jet test` (D-ECO-OUTPUT-*=A).**
- **The `jet prove` word discipline** — proved / passed / observed / met — it is the evidence axis, unchanged.
- **The Epoch 6 time-travel deferral (D-TIMETRAVEL1=C).** The recorder is its named prerequisite, not its reopening.

## Decisions for the owner

| Ballot | Question | Options (first = recommended) | Amends |
|--------|----------|-------------------------------|--------|
| D-RUN-LAW1 | Adopt "one run, one claim" as the law of this domain? | A adopt / B dev-loop half only / C testing half only / D decline | none (names the model) |
| D-CLAIM-WORD1 | One assertion word? | B `assert` family / A `require` family / C fresh `claim` family / D keep both | A: D-CORE-PRELUDE1. B/C: S43, D-PRELUDE-LAW1, D-FAIL-BREACH1 wording |
| D-CLAIM-BENCH1 | Benchmarks are measured tests? | A keep `#Bench` as the measured-test marker / B spell as `#Test` + `.measure` / C status quo | D-BENCH-MARKER1 wording; completes D-PERFBUDGET-BENCHMIGRATE1=B and GAMEMIGRATE1; fixes spec.md:2235 drift |
| D-RUN-WATCH1 | Watch as a modifier on every runnable verb? | A `--watch` everywhere + `jet dev` stays / B fold into `jet dev` lenses / C status quo | none (extends D-CLI-BARE1 pattern) |
| D-RUN-SESSION1 | One resident session with in-session keys and attaching tools? | A full session model / B keys only / C status quo | none |
| D-RUN-RECORD1 | Recorder on user verbs now, as the Epoch 6 on-ramp? | A `--record=` + `jet debug --replay=` / B `jet debug --replay=` only / C wait for Epoch 6 | A amends two D-JREPLAY1 clauses (producer: "no option changes `jet run`"; consumer: "consumption stays exactly `jet prove --replay`"); B amends the consumer clause only; respects D-TIMETRAVEL1=C |

Defect cards (no owner decision needed): package-wide claim collection; test-frame restoration (D-REPORT-TEST1=A); golden/fixture honest errors; `jetpack test` defers to `jet test`; parse-error exit alignment for `jet fmt --check`; `jet serve` leaves the generated help surfaces; the parity-audit triage table gets cards.

## Implementation shape

**Phase A — one substrate, no surface change.** One watch choreography for native/web/canvas. One harness substrate under `#Test` + Checks. Test failures render the one report frame (ratified debt). Package-wide collection. Honest golden/fixture errors. `ExitCodes` everywhere. All tests green; user-visible output changes only where ratified law already demanded it.

**Phase B — land the ratified-but-unbuilt on the substrate.** Script mode (the repl/notebook/entry law). Prelude additions as resolved by D-CLAIM-WORD1. Bench parity (#1452) and the optimized, labeled bench profile. `--json` on test/bench via the D-REPORT machine shape. Built once, on one substrate.

**Phase C — the balloted surface.** The claim-word migration across every example and doc. The `#Bench` unification. `--watch` on test/bench/check with affected-only honesty. Session keys and attaching tools. `--record`/`--replay` on user verbs. Each is a coherent greenfield migration that deletes the replaced form.
