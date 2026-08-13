# Surface frequency audit — 2026-08-04

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Executive summary

This audit measured what programmers actually write. It parsed 98 public projects across five
languages, nine domains, and five project strata: 12,790 files and 6,760,230 lexical tokens of
production source. Every count maps to a frozen catalog of 300 measurement keys, and every key maps
to a section of an official language specification.

The main result is a ranked list of twelve language-agnostic operations. Five appear in every
project measured: binding a value, defining a callable, splitting code into modules, choosing
between paths, and reacting to a missing value. Absence handling is the fifth densest operation, and
denser than error handling.

The second result is about Jet. Jet's current spellings were compared with peer spellings on eight
equivalent tasks, counted with one tokenizer. **Jet is at or below the peer median cost on all eight
tasks.** Jet is the cheapest of all six languages when falling back after a failed call, and the
cheapest of the sound peers when falling back on an absent value. Jet beats every sound peer on
error propagation. The audit found no high-frequency operation where Jet forces more ceremony than
the peer median.

The largest Jet opportunity is therefore not new syntax. It is example coverage of surfaces Jet
already ships. Two findings carry that point:

- Optional chaining (`?.`) works in the compiler and is ratified as S71/S35. **Zero examples use
  it.** In TypeScript, `?.` is 30% of all absence-handling sites and appears in 95% of projects.
- The loop-yield comprehension (`loop n, xs -> …`) is the cheapest Jet form for transforming a
  collection, at 26 tokens against 33 for the method chain. **Two example files use it.** The
  comprehension is the most used transform surface in the corpus by site count, at 3,774 sites: it
  is 47% of Python transform sites and 45% of Python iterate sites.

Both are invariant I5 problems, not design problems. A feature with no example is a feature users do
not find.

Three measured facts support current Jet decisions:

1. **Immutable default is what people write.** Where a language distinguishes, writers pick the
   immutable form: Rust `let` 75% against `let mut` 15%, TypeScript `const` 73% against `let` 8%.
   This supports `::` and `:=` (D-BIND-BARE1, D-MEM1).
2. **A separate `match` keyword does not earn its place.** `if` is 45% to 95% of branch sites in
   every language measured, Python's `match` recorded zero uses across 20 projects, and `switch` is
   about 1% in TypeScript and JavaScript. Rust is the exception at 26% `match` plus 24% `if let`.
3. **Failure handling is spelled short or it is skipped.** Rust's `?` is 31% of Rust error sites at
   100% prevalence; Python's broad `except` is 13% of Python error sites, in 70% of projects.

The strongest limit is corpus scope. Five of the twenty-eight baseline languages were measured, and
no adjacent declarative surface (HTML, CSS, build files, CI files, SQL) was. Rust and Go counts come
from a text-level scanner, not an official parser, so every Rust and Go row is graded `Weak`, and
dropping those two languages does move the operation ranking. Two of one hundred cells produced no
source: `go.networking.education` and `rust.systems.mature-oss`.

### Decision view

| Rank | Job or surface | Why it matters | Evidence strength | Jet action |
| --- | --- | --- | --- | --- |
| 1 | Collection transform, taught form | 98% of projects; the comprehension is the most used transform surface by site count | Moderate | Reduce friction (examples, not syntax) |
| 2 | Optional chaining `?.` | 30% of TypeScript absence sites; 95% of TypeScript projects; zero Jet examples | Moderate | Add examples |
| 3 | Immutable-default binding | Densest operation; 75–92% of bindings are immutable where the choice exists | Moderate | Keep |
| 4 | Absence handling `??` | 100% of projects; fifth densest operation; Jet is the cheapest sound form | Moderate | Keep |
| 5 | Failure-returning signature and `?` | Rust `?` in 100% of Rust projects; Jet beats every sound peer | Weak | Keep |
| 6 | Unified `if x == { … }` | `if` is 45–95% of branch sites; Python `match` unused | Moderate | Keep |
| 7 | Error-swallowing diagnostics | Broad `except` in 70% of Python projects, heaviest in one-off and education code | Moderate | Study |
| 8 | `#Test` blocks | 87% of projects declare tests | Moderate | Keep |
| 9 | Markers and auto-derive | 94% of projects use a metaprogramming surface | Weak | Keep |
| 10 | Concurrency surfaces | 78% of projects, the lowest of the twelve operations | Weak | Watchlist |

## What people do most

Twelve language-agnostic operations were measured. Each operation row counts sites of that operation
against normalized lexical tokens. "Project prevalence" is the share of eligible projects with at
least one use. "Balanced prevalence" averages prevalence inside each language-domain-stratum cell
first, so one large project cannot dominate. "Breadth" is the share of populated cells with at least
one use.

| Operation | Difficulty | Prevalence | Breadth | Median sites / 1k tokens | Confidence |
| --- | --- | --- | --- | --- | --- |
| bind a value to a name | entry | 1.00 | 1.00 | 19.90 | Moderate |
| define a callable | entry | 1.00 | 1.00 | 12.61 | Moderate |
| split and reuse code units | entry | 1.00 | 1.00 | 11.06 | Weak |
| choose between paths | entry | 1.00 | 1.00 | 10.62 | Moderate |
| react to a missing value | general | 1.00 | 1.00 | 5.44 | Moderate |
| prove the code works | general | 0.87 | 0.87 | 4.40 | Weak |
| react to failure | general | 0.97 | 0.97 | 3.29 | Moderate |
| declare a data shape | general | 0.97 | 0.97 | 3.08 | Moderate |
| write code that shapes code | expert | 0.94 | 0.94 | 2.16 | Weak |
| transform a collection | general | 0.98 | 0.98 | 2.05 | Moderate |
| repeat over data | entry | 0.96 | 0.96 | 1.89 | Weak |
| run work concurrently | expert | 0.78 | 0.78 | 0.89 | Weak |

Prevalence saturates: ten of twelve operations exceed 87% balanced prevalence. Density separates
them. Binding is nearly twice as dense as module work and twenty times as dense as concurrency.

Two results are worth stating plainly.

**Absence handling is a first-tier job, not a corner case.** It appears in every project measured,
and it is denser than error handling, data declaration, and collection transforms. Languages that
treat "the value might not be there" as an afterthought misjudge how often it happens.

**Iteration and transformation overlap by construction.** A comprehension counts once as an iterate
surface and once as a transform surface, because it is both. Both rows therefore rank low while the
work is common: 45% of Python iterate sites are comprehensions and 50% of Rust iterate sites are
iterator chains. Writers reach for a transform expression before a loop.

## Which surfaces they use

Shares are sites of that surface divided by sites of its parent operation, pooled across projects.
Parent and child rows are never mixed.

### Binding a value

| Language | Surface shares |
| --- | --- |
| Python | `name = value` 0.88, `name: T = value` 0.09, unpack 0.03, walrus 0.00 |
| TypeScript | `const` 0.73, `const x: T` 0.10, destructuring 0.09, `let` 0.08 |
| JavaScript | `const` 0.54, `var` 0.38, destructuring 0.04, `let` 0.04 |
| Rust | `let` 0.75, `let mut` 0.15, `const`/`static` 0.05, `let x: T` 0.05 |
| Go | `:=` 0.51, multi-assign 0.34, `var x T` 0.13, `const` 0.02 |

The JavaScript `var` share of 0.38 is misleading alone: only 30% of JavaScript projects use `var`,
and two large legacy projects carry the volume. Pooled shares follow bytes; prevalence follows
people. Both are reported.

### Reacting to failure

| Language | Surface shares |
| --- | --- |
| Python | `raise` 0.53, `try`/`except` 0.31, broad `except` 0.13, `assert` 0.02 |
| TypeScript | `throw` 0.46, `try`/`catch` 0.40, typed error result 0.08, `.catch()` 0.06 |
| JavaScript | `throw` 0.57, `try`/`catch` 0.39, `.catch()` 0.03, error callback 0.01 |
| Rust | `match` on `Result` 0.35, `.unwrap()`/`.expect()` 0.32, `?` 0.31, `panic!` 0.02 |
| Go | `if err != nil` 0.54, `errors.New`/`fmt.Errorf` 0.35, `panic` 0.06, `%w` wrap 0.04 |

Every Rust project measured uses `.unwrap()` or `.expect()`. A language cannot design the
abrupt-failure escape out of existence; it can only decide how clearly it reads.

### Reacting to a missing value

| Language | Surface shares |
| --- | --- |
| Python | `Optional[T]` 0.40, `is None` 0.29, `dict.get(k, d)` 0.22, `x or default` 0.08 |
| TypeScript | null comparison 0.48, `a?.b` 0.30, `a ?? b` 0.19, `x!` 0.02 |
| JavaScript | `a \|\| default` 0.66, `a?.b` 0.23, null comparison 0.07, `a ?? b` 0.04 |
| Rust | `match`/`if let Some` 0.42, `unwrap_or*` 0.34, `is_some`/`is_none` 0.13, `map`/`and_then` 0.11 |
| Go | `x == nil` 0.67, zero-value default 0.15, comma-ok 0.09, pointer optional 0.08 |

### Choosing between paths

| Language | Surface shares |
| --- | --- |
| Python | `if` 0.83, ternary 0.12, `elif` 0.05, `match` 0.00 |
| TypeScript | `if` 0.69, ternary 0.28, `else if` 0.03, `switch` 0.01 |
| JavaScript | `if` 0.67, ternary 0.29, `else if` 0.03, `switch` 0.01 |
| Rust | `if` 0.45, `match` 0.26, `if let`/`let else` 0.24, `else if` 0.04 |
| Go | `if` 0.95, `switch` 0.03, type switch 0.01, `else if` 0.01 |

The conditional expression is not niche: 28% of TypeScript and 29% of JavaScript branch sites, at
prevalence near 1.00.

### Repeating over data, and transforming a collection

| Language | Iterate shares | Transform shares |
| --- | --- | --- |
| Python | comprehension 0.45, for-each 0.43, range 0.09, while 0.03 | comprehension 0.47, aggregate 0.26, loop+append 0.17, `map`/`filter` 0.10 |
| TypeScript | for-of 0.67, classic for 0.13, `forEach` 0.11, while 0.09 | spread 0.64, `map` 0.24, `filter` 0.10, `reduce` 0.02 |
| JavaScript | for-of 0.44, classic for 0.36, while 0.13, `forEach` 0.06 | spread 0.44, `map` 0.32, `filter` 0.21, `reduce` 0.03 |
| Rust | iterator chain 0.50, for-in 0.43, while 0.04, `loop` 0.03 | `map` 0.57, `collect` 0.32, `filter` 0.07, fold/sum 0.04 |
| Go | range-for 0.89, classic for 0.06, condition for 0.03, `for {}` 0.02 | append loop 0.57, map index 0.35, slices/maps 0.04, sort 0.03 |

Index loops are a minority everywhere: 6% in Go, 9% in Python, 13% in TypeScript. JavaScript is the
exception at 36%, tracking the older code in that sample.

## Beginner adoption path

Entry-difficulty surfaces carry a 1.15 adoption factor, which changes priority only, never a count.

The most used entry surfaces are plain and few: `if`, a plain binding, a named function, a for-each
loop, and a null check. Fourteen entry surfaces have 1.00 project prevalence and a share above 0.60
inside their operation. A beginner needs roughly a dozen spellings to write most of this corpus.

Jet's cost on the entry tasks is competitive. Binding costs 12 tokens against a peer median of 15.
Defining a function costs 14 against 15. A two-way branch costs 20 against 28. Declaring a record
costs 10, second only to Go's 9.

Two beginner risks show up in the data.

**Silent failure is the beginner's default.** Broad `except` peaks at 28% of error sites in one-off
code and 14% in education code, against 1% to 3% in small libraries and mature projects. That is not
ignorance of `try`; it is a preference for making the failure disappear. Jet's `??` is the same
move, and it is already the most used spelling in Jet's own examples, in 155 files. It is a
diagnostics question, not a syntax question.

