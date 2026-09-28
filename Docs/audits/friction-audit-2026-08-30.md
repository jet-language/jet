# Friction audit — 2026-08-30

Whole-corpus review of real Jet programs: dogfood jetpack and tower, all examples, gauntlet
entries, agent workloads, site, and perf tools, with the test-fixture corpus frequency-mined.
Eight Luna (gpt-5.6, max reasoning) lanes read every assigned file; the orchestrator verified
the highest-stakes claims on the live binary. Goal: shorten the common cases and make Jet code
easier to reason about, without minting a method for everything.

**State boundary:** findings describe the audit-start corpus and binary. Since collection,
#2372, #2373, and #2374 are now done. #2372 includes the shared-builder regression;
#2373 and #2374 landed with verified compiler regressions. All eight owner decisions were
ratified as outcome A on 2026-08-30. Historical rows stay as evidence; current status is named explicitly.

## Thesis

Jet's designed surfaces are mostly right; the corpus does not use them. The single biggest
friction source is not a missing library — it is that the ratified typed CLI entry, duration
literals, structural Codable, effect inference, list equality, and `url.parse` already exist
while dogfood, gauntlet, and workload code hand-roll all of them. The real gaps are narrow and
provable: one silent CLI parser defect present at audit start, machine-hostile grouped
decimals, no configured delimited-record surface, no total DataTree scalar-to-text projection,
and a handful of one-line stdlib holes that dogfood re-implemented by hand. Fixing adoption
plus these narrow gaps shortens common Jet code more than any new mechanism would.

## Prevention law

The first version of this report treated migration as closure. That was wrong. A friction
finding is closed only when the product makes the canonical form the easy form **and** a
durable guard prevents the audited inferior form from returning.

Use the earliest reliable layer:

1. **Construction or default.** Make the wrong result impossible, or make the safe common
   result the direct spelling.
2. **Generated recipe.** `jet new`, package starters, docs, examples, scaffolds, and bindgen
   emit the canonical form.
3. **Compiler diagnostic.** Reject only forms that are wrong. Registered What/Why/Fix copy
   and a UI snapshot are mandatory.
4. **Default-on semantic lint.** Warn only after the replacement exists and the compiler can
   prove the exact inferior shape. Every warning has a mechanical fix or a choice between
   named APIs. The existing statement-scoped `#allow` is the expert escape.
5. **Canonical-corpus ratchet.** Every maintained source role and generated-source producer
   is inventoried. Exceptions bind one rule to one semantic occurrence and reason; a second
   occurrence fails.

Never use source-text matching, identifier spelling, line counts, a global strict mode, or a
lint for a missing API. Raw argv, parser builders, `body()`, explicit limits, raw delimited
splitting, explicit unit conversion, digest byte projections, DataTree matching, and low-level
FFI remain available. The expert path is explicit, local, and auditable.

Result of the prevention pass: **36 of 38 findings need a better API/default/recipe or
generator; two are compiler predicate bugs; zero may remain audit-only.**

## The four questions

1. **Beat the field on a level playing field.** Jet's typed `#CLI` entry — the input schema
   *is* the entry parameter, carried into compiled metadata — is a categorical win no peer
   ships (Python argparse, Rust clap, Swift ArgumentParser are all library objects beside the
   entry). At audit start, Jet lost on delimited text and machine numeric output; both gaps now
   have ratified outcome-A corrections. Python argparse also rejected surplus arguments while
   Jet did not; #2372 has since repaired that trust boundary.
2. **What we avoid.** Kotlin's duplicated string/sequence vocabulary (D-STR-DECLINE1 already
   guards this); Ruby's silent `to_i` junk-to-zero; clap's parser-beside-entry split; and a
   lint layer that guesses user intent from source text.
3. **AI-driven development.** The dominant agent costs found are context-economy (jetpack's
   120-line hand parser, tower's 100-line `semantic_equal`) and repair-determinism (false
   L0508 on the canonical print loop admits no correct source-level fix). Both are addressed
   below.
4. **Concrete surfaces.** Covered with proof, worth checking, and missing surfaces are named
   per finding; the Defaults map table is the roll-up.

## Domain scorecard

