# Say it once: one home for every truth in Jet

*First-principles audit of the whole corpus — every domain, every surface, every engine. 2026-08-07.*

## Executive summary

This audit swept all of Jet at once: syntax, types, patterns, memory, authority, failure, concurrency, comptime, stdlib, modules, packages, CLI, dev loop, backend engines, examples, web, Canvas, the decision ledger, and the negative space between them. Fifteen domain readers and four cross-cut passes, every claim pinned to a file and line.

The finding is one sentence. **Jet keeps ratifying "one X" laws, and the corpus keeps growing second copies of X, because nothing makes the first home the only possible home.** Every report you have read that said "the same machine wearing N coats" was looking at one limb of this single animal. We counted the whole animal: **18 cross-domain machines wearing roughly a hundred hand-sewn coats.**

The one idea, and the law this proposal asks you to adopt:

> **Say it once. Every truth in the corpus — a semantic, a table, a message, a spelling, a decision — has exactly one home. Every other appearance is rendered from that home. A law ships with its guard, or it is prose.**

Here is the "ohhh": you already ratified this law — for programs. D-FACT-LAW1 says a program fact has one registry, moves toward safety silently, and every exception is one written word on the record. D-FACT-WORD1 says the law's words are fixed and every surface uses them. The compiler enforces "one home per fact" for the user's code while keeping two hand-written schedulers, three copies of the effect-root list, four copies of one error message, and two decision ledgers about itself. This proposal extends the law you already have from *program facts* to *the corpus itself* — engines, tables, text, spellings, and decisions.

Why now, in three verified facts:

1. **The law's own organs are unbuilt.** The ratified fact registry covers 3 of 8 planes, the ratified drift guard does not exist, and `jet inspect gates` is not in the CLI. The strongest law in the constitution is currently prose.
2. **The drift detector is itself drifted.** `tests/truthfulness.rs` — the suite built to catch exactly this class of rot — is red on master and frozen until end of epoch (card #1462). New drift in its categories is invisible today.
3. **Tier parity has already forked in user-visible ways.** `core.data.variance([])` is an error on AOT and JIT but returns `0.0` on comptime and the interpreter — two different algorithms, four hand-written kernels, and a parity test whose fixtures are too small to notice.

What the ballots ask (details in "Decisions for the owner"): adopt the corpus law and its guard doctrine; resolve the one real contradiction in ratified law (I9's blanket tier parity vs D-VERDICT-1254-1's interpreter carve-out); pick the enforcement mechanism for retired spellings; pick dispositions for the overloaded words (`stream` and `yield`, `derive`, `wasm` — `grant` needs no ballot, only the finish of ratified law); heal the two-ledger decision record; adopt the stdlib verb table; give the three audited-escape markers one gate ladder; and settle the `@` sigil's unfulfilled reservation.

What does not change: no beginner spelling gains a word. This proposal is almost entirely *deletion and enforcement* — it deletes hand-written copies, finishes laws you already ratified, and makes the guard automatic. Where the surface changes, it changes by removing a second spelling, never by adding ceremony. And the landing zone is already prepared: every prior Tower decision is now ratified — this audit's ten ballots are the only open ones on the board — and six whole rethink families are 100% law and ~0% built. This proposal gives all of them one substrate to land on so they are built once.

## The problem, briefly

A "coat" is a hand-maintained second copy of a truth that already has a home. Coats are not style debt. They are the exact failure D-FACT-LAW1 forbids in programs: a fact that moved without a word, off the record. Kept in sync by comments ("matches AOT"-style promises across ten files), habit, or luck — and we caught them mid-divergence.

The eighteen machines. Every row verified at file:line in the audit evidence; the worst symptom shown for each.

| # | One job | Coats | Worst symptom today |
|---|---------|-------|---------------------|
| 1 | Execute one semantic on every tier | 5 | Two schedulers: `scheduler.rs` (2,216 lines, 80,470 B) vs `Prelude/Scheduler.rs` (2,726 lines, 99,398 B), disagreeing across most of their length |
| 2 | Dispatch one Core call | ~10 tables | `variance([])` errors on AOT/JIT, returns `0.0` on comptime/interpreter |
| 3 | Tell a human something went wrong | ~10 renderers | Runtime panics carry no code, no why, no fix; `jet` itself has no panic hook, so a real ICE prints raw Rust panic text |
| 4 | Name what a scope may do | 5 vocabularies | Effect roots exist as 3 hand-synced lists (10, 28, and 28 names) across 2 crates |
| 5 | Step off the safe path, on the record | 6 markers, 3 depths | `#Unsafe` gets a 5-scope, 6-mode org ladder; `#Impure` gets one CLI bool; `#Nondeterministic` gets nothing; ratified `jet inspect gates` unshipped |
| 6 | Hold a fact the compiler knows | 13 sentinels | Thirteen compiler-knowledge concepts smuggled as `"\0"`-prefixed strings, recognized by `==` at 30+ sites |
| 7 | Emit a versioned machine record | 8 envelopes | `TRACE_VERSION` is a `&str` in one file and a `u64` two files away; 5 different spellings of "give me JSON" |
| 8 | Describe a program to a consumer | 5 systems | `T.reflect()` and `reflect.of(x)` share one field name and no conversion path |
| 9 | Find the package root | 5 finders | Canvas only knows `package.jet`, so it cannot root most of the repo's own examples |
| 10 | Walk the call graph for a property | 5 walkers | One `IMPURE_BUILTINS` table, three independent walkers over it |
| 11 | Render help for a command | 7 paths | `jet build --help` is error E2102; the guard against it is hand-copied 21 times; compiled Jet programs get `--help` free via `#CLI` |
| 12 | Keep a closed list of legal names | 8 registries | `Syntax.rs:77` forbids a second marker table; `STDLIB_DSL_BLOCK_MARKERS` is that second table, and sema branches on it |
| 13 | Retire a spelling | 0 mechanisms | `run.jet` (ratified default): 0 files. `main.jet` ("compatibility fallback"): 94. Both accepted silently |
| 14 | Grant a type a capability | 4 spellings, 3 generators | One generator emits raw Rust with no sema re-check (I3) |
| 15 | Track what is not done | 6 ledgers | `jit_gaps.txt` holds 48 named parity holes; AGENTS.md forbids parking work there; the truthfulness suite is red and frozen |
| 16 | Record a ratified decision | 2 ledgers | 927 decision IDs cited in the spec; 675 have no Tower record; the drift linter scans neither |
| 17 | Keep untrusted text from a sink | 3 mechanisms | A propagated tag and a typed literal do the same job with hand-listed sink sets |
| 18 | Exit with the right code | 1 table, 5 bypasses | The table calls itself "the public contract"; two command files use raw `exit(1)`/`exit(2)` |

Three specimens, up close.

**Specimen 1 — the fork you can measure.** Four hand-written kernels compute `core.data` statistics. Two use compensated summation and Welford variance; two use the naive algorithms. The comptime copy's module doc claims it was "ported verbatim … byte-for-byte." It was not:

```rust
// AOT Prelude + JIT host (DataFlow.rs:210-241 jet_data_variance_checked, jet-jit/Data.rs):
// Neumaier-compensated sum, Welford variance, Result-returning on empty input.

// comptime + interpreter actually call DataLite.rs:89-105:
// naive two-pass sum/variance, infallible — and DataLite's own test
// asserts variance([]) == 0.0 at line 504.
```

The dedicated differential test (`tests/comptime_diff.rs`) passes, because its fixtures are small and clean. This is what "kept in sync by luck" looks like when the luck holds — and what I9 was written to make impossible.

**Specimen 2 — one sentence, four homes.** The deadline-exceeded failure text (E3003) is string-typed by hand in four files: `Prelude/TaskGroup.rs:73`, `scheduler.rs:66`, `MathRandomTime.rs:50`, `jet-jit/Concurrency.rs:530`. All four spell `"Why:"` with no leading space. The canonical renderer (`crates/jet-foundation/src/Diagnostics.rs:367`) and the E3001 crypto sibling spell `" Why:"` with one. The copies disagree with the law *and with each other*, and the I4 coverage test cannot object — it checks that a code exists somewhere, never that it exists exactly once.

**Specimen 3 — ratified spellings, zero enforcement.** The corpus's own files, counted in a clean worktree:

| Ratified current form | Files using it | Retired form | Files using it | What the loader says |
|---|---|---|---|---|
| `run.jet` (entry, D-VERDICT-678-1) | 0 | `main.jet` | 94 | silence |
| `package.jet` (D-ECO-FILEROOT1) | 7 | `pkg.jet` | 45 | silence |
| bare `name:`/`version:` fields (D-CONF-NAME1) | new | `payload:{}` gen-1 | a live sema test (`crates/jet-sema/tests/generic_module_body.rs:218-236`) | silence |
| `target@provider` package refs (D-JPK-REF1) | most | `provider@target` order | the repo's own `env.jet` | E1317 is registered, yet the file ships unflagged |

Greenfield law says the replaced form is deleted in the same change. Nothing implements that law: there is no diagnostic, no `fmt` rewrite, no ratchet. So every rename ratified this week joins this table next week. The mechanism is the missing feature; the rows are just its evidence.
## The proposal

Twelve elements. Each one deletes coats, and each one is shown as code on the page. Markers: **[ratified]** = already law, this finishes it; **[amends]** = changes a named ratified decision, the ballot says which; **[new]** = a fresh owner decision.

### 1. The corpus law, with its guard **[new — ballot B1]**

D-FACT-LAW1 gave program facts a registry where every row states its safe direction and its gate words, with a drift guard that fails a row stating neither. That row shape is the invention. This element applies the same row shape to the corpus: every unified truth registers its home, its renderers, and its guard.

```text
# a law row, today (ratified for program facts):
fact Effect        home: Facts.rs        direction: rights-shrink     gate: #Caps

# the same row shape, extended to corpus truths (proposed):
truth effect-roots   home: Facts.rs::EFFECT_ROOTS   renders: Effect enum, BuildEffect, CLI flags, E3503 copy   guard: tests/one_home.rs
truth E3003-text     home: diagnostics registry      renders: every tier via Report                             guard: grep-net "exactly once"
truth scheduler      home: Prelude/Scheduler.rs      renders: AOT include_str!, JIT include!                    guard: no second impl compiles
truth entry-file     home: Syntax::ENTRY_FILE        renders: loader, docs, examples                            guard: adoption ratchet
```

The guard doctrine is the second half, and it is the reason this time is different: **every "X exists" test gets an "X exists exactly once" sibling.** The I4 coverage net checks a diagnostic code exists somewhere — that net passed while E3003 lived in four places. A uniqueness sibling (`rg` for the literal, assert one home) costs minutes per truth and turns every future coat into a red test instead of an audit finding. A law without its guard stays what the fact law is right now: prose.

- Beginner rung: nothing. Beginners never see any of this — they just stop meeting forks like `variance([])`.
- Expert rung: `jet inspect gates` (ratified, D-FACT-GATE1) ships; the `$` fact read (ratified, D-FACT-READ1 — the mark, not a tools-only surface) reaches every registered plane; and a proposed `jet inspect facts` makes the row registry itself readable — one address to see every truth, its home, and its guard.
- The exit: a guard row can be waived only by a named Tower decision on the row — same as any other loosening, one written word on the record.

### 2. One source, every engine **[ratified I9 — plus one contradiction to resolve, ballot B2]**

The corpus already contains the correct pattern, four times, in the same directory as the violation:

```text
Prelude/TaskGroup.rs      "This exact Prelude source is compiled for JIT hosts and embedded in AOT programs."
Prelude/Stream.rs         one 149-line substrate "so suspension, cancellation, cleanup cannot drift by tier"
Prelude/SharedProtocol.rs one lock/condition protocol for native values and evaluator adapters
Prelude/CoreLib/Top/IoLineStream.rs  "so the JIT host can include! this exact source instead of re-encoding the logic a second time"
```

And the violation, in the highest-stakes file of the domain:

```text
BEFORE:  crates/jet-codegen/src/scheduler.rs         2,216 lines, compiled for JIT hosts
         crates/jet-codegen/src/Prelude/Scheduler.rs 2,726 lines, embedded in AOT programs
         — the two disagree across most of their length; E3003 text typed separately in each; "one mechanism" is a comment

AFTER:   crates/jet-codegen/src/Prelude/Scheduler.rs  the only scheduler
         jet-jit:  include!-s it, supplies representation adapters only
         AOT:      include_str!-s the same bytes
         guard:    a test that fails if a second file defines jet_scheduler_*
```

Same treatment for the other four engine coats: the `core.data` kernels (delete `DataLite`, comptime and the interpreter call the one Prelude function — and the differential test gains adversarial fixtures: empty input, catastrophic-cancellation arrays), the derive generators and the two purity walkers (both already named as shadow systems by the metaprogramming and authority slates — this element adds only their guards), and the JS `DomRuntime` port (which the audit verified is a faithful line-by-line port — so it gains a guard that diffs the two sources structurally, the cheapest possible parity proof).

`tests/jit_gaps.txt` — 48 named parity holes (36 `gaps:` + 12 `run_gaps:` stems) in a file AGENTS.md forbids parking work in — retires as a ledger. Each gap becomes a card; the file becomes a shrink-to-zero ratchet, then deletes. The ledger has already produced its natural end state once: card #1363 "JIT gaps → zero" closed `done` with all 48 gaps still on file. A closure that is itself a coat — and the sharpest argument for guards over promises.

One tension in ratified law has to be resolved by you, because both sides are law. I9's headline says the interpreter preserves one meaning for every feature. D-VERDICT-1254-1 says, in full: "Full JIT/AOT parity required, but the user-visible law is byte-identical default-run behavior. Interpreter-tier parity is a lower concern; implementation may close gaps or route around the interpreter." The verdict already demands byte-identical default runs — the open question is only the interpreter tier, and card #1616 shows where its lower-concern status leads: the interpreter silently skips view-range bounds checks today, so a beginner's deopt path is the least safe one. Ballot B2 asks you to pick the resolution once, in writing.

### 3. One table, rendered everywhere **[ratified in spirit (D-FACT-WORD1) — mechanism is new, ballot B1 covers it]**

A vocabulary is a table. Every surface renders the table; no surface restates it. Today:

```rust
// BEFORE — the effect roots, three hand-synced homes in two crates:
pub enum BuildEffect { Net, FS, IO, DB, Time, Rand, Env, Exec, Log, GPU }          // BuildEffects.rs
pub enum Effect { Net, FS, IO, DB, Time, Rand, Env, Exec, Log, GPU, /*+18*/ }      // jet-sema/Effects.rs
pub const EFFECT_ROOTS: [&str; 28] = ["Net", "FS", /* the same 28 again */];       // Facts.rs — "the canonical one"
// and a comment still calls Effect "the closed twelve-root enum" while the enum holds 28.
```

```rust
// AFTER — one table, everything else derived:
// Facts.rs owns the roots. The enums, the CLI --deny-* flags, the E3503 menu copy,
// the REPL completion list, and the spec table are all rendered from it.
// The drift guard fails the build if any surface string-matches a root it didn't derive.
```

The same move, across the board: the second DSL-marker table (`STDLIB_DSL_BLOCK_MARKERS`) is deleted and sema branches on the one registry it was shadowing; `resolve_type`'s five hand-rolled per-module leaf lists collapse into the generic registry arm that already exists twenty lines below them; the CLI's 54 twice-declared commands become one declaration; the 8 phantom flags that exist only in usage-string prose (`--ar --clang --facts --from --no-sign --pkg --registry --to`) become real `FlagSpec` rows and instantly appear in completions, the man page, and typo suggestions; `fact_covers` stops being reimplemented inline in the one file whose comment says the rule "lives in exactly one place."

Honest accounting: several instances here are already owned — the effect-root merge is ratified (D-AUTHORITY-ROOTS1), and the second-table deletions are named by the marker and names slates. What is new in this element is the doctrine: every such table registers its renderers and its guard, so the next second table cannot compile.

### 4. One voice when something goes wrong **[ratified — D-REPORT-LAW1/SEV1/TEST1/MACHINE1; the failure and toolchain slates own the build. New here: the uniqueness guards and the panic hook]**

You ratified the report law this week, and the failure and toolchain proposals already carry its build plan — this element does not restate them. It shows the landing, and adds the two pieces neither slate names: a uniqueness guard per message (the E3003 quartet passed every existing test) and a panic hook on the `jet` binary itself. The finish line, as what a user sees:

```text
BEFORE — a beginner's program divides by a bad index at runtime:
panic: index 12 is past the end (len 5)
  --> inventory.jet:14 in restock

AFTER — the same failure, wearing the same contract as every compile error:
Error [E7203]: index 12 is past the end (len 5)
  --> inventory.jet:14 in restock
    |
 14 |     bins[i] = count
    |     ^^^^^^^
 Why: `i` came from the loop over `orders`, which has more rows than `bins` has slots
 Fix: clamp the index, or iterate `bins` and look orders up by key
      run `jet explain E7203` for the full story
```

One `Report` type, rendered by one renderer, on every surface: compile diagnostics, runtime panics, contract failures, GC faults, FFI panics, scheduler fatals, test failures, tool errors, and the ICE banner. Runtime failures gain codes, so `jet explain` works on a crash for the first time. The four hand-typed E3003 copies become one registry row that all tiers materialize (this is the axes pass's sharpest catch: the law says "at runtime, no fact remains" — but a *failure* is the one place a fact must re-materialize, as the report). The `jet` binary installs a panic hook, so an internal crash prints a branded ICE report instead of raw Rust panic text — I2's promise at the exact moment it matters. The two JSON envelopes become the one ratified schema; the three test-summary shapes become one; the five exit-code bypasses go through the one table that already calls itself the public contract.

- Beginner rung: strictly better output, zero new ceremony.
- Expert rung: `--json` is one schema everywhere; every failure is `jet explain`-able; severity words obey D-REPORT-SEV1.
- The exit: none needed — there is no magic here, only fewer formats.

### 5. Retiring a spelling is a mechanism, not a memo **[new — ballot B3]**

Greenfield law says: migrate every consumer, delete the replaced form, same change. The corpus shows what happens without a mechanism: 94/94 entry files on the retired name, 45/52 manifests on the retired name, a ratified-this-week field shape already coexisting with two dead generations. The proposal: when a spelling is retired, the retirement ships as code —

```text
a retirement ships all three, in the same change:
  1. an error:      E13xx "`main.jet` was retired 2026-07-17; the entry file is `run.jet`"  (what/why/fix)
  2. a rewrite:     `jet fmt` / `jet fix` performs the rename mechanically where it can
  3. a ratchet:     a test asserting the retired form's count in-repo is 0 — and stays 0
```

Ballot B3 picks the default posture (hard error vs auto-rewrite-with-notice vs both by category). Either way, "silently accepted forever" stops being one of the outcomes.

The mechanism's own ladder, since it touches user files: see it — every rewrite prints what it changed and why, with the retirement row's ID; spell it — `jet fix --dry-run` shows the exact renames before any write; refuse it — a project switch pins a retired spelling only through the same named-exception path greenfield law already requires, so refusal is possible but never silent. The first sweep applies it to the known rows: `main.jet`, `pkg.jet`, `payload:{}`, `provider@target`, and — if B5 ratifies — the body-line `derive` (element 6).

### 6. One word, one mechanism **[new — ballots B4–B6]**

"One written word" only works if each word means one thing. The worst offenders, with proposed dispositions — each a separate ballot row so you can pick per word:

| Word | Meanings today | Proposed disposition |
|---|---|---|
| `stream` | generator `Stream<T>`; codec reader/writer mode; file-line iteration; `Event<T>` "occurrence stream"; future `core.data` streams; two schedulers' internals | `Stream<T>` keeps the word. Codec mode is spelled `reader`/`writer` (it already is — the *prose* stops calling it streaming). Events are events. One spec page: "what Jet means by stream" **[B4]** |
| `derive` | request a capability (body line, D-USERDERIVE1); request it (marker form); *define* a provider (`derive T.Trait {}`, D-METADERIVE1) | the marker form `#Comparable` is the one request spelling; the body line retires (element 5's mechanism, amending D-USERDERIVE1); the keyword keeps only D-METADERIVE1's meaning: *define a provider* **[B5]** |
| `grant` | `#Grant` — scoped compile-time effect grant; `jet trust grant` — durable on-disk authorization | already settled, only unfinished: D-AUTHORITY-SCOPE1 deletes `#Grant` (`#Caps` carries scoped effects), and D-AUTHORITY-WORD1 retired "capability" — yet the handle type is still named `Capability` (`effects_surface.rs:131`) and the trust verb still says `grant`. Finish the ratified migration; no new decision needed **[ratified, finish]** |
| `yield` | the suspension keyword; "yielding loop" prose for eager `->` comprehensions | the prose changes; comprehensions are "collecting loops." Zero code changes **[B4]** |
| `wasm` | `#Target(Wasm)` browser compute bucket; `target: plugin` sandbox kind | rename the plugin kind's user word to `sandbox` (it is about isolation, not the ISA) **[B6]** |
| `schema` | 4 unrelated meanings across envelopes and manifests | falls out of element 4's one-envelope rule |

Not on the menu on purpose: `module`'s six roles are already settled law — D-NAME-ROLEMOD1 and the names slate own that retirement; this table only stops short of re-deciding it.

### 7. If the compiler knows it, it has a type **[ratified — D-FACT-HOME1 names the phantom types; this extends the sweep]**

Thirteen `"\0"`-prefixed sentinel strings smuggle real compiler knowledge through fields meant for user identifiers, string-compared at 30+ sites. Two specimens and their honest forms:

```rust
// BEFORE: a numeric widening is a fake function call, recognized by name
call.name = "\0numeric.approx_widen";            // CheckerInfer/direct_calls.rs:87
// ...and four downstream consumers each special-case the magic string.

// AFTER: it is what it is
Expr::Widen { kind: WidenKind::Approx, from, to }
```

```rust
// BEFORE: a physical dimension round-trips through a string
Type::Apply { name: "\0Quantity", args: vec![base, Type::Named(dimension.identity())] }

// AFTER:
Type::Quantity { base, dimension }               // Dimension is already a real struct
```

Same sweep for the shared-guard access modes, the crypto nominal marker, the clock provenance tags, and the layout selector (whose own comment admits the shortcut). D-FACT-HOME1 already ratified retiring phantom types — this is its worklist, plus the widen node and quantity variant it did not name.

### 8. Siblings of one mechanism carry one depth **[new — ballot B7]**

Three markers share one shape — "I am stepping outside the default, here is my written reason" — and three unrelated depths of machinery behind it:

| Marker | Reason | Org policy ladder | Scope control | Ledger |
|---|---|---|---|---|
| `#Unsafe` | required | 5 scopes, 6 modes, env-var org file | per-site | `jet inspect unsafe` ✓ |
| `#Impure` | required | — (one CLI bool: `--allow-impure`) | — | — |
| `#Nondeterministic` | required | — | — | — |

The ledger half of this is already ratified: D-FACT-GATE1 explicitly names impure marks among what `jet inspect gates` shows, and card #1571 carries it. What remains — and what ballot B7 asks — is the *ladder* half: every audited escape rides the same org-policy ladder (`#Unsafe`'s — it is the good one), and `--allow-impure` retires so the org policy file speaks for all three. A beginner still types exactly one marker with one sentence; the difference is one policy surface instead of three shapes. The same one-depth rule then covers the smaller siblings the audit found: five project-level deny switches in five unrelated shapes, and bounded buffers where the channel can only block while `AsyncEvent` has three overflow policies.

### 9. One decision ledger **[new — ballot B8]**

The spec cites ~927 decision IDs as law; Tower holds 505; 675 of the cited IDs have no Tower record — including several this very proposal must lean on (D-ENUMDOT1, D-PLUGIN1, D-STDRUBRIC1, D-ONECORE1 live only as spec prose). The gap runs both ways: 253 Tower IDs are cited nowhere in the spec. `tower lint --docs` scans a directory that now contains a README; and D-VERDICT records can silently contradict each other one day apart with no supersession link (D-VERDICT-1231-1 vs -1308-1, comptime keyword, both "law"). Ballot B8 picks the direction: **(a)** Tower becomes the one home and the spec's decision blocks are imported once, then rendered; or **(b)** the spec stays the prose home and Tower rows link to spans with a lint that walks `docs/spec/**`; either way, supersession links become mandatory on verdicts, and the ledger gains the one query that matters: *is this ID current law?*

### 10. The surface dividends — beginner magic, expert control

Everything above is the machine room. These are the user-visible fixes that fall straight out, each one currently blocked only by a coat:

```text
jet --help                     works (today: E2101 at the top level, E2102 one level down; the args engine Jet ships to users renders it)
jet add http                   works from the terminal (today: the deps engine is reachable only from the Canvas GUI)
jet explain E7203              works on runtime failures (the failure slate's deliverable; the corpus law's guard is what keeps it single-voiced)
files.read(path)               takes a Path or a String everywhere (the corelib slate's D-CORE-PATH1 deliverable; listed as a dividend, not re-proposed)
bind-position destructure      the ratified-but-unbuilt form gets built once, in the spelling D-CHOOSE-TEST1=A just settled (card #1652)
jet inspect gates              one ledger for every step off the safe path (ratified, unshipped)
jet inspect reserved           the reserved-word and reserved-sigil list becomes readable (today: five teaching-reserved words are invisible to tools)
--quiet                        exists, one spelling, every command (today: nothing suppresses progress output anywhere)
```

Two deliberate *non*-items, per the epoch-scope rule: the UI story (`core.ui` vs `web.app()`, no `ui.run`, the unbuilt D-UITREE1 dot-construction spelling) and the `web.app()` graph that nothing serves are real findings, but they are future-epoch surface — this proposal only records them on cards with the minimal gate (the spec text stops promising the unbuilt spelling), and leaves the architecture to its own epoch and its own audit (card #1588 already exists).

### 11. One verb per job in Core collections **[new — ballot B9]**

The same job wears a different verb per collection, and the same verb does a different job per collection. Nobody can guess the second API after learning the first:

```jet
displaced :: prices.replace("eggs", 3.99)   // Map: upsert, returns the displaced value
swapped   :: names.replace("Bob", "Rob")    // List: swap the first equal value
canon     :: seen.replace(id)               // Set: canonical-swap, a third meaning
gone      :: prices.pop("eggs")             // Map: remove-and-return...
back      :: seen.take(id)                  // ...which Set spells take
```

And the written law drifts from the written code: the API rubric documents `Type.from_x()` as the conversion shape, while bare `.from()` dominates the real corpus 32 call sites to 21 with no rule blessing it. Ballot B9 adopts one verb table — one verb per job on every collection, the constructor law updated to match reality — and the retired verbs migrate through element 5's mechanism. Note for the record: the amended IDs (D-API-STORE1, D-STDRUBRIC1) are themselves spec-only today, which is element 9's disease; B9's amendment lands wherever B8 homes the ledger.

### 12. The at-sign, honestly stated **[amended by D-ONCE-AT1=D]**

The `@` sigil is position-sensitive. Prefix `@` marks compile-time names and
fact reads (`@config`, `T.@range`, `@build.profile`); infix `@` remains the
package-reference source separator, `textkit#1.2.0@vendor` and
`target@provider` (D-JPK-REF1, errors E0968/E0979/E1317). D-ONCE-AT1=D
supersedes B10's former reservation-only framing. A marker written with
leading `@` is still a teaching error: applied rules use `#`.

```jet
deps: { http: "jetlib/http#2.1@vendor" }   // infix: package refs
@config :: build.profile                    // prefix: compile-time/fact read
task :: work@node2                          // infix: package source ref
```

B10's reservation-only question is superseded. It does not touch D-JPK-REF1's
semantics — the infix package-reference meaning stays live, and D-ONCE-AT1=D
adds the prefix compile-time/fact-read meaning.
## The final vision

One session, both rungs. Everything marked *(proposed)* is not yet law; everything else is ratified today and merely unbuilt.

**A beginner's whole day.** They type nothing they don't type today — the language just stops disagreeing with itself:

```text
$ jet run shop.jet
Error [E7203]: index 12 is past the end (len 5)          (proposed: runtime failures wear the report contract)
  --> shop.jet:14 in restock
 Why: `i` counts orders, but `bins` has 5 slots
 Fix: iterate `bins` and look orders up by key. `jet explain E7203` for more.

$ jet explain E7203                                       (proposed: works on any failure, not just compile errors)
$ jet add http                                            (proposed: the Canvas-only deps engine, from the terminal)
$ jet --help                                              (proposed: E2101/E2102 retire)
```

And the same program means the same thing on `jet run`, `jet dev`, AOT, and comptime — `variance([])` has one answer, because there is one `variance`.

**An expert's audit, one address per question:**

```text
$ jet inspect gates            every step off the safe path: #Unsafe, #Impure, #Nondeterministic,
                               org policy, one ledger                     (ratified D-FACT-GATE1 — shipped)
$ jet inspect facts            every registered truth: home, renderers, guard        (proposed extension)
$ jet inspect reserved         every reserved word and sigil, with its reason        (proposed)
$ jet fmt                      rewrites retired spellings mechanically               (proposed mechanism)
```

**The corpus, before and after, as a tree:**

```text
BEFORE                                          AFTER
─────────────────────────────────────────       ─────────────────────────────────────────
scheduler.rs            2,216 lines  ┐          Prelude/Scheduler.rs      the only one
Prelude/Scheduler.rs    2,726 lines  ┘ fork     ├─ jit:  include! + adapters
DataLite.rs             naive math   ┐          └─ aot:  include_str!  (guard: no 2nd impl)
DataFlow.rs             real math    ┘ fork     Prelude/CoreLib/DataFlow.rs   the only one
BuildEffect (10) / Effect (28) /                Facts.rs::EFFECT_ROOTS        the only one
  EFFECT_ROOTS (28)     3 hand-syncs            └─ enums, flags, menus, docs: rendered
E3003 text × 4 files                            diagnostics registry row      the only one
Diagnostic + ~10 failure renderers              Report, one renderer, every surface
61 CommandSpec + 55 NestedCommandSpec           one command table → help, man, completions
  (54 names declared twice)
docs/spec IDs (927) vs Tower (505)              one ledger, supersession links, one query
jit_gaps.txt (48 holes) + 5 more ledgers        cards + shrink-to-zero ratchets
truthfulness.rs: red, frozen                    green, and grown a uniqueness net
```

If you read only this section: the language the user sees gets *smaller and truer* — one answer per question, one spelling per meaning, one voice when it fails — and every "one X" already ratified stops being a promise and starts being a build failure to violate.

## What this unlocks

- **The six ratified-unbuilt rethink families land once, not twice.** Concurrency (#1557-1565), metaprogramming (#1537-1545), type-system v2, config (#1517-1526), failure (#1527-1536), authority (#1566-1573) — all 100% law, ~0% built — land on a substrate where the engine copy problem is already gone. Building them before this cleanup means porting each one into four hand-written engines and re-auditing this same animal next quarter.
- **Agent throughput.** Every coat is a place where an agent (or you) must know which of N copies is real. One home per truth is the difference between "edit the registry row" and this audit.
- **Critical domains.** Simulation and finance get one numeric answer per operation across tiers (specimen 1 is disqualifying today). Security audits get one gate ledger instead of three surfaces and a grep.
- **Trivial domains.** One-liners and scripts inherit `jet explain`-able runtime failures and a `--help` that exists — the first-hour experience stops leaking the machine room.

## What stays

- **The borrow prover stays its own engine** (D-FACT-OWN1) — alias analysis is not a fold of registry rows, and the sigils stay the prover's surface, off the ledger. The wall is ratified and this proposal leans on it.
- **`Stream.rs`, `TaskGroup.rs`, `SharedProtocol.rs`, `IoLineStream.rs`** stay exactly as they are — they are the model the rest of the corpus is being brought to.
- **Canvas's source-backed projection** stays — the one-model success story, complete with its CI parity net.
- **`core.random` vs `core.crypto.random`** stays — the one same-name duplicate that was done right: ratified, documented, forces aliasing. It is the bar, not a bug.
- **Deliberate closed doors stay closed**: new effect roots remain owner-gated (D-EFF4/5), comptime never creates types, no macros, no HKT. Nothing here reopens a frozen wall.
- **Every kept spelling keeps its meaning.** No beginner program changes behavior except where two tiers currently disagree — and there, the ratified AOT meaning wins.

## Decisions for the owner

Each ballot stands alone; any subset adopts cleanly. Full worked options live in the Tower ballots.

| # | Ballot | The question | Touches ratified law? |
|---|---|---|---|
| B1 | corpus law + guards | Adopt "say it once": one home per truth, rendered surfaces, guard-or-prose — and the uniqueness-net doctrine | extends D-FACT-LAW1/WORD1; amends nothing |
| B2 | tier parity | Resolve I9's blanket parity vs D-VERDICT-1254-1's interpreter carve-out — full parity / named carve-out / tiered guarantee | amends one of I9 (text) or D-VERDICT-1254-1 |
| B3 | retirement mechanism | Retired spellings: hard error / auto-rewrite + notice / both by category — plus the first sweep (main.jet, pkg.jet, payload:{}, provider@target) | implements greenfield law; amends nothing |
| B4 | word: stream/yield | Adopt the vocabulary dispositions (prose + spec renames, zero user spelling changes; `module` excluded — D-NAME-ROLEMOD1 owns it) | amends spec prose only |
| B5 | word: derive | One request spelling (`#Marker`); body-line `derive X` retires; keyword keeps only the provider meaning | amends D-USERDERIVE1; keeps D-METADERIVE1 |
| B6 | word: wasm/sandbox | `target: plugin`'s user-facing word becomes `sandbox`; `Wasm` stays the browser bucket | amends D-PLUGIN1 naming |
| B7 | one gate ladder | `#Unsafe`/`#Impure`/`#Nondeterministic` share one org-policy ladder; `--allow-impure` retires (the shared ledger is already ratified: D-FACT-GATE1, card #1571) | extends ratified gate law; retires a CLI flag |
| B8 | one decision ledger | Tower-as-home (spec rendered) vs spec-as-home (Tower links + spec-walking lint); supersession links mandatory either way | process law; amends tower lint scope |
| B9 | stdlib verb table | One verb per job across collections (`replace`, `pop`/`take`, bare `.from`), rendered onto CLI verbs too | amends D-API-STORE1 family / D-STDRUBRIC1 |
| B10 | the @ sigil | The unfulfilled half of the reservation: design the location feature / narrow the reservation to the ratified packaging meaning / publish the reservation with a minting condition. D-JPK-REF1's live meaning stays under every option | amends the reservation text, never D-JPK-REF1 |

Killed before reaching you (the honest kill-check): a unified "observe" subsystem redesign (four trace systems is real debt, but the redesign belongs to the toolchain audit's card #1626 — here it would duplicate a mechanism); any UI-architecture ballot (future epoch, card #1588 owns it); a structured-reason taxonomy for `#Unsafe` (would add ceremony to the expert path for zero beginner gain).

## Implementation shape

- **Phase A — organs and guards, no surface change.** Build the ratified fact-law organs (registry rows for all 8 planes, drift guard, `jet inspect gates`), unfreeze and green `truthfulness.rs`, add the uniqueness nets for every machine in the coats table, install the `jet` panic hook. All tests green throughout; nothing a user types changes.
- **Phase B — delete the coats on the new substrate.** One card per machine: scheduler fork (#1637 exists), data kernels, effect-root tables (#1621–1624 exist), CLI registry, report renderers, E3003, resolve_type arms, purity walkers, `\0` sentinels, root-finders (Canvas), leading_dot_variant. Each closes by turning its guard on. Then land the six ratified-unbuilt families on the clean substrate — built once.
- **Phase C — the balloted surface unifications.** Each ratified ballot ships as one coherent greenfield migration that deletes the replaced form via the B3 mechanism: retirement sweep, derive spelling, gate-model merge, verb table, ledger reconciliation, @ disposition. Every migration ends with its ratchet at zero.

Effort is expendable; the outcome is one corpus that obeys its own best law.

---

*Evidence: 15 domain files + 4 cross-cut files from this audit's research pass (file:line for every claim), summarized in the coats table above. Prior ratified law consulted throughout: D-FACT-*, D-REPORT-*, D-CONF-*, D-AUTHORITY-*, D-CONC-*, D-META-*, D-NAME-*, D-BOUND-*, D-RUN-*/D-CLAIM-*, D-CHOOSE-* (ratified 2026-08-07, during this audit), I2/I3/I4/I5/I7/I8/I9.*