**Type annotations are optional in practice.** Annotated bindings are 9% of Python and 10% of
TypeScript bindings. Jet keeping types off bindings and on signatures and values (D-BIND-BARE1)
matches how people write.

## Expert production path

Expert surfaces are broad but thin. Concurrency reaches 78% of projects, the lowest prevalence of
the twelve operations, at 0.89 median sites per 1,000 tokens and 6.46 at the p90. Concurrency is
rare in most files and dense in a few.

Go is the clearest case. Channels appear in 100% of Go projects and are 41% of Go concurrency sites.
Goroutines are 12% and mutexes 32%. Coordination costs more surface than starting work does.

Metaprogramming reaches 94% of projects. Rust `derive` is 19% to 64% of Rust metaprogramming sites
depending on stratum, highest in education code; Python decorators are 23% to 88%; generic bounds
are 57% of Rust metaprogramming sites. These are ordinary production surfaces.

Testing reaches 87% of projects, with a p90 density of 19.53 against a median of 4.40: projects
either test seriously or barely.

Jet's expert opt-ins line up. `taskgroup`, `tasks.channel`, `#Known`, markers, and trait bounds all
have shipped examples. No measured expert surface is missing from Jet.

## Jet recommendations

Each recommendation separates the measurement from the Jet judgment. The priority index is
`100 × frequency × friction × audience × confidence`. Frequency here is
`0.80 × balanced prevalence + 0.20 × breadth`: opportunity share is not defined for operation rows,
because their denominator is lexical tokens rather than eligible semantic opportunities, so its 0.20
weight was redistributed to prevalence. This redistribution applies to every priority number below.

### 1. Reduce friction — teach the loop-yield comprehension (priority 23.7)

**Measured evidence.** Transforming a collection appears in 98% of projects. The comprehension is
the most used transform surface in the corpus by site count, at 3,774 sites: 47% of Python transform
sites and 45% of Python iterate sites, at 1.00 project prevalence. Rust reaches the same place
through iterator chains at 50% of iterate sites.

**Jet-specific inference.** Jet has the form. On the measured task, `loop n, xs -> { … }` costs 26
tokens and Jet's own method chain `xs.filter(…).map(…)` costs 33, which is 27% more. A Python
comprehension costs 18. Two example files use the yield arrow, while 155 use `??`. The cheapest Jet
form is the least visible one.

**Priority components.** Frequency 0.98, friction 0.31, audience 1.05, confidence 0.75.

**Beginner effect.** A beginner reading the examples reaches for the method chain and pays 27% more
tokens for the same result. **Expert effect.** None; both forms stay. **Gate.** No safety, control,
or diagnostics cost: this changes examples and docs, not semantics.

**Tower status.** `Covered` as design: D-LOOPMAP1 and D-LOOP-HEADER3 are ratified on card #1325
(done). `Not covered` as teaching: no card owns comprehension example coverage.

### 2. Add examples — optional chaining `?.` (priority: frequency 1.00, friction not measurable)

**Measured evidence.** `a?.b` is 30% of TypeScript absence sites at 95% project prevalence, and 23%
of JavaScript sites at 50%. Absence handling appears in 100% of projects.

**Jet-specific inference.** `?.` is ratified (S71/S35) and works: `u.address?.city ?? "unknown"`
compiled and ran during this audit. No file under `examples/` contains `?.`, and I5 requires an
executable example for every feature.

**Beginner effect.** A beginner cannot discover the surface. **Expert effect.** None. **Gate.** I5
compliance; the example also adds golden output on every applicable tier.

**Tower status.** `Not covered`. No card names optional-chaining example coverage.

### 3. Keep — `::` and `:=` immutable-default binding (priority 14.4)

**Measured evidence.** Binding is the densest operation at 19.90 median sites per 1,000 tokens and
1.00 prevalence. Where the language offers the choice, the immutable form wins: Rust 75% `let`
against 15% `let mut`; TypeScript 73% `const` against 8% `let`.

**Jet-specific inference.** Jet costs 12 tokens against a peer median of 15. The 0.17 gap against
Python's 10 tokens buys a mutability distinction Python does not have. **Gate.** That distinction is
the safety mechanism; do not trade it for two tokens.

**Tower status.** `Covered`. D-BIND-BARE1 ratified. D-MEM1 ratified.

### 4. Keep — the `??` fallback family

**Measured evidence.** Absence handling appears in 100% of projects at 7.17 median sites per 1,000
tokens, the fifth densest operation.

**Jet-specific inference.** Jet's fallback costs 11 tokens against a sound-peer median of 21. It is
the cheapest sound form measured. Python's `or` is 9 tokens, but it rewrites every falsy value, not
only an absent one. `?? next`, `?? break`, and `?? return` extend the form without a new
mechanism.

**Tower status.** `Covered`. D-ORRETURN-ERG1 ratified.

### 5. Keep — fallible signature `=> T ? E` with postfix `?`

**Measured evidence.** Rust's `?` is 31% of Rust error sites at 100% project prevalence. Go's manual
`if err != nil` is 54% of Go error sites.

**Jet-specific inference.** The propagation task costs 28 tokens in Jet, 31 in Rust, and 39 in Go.
The Python, TypeScript, and JavaScript forms are shorter only because they propagate nothing, so
they are not a sound baseline.

**Tower status.** `Covered`.

### 6. Keep — unified `if x == { … }` instead of a separate `match` (priority 8.6)

**Measured evidence.** `if` is 83% of Python branch sites, 69% of TypeScript, 67% of JavaScript, 95%
of Go, and 45% of Rust. Python's `match` recorded zero uses across 20 projects, and `switch` is 1%
in TypeScript and JavaScript.

**Jet-specific inference.** One branch mechanism with pattern arms matches real code and keeps the
beginner surface small (I8). Rust's combined 50% `match` and `if let` share shows the arms still
need to be good.

**Tower status.** `Covered`. D-BRANCH-TEACH1 and D-BRANCH-CODEGEN1 ratified, cards #1259 and #1260
done.

### 7. Keep — the conditional expression `if c -> a else -> b`

**Measured evidence.** Ternary is 28% of TypeScript and 29% of JavaScript branch sites, and 12% of
Python, at near-1.00 prevalence.

**Jet-specific inference.** The form works and appears in five example files.

**Tower status.** `Covered`.

### 8. Study — diagnostics for swallowed failures

**Measured evidence.** Broad `except` is 13% of Python error sites across 70% of projects, peaking
at 28% in one-off code and 14% in education code against 1% in small libraries.

**Jet-specific inference.** `??` is Jet's shortest failure exit and its most used example spelling.
The incentive that produces bare `except` in Python applies to `??` in Jet. The question is whether
a lint should mark a discarded error value, not whether `??` should change.

**Gate.** Diagnostics only; no syntax change, and no lint may punish deliberate fallback.

**Tower status.** `Not covered`.

## Keep

The evidence supports these Jet defaults as they stand. The first six are argued above; the last
three are Keep items with no measured friction.

- `::` and `:=` bare bindings with an immutable default.
- The `??` fallback family, including `?? next`, `?? break`, and `?? return`.
- Failure-returning types `=> T ? E` with postfix `?`.
- One branch mechanism, `if x == { … }`, with pattern arms.
- The conditional expression `if c -> a else -> b`.
- `struct`, `enum`, `trait`, and `Type.{ … }` construction.
- `#Test` blocks with `require` and `require_eq`: test declarations appear in 87% of projects, at a
  p90 density of 19.53 sites per 1,000 tokens against a median of 4.40. Tower `Covered`.
- Markers and auto-derive with an explicit opt-out: a metaprogramming surface appears in 94% of
  projects, and Rust `derive` reaches 64% of metaprogramming sites in education code. Tower
  `Covered`, D-AUTODERIVE1 ratified on card #1267.
- `...` spread and member spread: destructuring is 9% of TypeScript bindings at 95% prevalence, and
  spread is 64% of TypeScript transform sites at 100% prevalence. Seventeen example files contain a
  `...` form, a count that does not separate variadic parameters from spread. Tower `Covered`,
  D-SPREAD1 ratified on card #1341.
- `loop` with a yield arrow, and eager `map`/`filter` with opt-in `.lazy()`.

## Watchlist

- Walrus assignment: 0.00 share in Python and only 25% project prevalence. No Jet action.
- Non-null assertion `x!`: 2% of TypeScript absence sites. Jet has no equivalent escape, and the
  evidence does not ask for one.
- Type switches: 1% of Go branch sites.
- Table-driven tests: a Go convention with no cross-language equivalent measured.
- Benchmarks: measured only in Go, at low volume.
- `var` in JavaScript: 38% of pooled binding sites but only 30% project prevalence. This is legacy
  concentration, not current practice.
- Error wrapping (`%w`, `errors.Is`): 4% of Go error sites at 47% prevalence. Worth re-measuring
  with a real parser before drawing a conclusion for Jet.
- Concurrency surfaces: the lowest prevalence of the twelve operations at 78%, with the widest
  median-to-p90 spread. Go channels appear in 100% of Go projects. Revisit when an official parser
  covers Rust and Go.

## What changes the ranking

**Deduplication.** All 98 projects have distinct canonical identities. No fork, mirror, or vendored
copy entered the primary view. Deduplication changed nothing.

**Weighting.** Equal-language, equal-domain, and equal-stratum weighting all produce the same twelve
operations in the same bands. The largest change under any weighting is 0.04 in balanced prevalence.

**Ranking metric.** Every stability view below ranks operations by median sites per 1,000 tokens,
which is the metric the operation table publishes. An earlier draft ranked them by balanced
prevalence and reported no movement anywhere. That was an artifact: balanced prevalence is 1.00 for
five operations, so ties froze the order. Prevalence cannot separate the top group.

**Leave-one-out.** Dropping one language moves any operation by at most two places, but the number
of operations that move ranges from three to eight. Dropping Go is the only drop that changes which
operations sit in the top five. Dropping one domain moves any operation by at most one place, and
dropping web-frontend or data-science moves nothing at all.

**Parser class.** This is the largest lever in the run. Restricting the corpus to the three
languages with an official parser moves five operations, by up to four places, and changes the top
five. Declaring a data shape rises from rank eight to rank five. Reacting to a missing value falls
from five to seven. Reacting to failure falls from seven to eleven. Heuristic Rust and Go rows do
affect the ranking, which is why every Rust and Go row is graded `Weak` and why the report leans on
prevalence and breadth rather than density for its recommendations.

**Overlapping operations.** The `iterate` and `transform-collection` rows are not independent: a
comprehension counts in both. Surface shares inside each operation are unaffected.

## Coverage and limits

| Language | Projects | Files parsed | Files skipped | Production tokens |
| --- | --- | --- | --- | --- |
| Go | 19 | 1,621 | 0 | 1,034,064 |
| JavaScript | 20 | 1,932 | 0 | 1,076,047 |
| Python | 20 | 4,024 | 2 | 1,754,189 |
| Rust | 19 | 1,608 | 0 | 1,950,522 |
| TypeScript | 20 | 3,605 | 0 | 945,408 |
| **Total** | **98** | **12,790** | **2** | **6,760,230** |

Normalized lines: 1,093,386. Sampled source sites recorded as evidence: 12,784. Measurement rows:
5,880. Parse failures: two Python files, both syntax errors under the 3.13 grammar.

Declared scope and gaps:

- **Languages.** Five of the twenty-eight in the method baseline. C, C++, C#, Java, Kotlin, Swift,
  Objective-C, Ruby, PHP, Lua, Bash, PowerShell, SQL, R, Julia, Haskell, OCaml, F#, Elixir, Erlang,
  Zig, Nix, and WebAssembly text are `unavailable`: no sample was drawn.