| Domain / workload | Job | Grade | Top friction kind | Evidence |
|---|---|---|---|---|
| CLI tooling | jetpack package manager | friction | wrong-default | `dogfood/jetpack/src/cli/main.jet:44-164` hand parser |
| Server/web | tower board shadow | friction | missing-default | `dogfood/tower/run.jet` truthiness + equality ladders |
| Data/serde | JSON round-trips, CSV/TSV | friction | domain-blind | TSV framing repeated in 5 workload adapters |
| Text/files | scans, renames, walks | ships | missing-default | `fs.walk` + repeated `!entry.is_dir` filter |
| Concurrency | scans, races, pipelines | ships | wrong-default | `task.group` wrappers around single `task.race` |
| Numeric/format | gauntlet reports | friction | wrong-default | `.replace(",", "")` cleanup on `:Fixed` output |
| HTTP | client/server examples | ships | missing-default | `resp.body().text(8*1024*1024)` for "read the text" |
| Types/units | money, quantities | friction | domain-blind | `Usd.from_float(price.raw() * Float.from_int(qty))` |

## Findings (ranked by pain x frequency)

| # | Finding | Kind | Evidence | Frequency | Disposition |
|---|---|---|---|---|---|
| 1 | **Historical behavior, fixed:** typed/builder CLI parser silently dropped surplus bare arguments; wrong invocation exited 0 with defaults | no-reject / BUG | shared check now `Prelude/CoreLib/Top/Args.rs:763-777` | every `#CLI` and builder program | **#2372 done** with typed and shared-builder regressions |
| 2 | Raw argv walking dominates despite ratified typed entry: length checks, `argv[0]` tax, pending-value state machines, hand usage text | domain-blind | jetpack `cli/main.jet:44-164`; 79 lexical `process.argv()` lines include comments/generated material, so this is a signal rather than a call count | corpus-wide | ballot **D-CLI-RECIPE1**; cards #2375, #2376 |
| 3 | `:Fixed(n)` groups digits (`1,234.50`) so machine output needs `.replace(",", "")` | wrong-default | `Prelude/Core/Fmt.rs:7`; gauntlet binparse/csvtransform | 3 workarounds, every future CSV/protocol emitter | ballot **D-FMT-PLAIN1** |
| 4 | Dynamic `DataTree` work lacks a total scalar-to-text projection and unordered comparison; tower wrote divergent display/truthiness ladders and 100 lines of `semantic_equal` | missing-default | `dogfood/tower/run.jet:110-189, 258-367, 1546-1658`; strict `DataTree.text()` remains distinct | 662 DataTree lines in dogfood | ratified **D-DATATREE-ERGO1=A** |
| 5 | Delimited text (TSV, header skip, blank skip, row numbers) is hand-framed; `core.encoding.tsv` does not exist (E1001 verified) | domain-blind | `incident_report.jet:17-23` + 4 more adapters | 5 adapters + every future TSV job | ballot **D-ROWS1** |
| 6 | HTTP message-level reads and typed JSON request bodies are missing; routes force an unused `req` param | missing-default | 38 `body().text(` sites; `http_web_defaults.jet:44-46` | 38+29 sites | ballot **D-HTTP-MSG1** |
| 7 | Unit values cannot scale by a scalar: `price * qty` rejects (E0127 verified); code erases units mid-formula | domain-blind | `unit_family.jet:5-9`; 63 `#UnitFamily` uses | every money/quantity program | ballot **D-UNIT-SCALAR1** |
| 8 | `crypto.sha256_bytes` returns hex text, not bytes; the name lies beside typed `sha256 -> Digest256` | domain-blind | alias at `crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs:2330-2333`; known consumers plus stale registrations/docs | crypto surface | ratified **D-CRYPTO-DIGEST1=A** |
| 9 | Dogfood-proven one-line stdlib holes: ASCII fold as 26 `replace` calls; path containment at four sites; file-only walks repeat a dir filter; `argv[0]` skip ceremony | missing-default | `path_laws.jet:6-34`; fourth containment site `store/ingest.jet:129-135`; 7 `fs.walk` + 6 filters | dogfood + workloads | ballot **D-STDLIB-SMALL1** |
| 10 | **Historical, fixed:** false `L0508` on `loop x in xs -> print(x)` suggested two wrong repairs | BUG | flagship `first_hour.jet:11`; typed Unit/non-Unit regression | 104 sites | card **#2373** done |
| 11 | **Historical, fixed:** documented `fn build` entry drew `L0104` + `L0520` on plain `jet check` | BUG | compiler-root and neighboring-lint regressions | every programmable build | card **#2374** done |
| 12 | Examples teach retired ceremony: redundant `#Codable`, `Duration.hours(2) ??` for constants, effect rows, `~` on read-only calls, `task.group` wrappers, manual line counters, `fn run()` wrappers in first-contact docs | wrong-default (teaching) | see card body | corpus-wide | card **#2375** |
| 13 | Jetpack/tower re-implement stdlib: hand JSON strings (81 raw-object sites repo-wide), URL query reparse, `same_strings` vs `==`, non-idempotent `create_dir` | wrong-default | see card body | dogfood | card **#2376** |
| 14 | Generated FFI bindings repeat the status/JSON envelope per operation | domain-blind | actual renderer inventory includes Lua, R, Perl, PHP, Ruby, PowerShell, and Octave | all registered binding renderers | card **#2377** |

