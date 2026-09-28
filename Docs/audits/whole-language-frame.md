# One ledger: the whole-language frame

Status: proposal, 2026-09-01, revised after the beginner and rival-family review passes. Plan, cards, and ballots only. Nothing here is implemented. Every transcript marked **illustrative** shows what a command would print after its ballot is ratified and built; none of them runs today. Evidence rows cite the research lanes in `~/.cache/jet-luna/wla/lane{A..F}.md`, the review reports in `~/.cache/jet-luna/wla/reviews/`, and the repository at commit `8b9933668`.

## Executive summary

Jet already has one law for compiler-held facts: a fact moves toward safety silently, every move away is one written word on the record, and no fact survives to runtime (D-FACT-LAW1=B). It already has one law for the corpus: every truth has one home and every other copy is rendered from it (D-ONCE-LAW1=A). This audit found that six large parts of Jet still keep private copies of the same truth, and that the six directions the owner approved are not six new mechanisms. They are the two laws applied to the rest of the language.

**The one idea:** a Jet program is one ledger. Every truth about it — what a value is, what a scope may do, what a callable promises, what a type looks like on a wire, what a build was made from — is written once. Every tool (check, test, prove, dev, replay, serialize, inspect, fix) reads that ledger through one contract and keeps no copy of its own.

| element | today | proposed | ballots |
|---|---|---|---|
| rights | four private walkers and twenty flags answer one question | one walker, one frame, `--allow=`/`--deny=` | D-RIGHTS1, D-RIGHTS-CLI1, D-RIGHTS-DIAG1 |
| claims | twelve answers, six reports, no ladder | one evidence report; `jet test` generates from `#Pre`; `jet prove --lens solver` proves | D-CLAIM1, D-GRADE1, D-GRADE-POLICY1 |
| shapes | two JSON trees; args only at the entry function | one shape fact; `args.decode<T>()` and `Config.merge` | D-SHAPE-ONE1, D-SHAPE-PROJECT1 |
| live tier | swap by flag; a layout change restarts | swap by default; pinned published values migrate | D-LIVE1, D-LIVE-PERSIST1 |
| verdict loop | 83 of 762 rows carry a repair; two status shapes | per-plane ratchet; one `jet.status/v1` object | D-VERDICT-LOOP1, D-CLI-ONE1 |
| records | receipts, replays, and proofs with no index | one index and identity; safe capture by default in dev | D-RECORD1, D-RECORD-SPELL1, D-TIMETRAVEL2 |
| open tables | five hand tables repeat Jet declarations | generate where a twin exists; guard the rest | D-OPENTABLE1 |
| lexical space | no raw String, uneven commas, unenforced `;` | backtick literal, commas everywhere, E0373 enforced | D-RAWSTR1, D-TRAILCOMMA1, D-SEMI1, D-MARKERSHAPE1, D-MARKERARGS1 |

**What the ledger is, precisely.** Three stores by lifetime, one typed query over them: the static schema of fact kinds (the registry that exists today), the fact instances of one checked program, and the evidence records of one run. A claim's evidence is a record in the third store; a scope's rights are an instance in the second; a marker's legal sites are a row in the first. The rival-family review showed that two columns on the static registry could not hold a per-run count, and the design moved to the three stores.