- **Adjacent surfaces.** HTML, CSS, regular expressions, build files, CI files, query languages,
  infrastructure configuration, package manifests, and deployment configuration are `unavailable`.
- **Sample targets.** The method asks for at least 30 projects per language and per domain. This run
  has 19 to 20 per language and 4 to 25 per domain, and every language-domain cell holds one project
  against a target of five. **Every cell in this run is `weak` by the method's own rule.**
- **Empty cells.** Two cells produced no usable source: `go.networking.education` failed after three
  candidates and `rust.systems.mature-oss` after one. Both are `unavailable` units, not zeros.
- **Parser class.** Python used the CPython AST and tokenizer. TypeScript and JavaScript used the
  TypeScript compiler API. Rust and Go used a text-level scanner that blanks comments and string
  bodies before matching, except for three rules whose evidence lives inside comments or strings
  (Rust doc-tests, Go imports, Go internal packages); those three read the original source. No
  symbol table was built for any language, so API identity is name-level. Restricting the corpus to
  the official-parser languages changes the operation ranking, as recorded above.
- **Strata.** "Professional production" is approximated by organisation-owned, long-lived, popular
  public repositories. Proprietary production code is not public and was not measured.
- **Static source only.** These counts measure what is written, not what runs. No runtime telemetry
  was collected. Frequency in source is not importance, and it is not approval.
- **Trend.** No time-cohort analysis was run; the corpus is one retrieval window.
- **Catalog review.** The planned independent agent review of the five specification inventories did
  not complete; that agent hit a session limit. The inventories were verified mechanically instead:
  the Go inventory matches all 127 live spec anchors exactly; all 118 pages of the official Rust
  Reference table of contents are present, plus two live Reference pages missing from its sidebar;
  40 sampled section URLs across the five catalogs returned HTTP 200; and all 300 measurement keys
  were re-derived independently and matched. The Python, TypeScript, and JavaScript inventories were
  not re-derived from their tables of contents by a second party.

<details>
<summary>Complete coverage matrix and long-tail results</summary>

### Operation ranking (full)

| Operation | Difficulty | Project prevalence | Balanced prevalence | Breadth | Median sites / 1k tokens | p90 / 1k |
| --- | --- | --- | --- | --- | --- | --- |
| bind-value | entry | 1.00 | 1.00 | 1.00 | 19.90 | 35.21 |
| define-callable | entry | 1.00 | 1.00 | 1.00 | 12.61 | 19.80 |
| modularize | entry | 1.00 | 1.00 | 1.00 | 11.06 | 22.55 |
| branch | entry | 1.00 | 1.00 | 1.00 | 10.62 | 18.40 |
| handle-absence | general | 1.00 | 1.00 | 1.00 | 5.44 | 12.99 |
| test-and-verify | general | 0.87 | 0.87 | 0.87 | 4.40 | 19.53 |
| handle-error | general | 0.97 | 0.97 | 0.97 | 3.29 | 11.82 |
| define-data-type | general | 0.97 | 0.97 | 0.97 | 3.08 | 20.18 |
| metaprogram | expert | 0.94 | 0.94 | 0.94 | 2.16 | 6.84 |
| transform-collection | general | 0.98 | 0.98 | 0.98 | 2.05 | 6.40 |
| iterate | entry | 0.96 | 0.96 | 0.96 | 1.89 | 4.70 |
| concurrency | expert | 0.78 | 0.78 | 0.78 | 0.89 | 6.46 |

### Every measured surface (240 rows)