Audit-start probes confirmed findings 1, 3, 4, 5, 7, 9, and 10 on the then-current binary.
Current source and Tower evidence mark 1, 10, and 11 fixed; the remaining product gaps are
ratified implementation work, not open design.

## Defaults map

| Most likely use case | Today's default | Reject path | Override path | Hole |
|---|---|---|---|---|
| Fixed-shape CLI input | raw `process.argv()` in practice | surplus values now reject through #2372 | `core.args` builder or raw argv | typed `#CLI` must become the taught/scaffolded default |
| Print a number for a machine | `{x:Fixed(2)}` → grouped | none | explicit human `Grouped(n)` | no plain default (D-FMT-PLAIN1) |
| Compare two decoded JSON docs | `==` (order-sensitive) | n/a | explicit pattern matching / ordered `==` | no `equal_unordered` (D-DATATREE-ERGO1) |
| Read TSV with a header | `split("\n")` + counters | n/a | raw split for a different grammar | no delimiter/header/blank options on the existing CSV engine (D-ROWS1) |
| Read an HTTP response as text | `resp.body().text(limit)` | shared body cap | `body()` and explicit limit | no `resp.text()` (D-HTTP-MSG1) |
| `price * qty` on a linear unit | E0127 | affine/inverse forms stay rejected | explicit `.raw()` boundary | no scalar scaling (D-UNIT-SCALAR1) |
| Case-fold ASCII for a path law | 26 `replace` calls | n/a | Unicode `to_lower` for its different contract | no `to_ascii_lower` (D-STDLIB-SMALL1) |
| Test lexical path containment | normalized string-prefix helpers x4 | n/a | `canonicalize` for physical/symlink truth | no `Path.is_within` (D-STDLIB-SMALL1) |
| Whole-file IO, races, JSON round-trip, error propagation | good defaults | explicit | explicit | none — preserve |

## No-regression architecture

| Audited family | Primary correction | Durable recurrence guard | Expert escape | Owner |
|---|---|---|---|---|
| Surplus CLI arguments | Shared parser rejects every unconsumed value with exit 2 and usage | Typed and shared-builder regressions | Open builder/raw grammar | #2372 done |
| Hand-written fixed CLI | Typed `#CLI` becomes starter, docs, example, and dogfood recipe | Closed-corpus fixed-shape/dataflow rule; no noisy arbitrary-user CLI lint | `core.args`, raw argv, occurrence-scoped expert role | D-CLI-RECIPE1=A, #2378, #2396 |
| Grouped machine decimals | `Fixed(n)` becomes plain; `Grouped(n)` is explicit | Formatter/tier tests plus exact redundant-cleanup lint | `Grouped(n)` or lower-level formatter | D-FMT-PLAIN1, #2379, #2397 |
| DataTree ladders | `to_text()` and `equal_unordered()` own the mechanical semantics | Dogfood semantic rule rejects duplicate generic ladders | Explicit domain matching and ordered `==` | D-DATATREE-ERGO1, #2380, #2396 |
| Hand-framed delimited rows | Parameterize the existing CSV engine | Workload corpus rule and RFC-4180/tier proofs | Raw split for a different grammar | D-ROWS1, #2381, #2396 |
| HTTP body/JSON ceremony | Message text, typed JSON, and request-free route forms | Exact default-cap wrapper lint plus example/corpus rule | `body()`, explicit limits, binary/streaming | D-HTTP-MSG1, #2382, #2397 |
| Unit unwrap/rewrap scaling | Linear unit/scalar operators preserve the unit | Exact policy-free rewrap lint plus affine/inverse negative proofs | FFI, calibration, rounded conversion boundary | D-UNIT-SCALAR1, #2383, #2397 |
| Lying digest aliases | Delete aliases; typed digest owns `.hex()` and `.bytes()` | Public-surface, unknown-symbol, docs, table, and tier tests | Explicit `.bytes()` projection | D-CRYPTO-DIGEST1, #2384 |
| Small stdlib reimplementations | ASCII, path, walk, and args APIs | Exact semantic lint shapes plus dogfood/workload corpus rules | Raw traversal/path/process policy with local allow | D-STDLIB-SMALL1, #2385, #2397 |
| False `L0508` | Correct the typed Unit/non-Unit predicate | Positive Unit and neighboring non-Unit UI snapshots | Targeted allow only for a real deliberate discard | #2373 (done) |
| False build lints | Treat the selected build entry as a compiler liveness root | Build-root and neighboring-real-lint snapshots | None for compiler-owned roots | #2374 (done) |
| Retired example ceremony | Migrate to current syntax and APIs | Semantic canonical-corpus rule in existing golden/tier gates | Manifest role for expert/negative lessons | #2375, #2396 |
| Dogfood stdlib duplication | Delete local helpers after behavior parity | Scoped semantic dogfood policy | Documented different protocol/policy | #2376, #2396 |
| Repeated generated FFI envelopes | One decoder emitted per binding | Renderer IR tests assert one helper and delegation | Per-binding foreign-protocol override | #2377 |