**What the frame deletes.** Four private checkers that answer "may this code do X" (the purity walker, the replayable walker, the comptime purity stage split, and the CLI checker behind `--pure`; the flag's spelling stays) and twenty allow/deny flags. Twelve separate answers to "is this claim true" with six report shapes and no ladder between them. A second JSON value model. Four unlisted artifact families. Two CLI authorities. Nine compiler tables that have a declared Jet twin and drift from it. A semicolon the law already forbids and a missing raw string; the marker-shape challenge is declined and D-MARK-STACK1 stands.

**What the beginner types.** Nothing new, in any slate. The rungs above the beginner are opt-in: a rights row, a `#PublishedSchema` marker, a policy floor, a `--replay` flag.

**What the owner sees on the page.** A config struct that reads itself from flags, environment, and JSON with one stated precedence and no derive. A contract that gains generated evidence for free and solver evidence on request, with the runtime check erased once proved. A package that denies `Net` to its parsers, with one report frame that names the chain from the call to the refusing scope. A `jet dev` that swaps code on save by default, keeps the pinned world, and migrates it with the same `migration` block that already migrates wire data for published types. A crash in dev that leaves a safe replay you can keep as a regression claim. One `jet inspect <plane>` for every ledger question, and one machine status object on pass and on fail.

**The ballots.** Twenty-two on card #2500, in eight independent slates. Any subset can be adopted. Every ballot names the ratified decisions it amends. After the two review passes, fourteen recommendations moved: the ledger became three stores; rights kept the transaction checker; the rights error keeps its codes; claims keep `jet prove`; grade names became producer-plus-outcome; shapes stop at env and args; the live tier keeps `#Persist` and the layout-restart rule; the verdict loop became a per-plane ratchet on the ratified report line; records became an index over the ratified formats with the ratified safe capture as the dev default; time travel waits on a measured gate; open tables split by whether a twin exists; and marker stacking is a declined challenge. The fourteenth, typed marker arguments, narrowed to one signature.

**What does not change.** Every frozen wall stands: no macros, no HKT, no top type, comptime never creates types, I1–I9, no lifetime syntax. Every memory sigil, every keyword, `->`, `::`, `#`, `@`, `$`, `core.*`, `package.jet`, `-[…]>`, `#Pre`/`#Post`, `#Test`, `#Unsafe("reason")`, `migration`, `#FX`, `authority.holds`, `#Persist`, `jet prove --replay`, `.jetproof`, `.jetproof-replay`, D-MARK-STACK1, D-HOTSWAP1, the replay privacy law, the one-report-per-line law (the report object gains one optional field under D-VERDICT-LOOP1, schema `jet.report/v2`), and the five fix safety classes.

**Where Jet loses today** (measured, not argued): 9 of 9 blind matched-task choices went to Rust and 27/36 Jet projects were green against 36/36 (fresh-agent rerun 2026-08-31); Python wrote the same program in 125 lines against Jet's 220 (field audit 2026-07-26); `jet run` on the default tier is the tier that breaks (video mine 2026-08-28); native binary identity has never been proved byte-for-byte (lane D). Writing one page of examples for this proposal found three more default-tier defects (#2509, #2510, #2511). The frame does not fix reliability by itself. It removes the duplicate machinery that keeps producing the divergence.

**Strongest unverified assumption:** that every existing tool answer can be reproduced through one typed ledger query over three stores without a second registry. The parity test named in D-LEDGER1 is the proof; if it fails, the eight slates still stand as coordinated proposals and lose only their shared vocabulary.

## The problem, briefly

Jet is not fragmented at the level of keywords. Earlier rethinks already unified the type system (D-TYPE2), facts (D-FACT), failure (D-FAIL), names (D-NAME), callables (D-CALL), and the corpus (D-ONCE). The fragmentation left is one level up: the same question answered by several systems that each keep a private table, a private diagnostic, and a private report. The tables below are the evidence. Each is one question, and each row is one coat of paint on it.

### Question 1 — "May this code do X here?" Eleven coats

| # | coat | user spelling | home | defect |
|---|---|---|---|---|
| 1 | effect row on a callable | `fn f() -[FS.Read]>` | `crates/jet-sema/src/Sema/Effects.rs:438-500` | the canonical form; the rest re-implement it |
| 2 | purity checker | `-[]>` | `crates/jet-sema/src/Sema/Purity.rs:77-99,1134-1213` | second walker for "row is empty", own code E3401 |
| 3 | block narrowing, grant, and authority | `#FX(Net)`, `#FX(grant: FS)`, `#FX(authority: IO)` | `crates/jet-foundation/src/Syntax/effects_surface.rs:139-152` | three spellings for one scope row (ratified D-AUTHORITY-SCOPE1; kept) |
| 4 | package authority | `authority: { holds: { allow: [FS] } }` | `crates/jet-foundation/src/Authority.rs:239-326` | lowers to string-keyed sets, `AST/program_imports.rs:645-672`; does not refuse the package's own code at this commit (#2511) |
| 5 | module ceiling | manifest per-module allow | `crates/jet-sema/src/Sema/Effects.rs:232-280` | package-only, own code E0712 |
| 6 | replayable checker | `#Replayable` | `crates/jet-sema/src/Sema/Effects.rs:1480-1565` | a fixed forbidden set with its own walker, code E0725 |
| 7 | transaction irreversibility | `#Transact(name) { … }` | `crates/jet-foundation/src/Effects.rs:262-274`; `Sema/Effects.rs:555-591` | a hand-written table of irreversible effects; a different question (a duty: an undo is owed), kept as its own checker |
| 8 | comptime sandbox | `@ { … }` | `crates/jet-comptime/src/Comptime/Purity.rs:78-123` | a purity stage split and an `impure_builtin` leaf table, not a row |
| 9 | pure evaluation flag | `jet eval --pure` | `crates/jet-sema/src/Sema/Purity.rs:123-159` | CLI copy of coat 2; `--pure 'print("x")'` still prints (lane D probe 24) |
| 10 | invocation grants | `--allow-net --deny-net … --allow-gpu --deny-gpu` | `Source/main.rs:1570-1774` | twenty flags for one set; cannot name a leaf |
| 11 | deny-only roots | `!Mem.Alloc`, `Panic` | `crates/jet-sema/src/Sema/Effects.rs:66-132,307-346` | roots the grammar admits but cannot hold positively (kept: they are rights a scope can give up) |

Ten diagnostic codes report on this one question: E0740 (row), E0712 (block), E0750 (undeclared leaf; a vocabulary error, not a denial), E1803 (authority), E3521 (policy), E3401 (purity), E3403 (nondeterministic in pure), E0725 (replayable), E0746 (transaction), E0921 (`!Mem.Alloc`). A user who moves one `http.get` between a function and a block sees two different frames for the same denial. The codes carry meaning a tool needs (a transaction wants an undo, a pure block wants an injected clock), so the codes stay and the frame unifies.

### Question 2 — "Is this claim true?" Twelve coats, six reports, no ladder

| # | coat | user spelling | command | report | can it rise to stronger evidence without a rewrite? |
|---|---|---|---|---|---|
| 1 | assertion | `assert(x)`, `assert_eq(a, b)` | `jet test` | test output | no |
| 2 | example test | `#Test("name") { … }` | `jet test` | test output, proof `unit` row | no |
| 3 | property test | `#Test fn name(p: T)` | `jet test`, `jet fuzz` | counterexample row | no |
| 4 | doctest | `// => value` | `jet test` | doctest row | no |
| 5 | fuzz | `jet fuzz file.jet` | `jet fuzz` | corpus | no |
| 6 | contract | `#[Pre(c, "m"), Post(c, "m")]` | check, run, test, prove | E30xx breach or proof erasure | only by running `jet prove` by hand |
| 7 | stored fact | `fact Name(@holds: …)` | `jet inspect facts` | fact rows | n/a |
| 8 | proof | `jet prove T --lens solver` | `jet prove` | `.jetproof` | lens vocabulary is closed (E2941) |
| 9 | replay | `jet prove T --capture`, `jet prove T --replay A` | `jet prove` | `.jetproof-replay` | ratified consumer; no keep-as-claim |
| 10 | coverage | `jet test --coverage` | `jet test` | HIT/MISS table | no |
| 11 | measure and budget | `.measure { }`, `jet budget check` | `jet test --measure` | budget rows | no |
| 12 | gauntlet | `gauntlet/harness/run.mjs` | not a `jet` command | scoreboard | no |

Homes: `Source/CmdProve.rs:83-223`, `Source/ProveSolver.rs:16-21`, `Source/ProveReplay.rs:127-162`, `crates/jet-foundation/src/Syntax/markers.rs:14-21`. Lane D: "no observed automatic release/promotion edge from proof artifact to `jet build`". The same `#Post(result.is_sorted())` cannot be tested on examples, fuzzed on generated inputs, and proved, without the user learning three commands and reading three reports.

### Question 3 — "What does this value look like on the wire, on the command line, in the environment?" Eleven coats

| # | coat | user spelling | home | defect |
|---|---|---|---|---|
| 1 | one value tree | `DataTree` | `crates/jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs:1-21` | the canonical form (D-SERDE2); public law says six kinds, code ships nine |
| 2 | second JSON model | `json.parse` → `JSONValue` | `crates/jet-foundation/src/JSON.rs:6-15` | BTreeMap model that loses field order; not ratified public surface |
| 3 | twelve codecs, five-entry enum | `json`, `jsonl`, `csv`, `toml`, `yaml`, `xml`, `cbor`, `hex`, `base64`, `base64url`, `base32` | `EncodingTypes.rs:26-33` | `EncodingFormat` enumerates five of twelve |
| 4 | CLI arguments | `fn run(args: T)`, `#CLI`, `#[Short("p")]`, `#[Flag]` | D-CLI; `markers.rs:57-79` | decoded only at the entry function; no stated precedence against env |
| 5 | environment | `env.decode<T>(prefix:, file:, allow:)`, `#Env("PORT")` | `Sema/CheckerCoreLib/fixed_sigs.rs:4115-4121`; `examples/features/io/app_config.jet:38` | shipped and one codec with json/toml/csv; the audit's first draft wrongly called it missing |
| 6 | build settings | `@build.settings.key` | `crates/jet-foundation/src/Facts.rs:172-179,286-301` | string keys, separate plane |
| 7 | database rows | `core.db` | `docs/reference/core-library.md:3932-3975` | separate surface; unprobed |
| 8 | FFI layout | `#Layout(c)`, `#ABI` | `Prelude/Layout.rs:1-20`; `markers.rs:84-86` | shares only layout and skip with the wire facts |
| 9 | typed HTML | `#HTML("page.html")` | `core_surface.rs:464-467` | page path is a String while `Path{"…"}` exists |
| 10 | wire migration | `migration T { rename a -> b; add c: Bool = false }` | `Sema/SchemaMigration.rs:38-142` | ratified for `#PublishedSchema` types; `add`/`remove`/`change` ratified, only `rename` evidenced (lane A) |
| 11 | dev-state shape | `#Persist` and D-HOTSWAP1 | `examples/features/devloop/persist.jet:1-17` | a layout change restarts; the migration block is not consulted |

### Question 4 — "When is a fact checked, and when does it die?" Eight tiers, one closed enum

| entry | tier | hot reload | state kept | receipt | gap |
|---|---|---|---|---|---|
| `jet build` | AOT | no | none | BuildCache | native identity never byte-verified |
| `jet run` | tier-1 JIT or AOT | no | process | RunCache | reason visible only with `--trace-tiers` |
| `jet run --interpret` | tier 0 | no | process | none | rejects build flags, E2102 |
| `jet dev` | resident JIT/deopt | `--swap` or auto-detect | `#Persist` | watch stamps | swap not the stated default; layout change restarts |
| `jet run --target=web` | web | browser | browser | none | native run rejected E-WEB-RUN |
| `jet repl` / `jet notebook` | evaluator | session | session | none | separate path |
| comptime | tier 0 | no | build | build identity | separate authority tiers |

`Source/CmdCompile.rs:1177-1181` holds a three-value tier enum (`Interpreter`, `Jit`, `Aot`); web, REPL, and notebook are routed around it.

### Question 5 — "What happened in this build or run?" Seven records, no index

| record | spelling | home | format |
|---|---|---|---|
| invocation receipt | `jet check --receipt=N` | `Source/ReceiptStore.rs:1-32` | `jet-receipt-v2` |
| proof | `jet prove` | `Source/CmdProve.rs:3128-3207` | `.jetproof` canonical JSON (D-JPROOF1) |
| replay capture | `jet prove --capture`, `jet run --record=N` in the binary | `Source/ProveReplay.rs:1-21` | `.jetproof-replay` framed binary (D-JREPLAY1) |
| production receipt | build | `Source/ProductionReceipt.rs:1-58` | own format |
| tier trace | `--trace-tiers` | `crates/jet-jit/src/jit/tiers.rs:41-57` | process-local text |
| memory ledger | `jet audit memory` | `Source/CmdMemory.rs:1-50` | E2112 in this checkout |
| gauntlet report | harness | `gauntlet/matrix.json` | `gauntlet-report-v1` |

Lane D: "distinct magic/path/participant contracts; no single user query spans all four." The proof and replay formats are ratified with exact byte laws; the defect is the missing index, not the formats.

### Question 6 — "What did the compiler decide, and how do I see it?" Two authorities, two status shapes, one in nine repairs

| surface | evidence | home |
|---|---|---|
| 58 top-level `jet` commands, 32 `inspect` actions | `registry, inspect, project, self, diff, merge, review, run, jobs, check, fill, test, prove, status, build, cc, c++, dev, learn, try, debug, repl, notebook, import, new, fmt, fix, audit, lint, doc, explain, env, shared-store, cache, remote, trust, image, os, add, remove, fetch, search, find, update, init, split, Fold, gc, clean, emit, eval, budget, perf, report, fuzz, version, help` | `crates/jet-cli/src/CLI.rs:725-1194` |
| two CLI authorities | hand parser and declarative registry both know every flag | `Source/main.rs:1570-1774`; `CLI.rs:1404-1575` |
| two `check --json` status shapes | pass: `{"status":"passed","schema_version":1…}`; fail: report lines | `Source/CmdInspect.rs:959-1038` |
| `fix --json` prints prose | `…hello.jet: no changes made` | lane D probe 28 |
| repairs | 762 active diagnostic rows, 83 with `fix_edits` | `Prelude/Diagnostics.jet`; `Registry.rs:1009-1017` |
| report payload | `with_fields` accepts pre-encoded JSON text | `crates/jet-foundation/src/Report.rs:297-310` |

### The silhouette — nineteen compiler tables and twenty-five smuggled strings

| table | home | declared Jet twin? |
|---|---|---|
| effect roots and leaves | `Authority.rs:11-45`; `effects_surface.rs:28-57` | yes: `effect FS.Read` in `Prelude/Effects.jet` |
| FFI languages | `effects_surface.rs:28-57` | yes: `#FFI` declarations |
| marker vocabulary and sites | `Prelude/Markers.jet:1-23`; `CheckerMarkers.rs:1121-1200` | yes: `marker Name(…, @sites)` |
| builtin traits | `Generics.rs:53-84` | yes: trait declarations |
| printable/debug admission | `Traits.rs:1997-2150` | yes: impls |
| `EncodingFormat` | `EncodingTypes.rs:26-33` | no today; proposed format rows |
| `FactRead` and `MeasureRule` | `Registry.rs:578-613`; `AST/types.rs:189-198` | yes: `fact` declarations |
| execution tier enum | `Source/CmdCompile.rs:1177-1181` | no |
| report moments, severities, fix safety | `Diagnostics.rs:59-82`; `Report.rs:50-89` | no (ratified closed sets) |
| CLI command inventory | `CLI.rs:351-594,725-1194` | no; the registry is the home |
| proof lenses | `Source/CmdProve.rs` (E2941) | no (ratified presentation views) |
| taint sinks | `crates/jet-foundation/src/Syntax/sinks.rs:31-125` | proposed `@sink` fact |
| transaction irreversibility | `Effects.rs:262-274` | proposed `@irreversible` fact |
| Core call rows (622) and ambient routes (145) | `Sema/CheckerCoreLib/core_call.rs:420-596` | yes: Core declarations |
| Core export classifier | `CoreModuleExports.rs:20-135` | yes: module declarations |
| lexer keyword map | `crates/jet-lexer/src/Lexer/mod.rs:46-84` | no; `Syntax.rs` is the home |
| command role filenames | `Syntax/package_files.rs:44-66` | no; `Syntax.rs` is the home |
| generated-name prefix `__jet_` | `Syntax/predicates.rs:390-393` | no |
| Core surface registry | `Syntax/core_surface.rs:580-604` | yes: Core declarations |

Where a twin exists, two homes drift: `core.services` (dispatcher) versus `core.service` (inventory) is the proof (lane F). Where no twin exists, the Rust table is the one home and needs only a guard. The twenty-five smuggled strings (`#Unsafe("reason")`, `#[Rename("k")]`, `#HTML("page.html")`, `FieldError.path`, `$origin.fs`, `Panicked(String)`, host fault prefixes, manifest effect keys, policy arguments, taint origins, classification payloads, registered plane names, and more) are the same story at the value level. Most carry text on purpose; one, `#HTML`, carries a path that has a checked type.

## The one idea, the axes, the law

**One sentence.** A Jet program is one ledger, read by every tool through one contract, and no tool keeps a copy.

**The three stores.**

| store | what it holds | lifetime | exists today |
|---|---|---|---|
| schema | fact kinds, marker sites, effect roots, diagnostic rows | process-static | yes: `Registry.rs` (D-META-REG1) |
| instances | a checked program's facts: types, rights rows, shapes, structure, build facts | one compile | yes, spread: `FactRegistry` (`Facts.rs:337-494`), sema tables |
| evidence | one run's records: claim outcomes, replay captures, receipts, tier traces | one run | yes, spread: `.jet/receipts`, `.jetproof`, `.jetproof-replay`, test output |

**The axes.** Every question a tool answers names a plane and a time.

| axis | values | today | proposed |
|---|---|---|---|
| plane (what the fact is about) | types, rights, claims, shapes, structure, build, gates | ratified planes: type (D-TYPE2), rights (D-AUTHORITY), structure (D-STRUCT-PLANE1), build (D-CONF) | claims and shapes join as planes of the same query |
| evidence (how a claim is known) | `proved`, `generated(n)`, `examples(n)`, `checked`, `unchecked`; kept in parallel, summarized by one acceptance order | implicit across six reports | evidence records on claims only; gates keep their own kind |
| time (when a fact is enforced) | comptime, check, test, dev, release, replay | a three-value enum and side paths | a value on the query |

**The law** (proposed amendment to D-FACT-LAW1, stated so every ratified rule remains a theorem):

> A fact is written once. It moves toward safety silently. A claim's evidence rises silently and is never overwritten. Every move away from safety, and every claim held below what its scope demands, is one written word at the site, on the record. Tools read the ledger; none re-encodes it. At runtime no fact remains, unless the tier keeps it on purpose.

Ratified rules that are theorems of this law, unchanged:

| ratified rule | the law, in that plane |
|---|---|
| Knowledge is never lost silently (D-TYPE2-EXACT1) | losing certainty is the away-move |
| Rights only shrink as scope nests (D-AUTHORITY-MODEL1) | gaining a right is the away-move; `#FX(grant:)` is the word |
| Package policy only tightens (D-PACKAGE-POLICY-SCOPE1) | the same law at package scope |
| All audited escapes share one gate ladder (D-ONCE-GATE1) | `#Unsafe`, `#Impure`, `#Nondeterministic` are the same word in three planes |
| Every gate lands in one ledger (D-FACT-GATE1) | a gate is a rights instance with its kind, never a claim grade |
| Contracts checked in every build (D-PREPOST1) | `checked` is the evidence a contract has before any tool ran |
| Proof may erase a runtime check (D-PROVE-SEM1) | `proved` supersedes `checked`; the check dies at release |
| Engines compile one source (D-ONCE-TIER1, I9) | engines are projections; they hold no facts |
| Diagnostics are products (I4) | a diagnostic is a conflict between an instance and the schema, with its repair |
| Every truth has one home (D-ONCE-LAW1) | a wire format is a renderer of the shape instance; a generated table is a renderer of a declaration |
| One registry row table (D-META-REG1) | the schema store already exists; the other two stores get its query |

The three reader reactions this frame owes the owner: **duh** — of course a contract, a test, and a proof are one claim with three kinds of evidence; **how did I not see it** — the `migration` block that walks an old JSON shape is exactly the rule that lets `jet dev` keep a pinned world when its type gains a field, because a running process and a file are two projections of the same published shape; **ohhh** — `jet inspect` has thirty-two actions because it has seven questions and four scopes, and nobody wrote the grid down.

### D-LEDGER1 — one typed ledger over three stores

| option | what |
|---|---|
| A | One table with grade and time columns: the static registry gains two columns, grade and time, and every tool reads that one table. Rejected by the rival review: registry rows are process-static (`Registry.rs:150-182`) and cannot hold per-run values such as `generated(4000)`. |
| B | Keep the current law; six separate proposals: rights, claims, shapes, tiers, records, and tables are each fixed alone, with no shared query and no shared vocabulary, and each may keep a private table. |
| C | Planes and time, no evidence: rights, shapes, and tiers are read through the ledger; claims keep their own reports because evidence records are judged too new. Amends D-FACT-LAW1 with the time clause only. |
| D (recommended) | One typed ledger over three stores: the static schema of fact kinds (the registry, unchanged), the fact instances of one checked program, and the evidence records of one run, read through one typed query contract by every projection (check, test, prove, inspect, dev, receipts). Evidence grade lives on claims only; rights keep their gate kind and provenance and never receive a grade. Time is a value on the query, not a column on the schema. |

Amends: D-FACT-LAW1=B (the evidence clause for claims and the shared-query clause for tools). `FactRegistry` (`Facts.rs:337-494`) is not folded into `Registry.rs`; both stay behind the contract, and registration rows stay `&'static`. Precondition: a parity test proves that every existing tool answer can be reproduced through the contract before any tool switches.

## The proposal, element by element

Each element is independently adoptable and has its own proposal document and ballots. Each shows the beginner rung first, then the expert rungs, then the three exits every magic default owes. No upper rung changes what the lowest rung does.

### Element 1 — Rights: one walker, one frame, ten codes (`rights-one-row.md`)

Look at the frame. Today the same denied call produces two different frames depending on where it sits, and moving it to a third place produces a third. Proposed, every rights report shares one frame with the chain, and keeps its code so a tool can tell a package denial from a transaction rule.

Today (real; codes from lane C probes on `tests/ui/effect_out_of_set.jet` and `tests/ui/effect_authority_out_of_set.jet`):

```jet
// package.jet
authority: { holds: { allow: [FS, Time] } }

// run.jet
use core.http as http
fn fetch(url: String) String -[FS]> { return http.get(url).body().text(1024) }   // E0740: `fetch` uses the effect `Net`, which its signature doesn't allow
fn run() {
    #FX(FS) { fetch("https://x") }                                                // E0712: this `#FX` region uses the effect `Net`, which it has no authority for
}
```

A third site is missing from that block on purpose. Declare the row honestly (`-[Net]>`) and call `fetch` from `run`, and the package's `allow: [FS, Time]` refuses nothing: `jet check` exits 0 with two lint warnings and `jet build` links (verification block `rights`, card #2511). The law says the authority block holds package bounds; the checker enforces them on dependencies. That gap is the same defect as the two frames: the question is answered in more than one place.

Proposed frame (**illustrative**, ballot D-RIGHTS-DIAG1 option B):

```text
Error [E0712]: this `#FX` region uses the effect `Net`, which it has no authority for
  run.jet:8:15   fetch("https://x")   reaches Net through fetch → core.http.get
  scope chain:   #FX(FS) at run.jet:8 → run → package orders (holds [FS, Time])
  nearest scope that could grant Net: package orders (package.jet:2)
  fix (needs-review):        add Net to authority.holds.allow in package.jet
  fix (behavior-preserving): move the call out of the #FX(FS) block