| Feature id | Language | Operation | Surface | Share of operation sites | Project prevalence | Sites |
| --- | --- | --- | --- | --- | --- | --- |
| go:bind-value-short-var | go | bind-value | := | 0.510 | 1.00 | 8,881 |
| go:bind-value-multi-assign | go | bind-value | a, b = f() | 0.338 | 1.00 | 5,879 |
| go:bind-value-var-decl | go | bind-value | var x T | 0.128 | 1.00 | 2,219 |
| go:bind-value-const-decl | go | bind-value | const | 0.024 | 1.00 | 422 |
| javascript:bind-value-const | javascript | bind-value | const | 0.542 | 1.00 | 13,950 |
| javascript:bind-value-var | javascript | bind-value | var | 0.381 | 0.30 | 9,803 |
| javascript:bind-value-destructuring | javascript | bind-value | const {a, b} = x | 0.039 | 0.85 | 1,007 |
| javascript:bind-value-let | javascript | bind-value | let | 0.038 | 0.85 | 972 |
| python:bind-value-plain-assign | python | bind-value | name = value | 0.882 | 1.00 | 51,979 |
| python:bind-value-annotated-assign | python | bind-value | name: T = value | 0.091 | 0.70 | 5,339 |
| python:bind-value-unpack-assign | python | bind-value | a, b = value | 0.027 | 1.00 | 1,572 |
| python:bind-value-walrus | python | bind-value | := | 0.000 | 0.25 | 28 |
| rust:bind-value-let | rust | bind-value | let | 0.746 | 1.00 | 18,338 |
| rust:bind-value-let-mut | rust | bind-value | let mut | 0.155 | 1.00 | 3,805 |
| rust:bind-value-const-static | rust | bind-value | const / static | 0.050 | 0.89 | 1,236 |
| rust:bind-value-annotated | rust | bind-value | let x: T | 0.049 | 1.00 | 1,192 |
| typescript:bind-value-const | typescript | bind-value | const | 0.731 | 1.00 | 14,272 |
| typescript:bind-value-annotated | typescript | bind-value | const x: T = v | 0.098 | 1.00 | 1,921 |
| typescript:bind-value-destructuring | typescript | bind-value | const {a, b} = x | 0.089 | 0.95 | 1,733 |
| typescript:bind-value-let | typescript | bind-value | let | 0.082 | 0.90 | 1,605 |
| go:branch-if | go | branch | if | 0.945 | 1.00 | 13,271 |
| go:branch-switch | go | branch | switch | 0.032 | 0.84 | 444 |
| go:branch-type-switch | go | branch | switch x.(type) | 0.012 | 0.47 | 165 |
| go:branch-else-if | go | branch | else if | 0.012 | 0.74 | 162 |
| javascript:branch-if | javascript | branch | if | 0.670 | 1.00 | 12,138 |
| javascript:branch-ternary | javascript | branch | c ? a : b | 0.293 | 0.95 | 5,308 |
| javascript:branch-else-if | javascript | branch | else if | 0.028 | 0.75 | 502 |
| javascript:branch-switch | javascript | branch | switch | 0.009 | 0.55 | 155 |
| python:branch-if | python | branch | if | 0.826 | 1.00 | 17,505 |
| python:branch-ternary | python | branch | a if c else b | 0.120 | 1.00 | 2,538 |
| python:branch-else-if | python | branch | elif | 0.054 | 0.95 | 1,139 |
| python:branch-match | python | branch | match | 0.000 | 0.00 | 0 |
| rust:branch-if | rust | branch | if | 0.450 | 1.00 | 3,870 |
| rust:branch-match | rust | branch | match | 0.264 | 1.00 | 2,277 |
| rust:branch-if-let | rust | branch | if let / let else | 0.244 | 1.00 | 2,098 |
| rust:branch-else-if | rust | branch | else if | 0.042 | 0.74 | 364 |
| typescript:branch-if | typescript | branch | if | 0.686 | 1.00 | 7,487 |
| typescript:branch-ternary | typescript | branch | c ? a : b | 0.279 | 1.00 | 3,040 |
| typescript:branch-else-if | typescript | branch | else if | 0.028 | 0.85 | 302 |
| typescript:branch-switch | typescript | branch | switch | 0.008 | 0.80 | 82 |
| go:concurrency-channel | go | concurrency | chan / <- | 0.415 | 1.00 | 325 |
| go:concurrency-mutex | go | concurrency | sync.Mutex / RWMutex | 0.323 | 0.77 | 253 |
| go:concurrency-waitgroup | go | concurrency | sync.WaitGroup | 0.145 | 0.62 | 114 |
| go:concurrency-goroutine | go | concurrency | go f() | 0.117 | 0.92 | 92 |
| javascript:concurrency-await | javascript | concurrency | await | 0.878 | 0.79 | 2,734 |
| javascript:concurrency-callback-async | javascript | concurrency | setTimeout / event callback | 0.081 | 0.89 | 251 |
| javascript:concurrency-then-chain | javascript | concurrency | .then(...) | 0.022 | 0.47 | 69 |
| javascript:concurrency-promise-all | javascript | concurrency | Promise.all | 0.020 | 0.26 | 61 |
| python:concurrency-await | python | concurrency | await | 0.705 | 0.80 | 761 |
| python:concurrency-thread-or-process | python | concurrency | threading / multiprocessing | 0.182 | 0.70 | 197 |
| python:concurrency-lock | python | concurrency | Lock / Semaphore | 0.061 | 0.50 | 66 |
| python:concurrency-gather | python | concurrency | asyncio.gather / TaskGroup | 0.052 | 0.40 | 56 |
| rust:concurrency-async-await | rust | concurrency | .await | 0.605 | 0.65 | 2,411 |
| rust:concurrency-arc-mutex | rust | concurrency | Arc / Mutex / RwLock | 0.311 | 0.76 | 1,240 |
| rust:concurrency-channel | rust | concurrency | channel / mpsc | 0.070 | 0.65 | 277 |
| rust:concurrency-thread-spawn | rust | concurrency | thread::spawn / tokio::spawn | 0.014 | 0.71 | 57 |
| typescript:concurrency-await | typescript | concurrency | await | 0.802 | 1.00 | 2,541 |
| typescript:concurrency-then-chain | typescript | concurrency | .then(...) | 0.167 | 0.76 | 528 |
| typescript:concurrency-promise-all | typescript | concurrency | Promise.all | 0.031 | 0.65 | 98 |
| typescript:concurrency-worker | typescript | concurrency | Worker / worker_threads | 0.000 | 0.00 | 0 |
| go:define-callable-method | go | define-callable | func (r T) f() | 0.409 | 0.95 | 3,983 |
| go:define-callable-func-decl | go | define-callable | func f() | 0.307 | 1.00 | 2,984 |
| go:define-callable-func-literal | go | define-callable | func() { ... } | 0.283 | 1.00 | 2,756 |
| go:define-callable-generic-func | go | define-callable | func f[T any] | 0.001 | 0.21 | 10 |
| javascript:define-callable-function-decl | javascript | define-callable | function f() | 0.610 | 0.95 | 8,905 |
| javascript:define-callable-arrow | javascript | define-callable | () => {} | 0.271 | 1.00 | 3,953 |
| javascript:define-callable-async | javascript | define-callable | async function | 0.086 | 0.75 | 1,254 |
| javascript:define-callable-method | javascript | define-callable | class or object method | 0.033 | 0.65 | 479 |
| python:define-callable-def | python | define-callable | def | 0.850 | 1.00 | 13,272 |
| python:define-callable-decorated-def | python | define-callable | @decorator def | 0.094 | 0.75 | 1,475 |
| python:define-callable-async-def | python | define-callable | async def | 0.031 | 0.40 | 487 |
| python:define-callable-lambda | python | define-callable | lambda | 0.024 | 0.70 | 376 |
| rust:define-callable-fn | rust | define-callable | fn | 0.659 | 1.00 | 15,134 |
| rust:define-callable-closure | rust | define-callable | |x| ... | 0.233 | 1.00 | 5,345 |
| rust:define-callable-generic-fn | rust | define-callable | fn f<T> | 0.060 | 0.95 | 1,388 |
| rust:define-callable-async-fn | rust | define-callable | async fn | 0.048 | 0.58 | 1,114 |
| typescript:define-callable-arrow | typescript | define-callable | () => {} | 0.573 | 1.00 | 8,254 |
| typescript:define-callable-function-decl | typescript | define-callable | function f() | 0.203 | 1.00 | 2,920 |
| typescript:define-callable-async | typescript | define-callable | async function | 0.122 | 0.85 | 1,760 |
| typescript:define-callable-method | typescript | define-callable | class method | 0.102 | 0.75 | 1,467 |
| go:define-data-type-struct | go | define-data-type | type X struct | 0.722 | 1.00 | 1,207 |
| go:define-data-type-embedded | go | define-data-type | embedded field | 0.106 | 0.53 | 178 |
| go:define-data-type-interface | go | define-data-type | type X interface | 0.087 | 0.58 | 145 |
| go:define-data-type-defined-type | go | define-data-type | type X Y | 0.085 | 0.68 | 142 |
| javascript:define-data-type-object-literal | javascript | define-data-type | { key: value } | 0.897 | 1.00 | 20,407 |
| javascript:define-data-type-jsdoc-type | javascript | define-data-type | @typedef / @param JSDoc | 0.078 | 0.45 | 1,773 |
| javascript:define-data-type-factory-function | javascript | define-data-type | function make...() returning object | 0.022 | 0.40 | 509 |
| javascript:define-data-type-class | javascript | define-data-type | class | 0.003 | 0.50 | 67 |
| python:define-data-type-class | python | define-data-type | class | 0.834 | 1.00 | 2,127 |
| python:define-data-type-dataclass | python | define-data-type | @dataclass | 0.128 | 0.71 | 327 |
| python:define-data-type-enum | python | define-data-type | Enum | 0.031 | 0.47 | 79 |
| python:define-data-type-typed-alias | python | define-data-type | TypeAlias / NamedTuple / TypedDict | 0.007 | 0.29 | 17 |
| rust:define-data-type-struct | rust | define-data-type | struct | 0.550 | 1.00 | 1,843 |
| rust:define-data-type-type-alias | rust | define-data-type | type X = ... | 0.231 | 0.79 | 773 |
| rust:define-data-type-enum | rust | define-data-type | enum | 0.156 | 1.00 | 521 |
| rust:define-data-type-trait | rust | define-data-type | trait | 0.063 | 0.68 | 212 |
| typescript:define-data-type-union-type | typescript | define-data-type | A | B | 0.420 | 1.00 | 3,142 |
| typescript:define-data-type-type-alias | typescript | define-data-type | type X = ... | 0.302 | 0.90 | 2,254 |
| typescript:define-data-type-interface | typescript | define-data-type | interface | 0.250 | 0.85 | 1,868 |
| typescript:define-data-type-class | typescript | define-data-type | class | 0.028 | 0.60 | 211 |
| go:handle-absence-nil-check | go | handle-absence | x == nil | 0.677 | 1.00 | 6,770 |
| go:handle-absence-zero-value | go | handle-absence | explicit zero-value default | 0.148 | 1.00 | 1,481 |
| go:handle-absence-comma-ok | go | handle-absence | v, ok := m[k] | 0.095 | 0.89 | 952 |
| go:handle-absence-pointer-optional | go | handle-absence | *T optional field | 0.080 | 0.68 | 799 |
| javascript:handle-absence-or-default | javascript | handle-absence | a || default | 0.665 | 1.00 | 8,176 |
| javascript:handle-absence-optional-chain | javascript | handle-absence | a?.b | 0.227 | 0.50 | 2,786 |
| javascript:handle-absence-null-check | javascript | handle-absence | x == null / typeof x | 0.065 | 0.70 | 801 |
| javascript:handle-absence-nullish-coalesce | javascript | handle-absence | a ?? b | 0.043 | 0.35 | 532 |
| python:handle-absence-optional-annotation | python | handle-absence | Optional[T] / T | None | 0.404 | 0.70 | 4,263 |
| python:handle-absence-none-check | python | handle-absence | is None / is not None | 0.292 | 0.90 | 3,083 |
| python:handle-absence-get-default | python | handle-absence | dict.get(k, default) | 0.220 | 0.90 | 2,327 |
| python:handle-absence-or-default | python | handle-absence | x or default | 0.084 | 0.80 | 890 |
| rust:handle-absence-option-match | rust | handle-absence | match / if let Some | 0.418 | 1.00 | 1,864 |
| rust:handle-absence-unwrap-or | rust | handle-absence | .unwrap_or / .unwrap_or_else / .unwrap_or_default | 0.341 | 1.00 | 1,524 |
| rust:handle-absence-is-some-none | rust | handle-absence | .is_some() / .is_none() | 0.127 | 0.79 | 565 |
| rust:handle-absence-map-option | rust | handle-absence | .map / .and_then on Option | 0.114 | 0.79 | 511 |
| typescript:handle-absence-null-check | typescript | handle-absence | x == null / x !== undefined | 0.484 | 1.00 | 3,774 |
| typescript:handle-absence-optional-chain | typescript | handle-absence | a?.b | 0.302 | 0.95 | 2,358 |
| typescript:handle-absence-nullish-coalesce | typescript | handle-absence | a ?? b | 0.193 | 0.90 | 1,507 |
| typescript:handle-absence-non-null-assert | typescript | handle-absence | x! | 0.020 | 0.65 | 158 |
| go:handle-error-if-err-nil | go | handle-error | if err != nil | 0.541 | 1.00 | 2,867 |
| go:handle-error-error-construct | go | handle-error | errors.New / fmt.Errorf | 0.353 | 1.00 | 1,868 |
| go:handle-error-panic | go | handle-error | panic / log.Fatal | 0.062 | 0.74 | 328 |
| go:handle-error-error-wrap | go | handle-error | %w wrapping / errors.Is / errors.As | 0.044 | 0.47 | 232 |
| javascript:handle-error-throw | javascript | handle-error | throw | 0.571 | 0.89 | 1,259 |
| javascript:handle-error-try-catch | javascript | handle-error | try/catch | 0.390 | 0.89 | 859 |
| javascript:handle-error-promise-catch | javascript | handle-error | .catch(...) | 0.028 | 0.44 | 61 |
| javascript:handle-error-callback-error | javascript | handle-error | (err, value) callback | 0.012 | 0.56 | 26 |
| python:handle-error-raise | python | handle-error | raise | 0.532 | 0.80 | 4,589 |
| python:handle-error-try-except | python | handle-error | try/except | 0.313 | 0.90 | 2,700 |
| python:handle-error-broad-except | python | handle-error | except Exception / bare except | 0.134 | 0.70 | 1,156 |
| python:handle-error-assert | python | handle-error | assert | 0.022 | 0.50 | 187 |
| rust:handle-error-match-result | rust | handle-error | match on Result | 0.346 | 1.00 | 5,388 |
| rust:handle-error-unwrap-expect | rust | handle-error | .unwrap() / .expect() | 0.322 | 1.00 | 5,015 |
| rust:handle-error-question-mark | rust | handle-error | ? | 0.311 | 1.00 | 4,836 |
| rust:handle-error-panic | rust | handle-error | panic! / unreachable! | 0.021 | 0.74 | 331 |
| typescript:handle-error-throw | typescript | handle-error | throw | 0.463 | 0.95 | 769 |
| typescript:handle-error-try-catch | typescript | handle-error | try/catch | 0.398 | 0.89 | 661 |
| typescript:handle-error-typed-error-result | typescript | handle-error | Result / discriminated error union | 0.083 | 0.26 | 138 |
| typescript:handle-error-promise-catch | typescript | handle-error | .catch(...) | 0.056 | 0.53 | 93 |
| go:iterate-range-for | go | iterate | for i, v := range xs | 0.894 | 1.00 | 2,056 |
| go:iterate-classic-for | go | iterate | for i := 0; ... | 0.056 | 0.84 | 130 |
| go:iterate-condition-for | go | iterate | for cond | 0.031 | 0.58 | 71 |
| go:iterate-infinite-for | go | iterate | for { } | 0.019 | 0.63 | 44 |
| javascript:iterate-for-of | javascript | iterate | for (const x of xs) | 0.438 | 0.78 | 1,014 |
| javascript:iterate-classic-for | javascript | iterate | for (let i = 0; ...) | 0.364 | 0.83 | 842 |
| javascript:iterate-while | javascript | iterate | while | 0.134 | 0.56 | 310 |
| javascript:iterate-foreach-callback | javascript | iterate | .forEach(...) | 0.064 | 0.89 | 147 |
| python:iterate-comprehension | python | iterate | [f(x) for x in xs] | 0.451 | 1.00 | 3,774 |
| python:iterate-for-each | python | iterate | for x in xs | 0.432 | 1.00 | 3,612 |
| python:iterate-range-index | python | iterate | for i in range(...) | 0.088 | 0.65 | 738 |
| python:iterate-while | python | iterate | while | 0.029 | 0.65 | 240 |
| rust:iterate-iterator-chain | rust | iterate | .iter().map(...) | 0.498 | 1.00 | 1,936 |
| rust:iterate-for-in | rust | iterate | for x in xs | 0.430 | 1.00 | 1,672 |
| rust:iterate-while | rust | iterate | while | 0.038 | 0.84 | 147 |
| rust:iterate-loop | rust | iterate | loop | 0.034 | 0.95 | 132 |
| typescript:iterate-for-of | typescript | iterate | for (const x of xs) | 0.673 | 0.89 | 690 |
| typescript:iterate-classic-for | typescript | iterate | for (let i = 0; ...) | 0.129 | 0.78 | 132 |
| typescript:iterate-foreach-callback | typescript | iterate | .forEach(...) | 0.107 | 0.72 | 110 |
| typescript:iterate-while | typescript | iterate | while | 0.091 | 0.61 | 93 |
| go:metaprogram-struct-tag | go | metaprogram | `json:"..."` | 0.941 | 0.95 | 2,829 |
| go:metaprogram-reflection | go | metaprogram | reflect package | 0.048 | 0.26 | 144 |
| go:metaprogram-build-tag | go | metaprogram | //go:build | 0.009 | 0.26 | 27 |
| go:metaprogram-go-generate | go | metaprogram | //go:generate | 0.002 | 0.05 | 5 |
| javascript:metaprogram-dynamic-property | javascript | metaprogram | obj[name] | 0.800 | 1.00 | 4,598 |
| javascript:metaprogram-prototype | javascript | metaprogram | prototype / __proto__ | 0.150 | 0.37 | 861 |
| javascript:metaprogram-reflection | javascript | metaprogram | Object.keys / Reflect / Proxy | 0.048 | 0.68 | 274 |
| javascript:metaprogram-dynamic-exec | javascript | metaprogram | eval / new Function | 0.002 | 0.21 | 13 |
| python:metaprogram-decorator | python | metaprogram | @decorator | 0.561 | 0.88 | 1,884 |
| python:metaprogram-reflection | python | metaprogram | getattr / setattr / hasattr | 0.313 | 0.82 | 1,051 |
| python:metaprogram-dynamic-exec | python | metaprogram | eval / exec / __import__ | 0.109 | 0.82 | 367 |
| python:metaprogram-generic-type | python | metaprogram | TypeVar / Generic | 0.016 | 0.18 | 54 |
| rust:metaprogram-generic-bound | rust | metaprogram | where / impl Trait bounds | 0.565 | 0.95 | 2,863 |
| rust:metaprogram-derive | rust | metaprogram | #[derive(...)] | 0.274 | 1.00 | 1,387 |
| rust:metaprogram-cfg-attr | rust | metaprogram | #[cfg(...)] | 0.132 | 0.58 | 670 |
| rust:metaprogram-macro-rules | rust | metaprogram | macro_rules! | 0.029 | 0.37 | 146 |
| typescript:metaprogram-generic-param | typescript | metaprogram | <T> | 0.414 | 0.94 | 919 |
| typescript:metaprogram-mapped-conditional-type | typescript | metaprogram | keyof / infer / extends ? | 0.348 | 0.78 | 772 |
| typescript:metaprogram-reflection | typescript | metaprogram | Object.keys / Reflect / Proxy | 0.195 | 0.94 | 433 |
| typescript:metaprogram-decorator | typescript | metaprogram | @decorator | 0.044 | 0.06 | 97 |
| go:modularize-import | go | modularize | import | 0.451 | 1.00 | 3,689 |
| go:modularize-exported-name | go | modularize | exported identifier | 0.330 | 1.00 | 2,699 |
| go:modularize-package-decl | go | modularize | package | 0.136 | 1.00 | 1,115 |
| go:modularize-internal-pkg | go | modularize | internal/ package path | 0.084 | 0.58 | 684 |
| javascript:modularize-import | javascript | modularize | import x from | 0.426 | 0.65 | 3,597 |
| javascript:modularize-named-export | javascript | modularize | export const/function | 0.396 | 0.50 | 3,340 |
| javascript:modularize-commonjs | javascript | modularize | require / module.exports | 0.163 | 0.85 | 1,374 |
| javascript:modularize-dynamic-import | javascript | modularize | import(...) | 0.015 | 0.20 | 128 |
| python:modularize-from-import | python | modularize | from x import y | 0.641 | 0.95 | 8,103 |
| python:modularize-import | python | modularize | import x | 0.307 | 1.00 | 3,880 |
| python:modularize-relative-import | python | modularize | from . import y | 0.051 | 0.70 | 640 |
| python:modularize-wildcard-import | python | modularize | from x import * | 0.002 | 0.10 | 25 |
| rust:modularize-pub | rust | modularize | pub | 0.619 | 1.00 | 11,523 |
| rust:modularize-use | rust | modularize | use | 0.280 | 1.00 | 5,220 |
| rust:modularize-mod | rust | modularize | mod | 0.078 | 1.00 | 1,444 |
| rust:modularize-re-export | rust | modularize | pub use | 0.024 | 0.74 | 440 |
| typescript:modularize-import | typescript | modularize | import x from | 0.501 | 1.00 | 8,924 |
| typescript:modularize-named-export | typescript | modularize | export const/function | 0.467 | 0.95 | 8,333 |
| typescript:modularize-default-export | typescript | modularize | export default | 0.030 | 0.85 | 539 |
| typescript:modularize-dynamic-import | typescript | modularize | import(...) / require | 0.002 | 0.50 | 29 |
| go:test-and-verify-t-error-fatal | go | test-and-verify | t.Error / t.Fatal | 0.661 | 0.94 | 4,708 |
| go:test-and-verify-test-func | go | test-and-verify | func TestX(t *testing.T) | 0.221 | 1.00 | 1,575 |
| go:test-and-verify-table-test | go | test-and-verify | table-driven test | 0.109 | 0.71 | 778 |
| go:test-and-verify-benchmark | go | test-and-verify | func BenchmarkX | 0.009 | 0.24 | 62 |
| javascript:test-and-verify-expect-assert | javascript | test-and-verify | expect(...) | 0.527 | 0.56 | 3,914 |
| javascript:test-and-verify-test-function | javascript | test-and-verify | it / test | 0.460 | 1.00 | 3,416 |
| javascript:test-and-verify-describe-group | javascript | test-and-verify | describe | 0.010 | 0.38 | 73 |
| javascript:test-and-verify-mock | javascript | test-and-verify | jest.mock / sinon | 0.004 | 0.12 | 27 |
| python:test-and-verify-assert-in-test | python | test-and-verify | assert in test file | 0.537 | 0.88 | 25,066 |
| python:test-and-verify-test-function | python | test-and-verify | def test_* | 0.281 | 1.00 | 13,111 |
| python:test-and-verify-mock | python | test-and-verify | unittest.mock / monkeypatch | 0.165 | 0.65 | 7,706 |
| python:test-and-verify-fixture | python | test-and-verify | @pytest.fixture / setUp | 0.018 | 0.71 | 823 |
| rust:test-and-verify-assert-macro | rust | test-and-verify | assert! / assert_eq! | 0.632 | 1.00 | 9,629 |
| rust:test-and-verify-test-attr | rust | test-and-verify | #[test] | 0.306 | 1.00 | 4,665 |
| rust:test-and-verify-doc-test | rust | test-and-verify | /// ``` example | 0.031 | 0.47 | 479 |
| rust:test-and-verify-cfg-test-mod | rust | test-and-verify | #[cfg(test)] mod tests | 0.031 | 1.00 | 474 |
| typescript:test-and-verify-expect-assert | typescript | test-and-verify | expect(...) | 0.544 | 0.83 | 11,930 |
| typescript:test-and-verify-test-function | typescript | test-and-verify | it / test | 0.310 | 1.00 | 6,807 |
| typescript:test-and-verify-mock | typescript | test-and-verify | jest.mock / vi.mock | 0.084 | 0.50 | 1,854 |
| typescript:test-and-verify-describe-group | typescript | test-and-verify | describe | 0.062 | 0.83 | 1,356 |
| go:transform-collection-append-loop | go | transform-collection | for ... append(...) | 0.567 | 1.00 | 1,421 |
| go:transform-collection-map-index | go | transform-collection | m[k] = v | 0.354 | 0.89 | 887 |
| go:transform-collection-slices-maps-pkg | go | transform-collection | slices / maps package helpers | 0.045 | 0.37 | 112 |
| go:transform-collection-sort-call | go | transform-collection | sort.Slice / slices.Sort | 0.034 | 0.47 | 84 |
| javascript:transform-collection-spread | javascript | transform-collection | [...xs] / {...o} | 0.439 | 0.83 | 1,482 |
| javascript:transform-collection-map | javascript | transform-collection | .map(...) | 0.324 | 0.89 | 1,094 |
| javascript:transform-collection-filter | javascript | transform-collection | .filter(...) | 0.210 | 0.78 | 707 |
| javascript:transform-collection-reduce | javascript | transform-collection | .reduce(...) | 0.027 | 0.50 | 90 |
| python:transform-collection-comprehension-transform | python | transform-collection | comprehension | 0.466 | 1.00 | 3,774 |
| python:transform-collection-aggregate | python | transform-collection | sum / any / all / min / max | 0.258 | 0.65 | 2,085 |
| python:transform-collection-append-loop | python | transform-collection | loop + append | 0.172 | 0.95 | 1,388 |
| python:transform-collection-builtin-hof | python | transform-collection | map / filter / sorted | 0.104 | 0.80 | 844 |
| rust:transform-collection-iter-map | rust | transform-collection | .map(...) | 0.566 | 1.00 | 1,619 |
| rust:transform-collection-collect | rust | transform-collection | .collect() | 0.323 | 1.00 | 924 |
| rust:transform-collection-iter-filter | rust | transform-collection | .filter(...) | 0.075 | 0.68 | 214 |
| rust:transform-collection-fold-sum | rust | transform-collection | .fold / .sum / .reduce | 0.036 | 0.58 | 103 |
| typescript:transform-collection-spread | typescript | transform-collection | [...xs] / {...o} | 0.641 | 1.00 | 2,334 |
| typescript:transform-collection-map | typescript | transform-collection | .map(...) | 0.238 | 0.95 | 866 |
| typescript:transform-collection-filter | typescript | transform-collection | .filter(...) | 0.098 | 0.85 | 355 |
| typescript:transform-collection-reduce | typescript | transform-collection | .reduce(...) | 0.023 | 0.55 | 85 |