### Light semantic lint set

Card #2397 targets seven default-on warnings in the existing diagnostics system. Each rule
may activate only after its replacement lands and only for the normalized semantic shape below:

| Rule | Positive trigger | Must stay quiet |
|---|---|---|
| `process_args_view` | Direct resolved `process.argv().skip(1)` | Alias/index/dataflow cases; code needing `argv[0]` |
| `message_text` | Sema exposes the public default-cap identity and constant evaluation proves `body().text` uses it | Explicit different limit, binary, streaming |
| `redundant_fixed_cleanup` | The relevant output component is solely a plain `Fixed(n)` projection and immediately removes grouping | Mixed or arbitrary comma cleanup |
| `unit_scalar_rewrap` | One typed expression unwraps the same linear unit, scales, and immediately rewraps with no policy call | Helpers, FFI, calibration, rounding, affine points |
| `path_containment_string_prefix` | Typed `Path` provenance reaches the normalized separator-aware prefix idiom | String-only legacy helpers; physical/symlink policy |
| `complete_ascii_case_ladder` | Complete constant A-Z or a-z same-value ladder with no intervening observation | Partial/domain replacement |
| `walk_files_filter` | Direct `fs.walk` control flow only excludes directories and remaining uses are path-only | Aliases, metadata, custom traversal, early-exit policy |

Manual fixed-shape CLI, delimited-reader intent, DataTree truthiness, HTTP JSON intent,
unused request parameters, and nontrivial aliases are **not** arbitrary-user lints. The
compiler cannot prove their intent without noise. Card #2396 handles known maintained
recipes with exact semantic rules and occurrence-scoped exceptions.

### Canonical-corpus ratchet

Card #2396 specifies one checked-in manifest of every maintained Jet source role and every
generated-source producer: executable docs blocks, site sources, nested examples and learn
solutions, dogfood, gauntlet, workloads, performance receipts, starters, fixture roles, and
all binding renderers. Once implemented, a new eligible file or producer without a
classification must fail.

Its acceptance terms require one `tests/support` `CorpusPolicy` adapter to own the inventory,
semantic rule registry, exception validation, and producer-to-artifact provenance. Existing
domain gates will call it; the lexical retirement ratchet stays separate. Exceptions must
record rule, stable semantic site/span, expected occurrence, and reason. A second occurrence
must fail. Each failure must report file, rule, site, why, and the mechanical replacement.

## Celebrated pragmatism (preserve)

- Typed `#CLI` entry: the schema is the entry parameter, one parser under typed and builder
  surfaces, `#Env`/`#Short` precedence proven, argv tier-parity tested.
- The implicit top-level `fn run` body; `print` with interpolation as the one output form;
  `??` propagation; `?? panic("context")` as the honest fixture idiom (no unwrap/expect).
- `fs.read`/`fs.write` one-liners that already propagate in `fn run()`; `files.open().lines()`;
  `io.stdin().lines()`; `String | Path` acceptance.
- Eager collections with `.lazy()` as the single visible escape; `.counts()`, `.group_by()`,
  `.top_n()`; structural Codable by default with `#!Codable` reject.
- `task.race/any/all` as direct expressions; ownership sigils `&`/`^`/`~`; effect inference
  with `-[]>`/`-[!E]>` as expert contracts; duration literals `2h`, `5min`.
- `process.cmd` capture defaults; `process.pipeline().run_checked()`; `http.get/post` one-shot
  rung beside the builder; `core.math.random` vs `core.crypto.random` split.