```

A package grant is never a safe automatic edit: rights only widen by a written word (D-AUTHORITY-MODEL1), so that repair carries the ratified `needs-review` class and `jet fix` skips it by default. Removing or moving the call is `behavior-preserving`.

Rungs:

| rung | who | code | what happens |
|---|---|---|---|
| 0 | beginner | `fn save(text: String) { fs.write("out.txt", text) }` | rights inferred: `FS.Write`; nothing to type (ratified D-EFFECT-OMIT1) |
| 1 | intermediate | `fn save(text: String) -[FS.Write]> { … }` | the row is a claim; the checker holds the function to it (ratified) |
| 2 | expert, scope | `#FX(FS.Read) { … }`; `#FX(grant: Net) { … }` | narrow a block, or grant with one word on the record (ratified) |
| 3 | expert, package | `authority: { holds: { allow: [FS, Time] } }` | the top of the chain (ratified D-EFFBUDGET1; enforcement gap carded #2511) |
| 4 | expert, invocation | `jet run --allow=FS.Read,Time --deny=Net app.jet` | **proposed** replaces twenty `--allow-x/--deny-x` flags; deny wins after leaf expansion (ballot D-RIGHTS-CLI1) |

One walker (**proposed**, ballot D-RIGHTS1 option C): `-[]>` is the empty row, `#Replayable` lowers to `-[!Time, !Rand, !Net, !IO]>`, a compile-time block holds the row that allows only `Mem`, and the purity walker, the replayable walker, and the comptime branch are deleted after a differential matrix proves the verdicts identical. The transaction checker stays: it asks whether an irreversible effect inside `#Transact` has an undo (`on_commit` defers it, `#Undo` names an inverse; D-TXN1-4, D-BOUND-UNDO1), which a row by effect name cannot express. Which effects count as irreversible becomes a declared fact it reads (element 7).

Three exits for the inference magic:

| exit | spelling |
|---|---|
| see what it resolved | `jet inspect rights app.jet` → one line per callable: `fetch  -[Net]>  inferred via core.http.get` **illustrative**; today `f.@effects` (ratified) and `jet inspect authority` |
| write it explicitly | `-[Net]>` on the signature (ratified) |
| refuse it project-wide | `authority: { holds: { allow: [] } }` (ratified) |

### Element 2 — Claims: one report, two producers, evidence that rises (`claims-one-ladder.md`)

Look at what one line buys. Today a `#Post` is checked when the code runs. Proposed, the same line gains generated evidence under `jet test` when the function is effect-free, and solver evidence under `jet prove`, and the runtime check disappears once it is proved. Both commands write one report.

Today (real):

```jet
#[Pre(cents > 0, "cents must be positive"), Post(result > cents, "fee must be added")]
fn add_fee(cents: Int) Int -> { return cents + 5 }

#Test("fee on 100") { assert_eq(add_fee(100), 105) }
```

```text
$ jet test app.jet          # runs the example
$ jet fuzz app.jet          # separate command, separate output
$ jet prove app.jet --lens solver   # separate command, `.jetproof`
```

At commit `8b9933668` that contract does not run on any tier: `jet run` stops with E0956 (`comparing these values` isn't supported by the current evaluator yet), `--interpret` with E2201, and `jet build` with ICE 101, using the exact line from `examples/features/contracts/pre_post.jet:7` (verification block `claims`, card #2509). The ladder below assumes the ratified behavior; the card owns the repair.

Proposed (**illustrative**, ballots D-CLAIM1 option C and D-GRADE1 option D; the source is unchanged):

```text
$ jet test app.jet
claims  3 rows   generated 1   examples 1   not generated 0   failing 0
  add_fee.Pre    generation domain    cents > 0 selects inputs; a caller obligation, not a claim about all inputs
  add_fee.Post   generated(1000)      inputs from Pre
  fee on 100     examples(1)
$ jet prove app.jet --lens solver
  add_fee.Post   proved               solver: cents > 0 implies cents + 5 > cents; runtime check erased
```

Evidence words (**proposed**, D-GRADE1 option D): every record is producer plus outcome plus count, kept in parallel; the summary shows the strongest rung by one narrow acceptance order used only for package floors: `proved` above `generated(n)` above `examples(n)` above `checked` above `unchecked`. Compiler facts are not claims and have no rung; gates are rights instances and keep their kind.

Rungs:

| rung | who | code | evidence reached |
|---|---|---|---|
| 0 | beginner | `#Test("fee on 100") { assert_eq(add_fee(100), 105) }` | `examples(1)` (ratified) |
| 1 | intermediate | `#Post(result > cents, "fee must be added")` on an effect-free function | `checked` in every build (ratified D-PREPOST1); **proposed** `generated(1000)` under `jet test` with no extra text |
| 2 | expert | `#Test fn fee_grows(cents: Int(1..)) { assert(add_fee(cents) > cents) }` | `generated(n)` with your own domain (ratified property form and inline range, D-RANGETYPE1) |
| 3 | expert | `jet prove app.jet --lens solver` | the ratified umbrella adds `proved` rows; proved claims erase their check (D-PROVE-SEM1) (the solver is the only producer-enabling lens, D-PROVE-LENS1; bare `jet prove` runs every non-solver producer) |
| 4 | expert, package | `policy: .{ claims: .{ min: generated(1000) } }` | **proposed** a claim below the floor is a diagnostic; lowering the floor is one visible edit (ballot D-GRADE-POLICY1) |

Eligibility is part of the design: default generation runs only for callables whose inferred row is `-[]>`; an effectful contract, a precondition that names another function, or a type without a generator is reported as not generated, with the reason. No false `generated` is possible because the generator refuses rather than guesses.

Three exits for the auto-generation magic:

| exit | spelling |
|---|---|
| see what it did | the report line names the input count and the kept smallest failing input |
| write it explicitly | `#Test fn name(p: T)` with your own generator bounds |
| refuse it | `jet test --grade=examples`; `policy: .{ claims: .{ min: examples(1) } }` states a floor without generation |

### Element 3 — Shapes: one fact, text formats, env, and args (`shapes-one-fact.md`)

Look at how much already exists. A struct is Codable by default (D-META-AUTO1), field defaults live on the declaration, `env.decode<T>(prefix:, file:, allow:)` is shipped and shares one codec with JSON, TOML, and CSV. What is missing is the last projection, arguments anywhere in a program, and one stated precedence among flags, environment, and defaults.

Today (real; verified on every tier, verification block `shapes`; `#Env` per `markers.rs:78-79`, `#CLI` per `examples/features/cli/typed_entry_args.jet:15`):

```jet
use core.encoding.json as json
use core.sys as env

#CLI
struct Config {
    port: Int{8080}
    host: String{"localhost"}
    #Env("DATA_DIR") data_dir: String{"./data"}
}

fn run(args: Config) ![FieldError] {            // CLI owned by the parameter type (ratified D-CLI)
    text :: "{{\"port\": 9000, \"host\": \"example\"}}"
    file :: json.decode<Config>(text)           // JSON owned by the shape (ratified)
    settings :: env.decode<Config>(prefix: "APP_")   // env: shipped, one codec with json
    print(file.port)                            // 9000
}
```

Proposed (**illustrative**, ballot D-SHAPE-PROJECT1 option A; the struct is unchanged):

```jet
use core.args as args
use core.sys as env

#CLI
struct Config {
    port: Int{8080}
    host: String{"localhost"}
}

fn run() ![FieldError] {
    flags :: args.decode<Config>()                     // proposed: through the same core.args builder as fn run(args: T)
    settings :: env.decode<Config>(prefix: "APP_")
    cfg :: Config.merge(flags, settings)               // proposed: flag, then env, then field default; invalid values are FieldError
    print(cfg.port)
}
```

```text
$ APP_PORT=9000 jet run app.jet -- --port 7000
7000
$ APP_PORT=nine jet run app.jet
Error: at `port`: expected Int, found text "nine"
```

One shape fact, read by every format that understands its field facts (**proposed**, D-SHAPE-ONE1 option C):

| field fact | spelling | JSON | CBOR/CSV/TOML/YAML/XML | args | env |
|---|---|---|---|---|---|
| default | `port: Int{8080}` (ratified) | yes | yes | yes | yes |
| rename, shared | `#[Rename("port_number")]` (ratified) | key | key | `--port-number` | `PORT_NUMBER` |
| rename, per format | `#[Rename("port_number", env: "PORT")]` **proposed** | key | key | `--port-number` | `PORT` |
| skip | `#Skip` (ratified) | yes | yes | yes | yes |
| short flag | `#[Short("p")]` (ratified) | ignored | ignored | `-p` | ignored |
| version | `migration T { … }` on a `#PublishedSchema` type (ratified D-MIGRATE1) | old shape decodes | old shape decodes | n/a | n/a |

Database rows and FFI layout stay separate until a probe shows typed queries and layout survive; the rival-family review showed a field diff cannot name a table, and FFI shares only layout and skip. The second JSON model is deleted as conformance to D-SERDE13, and the five-entry format enum becomes declared format rows.

Three exits for the auto-Codable magic (ratified D-META-AUTO1, unchanged): `jet inspect shapes app.jet` shows the wire shape per type **illustrative**; `derive Config.Wire { … }` writes it by hand (ratified D-ONCE-DERIVE1); the ratified policy field that turns auto-derive off refuses it (its exact key is quoted in the shapes proposal).

### Element 4 — The live tier: swap by default, keep the pinned world (`tier-live-dev.md`)

Look at the loop. Today `jet dev` can swap a changed function into the resident process, `#Persist` pins the world (it is the one module binding a function may write, D-PERSIST-DEVSTATE1=A), and a layout change restarts with an announcement (D-HOTSWAP1). What is missing is the default and the migration: swap is chosen by flag or auto-detection, and a pinned value whose type gained a field is thrown away even when a one-line `migration` exists.

Today (real, `examples/features/devloop/persist.jet` shape):

```jet
#Persist counter := 0

fn run() {
    counter += 1
    print("run {counter}")
}
```

```text
$ jet dev --swap app.jet     # swap by flag; `counter` survives; a layout change restarts
```

Proposed (**illustrative**, ballots D-LIVE1 option A and D-LIVE-PERSIST1 option A; `#Persist` stays):

```jet
#[PublishedSchema, Codable]
struct Enemy {
    speed: Float{1.0}
}

#Persist world := World{}        // world.enemies is [Enemy]

fn chase(e: Enemy, w: World) Enemy -> { return e.move_toward(w.player) }

fn run() {
    loop {
        world.enemies = world.enemies.map((e: Enemy) -> chase(e, world))
        draw(world)
    }
}
```

```text
$ jet dev game.jet
live  game.jet  watching 1 file
# edit chase(), save:
saved game.jet  swapped chase  kept world
# add `health: Int{100}` to Enemy and `migration Enemy { add health: Int = 100 }`, save:
saved game.jet  swapped chase  migrated world.enemies (Enemy +health = 100)
# change a layout with no migration, save:
saved game.jet  layout changed: Enemy gained armor  restarting; to keep the world write `migration Enemy { add armor: Int = 0 }`
```

Rungs:

| rung | who | spelling | what happens |
|---|---|---|---|
| 0 | beginner | `jet dev game.jet` | save, keep running; **proposed** swap by default with one line per save |
| 1 | intermediate | `#Persist world := World{}` | the pinned world survives a swap (ratified) |
| 2 | intermediate | `#[PublishedSchema, Codable] struct Enemy` plus `migration Enemy { add health: Int = 100 }` | a shape change migrates the pinned value in place (**proposed**, same block wire data uses) |
| 3 | expert | `jet dev --restart game.jet` | fresh process on every save (ratified flag) |
| 4 | expert | `dev: .{ swap: false }` in `package.jet` | the project refuses swap **proposed** |

Three exits: every save prints swapped, kept, migrated, or restarting; the explicit spelling is `#Persist` for what survives and `migration` for how; `--restart` refuses for the session and the package field for the project. Meaning under `jet run` and `jet build` never changes (I9); the precondition proof, a long-lived edit with callers mid-execution on every engine, is carded before the default flips.

### Element 5 — The verdict loop: repair coverage by ratchet, one status object (`verdict-loop.md`)

Look at the shape of an answer. Every Jet report is already one `jet.report/v1` object per line with what, why, fix, location, and `fix_edits` in one of five safety classes (D-REPORT-MACHINE1, D-REPORT-FIXGRADE1=D). What is missing is coverage (83 of 762 rows carry `fix_edits`), one machine status object on pass and on fail, and a way to keep the hole from growing.

Today (real, lane D probes 10, 28, 30):

```text
$ jet check --json ok.jet    → {"status":"passed","schema_version":1,"scope":"explicit-file"…}
$ jet check --json bad.jet   → {"schema":"jet.report/v1","moment":"compile","severity":"error","code":"E0003"…}
$ jet fix --json bad.jet     → bad.jet: no changes made
```

Proposed (**illustrative**, ballot D-VERDICT-LOOP1 option D; the report line is unchanged):

```json
{"schema":"jet.status/v1","action":"check","ok":false,"reports":[
 {"schema":"jet.report/v1","moment":"compile","severity":"error","code":"E0712",
  "what":"this `#FX` region uses the effect `Net`, which it has no authority for",
  "location":"run.jet:8:15",
  "fix_edits":[{"file":"package.jet","span":"2:31-2:31","new_text":", Net","safety":"needs-review"}]}
]}
```

The loop: `jet check --json` → `jet fix` applies every `formatting` and `behavior-preserving` edit and lists the skipped `needs-review` one → `jet check --json` again → `"ok": true` with an empty `reports` list. Every row gains one of two things: `fix_edits`, or a typed `no_fix_reason` with a reviewed next action. A per-plane coverage baseline never decreases: a new row without either fails the registry today; existing rows are classified plane by plane; structured-edit coverage is reported separately from reason coverage so a reason cannot masquerade as a repair.

The tool tree (**proposed**, ballot D-CLI-ONE1 option A): `jet inspect <plane> [TARGET] [--live PID | --replay ARTIFACT]` for the seven planes. Ledger views fold (`facts`→`types`, `authority`→`rights`, `unsafe`→`gates`, `guarantees`→`rights`, `report` and `dossier`→`build`); task actions (`schema`, `codemod`, `sbom`, `bind`, `logs`, and the rest) keep their names; the registry generates the parser under every option.

```text
$ jet inspect rights run.jet
load   -[FS]>   declared; package holds FS
run    -[FS]>   inferred via load
$ jet inspect claims run.jet
add_fee.Post   generated(1000)   also: examples(1)
$ jet inspect build --coverage
plane rights   fix_edits 41/52   reasons 11/52   uncovered 0   closed
```

### Element 6 — Records: one index, safe capture by default in dev (`records-one-receipt.md`)

Look at what a crash leaves behind. The replay law is ratified in detail: `jet prove TARGET --capture` records the clock only (safe form) and refuses a program that reaches randomness, raw input, or the network; `--capture-sensitive` records those after a typed consent phrase; `jet prove TARGET --replay ARTIFACT` is the only consumer, and no option creates `jet replay` or changes `jet run` (D-JREPLAY1, D-PROVE-REPLAY1). What is missing is an index over the artifacts, a default, and a verifier: nothing is captured unless `--record=NAME` or `--capture` is passed, and no command rebuilds from a receipt's inputs and compares bytes.

Today (source-verified at `8b9933668` against the CLI registry and the receipt store; the verification lane did not execute these commands):

```text
$ jet check app.jet                      # consults the jet-receipt-v2 receipt store before doing work (D-DEVR-TWICE1; Source/ReceiptStore.rs:1-32)
$ jet status                             # shows what the project has proved by reading receipts (D-DEVR-STATUS1; crates/jet-cli/src/CLI.rs:832-836)
$ jet run --record=crash app.jet         # opt-in named replay capture on run, dev, and test (crates/jet-cli/src/CLI.rs:1556)
$ jet prove app.jet --capture            # ratified safe capture → .jet/replays/<id>.jetproof-replay (Time only; D-JREPLAY1)
$ jet prove app.jet --replay .jet/replays/<id>.jetproof-replay
```

Proposed (**illustrative**, ballots D-RECORD1 option B and D-RECORD-SPELL1 option D):

```text
$ jet dev app.jet
… Runtime fault [R0802]: use of storage after its lifetime ended
  replay: .jet/replays/c10aee50.jetproof-replay   (safe form: 14 clock events; no Rand, IO, or Net reached)