### Weighting views (balanced prevalence by weighting rule)

| Operation | Equal language | Equal domain | Equal stratum | Total-weighted sites / 1k tokens |
| --- | --- | --- | --- | --- |
| bind-value | 1.00 | 1.00 | 1.00 | 21.62 |
| define-callable | 1.00 | 1.00 | 1.00 | 11.44 |
| modularize | 1.00 | 1.00 | 1.00 | 9.72 |
| branch | 1.00 | 1.00 | 1.00 | 10.78 |
| handle-absence | 1.00 | 1.00 | 1.00 | 6.67 |
| test-and-verify | 0.87 | 0.83 | 0.87 | 9.46 |
| handle-error | 0.97 | 0.95 | 0.97 | 4.94 |
| define-data-type | 0.97 | 0.97 | 0.97 | 5.59 |
| metaprogram | 0.94 | 0.95 | 0.94 | 2.87 |
| transform-collection | 0.98 | 0.97 | 0.98 | 3.03 |
| iterate | 0.96 | 0.98 | 0.96 | 2.65 |
| concurrency | 0.78 | 0.76 | 0.77 | 1.79 |

### Leave-one-out rank stability

| Dropped slice | Largest rank shift among the twelve operations |
| --- | --- |
| without-go | 2 |
| without-javascript | 2 |
| without-python | 2 |
| without-rust | 2 |
| without-typescript | 2 |
| without-domain-cli-automation | 1 |
| without-domain-data-science | 0 |
| without-domain-devops-infrastructure | 1 |
| without-domain-games | 1 |
| without-domain-libraries | 1 |
| without-domain-networking | 1 |
| without-domain-systems | 1 |
| without-domain-web-backend | 1 |
| without-domain-web-frontend | 0 |

### Priority index components

| Operation | Frequency | Friction | Audience | Confidence | Priority |
| --- | --- | --- | --- | --- | --- |
| transform-collection | 0.98 | 0.31 | 1.05 | Moderate | 23.7 |
| bind-value | 1.00 | 0.17 | 1.15 | Moderate | 14.4 |
| define-callable | 1.00 | 0.14 | 1.15 | Moderate | 12.3 |
| branch | 1.00 | 0.10 | 1.15 | Moderate | 8.6 |
| define-data-type | 0.97 | 0.10 | 1.05 | Moderate | 7.6 |
| iterate | 0.96 | 0.00 | 1.15 | Weak | 0.0 |
| handle-error | 0.97 | 0.00 | 1.05 | Moderate | 0.0 |
| handle-absence | 1.00 | 0.00 | 1.05 | Moderate | 0.0 |
| concurrency | 0.78 | 0.00 | 1.00 | Weak | 0.0 |
| modularize | 1.00 | 0.00 | 1.15 | Weak | 0.0 |
| test-and-verify | 0.87 | 0.00 | 1.05 | Weak | 0.0 |
| metaprogram | 0.94 | 0.00 | 1.00 | Weak | 0.0 |

### Coverage matrix (language x domain x stratum)