- Jet is already shorter than Python in 13 of 16 paired gauntlet entries.

## Audit-start losses and current response

- Delimited text and enumerate-style iteration lost to Python in the audited workload
  adapters. `D-ROWS1=A` now requires one configured CSV engine and workload migration.
- Machine numeric output lost to every peer because `Fixed` grouped by default.
  `D-FMT-PLAIN1=A` now makes plain output canonical and grouping explicit.
- CLI trust was inverted: Python argparse rejected surplus arguments while Jet silently
  discarded them. The shared parser fix and typed/builder regressions landed; #2372 is done.

## Next actions

Ratified outcome A: D-CLI-RECIPE1, D-FMT-PLAIN1, D-DATATREE-ERGO1, D-ROWS1,
D-HTTP-MSG1, D-UNIT-SCALAR1, D-CRYPTO-DIGEST1, and D-STDLIB-SMALL1.

Implementation owners:

- #2372: parser fix plus typed and shared-builder regressions landed. #2373-#2374: landed
  compiler fixes with positive and neighboring-negative proofs. All three cards are done.
- #2375: exact teaching-root migration inventory plus #2396 reintroduction guard.
- #2376: ready dogfood migration with explicit CLI, JSON, URL, equality, directory, path,
  DataTree comparison, and named domain-truthiness criteria.
- #2377: generator-registry-derived envelope decoder coverage for all seven renderers.
- #2378-#2385: ratified product implementations now have atomic behavior, negative,
  applicable-tier, migration, and expert-escape criteria. The overbroad
  `L0515 manual_cli_when_typed` source, spec, and fixture surface was removed and verified;
  fixed-shape CLI remains corpus policy.
- #2396: closed-perimeter semantic canonical-corpus ratchet is now in-tree and its focused
  manifest, provenance, occurrence-exception, AST-shape, and protocol tests pass; the card
  stays open until every domain gate and every named finding/producer criterion is proved.
- #2397: the seven-rule semantic guidance implementation has started and lint snapshots pass;
  the card stays open until all exact-trigger, quiet-neighbor, one-diagnostic, edit, allow,
  precedence, and diagnostics-coverage criteria are proved.
- #2398: statement-position `if` inference/lowering defect found during prevention proof is
  fixed and closed; five all-tier regressions and a Luna-max semantic review pass.

## Disposition table

| Finding | Product correction | Recurrence owner |
|---|---|---|
| Fentry-3 surplus bare args | #2372 | Shared parser regression; no lint |
| Fentry-1/2/4, Frealprog-1, Ffixtures-2, Fdogjet-1, fixed-shape Fex-core-2 | D-CLI-RECIPE1=A / #2378 | #2396; no arbitrary-user intent lint |
| Frealprog-5 grouped Fixed | D-FMT-PLAIN1 / #2379 | Formatter proofs + #2397 |
| Fdogjet-3/4 DataTree ladders | D-DATATREE-ERGO1 / #2380 | #2396; explicit domain truthiness only |
| Frealprog-3 TSV rows | D-ROWS1 / #2381 | #2396; no unsafe intent lint |
| Fex-app-1/2/3 HTTP message | D-HTTP-MSG1 / #2382 | #2396 + #2397 |
| Fex-core-3 unit scaling | D-UNIT-SCALAR1 / #2383 | Type negative proofs + #2397 |
| Fex-app-4 crypto naming | D-CRYPTO-DIGEST1 / #2384 | Removed public surface and table/docs proofs |
| Fdogjet-2/8, Frealprog-4, args-view Fex-core-2 | D-STDLIB-SMALL1 / #2385 | #2396 + #2397 |
| Fex-core-4 L0508 | #2373 | Typed positive/negative UI snapshots |
| Fex-core-6 build lints | #2374 | Build-root and neighboring-lint snapshots |
| Fex-core-1/5, Fex-data-1/2, Fex-sys-1/2/3/4, Frealprog-2/6, Ffixtures-1 | #2375 | #2396 |
| Fdogjet-5/6/7/9 | #2376 | #2396 |
| Frealprog-7 | #2377 | Generator IR/renderer structural tests |

Every one of the 38 lane findings maps through this table. **Audit-only: zero.**

## Strongest unverified assumption

Lexical `rg` counts approximate real developer frequency; generated time-accuracy fixtures and
comments inflate some rows, and no out-of-repo Jet corpus exists yet to check against.

Lane evidence: `~/.cache/jet-luna/friction-2026-08-30/out/*.md` (8 corpus lanes, 38 findings, plus 5 Luna-max systemic-prevention lanes).