$ jet dev net_app.jet
live  net_app.jet   no capture: program reaches Net; run `jet prove net_app.jet --capture-sensitive` to record it
$ jet prove app.jet --replay .jet/replays/c10aee50.jetproof-replay
replayed 14 events  fault reproduced at app.jet:31
$ jet prove app.jet --keep .jet/replays/c10aee50.jetproof-replay
kept claim `replay c10aee50`                          # runs first on every jet prove of this target; stale reports E3621
$ jet build --verify app.jet
built app (3.1 MB)   receipt 9f3a…
verified: identical bytes                              # or: differs at input crates/jet-runtime-core (toolchain build id changed)
```

One index under `.jet/records` lists every proof, replay, and receipt with one identity key (target `inputSha256`, tool, engine) and cross-links them; no ratified byte format changes. The dev tier captures the ratified safe form on every run within a retention budget (256 MB, 200 records, oldest evicted). The sensitive form stays consent-gated exactly as ratified. Backward stepping over a replay (D-TIMETRAVEL2) waits on a measured gate: one shipped release with default safe capture, replay divergence under one in a thousand, and a green debug-adapter conformance suite.

### Element 7 — Open tables: generate where a twin exists, guard the rest (`open-tables.md`)

Look at the drift. `core_call.rs` dispatches 18 rows for `core.services`; the inventory names `core.service`. Both were written by hand. Proposed (**proposed**, ballot D-OPENTABLE1 option D): every table that has a declared Jet twin is generated from it with a drift guard; every table without a twin keeps its Rust home as the one home under a guard against a second copy; irreversibility and sinks become declared facts.

```jet
// Prelude/Effects.jet — proposed additions to a ratified file
effect FS.Write @irreversible
effect Net @irreversible
effect Exec @irreversible
effect FFI @irreversible
fn print(text: String) -[IO]> @sink(Credential)
```

```text
$ jet inspect rights core.files.write
core.files.write   -[FS.Write]>   @irreversible   declared Prelude/Effects.jet:14
error: core_call.rs:4412 `core.services.start` has no declaration; declare it or remove the row     # illustrative guard failure
```

Generation is a workspace build step over committed declarations, checked in like the embedded Prelude; the lexer that reads the declarations is never generated from them. A derivability census of the 622 dispatcher rows is the precondition: every row the generator cannot derive is a hidden fact to declare or a card.

### Element 8 — The lexical space: reservations and five decisions (`syntax-lexical-space.md`)

Every user-typeable form is ratified (I7). The unclaimed space is the audit's job too. This table is the reservation statement; the five ballots are independent.

| slot | today | ballot and recommendation |
|---|---|---|
| backtick `` ` `` | unclaimed (`Lexer/Tokens.rs:20-188`); plain Strings need doubled backslashes and braces; typed heads `Regex{"\d+"}` and `Path{"C:\Users"}` already pass text through | D-RAWSTR1: backtick raw String, no escapes, literal braces, multi-line, cannot contain a backtick (A) |
| trailing comma | mixed: list and map literals accept it; parameters, arguments, marker groups, effect rows, imports, and type arguments reject it (two E0003 at `)`) | D-TRAILCOMMA1: accept in every comma list; `jet fmt` keeps it on multi-line lists and removes it on one line (A) |
| explicit `;` | S6 ratifies E0373; the lexer accepts it (`Terminators.rs:12-18`) | D-SEMI1: enforce E0373 with a contextual fix (line break when code follows on the same line, removal at line end) after a corpus census (A) |
| `#Name(…)` vs `#[Name(…)]` | D-MARK-STACK1=A: one bare rule, one `#[A, B]` list for several, E0999 with the rewrite otherwise | D-MARKERSHAPE1: a challenge, recommended declined; the ratified rule stands (B) |
| marker string arguments | `#Every("03:00")` is compile-checked (E0926); `#Policy` takes typed settings; `#HTML("page.html")` stores a path as text | D-MARKERARGS1: `#HTML` takes `Path{"…"}` (A); nothing else changes |
| `_name` | soft-public, discard, liveness gate (ratified) | unchanged |
| `__name` | rejected E0067; `__jet_` is the machine namespace (ratified) | unchanged, reserved to the compiler |
| `$name`, `$[…]$` | configuration paths (ratified) | unchanged |
| `@name`, `@[…]@`, `pkg@source` | compile-time names, fence, package reference (ratified D-ONCE-AT1) | unchanged |
| `'label` | Char literals only | reserved: none; labels are `outer :: loop` |
| `|>` | declined D-SHAPE-PIPE1 | stays declined |
| `::<T>` | no parser path; `f<T>(…)` is explicit | stays declined |
| ALLCAPS values | rejected E0357 | stays rejected; constants are `::` bindings |
| trailing `_` | rejected | reserved: none |
| `!` suffix | `T !E` failure contract only | reserved: none beyond `!E` |
| `switch`, `for`, `continue` | teaching aliases; `Stmt::Switch` is a phantom AST node | delete the phantom node (card #2512, no ballot) |
| `_test.jet` | no convention; `@test.jet` is the role file | stated: no test-file suffix convention |
| `Type.{…}` | retired; `Type{…}` canonical | unchanged |

## The final vision

One program, today and proposed. The job: a small order service that reads its config, loads orders from a file, adds a fee under a contract, and keeps the network out of its parsers. Every "today" line is real Jet at commit `8b9933668`; every "proposed" line is marked and the transcripts are illustrative.

Today:

```jet
// package.jet
name:    "orders"
version: "0.1.0"
edition: "2026"
authority: { holds: { allow: [FS, Time] } }
```

```jet
// run.jet
use core.files as fs
use core.encoding.json as json

#CLI
struct Config {
    port: Int{8080}
    #Env("ORDERS_DIR") dir: String{"./data"}
}

struct Order {
    id: Int
    cents: Int
}

#[Pre(cents > 0, "cents must be positive"), Post(result > cents, "fee must be added")]
fn add_fee(cents: Int) Int -> { return cents + 5 }

fn load(dir: String) [Order] !(IOError | [FieldError]) -[FS]> {
    json.decode<[Order]>(fs.read("{dir}/orders.json"))
}

fn run(args: Config) !(IOError | [FieldError]) {
    orders :: load(args.dir)
    loop order in orders { print("{order.id}: {add_fee(order.cents) ?? 0}") }
}

#Test("fee on 100") { assert_eq(add_fee(100), 105) }
```

That program passes `jet check` (verification block `final-run`, corrected spelling). It does not run at commit `8b9933668`: the default tier stops at `json.decode` with a computed text argument (E0956, card #2510) and the AOT build stops at the contract (ICE 101, card #2509). Both are default-tier defects the audit found while writing this page; neither changes the design.

```text
$ jet test run.jet                # example passes
$ jet fuzz run.jet                # separate
$ jet prove run.jet --lens solver # separate
$ jet dev --swap run.jet          # swap by flag
$ jet prove run.jet --capture     # opt-in safe capture
$ jet inspect gates run.jet; jet inspect authority run.jet; jet inspect facts   # three views
```

Proposed (the source gains one marker on `Order`, one `#PublishedSchema` for the day its shape changes, and one line that states config precedence; the rest is what the tools say):

```jet
// run.jet — proposed lines marked
use core.files as fs
use core.encoding.json as json
use core.args as args
use core.sys as env

#CLI
struct Config {
    port: Int{8080}
    dir: String{"./data"}
}

#[PublishedSchema, Codable]                                          // proposed use: the shape may change later
struct Order {
    id: Int
    cents: Int
}

#[Pre(cents > 0, "cents must be positive"), Post(result > cents, "fee must be added")]
fn add_fee(cents: Int) Int -> { return cents + 5 }

fn load(dir: String) [Order] !(IOError | [FieldError]) -[FS]> {
    json.decode<[Order]>(fs.read("{dir}/orders.json"))
}

fn run() !(IOError | [FieldError]) {
    cfg :: Config.merge(args.decode<Config>(), env.decode<Config>(prefix: "ORDERS_"))   // proposed: flag, env, default
    orders :: load(cfg.dir)
    loop order in orders { print("{order.id}: {add_fee(order.cents) ?? 0}") }
}

#Test("fee on 100") { assert_eq(add_fee(100), 105) }
```

```text
$ jet test run.jet                                   # illustrative
claims  2 rows   generated 1   examples 1   not generated 0   failing 0
  add_fee.Post   generated(1000)    inputs from Pre
  fee on 100     examples(1)
$ jet prove run.jet --lens solver                    # illustrative
  add_fee.Post   proved             solver: cents > 0 implies cents + 5 > cents; runtime check erased
$ jet dev run.jet                                    # illustrative
live  run.jet  watching 1 file
saved run.jet  swapped add_fee
$ jet inspect rights run.jet                         # illustrative
load   -[FS]>   declared; package holds FS
run    -[FS]>   inferred via load
$ jet check --json run.jet                           # illustrative
{"schema":"jet.status/v1","action":"check","ok":true,"reports":[]}
$ jet build --verify run.jet                         # illustrative
built orders (3.1 MB)  receipt 9f3a…  verified: identical bytes
```

The structure of the end state:

```text
ledger (one typed query)
├── stores
│   ├── schema      fact kinds, marker sites, effect roots, diagnostic rows      (Registry.rs, ratified D-META-REG1)
│   ├── instances   one checked program's facts                                  (FactRegistry + sema, behind the query)
│   └── evidence    one run's records: claims, replays, receipts, tier traces    (indexed under .jet/records)
├── planes   types · rights · claims · shapes · structure · build · gates
├── evidence proved | generated(n) | examples(n) | checked | unchecked           (claims only; gates keep their kind)
├── time     comptime · check · test · dev · release · replay
└── projections (dumb, hold nothing)
    ├── engines    AOT · JIT · interpreter · web                                  (ratified I9)
    ├── verdicts   jet check · jet test · jet prove · jet fix                     (elements 2, 5)
    ├── views      jet inspect <plane> [TARGET] [--live PID | --replay ARTIFACT]  (element 5)
    ├── formats    json · cbor · csv · toml · yaml · xml · env · args             (element 3)
    ├── tiers      jet build · jet run · jet dev (swap by default)               (element 4)
    └── records    .jetproof · .jetproof-replay · receipts, one index            (element 6)
```

## What this unlocks

| domain | what falls out |
|---|---|
| beginners | one command per question and one truth per line; a contract they wrote for safety gains generated evidence for free |
| services | a package-level `Net` denial with one frame that names the exact call chain; a safe replay that reproduces a dev fault |
| games and UI | save the file, keep the world; a published struct gains a field and the pinned list migrates |
| data work | one struct, every text format plus env and args, no derive; canonical bytes proven by `--verify` |
| systems | `#Unsafe` gates and unproved claims in one ledger with one audit view |
| agents | every report carries an edit or a reviewed reason; one status object; the loop `check → fix → check → ok` closes without guessing |
| the compiler itself | nine tables become generated views with drift guards; `core.services` cannot happen again |

## What stays

| kept on purpose | why |
|---|---|
| every frozen wall | no evidence justified a challenge; each slate names the walls it touches and touches none |
| `-[…]>`, `#FX`, `authority.holds`, `#Unsafe("reason")` | they are the one row and the one gate word; the slate deletes their shadows, not them |
| `#Pre`/`#Post`, `#Test`, `#Test fn`, `jet prove` | distinct structure: a claim on a callable, an example, a property; and the ratified umbrella that produces proof and consumes replay |
| `DataTree`, `migration` on `#PublishedSchema`, field defaults `T{expr}`, `env.decode` | the shape fact already has one home and two shipped projections |
| `#Persist`, D-HOTSWAP1's restart on layout change | the pin is the one writable module binding; restart is the safe fallback when no migration exists |
| the transaction checker | it asks a different question: an irreversible effect owes an undo |
| ten rights codes | a tool needs the discriminator; the frame unifies, the code stays |
| the one-report-per-line schema and five fix classes | ratified and load-bearing; the status object wraps them |
| `.jetproof`, `.jetproof-replay`, `jet prove --replay` | ratified byte laws and the ratified consumer |
| D-MARK-STACK1 | one canonical marker spelling with a formatter rewrite; the challenge did not meet the bar |
| four branch forms, `loop`, `->`, `::` | ratified and consistent; no shadow found |
| ambient default rights (IO, Mem.Alloc, Exec) for a file with no manifest | ratified D-AUTH-AMBIENT1; worth checking (`Exec`) but not this slate |

## Decisions for the owner

| ballot | question | recommended | amends |
|---|---|---|---|
| D-LEDGER1 | one typed ledger over three stores | D three stores | D-FACT-LAW1 (evidence and shared-query clauses); D-META-REG1 unchanged as the schema store |
| D-RIGHTS1 | one rights walker; transactions keep their checker | C | D-REPLAY1, D-SHAPE8 (lowerings); D-TXN1-4 unchanged |
| D-RIGHTS-CLI1 | invocation rights spelling | B `--allow=… --deny=…`, deny wins | D-CLI-GLOBAL1 flag set |
| D-RIGHTS-DIAG1 | one frame with the chain; codes stay | B | I4 snapshots of nine rows; D-REPORT-MACHINE1 detail fields |
| D-CLAIM1 | one evidence report; `jet test` to `generated`, `jet prove` adds `proved` | C | D-JPROOF1 (additive report field); `jet fuzz` folds into `jet test` |
| D-GRADE1 | evidence words | D producer plus outcome; narrow order for policy | new |
| D-GRADE-POLICY1 | minimum evidence field | A `policy: .{ claims: .{ min: … } }` | D-PACKAGE-POLICY-SCOPE1 field set |
| D-SHAPE-ONE1 | one shape fact; text formats, env, args | C | D-ENCSTREAM-SURFACE1 (format rows); D-SERDE13 conformance; D-MARKSIG1 (per-format rename) |
| D-SHAPE-PROJECT1 | args as a decode format with precedence | A `args.decode<T>()` and `Config.merge` | D-CLI builder floor (bound, not amended) |
| D-LIVE1 | swap by default with a per-save line | A | D-DEV4, D-DEVMODE1, D-HOTSWAP1 defaults; restart rule unchanged |
| D-LIVE-PERSIST1 | migrate pinned `#PublishedSchema` values; restart otherwise | A | D-MIGRATE4 (resident values); D-HOTSWAP1 fallback unchanged |
| D-VERDICT-LOOP1 | repair coverage by per-plane ratchet; one status object | D | D-DX1 (status envelope); I4 registry guard baseline; D-REPORT-MACHINE1 additively (one optional field `no_fix_reason`, schema `jet.report/v2`) |
| D-CLI-ONE1 | one inspect per plane, four scopes, task tools kept | A | D-DX4, D-SHAPE-CLI-COMPLETE1 |
| D-RECORD1 | one index; safe capture by default in dev | B | D-DEVR-TWICE1 (identity key and `consumed` link on the receipt); D-JREPLAY1 (dev default for the safe form only); D-DEV4 (capture line) |
| D-RECORD-SPELL1 | keep the ratified consumer; add `--keep` | D | D-PROVE-SEM1 (kept replays run first) |
| D-TIMETRAVEL2 | backward stepping | B measured gate | none; D-TIMETRAVEL1=C restated |
| D-OPENTABLE1 | generate tables with declared twins; guard the rest | D | D-META-ONE1 scope note; D-META-REG1 as generator input |
| D-RAWSTR1 | raw ordinary String | A backtick | S8, S20 (inside the literal); I7 entry |
| D-TRAILCOMMA1 | trailing commas everywhere | A | parser rules per the grammar inventory |
| D-SEMI1 | enforce S6 | A E0373 with a contextual fix | none new; E0373 row gains `fix_edits` |
| D-MARKERSHAPE1 | challenge to D-MARK-STACK1 | B keep | none |
| D-MARKERARGS1 | `#HTML` takes `Path{"…"}` | A | D-MARKSIG1 signature row |

## Implementation shape

| phase | what | surface change | proof |
|---|---|---|---|
| A | the ledger query over the three stores with a parity test; generated tables where a twin exists; one status object; the differential matrix for the rights walkers; the dispatcher derivability census | none | all tests green; parity test green; drift guard rejects a hand row |
| B | land ratified-but-unbuilt work on the substrate: `migration add/remove/change`, `#Pre`-driven generation for effect-free contracts, resident-value migration through `SchemaMigration`, `jet build --verify`, the record index | none | golden examples per I5, every tier per I9 |
| C | balloted surface unifications, each a greenfield migration that deletes the replaced form: walkers and flags, the evidence report, `args.decode` and `Config.merge`, swap by default, the ratchet and the tool tree, dev safe capture and `--keep`, the lexical decisions | per ballot | examples, snapshots, and the beginner friction table re-run |

## Verification of the today code

Every "today" program block in elements 1-4, the final program, and the three lexical probes was extracted and run through `scripts/agent/jet-env jet` on four tiers (report: `~/.cache/jet-luna/wla/verify-frame.md`, 27 rows). The records block in element 6 and the status transcripts in element 5 are source-verified against the CLI registry, the receipt store, and lane D's probes, not executed; they are labelled so in place. Blocks marked illustrative were not run; they do not exist yet.

| block | check | jet run | --interpret | build + binary | note |
|---|---|---|---|---|---|
| rights | pass | n/a (DNS) | n/a (DNS) | links | the package allow list refused nothing: card #2511 |
| claims (`pre_post.jet:7`) | pass | E0956 | E2201 | ICE 101 | card #2509 |
| shapes | pass | `9000` | `9000` | `9000` | after the `#Env` and `#CLI` spelling fix now shown |
| live (`#Persist`) | pass | `run 1` | `run 1` | `run 1` | |
| final program | pass | E0956 at `json.decode` | E2201 | ICE 101 at the contract | cards #2510, #2509 |
| trailing comma in a parameter list | E0003 | | | | as claimed; list literals accept it (`collections_structs.rs:43-89`) |
| `r"…"` | E0003 | | | | as claimed |
| explicit `;` | pass | | | | accepted; S6 ratifies E0373 |
| records (element 6) | source-verified | | | | `crates/jet-cli/src/CLI.rs:832-836,1556-1557`, `Source/ReceiptStore.rs:1-32`; not executed |
| status transcripts (element 5) | source-verified | | | | `Source/CmdInspect.rs:959-1038`; lane D probes 10, 28, 29, 30; not executed by the lane |

Three defects and one spelling repair came out of writing one page of examples. That rate is the audit's own evidence that the default tier is where the story breaks, as the video mine said a week earlier.

## Review passes

Two passes ran on the twenty-two draft ballots and the nine proposal documents before this revision: five fresh beginner agents with the `rli5` skill (`~/.cache/jet-luna/wla/reviews/beginner-Rli5{A..E}.md`, 134 findings), one beginner agent over the proposal surfaces (`surface-rli5.md`, 55 rows), and four rival-family adversarial reviewers (`adversarial-Adv{A..D}.md`, 119 findings, 41 fatal). Every fatal finding changed the design; the table below is the disposition of the surface friction table's blocker rows.

| friction (surface pass) | disposition |
|---|---|
| proposed transcripts read as runnable | every future transcript is now marked illustrative, and the status line says so |
| config precedence undefined; env mapping unstated | `Config.merge` states flag, env, default; the rename table states the env and args names |
| claim counts disagree with rows | headers count rows; every transcript's header equals its rows |
| `counter := 0` shown as writable module state | the live examples use `#Persist`, the ratified pin, and the module mutation law is quoted |
| `migration` shown on plain structs | every migrated type is `#PublishedSchema`, as ratified |
| `#[Env("…")]` status contradictory between documents | `#Env` is the shipped env rename; the per-format `#[Rename(…, env: …)]` is the proposed generalization; both documents say so |
| rights diagnostic with no matching source | the frame and the rights proposal show the two files and matching line numbers |
| record contents, redaction, and byte identity presented as fact | the record section quotes the ratified safe form, and `--verify` shows both outcomes |
| marker examples elided; typed literal forms invalid | complete functions everywhere; `Path{"…"}` is the only typed argument proposed |
| `Path"…"`, `Raw"…"`, `#Every(03:00)`, `-[!@irreversible]>` | removed; they were not Jet |

Recommendations that moved after the adversarial pass: D-LEDGER1 A→D, D-RIGHTS1 A→C, D-RIGHTS-DIAG1 A→B, D-CLAIM1 A→C, D-GRADE1 A→D, D-SHAPE-ONE1 A→C, D-SHAPE-PROJECT1 redrawn, D-VERDICT-LOOP1 A→D, D-RECORD1 A→B, D-RECORD-SPELL1 A→D, D-TIMETRAVEL2 A→B, D-OPENTABLE1 A→D, D-MARKERSHAPE1 A→B, D-MARKERARGS1 narrowed. The dissent pass did what it is for.

A third pass ran on the integrated deliverable after revision: one fresh-context reviewer over the nine documents, the twenty-two minted ballots, and cards #2500-#2513 (`~/.cache/jet-luna/wla/reviews/fresh-context-review.md`: 2 fatal, 19 material, 14 minor; ten citation spot checks, 7 pass, 3 fail). Every finding was fixed in place before this revision closed.

| fresh-context finding | fix |
|---|---|
| build receipts and `jet build --verify` described as ratified and shipped; `jet check --receipt` is not a check flag | the records baseline now cites the shipped receipt store (D-DEVR-TWICE1), `jet status`, and `--record=NAME`; `--verify` is proposed; the block is labelled source-verified, not executed |
| bare `jet prove` shown producing `proved` rows | every proved row now names `--lens solver` (D-PROVE-LENS1); ballot D-CLAIM1 and card #2502 updated |
| `no_fix_reason` added while D-REPORT-MACHINE1 was declared unchanged | the amendment is named: one optional field under `jet.report/v2`; the envelope clause is quoted |
| option meanings and Amends lines drifted between documents and Tower (D-OPENTABLE1, D-RECORD1, D-SHAPE-ONE1) | documents carry the Tower option text; ballots D-RIGHTS1, D-SHAPE-ONE1, D-SHAPE-PROJECT1, D-VERDICT-LOOP1 updated where the ballot was the stale side |
| retention budget as size plus age; `jet dev` "writes no record at all" | 256 MB and 200 records everywhere; dev captures by opt-in `--record=NAME` today |
| invented refusal key `policy.derives.auto`; precedence described as missing | `policy: .{ lints: .{ deny: [auto_derive] } }` and `#!Codable` (S55); D-CLI-GLOBAL1's precedence cited and applied outside the entry function |
| slate criteria requiring `jet inspect <plane>` regardless of D-CLI-ONE1; `--pure` gated on the wrong ballot; #2505's loop lacking the manual decide step; #2511 allowing own code out of scope | criteria rewritten on #2501-#2507, #2511, #2513 |
| stale citations (`CmdCompile.rs`, `Sema/Purity.rs:43-60`), `core.task` versus `core.tasks`, S8 and S20 labels swapped, twelve versus fourteen | corrected in place |

## The four standing questions

| question | answer |
|---|---|
| How does Jet beat this on a level playing field? | No peer has one ledger read by every tool. Rust has one type system and separate ecosystems for tests, fuzzing, proofs, and serde. Koka has one effect mechanism and no claims plane. Dart has hot reload and no published-shape migration. A competitor cannot add a shared ledger without removing its private tables, which is the model change it cannot make in one release. |
| What do we avoid? | Java's SecurityManager (a permission system beside the language, retired); Go's JSON v1 case-insensitivity (a wire rule outside the type, forced v2); Python's dynamic first draft (no ledger at all); Zig's comptime that creates types (the wall stays); Erlang's hot code loading without a migration contract (the live tier migrates by published shape or restarts, never guesses); rr's all-or-nothing recording (Jet's safe form records what is safe and names the consent path for the rest). |
| What does this say about AI-driven development? | Verdict fidelity rises because a contract gains generated evidence without being asked. Latency stays per-file because the ledger query is per-file. Actionability becomes structural: every row carries an edit or a reviewed reason, and the status object is one shape. Context economy improves because one report replaces six. Repair determinism improves because one rights walker admits one chain, and because a grant is never a silent safe edit. |
| What concrete surfaces must Jet cover? | Covered with proof: `-[…]>`, `#FX`, `#Test`, `DataTree`, `#Persist`, `env.decode<T>`, `jet prove --capture`, `jet inspect gates`, `json.decode<T>` on a literal. Defective at this commit: `#Pre`/`#Post` on every tier (#2509), `json.decode<T>` on a computed text (#2510), `authority.holds` on own code (#2511). Worth checking: `jet dev --swap` state parity, native build byte identity, `#Replayable` coverage, `fact_reads.jet` (E0956), `migration add/remove/change`. Missing: raw strings, uniform trailing commas, `args.decode`, `Config.merge`, evidence words, `jet inspect <plane>`, the status object, the record index, `--keep`, `--allow=`/`--deny=`. |

## Where Jet loses today

| peer | loss | evidence |
|---|---|---|
| Rust | matched-task completion and release reliability | 27/36 vs 36/36; 9/9 blind choices (fresh-agent rerun 2026-08-31) |
| Python | first-draft density | 220 vs 125 lines (field audit 2026-07-26) |
| Rust, Go, Swift, Kotlin, Zig, Python | uniform trailing commas and a raw string | lane A probes p17, p19; `collections_structs.rs:43-89` |
| Dart, Erlang | state-preserving reload with a migration contract | lane D; D-HOTSWAP1 restarts on layout change |
| rr | replay as the default debugging artifact | lane D; capture is opt-in |
| Nix, Bazel | proven byte-identical outputs | lane D: native identity unverified |
| Koka | one effect abstraction | lane C: eleven coats |
| Dafny, Lean | proof in the acceptance path | lane D: no promotion edge |

## Existing proposals, disposed

| proposal | disposition |
|---|---|
| `stored-invariant-facts.md` | absorbed: a stored predicate is a claim-plane instance whose evidence is `proved` by the checker; its "no second fact carrier" rule is the frame's rule |
| `structure-program-is-a-value.md` | independent survivor, ratified D-STRUCT-PLANE1; the structure plane is one of the seven |
| `transactional-rollback-regions.md` | independent survivor; the rights slate leaves the transaction checker alone and only declares which effects are irreversible |
| `yielding-loops.md` | shipped law; no overlap |
| `automatic-build-optimization.md` | absorbed into element 6 for identity and `--verify`; its cache-promotion plan stands on its own cards |
| `ecosystem-shape.md` | independent survivor; `package.jet` is the top of the rights chain and the home of policy fields |
| `hardening-rig.md` | independent survivor; the rig consumes the record index and the evidence report |
| `streamline-one-repo.md` | independent survivor; its eleven live-drift rows are D-ONCE-LAW1 work |
| `dogfood-jet-experience-5-of-5.md` | evidence input; its ledger of complaints is cited above |

## Kill-check

| slice | risk named | result |
|---|---|---|
| rights | hollowing the beginner default by requiring rows | killed: inference stays; no row is required |
| rights | a row cannot express a transaction's undo exception | narrowed: the transaction checker stays (D-RIGHTS1 C) |
| claims | auto-generation runs effectful bodies | narrowed: effect-free callables only, budgeted; `--grade=examples` refuses it |
| claims | a precondition reported as proved | killed: the solver proves the implication; `Pre` is the generation domain |
| shapes | database migration from a field diff | killed for now: db and ffi wait for their own probe |
| live tier | persisting unmarked values | killed: `#Persist` stays the pin and the mutation gate; migration only for published types; restart otherwise |
| verdict loop | a global guard blocks the registry; bulk reasons pass it | narrowed: per-plane ratchet; edit coverage reported apart from reasons |
| records | recording by default leaks secrets | narrowed: only the ratified safe form records by default; sensitive stays consent-gated |
| records | a new container reopens two ratified codecs | killed: an index over the ratified formats |
| open tables | generating the lexer from a file the lexer reads | killed: tables without a twin keep their Rust home under a guard |
| lexical | reversing D-MARK-STACK1 without evidence | killed: the challenge is recorded and recommended declined |

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 eleven rights coats | card | #2501 |
| F2 twelve claim coats | card | #2502 |
| F3 eleven shape coats | card | #2503 |
| F4 closed tier enum and opt-in swap | card | #2504 |
| F5 seven record formats without an index | card | #2506 |
| F6 two CLI authorities, two status shapes, 83/762 repairs | card | #2505 |
| F7 nineteen compiler tables, nine with drifting twins | card | #2507 |
| F8 lexical space decisions | card | #2508 |
| F9 phantom `Stmt::Switch` | card | #2512 |
| F10 `core.services` vs `core.service` drift | card | #2513 |
| F11 `check --json` status drift | card | #2505 |
| F12 `eval --pure` prints | card | #2501 |
| F13 native build byte identity unverified | card | #2506 |
| F14 `fact_reads.jet` E0956 | no-action | already carded as a default-tier defect under the silent-data slate (#2252 net) |
| F15 `#Require`/`#Ensure` spellings in the prompt | no-action | archived: the real spellings are `#Pre`/`#Post` (D-PREPOST1); the prompt's names were placeholders |
| F16 contracts fail on every tier at 8b9933668 | card | #2509 |
| F17 `json.decode<T>` with a computed text argument fails on the default tier | card | #2510 |
| F18 `authority.holds` allow list does not refuse the package's own code | card | #2511 |
| F19 `#Every`, `#Policy`, `Path"…"` misreported as string-smuggled or mis-spelled in the first draft | no-action | archived: corrected by the adversarial pass; only `#HTML` remains (D-MARKERARGS1) |
| F20 `env.decode<T>` misreported as missing in the first draft | no-action | archived: shipped at `fixed_sigs.rs:4115-4121`; the shapes slate now adds only args and precedence |
<!-- /audit-dispositions -->

The audit itself is card #2500; the eight slate cards are #2501 through #2508, each in `deciding` and blocked by #2500 until its ballots are ratified.