| Cell | Status | Project | Production tokens |
| --- | --- | --- | --- |
| go.cli-automation.education | included | tamnd/jsinfo-cli | 4,494 |
| go.cli-automation.mature-oss | included | Tenderly/tenderly-cli | 65,448 |
| go.cli-automation.one-off | included | ivuorinen/gibidify | 17,923 |
| go.cli-automation.production | included | wagoodman/dive | 42,118 |
| go.cli-automation.small-lib-app | included | suzuki-shunsuke/ghir | 2,379 |
| go.devops-infrastructure.education | included | forrestIsRunning/asynq-setup | 5,111 |
| go.devops-infrastructure.mature-oss | included | grafana/k6-operator | 29,998 |
| go.devops-infrastructure.one-off | included | kadeksuryam/pandu | 202,742 |
| go.devops-infrastructure.production | included | quay/clair | 72,122 |
| go.devops-infrastructure.small-lib-app | included | sapcc/keppel | 126,442 |
| go.networking.education | unavailable | none | 0 |
| go.networking.mature-oss | included | naggie/dsnet | 12,539 |
| go.networking.one-off | included | Slashas632/goPort | 4,062 |
| go.networking.production | included | tidwall/evio | 8,835 |
| go.networking.small-lib-app | included | FrontierTM/Pantegnos | 92,438 |
| go.web-backend.education | included | NDXDeveloper/go-rest-api-mariadb-sans-orm | 31,049 |
| go.web-backend.mature-oss | included | goadesign/goa | 262,102 |
| go.web-backend.one-off | included | tickerdb/tickerdb-go | 5,594 |
| go.web-backend.production | included | gothinkster/golang-gin-realworld-example-app | 10,200 |
| go.web-backend.small-lib-app | included | dofusdude/doduda | 38,468 |
| javascript.cli-automation.education | included | crllect/ProxDocs | 10,936 |
| javascript.cli-automation.mature-oss | included | fastify/fastify-cli | 6,100 |
| javascript.cli-automation.one-off | included | khasky/web-reactions-verifier | 4,576 |
| javascript.cli-automation.production | included | terkelg/prompts | 11,641 |
| javascript.cli-automation.small-lib-app | included | rexleimo/harness-cli | 322,673 |
| javascript.games.education | included | rafaelcastrocouto/P2P-Web-Game-Tutorial | 8,393 |
| javascript.games.mature-oss | included | MattSurabian/DuckHunt-JS | 8,178 |
| javascript.games.one-off | included | DontFretBrett/towersofhanoi-3d | 7,328 |
| javascript.games.production | included | hiloteam/Hilo | 320,115 |
| javascript.games.small-lib-app | included | react-puzzle-games/15-puzzle | 2,512 |
| javascript.web-backend.education | included | JeanCaicedo/Graphql | 9,968 |
| javascript.web-backend.mature-oss | included | mdn/express-locallibrary-tutorial | 6,358 |
| javascript.web-backend.one-off | included | bensblueprints/Link-Leaf-mvp | 20,244 |
| javascript.web-backend.production | included | CodeGenieApp/serverless-express | 8,292 |
| javascript.web-backend.small-lib-app | included | iammelvink/react-complete-e-commerce | 18,273 |
| javascript.web-frontend.education | included | a2rp/react-concept-jsx-essentials | 4,475 |
| javascript.web-frontend.mature-oss | included | baidu/san | 47,890 |
| javascript.web-frontend.one-off | included | selcanakturk/EduQA_frontend | 17,220 |
| javascript.web-frontend.production | included | mdbootstrap/material-design-for-bootstrap | 210,558 |
| javascript.web-frontend.small-lib-app | included | KwokKwok/Silo | 30,317 |
| python.cli-automation.education | included | egouilliard-leyton/python-tutor-skill | 4,269 |
| python.cli-automation.mature-oss | included | earwig/git-repo-updater | 3,424 |
| python.cli-automation.one-off | included | jakegold1647/sam-doctor | 20,649 |
| python.cli-automation.production | included | jdepoix/youtube-transcript-api | 5,322 |
| python.cli-automation.small-lib-app | included | douglasmonsky/codex-usage-tracker | 373,721 |
| python.data-science.education | included | hmzainjamil/ai-engineering-from-scratch | 249,532 |
| python.data-science.mature-oss | included | sdv-dev/Copulas | 18,891 |
| python.data-science.one-off | included | justinbrianhwang/FALCON | 46,076 |
| python.data-science.production | included | triton-inference-server/server | 188,864 |
| python.data-science.small-lib-app | included | ultralytics/mnist | 8,063 |
| python.devops-infrastructure.education | included | spinov001-art/python-data-scripts | 2,552 |
| python.devops-infrastructure.mature-oss | included | cobrateam/splinter | 17,352 |
| python.devops-infrastructure.one-off | included | mairhythmhoon/email-bot | 5,075 |
| python.devops-infrastructure.production | included | StackStorm/st2 | 344,666 |
| python.devops-infrastructure.small-lib-app | included | jefftriplett/dotfiles | 9,338 |
| python.web-backend.education | included | venkateshTechmates/AIPython_Tutorial | 33,666 |
| python.web-backend.mature-oss | included | slackapi/bolt-python | 86,101 |
| python.web-backend.one-off | included | nexifyai-dev/nexify-agentur-plattform | 283,172 |
| python.web-backend.production | included | fastapi/sqlmodel | 49,627 |
| python.web-backend.small-lib-app | included | presidio-v/presidio-hardened-fastapi | 3,829 |
| rust.cli-automation.education | included | dmelim/learn-rust-by-building | 17,399 |
| rust.cli-automation.mature-oss | included | pamburus/hl | 371,656 |
| rust.cli-automation.one-off | included | user137/uacrypt | 160,649 |
| rust.cli-automation.production | included | kdheepak/taskwarrior-tui | 86,473 |
| rust.cli-automation.small-lib-app | included | a-chacon/procman | 7,748 |
| rust.libraries.education | included | Toperythroblast876/omem | 143,099 |
| rust.libraries.mature-oss | included | Boddlnagg/midir | 28,035 |
| rust.libraries.one-off | included | bounded-systems/git-ast | 28,057 |
| rust.libraries.production | included | jonhoo/fantoccini | 34,083 |
| rust.libraries.small-lib-app | included | djc/gcp_auth | 7,394 |
| rust.systems.education | included | washimimizuku/rust-tutorials | 31,286 |
| rust.systems.mature-oss | unavailable | none | 0 |
| rust.systems.one-off | included | CanadianCowboy/a2x | 226,305 |
| rust.systems.production | included | tnballo/high-assurance-rust | 44,450 |
| rust.systems.small-lib-app | included | weizhiao/Relink | 286,032 |
| rust.web-backend.education | included | 0xdea/zero2prod | 13,288 |
| rust.web-backend.mature-oss | included | Isona/dirble | 23,607 |
| rust.web-backend.one-off | included | Shyam-Chen/Rust-Journey | 7,104 |
| rust.web-backend.production | included | graphql-rust/juniper | 357,768 |
| rust.web-backend.small-lib-app | included | ppmpreetham/fastrapi | 76,089 |
| typescript.cli-automation.education | included | deathlegionteamlk/legion-hutta | 90,206 |
| typescript.cli-automation.mature-oss | included | listr2/listr2 | 17,102 |
| typescript.cli-automation.one-off | included | elberacasa/umbra | 20,024 |
| typescript.cli-automation.production | included | qawolf/cli | 73,009 |
| typescript.cli-automation.small-lib-app | included | ouijit/ouijit | 146,566 |
| typescript.libraries.education | included | jevonhou/exam-loop-core | 6,344 |
| typescript.libraries.mature-oss | included | mdbootstrap/mdb-react-ui-kit | 25,239 |
| typescript.libraries.one-off | included | codiume/hooks | 2,256 |
| typescript.libraries.production | included | intlify/vue-i18n | 48,321 |
| typescript.libraries.small-lib-app | included | rottenronin/cresh-ui | 39,142 |
| typescript.web-backend.education | included | didinj/node-express-mongodb-reactjs-graphql | 2,611 |
| typescript.web-backend.mature-oss | included | 0xb4lamx/nestjs-boilerplate-microservice | 6,815 |
| typescript.web-backend.one-off | included | SrjAdhikari/Manakuru | 68,201 |
| typescript.web-backend.production | included | yagop/node-telegram-bot-api | 39,121 |
| typescript.web-backend.small-lib-app | included | neynarxyz/nodejs-sdk | 136,055 |
| typescript.web-frontend.education | included | peckem/OpTeamUs | 27,150 |
| typescript.web-frontend.mature-oss | included | Bowen7/regex-vis | 36,193 |
| typescript.web-frontend.one-off | included | Chris0Jeky/developer-lens | 67,130 |
| typescript.web-frontend.production | included | vadimdemedes/ink | 35,187 |
| typescript.web-frontend.small-lib-app | included | Weaverse/weaverse | 58,736 |

</details>

## Methods and provenance

**Frozen method.** The run pinned SKILL.md, method.md, report-template.md, ontology.md,
checkpoint.py, and aggregate.py by SHA-256 at initialization, and validated with zero errors at
result-set digest `f04c423c93c90ab418f7a33c67b9744d9c8f1bc8d32ec3940e07f9d37394384d`.

**Catalogs.** Five catalogs were frozen before collection: `python-3.13`, `typescript-5`,
`javascript-es2025`, `rust-2021`, `go-1.23`. They inventory 617 official specification sections: 280
mapped to measurements, 337 unmatched with a stated reason. Each carries 60 measurement keys, 300 in
total, every key mapped from at least one official section, and the section counts reconcile. Five
separate agents built the inventories; review was as stated above.

**Sampling frame.** One cell is a language, a domain, and a stratum: five languages, four domains
each, five strata each, so one hundred cells. Each cell recorded a frozen GitHub search query, its
retrieval time, and its full candidate count, then ordered candidates by
`SHA-256(seed + canonical source id)` with the seed `2026-08-04`. Collection walked that order and
took the first candidate that cloned and held at least five parsed files and 2,000 production
tokens. Rejections are recorded per cell with a reason, and every canonical source identity is
capped at one project. Queries excluded forks, archived repositories, and repositories over 60 MB,
and required a push after 2025-02-01. Three cells needed the recorded fallback query.

**Measurement model.** An operation row counts its sites against normalized lexical tokens in its
scope. A surface row counts its sites against its parent operation's total, so surface shares sum
inside their operation by construction. The `test-and-verify` rows carry a test-inclusive scope;
every other row excludes test files. Generated, vendored, example, benchmark, and documentation
directories are excluded from all scopes.

**Friction.** Eight equivalent tasks were written in all six languages and counted with one
tokenizer. Every Jet snippet was compiled and run with the current `target/debug/jet` binary before
it was counted; two were corrected after the compiler rejected the first draft. A shorter peer form
counts as a baseline only when it does the same job, so the propagation and absence tasks exclude
peers that skip the work.

**Formulas.** `frequency = 0.80 × balanced prevalence + 0.20 × breadth`, with the opportunity-share
weight redistributed as stated. `friction` is the mean of positive normalized cost gaps.
`priority = 100 × frequency × friction × audience × confidence`. Audience factors are 1.15 for
entry, 1.05 for general, and 1.00 for expert. Confidence factors are 1.00, 0.75, and 0.50.

**Scanner defects found and fixed.** Nine detection rules were corrected in two rounds. Every
affected project was re-fetched at its recorded commit pin and re-measured.

A self-check found two rules that could never fire: the Rust doc-test rule and the Go import rule
both matched after comments and string bodies are blanked. Both now read the original source, which
raised the median density of "split and reuse code units" from 10.12 to 11.06 and moved it from
fourth to third.

The review found six rules that counted something other than their label. The Rust absence rule
counted every `Some` and `None` identifier, including type positions; corrected to real match arms
and if-let sites, its share fell from 0.71 to 0.42. The Go zero-value rule counted every `make(` and
`new(`. The Go map-index rule fired on the `map[...]` type syntax. Go methods were also counted as
function literals. The Python `or` rule counted boolean tests rather than value fallbacks, and its
share fell from 0.29 to 0.08. The `.get(k, default)` rule counted HTTP client calls, and the Rust
`is_some` rule included `is_empty`. These changed shares in the absence, transform, and callable
tables, and changed no recommendation or priority number. Two surfaces still record zero uses
corpus-wide, and both are real: Python's `match` and the TypeScript `Worker` constructor.

**Review procedure.** An independent fresh-context reviewer returned 21 findings: six high, ten
medium, five low. It confirmed that all 240 surface rows, all 12 operation rows, the weighting,
priority, and coverage tables, the denominator model, every Tower citation, and every Jet example
claim reconcile with the data. It rejected the stability section, two Jet cost claims, and six
detection rules. All 21 findings were acted on. The reviewer read an earlier snapshot, so its stated
counts for the corrected surfaces describe the pre-fix state. Every Jet claim here was checked
against the current compiler or a named repository file.

<details>
<summary>Corpus manifest</summary>

### Corpus manifest

| Cell | Source | Commit pin | License | Stratum | Domain | Stars | Retrieved |
| --- | --- | --- | --- | --- | --- | --- | --- |
| go.cli-automation.education | [tamnd/jsinfo-cli](https://github.com/tamnd/jsinfo-cli) | `38e0227ba0b1` | Apache-2.0 | education | cli-automation | 0 | 2026-08-04 |
| go.cli-automation.mature-oss | [Tenderly/tenderly-cli](https://github.com/Tenderly/tenderly-cli) | `dd8f86c9dbaa` | GPL-3.0 | mature-oss | cli-automation | 586 | 2026-08-04 |
| go.cli-automation.one-off | [ivuorinen/gibidify](https://github.com/ivuorinen/gibidify) | `1f48f3f2b57e` | MIT | one-off | cli-automation | 1 | 2026-08-04 |
| go.cli-automation.production | [wagoodman/dive](https://github.com/wagoodman/dive) | `d6c691947f8f` | MIT | production | cli-automation | 54412 | 2026-08-04 |
| go.cli-automation.small-lib-app | [suzuki-shunsuke/ghir](https://github.com/suzuki-shunsuke/ghir) | `4f82dfbc0891` | MIT | small-lib-app | cli-automation | 35 | 2026-08-04 |
| go.devops-infrastructure.education | [forrestIsRunning/asynq-setup](https://github.com/forrestIsRunning/asynq-setup) | `78542aabfd2a` | NOASSERTION | education | devops-infrastructure | 0 | 2026-08-04 |
| go.devops-infrastructure.mature-oss | [grafana/k6-operator](https://github.com/grafana/k6-operator) | `8f42d4fb3cb3` | Apache-2.0 | mature-oss | devops-infrastructure | 793 | 2026-08-04 |
| go.devops-infrastructure.one-off | [kadeksuryam/pandu](https://github.com/kadeksuryam/pandu) | `da474fe9a58c` | NOASSERTION | one-off | devops-infrastructure | 0 | 2026-08-04 |
| go.devops-infrastructure.production | [quay/clair](https://github.com/quay/clair) | `3953837d2f51` | Apache-2.0 | production | devops-infrastructure | 11038 | 2026-08-04 |
| go.devops-infrastructure.small-lib-app | [sapcc/keppel](https://github.com/sapcc/keppel) | `a1de529451e8` | Apache-2.0 | small-lib-app | devops-infrastructure | 123 | 2026-08-04 |
| go.networking.education | none | none | none | education | networking | 0 | none |
| go.networking.mature-oss | [naggie/dsnet](https://github.com/naggie/dsnet) | `d5eca4e18b93` | MIT | mature-oss | networking | 754 | 2026-08-04 |
| go.networking.one-off | [Slashas632/goPort](https://github.com/Slashas632/goPort) | `16407d616d16` | MIT | one-off | networking | 2 | 2026-08-04 |
| go.networking.production | [tidwall/evio](https://github.com/tidwall/evio) | `6dff809d85b7` | MIT | production | networking | 6041 | 2026-08-04 |
| go.networking.small-lib-app | [FrontierTM/Pantegnos](https://github.com/FrontierTM/Pantegnos) | `affe70b2a113` | MIT | small-lib-app | networking | 31 | 2026-08-04 |
| go.web-backend.education | [NDXDeveloper/go-rest-api-mariadb-sans-orm](https://github.com/NDXDeveloper/go-rest-api-mariadb-sans-orm) | `f2f9ebb6b2d4` | MIT | education | web-backend | 0 | 2026-08-04 |
| go.web-backend.mature-oss | [goadesign/goa](https://github.com/goadesign/goa) | `86484276fd41` | MIT | mature-oss | web-backend | 6090 | 2026-08-04 |
| go.web-backend.one-off | [tickerdb/tickerdb-go](https://github.com/tickerdb/tickerdb-go) | `ee5fc039338b` | NOASSERTION | one-off | web-backend | 1 | 2026-08-04 |
| go.web-backend.production | [gothinkster/golang-gin-realworld-example-app](https://github.com/gothinkster/golang-gin-realworld-example-app) | `626c372d2594` | MIT | production | web-backend | 2705 | 2026-08-04 |
| go.web-backend.small-lib-app | [dofusdude/doduda](https://github.com/dofusdude/doduda) | `41d1ae7ec20d` | GPL-3.0 | small-lib-app | web-backend | 49 | 2026-08-04 |
| javascript.cli-automation.education | [crllect/ProxDocs](https://github.com/crllect/ProxDocs) | `fcf9d303d674` | AGPL-3.0 | education | cli-automation | 6 | 2026-08-04 |
| javascript.cli-automation.mature-oss | [fastify/fastify-cli](https://github.com/fastify/fastify-cli) | `35a26f7e612c` | MIT | mature-oss | cli-automation | 730 | 2026-08-04 |
| javascript.cli-automation.one-off | [khasky/web-reactions-verifier](https://github.com/khasky/web-reactions-verifier) | `efac727eac85` | GPL-3.0 | one-off | cli-automation | 0 | 2026-08-04 |
| javascript.cli-automation.production | [terkelg/prompts](https://github.com/terkelg/prompts) | `58771d2911fc` | MIT | production | cli-automation | 9309 | 2026-08-04 |
| javascript.cli-automation.small-lib-app | [rexleimo/harness-cli](https://github.com/rexleimo/harness-cli) | `3f2de857c230` | NOASSERTION | small-lib-app | cli-automation | 49 | 2026-08-04 |
| javascript.games.education | [rafaelcastrocouto/P2P-Web-Game-Tutorial](https://github.com/rafaelcastrocouto/P2P-Web-Game-Tutorial) | `3eb0944a951a` | NOASSERTION | education | games | 3 | 2026-08-04 |
| javascript.games.mature-oss | [MattSurabian/DuckHunt-JS](https://github.com/MattSurabian/DuckHunt-JS) | `5a28db7442eb` | MIT | mature-oss | games | 630 | 2026-08-04 |
| javascript.games.one-off | [DontFretBrett/towersofhanoi-3d](https://github.com/DontFretBrett/towersofhanoi-3d) | `adb891f85b0b` | NOASSERTION | one-off | games | 1 | 2026-08-04 |
| javascript.games.production | [hiloteam/Hilo](https://github.com/hiloteam/Hilo) | `807eecdececb` | MIT | production | games | 5936 | 2026-08-04 |
| javascript.games.small-lib-app | [react-puzzle-games/15-puzzle](https://github.com/react-puzzle-games/15-puzzle) | `d9027cf99188` | MIT | small-lib-app | games | 51 | 2026-08-04 |
| javascript.web-backend.education | [JeanCaicedo/Graphql](https://github.com/JeanCaicedo/Graphql) | `6b07257c1daa` | MIT | education | web-backend | 0 | 2026-08-04 |
| javascript.web-backend.mature-oss | [mdn/express-locallibrary-tutorial](https://github.com/mdn/express-locallibrary-tutorial) | `b66865ca458c` | CC0-1.0 | mature-oss | web-backend | 1275 | 2026-08-04 |
| javascript.web-backend.one-off | [bensblueprints/Link-Leaf-mvp](https://github.com/bensblueprints/Link-Leaf-mvp) | `aa3dd66c1b78` | MIT | one-off | web-backend | 1 | 2026-08-04 |
| javascript.web-backend.production | [CodeGenieApp/serverless-express](https://github.com/CodeGenieApp/serverless-express) | `4205db8998b3` | Apache-2.0 | production | web-backend | 5262 | 2026-08-04 |
| javascript.web-backend.small-lib-app | [iammelvink/react-complete-e-commerce](https://github.com/iammelvink/react-complete-e-commerce) | `8eb0eac5bfcd` | GPL-2.0 | small-lib-app | web-backend | 44 | 2026-08-04 |
| javascript.web-frontend.education | [a2rp/react-concept-jsx-essentials](https://github.com/a2rp/react-concept-jsx-essentials) | `a1844163e602` | NOASSERTION | education | web-frontend | 0 | 2026-08-04 |
| javascript.web-frontend.mature-oss | [baidu/san](https://github.com/baidu/san) | `7818ad38bda7` | MIT | mature-oss | web-frontend | 4740 | 2026-08-04 |
| javascript.web-frontend.one-off | [selcanakturk/EduQA_frontend](https://github.com/selcanakturk/EduQA_frontend) | `898923221ba1` | NOASSERTION | one-off | web-frontend | 0 | 2026-08-04 |
| javascript.web-frontend.production | [mdbootstrap/material-design-for-bootstrap](https://github.com/mdbootstrap/material-design-for-bootstrap) | `b9fce595296f` | MIT | production | web-frontend | 9252 | 2026-08-04 |
| javascript.web-frontend.small-lib-app | [KwokKwok/Silo](https://github.com/KwokKwok/Silo) | `552127d67211` | MIT | small-lib-app | web-frontend | 257 | 2026-08-04 |
| python.cli-automation.education | [egouilliard-leyton/python-tutor-skill](https://github.com/egouilliard-leyton/python-tutor-skill) | `1b699e212510` | MIT | education | cli-automation | 1 | 2026-08-04 |
| python.cli-automation.mature-oss | [earwig/git-repo-updater](https://github.com/earwig/git-repo-updater) | `fb0275bbfded` | MIT | mature-oss | cli-automation | 837 | 2026-08-04 |
| python.cli-automation.one-off | [jakegold1647/sam-doctor](https://github.com/jakegold1647/sam-doctor) | `6f44b66c18b9` | MIT | one-off | cli-automation | 3 | 2026-08-04 |
| python.cli-automation.production | [jdepoix/youtube-transcript-api](https://github.com/jdepoix/youtube-transcript-api) | `72d79711ec4d` | MIT | production | cli-automation | 7990 | 2026-08-04 |
| python.cli-automation.small-lib-app | [douglasmonsky/codex-usage-tracker](https://github.com/douglasmonsky/codex-usage-tracker) | `a152c7558281` | MIT | small-lib-app | cli-automation | 192 | 2026-08-04 |
| python.data-science.education | [hmzainjamil/ai-engineering-from-scratch](https://github.com/hmzainjamil/ai-engineering-from-scratch) | `e689a74468dd` | MIT | education | data-science | 0 | 2026-08-04 |
| python.data-science.mature-oss | [sdv-dev/Copulas](https://github.com/sdv-dev/Copulas) | `83a004b63323` | NOASSERTION | mature-oss | data-science | 648 | 2026-08-04 |
| python.data-science.one-off | [justinbrianhwang/FALCON](https://github.com/justinbrianhwang/FALCON) | `b624627966a2` | NOASSERTION | one-off | data-science | 0 | 2026-08-04 |
| python.data-science.production | [triton-inference-server/server](https://github.com/triton-inference-server/server) | `dde37f5a1360` | BSD-3-Clause | production | data-science | 10904 | 2026-08-04 |
| python.data-science.small-lib-app | [ultralytics/mnist](https://github.com/ultralytics/mnist) | `88a8f0a2d1c9` | AGPL-3.0 | small-lib-app | data-science | 63 | 2026-08-04 |
| python.devops-infrastructure.education | [spinov001-art/python-data-scripts](https://github.com/spinov001-art/python-data-scripts) | `a5e7d4387c68` | NOASSERTION | education | devops-infrastructure | 0 | 2026-08-04 |
| python.devops-infrastructure.mature-oss | [cobrateam/splinter](https://github.com/cobrateam/splinter) | `861539107381` | BSD-3-Clause | mature-oss | devops-infrastructure | 2751 | 2026-08-04 |
| python.devops-infrastructure.one-off | [mairhythmhoon/email-bot](https://github.com/mairhythmhoon/email-bot) | `8b810bc9b3bb` | MIT | one-off | devops-infrastructure | 1 | 2026-08-04 |
| python.devops-infrastructure.production | [StackStorm/st2](https://github.com/StackStorm/st2) | `a3ad2a4a89fe` | Apache-2.0 | production | devops-infrastructure | 6514 | 2026-08-04 |
| python.devops-infrastructure.small-lib-app | [jefftriplett/dotfiles](https://github.com/jefftriplett/dotfiles) | `1318538d1caf` | BSD-3-Clause | small-lib-app | devops-infrastructure | 63 | 2026-08-04 |
| python.web-backend.education | [venkateshTechmates/AIPython_Tutorial](https://github.com/venkateshTechmates/AIPython_Tutorial) | `80cdec50b291` | NOASSERTION | education | web-backend | 0 | 2026-08-04 |
| python.web-backend.mature-oss | [slackapi/bolt-python](https://github.com/slackapi/bolt-python) | `2572efb6550b` | MIT | mature-oss | web-backend | 1319 | 2026-08-04 |
| python.web-backend.one-off | [nexifyai-dev/nexify-agentur-plattform](https://github.com/nexifyai-dev/nexify-agentur-plattform) | `07bc3a94bf45` | NOASSERTION | one-off | web-backend | 0 | 2026-08-04 |
| python.web-backend.production | [fastapi/sqlmodel](https://github.com/fastapi/sqlmodel) | `d9cebbf914d9` | MIT | production | web-backend | 18241 | 2026-08-04 |
| python.web-backend.small-lib-app | [presidio-v/presidio-hardened-fastapi](https://github.com/presidio-v/presidio-hardened-fastapi) | `371fea4e2739` | MIT | small-lib-app | web-backend | 56 | 2026-08-04 |
| rust.cli-automation.education | [dmelim/learn-rust-by-building](https://github.com/dmelim/learn-rust-by-building) | `f95f4e3ba513` | NOASSERTION | education | cli-automation | 0 | 2026-08-04 |
| rust.cli-automation.mature-oss | [pamburus/hl](https://github.com/pamburus/hl) | `d840e32d43aa` | MIT | mature-oss | cli-automation | 3236 | 2026-08-04 |
| rust.cli-automation.one-off | [user137/uacrypt](https://github.com/user137/uacrypt) | `01eb6b308898` | Apache-2.0 | one-off | cli-automation | 0 | 2026-08-04 |
| rust.cli-automation.production | [kdheepak/taskwarrior-tui](https://github.com/kdheepak/taskwarrior-tui) | `ecbcda62d420` | MIT | production | cli-automation | 2098 | 2026-08-04 |
| rust.cli-automation.small-lib-app | [a-chacon/procman](https://github.com/a-chacon/procman) | `7257d274a407` | GPL-3.0 | small-lib-app | cli-automation | 46 | 2026-08-04 |
| rust.libraries.education | [Toperythroblast876/omem](https://github.com/Toperythroblast876/omem) | `9c175670c1cd` | NOASSERTION | education | libraries | 0 | 2026-08-04 |
| rust.libraries.mature-oss | [Boddlnagg/midir](https://github.com/Boddlnagg/midir) | `e5a60b551de6` | MIT | mature-oss | libraries | 819 | 2026-08-04 |
| rust.libraries.one-off | [bounded-systems/git-ast](https://github.com/bounded-systems/git-ast) | `95b1d0718500` | MIT | one-off | libraries | 0 | 2026-08-04 |
| rust.libraries.production | [jonhoo/fantoccini](https://github.com/jonhoo/fantoccini) | `e5cddafcb41c` | Apache-2.0 | production | libraries | 2013 | 2026-08-04 |
| rust.libraries.small-lib-app | [djc/gcp_auth](https://github.com/djc/gcp_auth) | `d602a2765f3a` | NOASSERTION | small-lib-app | libraries | 76 | 2026-08-04 |
| rust.systems.education | [washimimizuku/rust-tutorials](https://github.com/washimimizuku/rust-tutorials) | `63502eed65cd` | MIT | education | systems | 0 | 2026-08-04 |
| rust.systems.mature-oss | none | none | none | mature-oss | systems | 0 | none |
| rust.systems.one-off | [CanadianCowboy/a2x](https://github.com/CanadianCowboy/a2x) | `0a554dbe7d76` | AGPL-3.0 | one-off | systems | 3 | 2026-08-04 |
| rust.systems.production | [tnballo/high-assurance-rust](https://github.com/tnballo/high-assurance-rust) | `bd59fb1562af` | NOASSERTION | production | systems | 1407 | 2026-08-04 |
| rust.systems.small-lib-app | [weizhiao/Relink](https://github.com/weizhiao/Relink) | `0d785cabb12b` | Apache-2.0 | small-lib-app | systems | 143 | 2026-08-04 |
| rust.web-backend.education | [0xdea/zero2prod](https://github.com/0xdea/zero2prod) | `a303afd5950e` | MIT | education | web-backend | 6 | 2026-08-04 |
| rust.web-backend.mature-oss | [Isona/dirble](https://github.com/Isona/dirble) | `e2dea9f16dee` | GPL-3.0 | mature-oss | web-backend | 633 | 2026-08-04 |
| rust.web-backend.one-off | [Shyam-Chen/Rust-Journey](https://github.com/Shyam-Chen/Rust-Journey) | `4c471877ee2e` | NOASSERTION | one-off | web-backend | 3 | 2026-08-04 |
| rust.web-backend.production | [graphql-rust/juniper](https://github.com/graphql-rust/juniper) | `768071f64b48` | NOASSERTION | production | web-backend | 5967 | 2026-08-04 |
| rust.web-backend.small-lib-app | [ppmpreetham/fastrapi](https://github.com/ppmpreetham/fastrapi) | `03d37619abc1` | MIT | small-lib-app | web-backend | 99 | 2026-08-04 |
| typescript.cli-automation.education | [deathlegionteamlk/legion-hutta](https://github.com/deathlegionteamlk/legion-hutta) | `90af65763569` | MIT | education | cli-automation | 0 | 2026-08-04 |
| typescript.cli-automation.mature-oss | [listr2/listr2](https://github.com/listr2/listr2) | `3f8ef3dd3008` | MIT | mature-oss | cli-automation | 679 | 2026-08-04 |
| typescript.cli-automation.one-off | [elberacasa/umbra](https://github.com/elberacasa/umbra) | `87e05ced51be` | MIT | one-off | cli-automation | 0 | 2026-08-04 |
| typescript.cli-automation.production | [qawolf/cli](https://github.com/qawolf/cli) | `6a0604483fd6` | Apache-2.0 | production | cli-automation | 3441 | 2026-08-04 |
| typescript.cli-automation.small-lib-app | [ouijit/ouijit](https://github.com/ouijit/ouijit) | `2f1353ea63ee` | AGPL-3.0 | small-lib-app | cli-automation | 141 | 2026-08-04 |
| typescript.libraries.education | [jevonhou/exam-loop-core](https://github.com/jevonhou/exam-loop-core) | `5007b679f482` | MIT | education | libraries | 1 | 2026-08-04 |
| typescript.libraries.mature-oss | [mdbootstrap/mdb-react-ui-kit](https://github.com/mdbootstrap/mdb-react-ui-kit) | `8f79799956f5` | NOASSERTION | mature-oss | libraries | 1417 | 2026-08-04 |
| typescript.libraries.one-off | [codiume/hooks](https://github.com/codiume/hooks) | `d4e888c098d8` | NOASSERTION | one-off | libraries | 3 | 2026-08-04 |
| typescript.libraries.production | [intlify/vue-i18n](https://github.com/intlify/vue-i18n) | `cee57384e0ed` | MIT | production | libraries | 2704 | 2026-08-04 |
| typescript.libraries.small-lib-app | [rottenronin/cresh-ui](https://github.com/rottenronin/cresh-ui) | `635592da85f7` | MIT | small-lib-app | libraries | 97 | 2026-08-04 |
| typescript.web-backend.education | [didinj/node-express-mongodb-reactjs-graphql](https://github.com/didinj/node-express-mongodb-reactjs-graphql) | `0a8e6300e46f` | MIT | education | web-backend | 38 | 2026-08-04 |
| typescript.web-backend.mature-oss | [0xb4lamx/nestjs-boilerplate-microservice](https://github.com/0xb4lamx/nestjs-boilerplate-microservice) | `3a02eb16f091` | MIT | mature-oss | web-backend | 631 | 2026-08-04 |
| typescript.web-backend.one-off | [SrjAdhikari/Manakuru](https://github.com/SrjAdhikari/Manakuru) | `07d2337fe8ee` | NOASSERTION | one-off | web-backend | 0 | 2026-08-04 |
| typescript.web-backend.production | [yagop/node-telegram-bot-api](https://github.com/yagop/node-telegram-bot-api) | `62574d3c1da4` | MIT | production | web-backend | 9192 | 2026-08-04 |
| typescript.web-backend.small-lib-app | [neynarxyz/nodejs-sdk](https://github.com/neynarxyz/nodejs-sdk) | `8e4c492ca234` | MIT | small-lib-app | web-backend | 70 | 2026-08-04 |
| typescript.web-frontend.education | [peckem/OpTeamUs](https://github.com/peckem/OpTeamUs) | `eb83ddf6a5c2` | NOASSERTION | education | web-frontend | 1 | 2026-08-04 |
| typescript.web-frontend.mature-oss | [Bowen7/regex-vis](https://github.com/Bowen7/regex-vis) | `a6d920601ca4` | MIT | mature-oss | web-frontend | 4442 | 2026-08-04 |
| typescript.web-frontend.one-off | [Chris0Jeky/developer-lens](https://github.com/Chris0Jeky/developer-lens) | `6cd30d1cd663` | NOASSERTION | one-off | web-frontend | 0 | 2026-08-04 |
| typescript.web-frontend.production | [vadimdemedes/ink](https://github.com/vadimdemedes/ink) | `cdc18fa4942b` | MIT | production | web-frontend | 39560 | 2026-08-04 |
| typescript.web-frontend.small-lib-app | [Weaverse/weaverse](https://github.com/Weaverse/weaverse) | `378716e9022f` | MIT | small-lib-app | web-frontend | 86 | 2026-08-04 |

</details>

## Sources

**Official specifications.** Python Language Reference (`https://docs.python.org/3.13/reference/`),
TypeScript Handbook (`https://www.typescriptlang.org/docs/handbook/intro.html`), ECMA-262 ES2025
(`https://tc39.es/ecma262/2025/multipage/`), The Rust Reference
(`https://doc.rust-lang.org/reference/`), The Go Programming Language Specification
(`https://go.dev/ref/spec`).

**Corpus.** 98 public repositories, each pinned to an exact commit. See the corpus manifest above.

**Tools.** CPython 3.14.6 `ast` and `tokenize`; TypeScript compiler API 5.9.3; a text-level scanner
written for this run for Rust and Go.

**Local Jet evidence.** `examples/features/basics/loop_values.jet`,
`examples/features/errors/errors.jet`, `examples/features/errors/qq_control.jet`,
`examples/features/collections/iter_adapters.jet`,
`examples/features/collections/member_spread.jet`,
`examples/features/patterns/struct_destructure.jet`, `examples/features/tooling/tests.jet`,
`examples/features/concurrency/bounded_workers.jet`, `docs/spec/syntax-decisions.md` lines 229–230
and 519–520 and 3116, `crates/jet-foundation/src/Syntax.rs`.

**Tower records, read only.** D-BIND-BARE1, D-MEM1, D-ORRETURN-ERG1, D-FLOWTYPE1 (card #746),
D-LOOPMAP1 and D-LOOP-HEADER3 (card #1325), D-LOOP-COMMA1 (card #1336), D-BRANCH-TEACH1 (card
#1259), D-BRANCH-CODEGEN1 (card #1260), D-AUTODERIVE1 (card #1267), D-SPREAD1 (card #1341),
D-DESTRUCT1, D-ITERTOOLS1. No Tower record was created or changed by this audit.
