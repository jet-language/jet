# Test & example trim — cut rewrite-fat, keep the safety net

Status: PROPOSAL (owner approves before any cut). Plan-only; nothing removed yet.

Goal: cut the fat that makes every syntax tweak slow/wasteful to re-edit and
re-bless, **without** dropping a single assertion. The test suite is the
owner's safety net — when a cut's coverage isn't provably preserved elsewhere,
it STAYS.

## How the suite is wired (why edits hurt)

`examples/features/*.jet` is the load-bearing set: **142 top-level + 14 dir
programs**. Each one is exercised by *four* auto-discovering test passes, so a
single example carries four test executions:

- `golden.rs` — front end + (if rustc) compile through rustc + run, output must
  equal `expected/<stem>.out` byte-for-byte. The expensive pass (one rustc per
  file).
- `dev.rs` — runs it in the interpreter twice: differential vs the compiled
  binary (miscompile guard) and vs `expected/<stem>.out` (I5 golden).
- `fuzz_sema.rs` — sema smoke over the same dir.
- `truthfulness.rs` — enforces `.jet` ↔ `expected/*.out` pairing **both ways**
  (orphan output = fail; missing output = fail).

Consequence for cuts: removing an example means removing **both** the `.jet`
**and** its `expected/<stem>.out` together, or `truthfulness.rs` goes red.

Cost model, honestly:
- **Edit cost** (the syntax-sweep pain) scales with file count, but pervasive
  tokens (`fn`, `print`, `::`, `->`) touch *every* file regardless — cuts shave
  the count, not the per-file inevitability.
- **Golden run cost** scales ~linearly with example count (rustc per file).
- **Re-bless cost** hits only on *output-affecting* changes, not pure syntax
  renames.

So the example dir is **not** where large fat hides: it is mostly
non-redundant (see below). The real lever is policy, not deletion (last
section).

## The discriminator (applied to every candidate)

`canon.jet` is itself an I5 example for **every feature it demonstrates**.
Folding a per-feature file whose feature canon already compiles+runs, and which
adds no unique assertion, does **not** breach I5 — canon still ships the
runnable demonstration. A fold is clean only if **all four** hold:

1. canon.jet already compiles+runs the same feature;
2. the example adds **no** edge case / assertion canon lacks;
3. **no** non-golden `.rs` test references it (the hard-keep grep below);
4. cutting it + its `expected/*.out` still leaves the feature demonstrated by
   canon.

Fail any → KEEP. (2) arguable → OWNER QUESTION. The "preserving test" the cut
relies on is **canon.jet**, cited by line.

### HARD KEEP — pinned by a non-golden test (cutting breaks the build)

These are referenced by name in a `.rs` test, so they are not golden-only:

| Example | Pinned by |
|---|---|
| `01_hello` | `repl.rs` (`:load`), `small.rs`, M0 smoke |
| `16_wordcount` | `lsp.rs`, `small.rs`, `dev.rs` |
| `22_ffi` | `ffi.rs` |
| `20_tests` | `jet_test.rs` |
| `105_bench`, `114_property_tests`, `115_doctests`, `123_bench_target` | `jet_test.rs` |
| `25_traits`, `26_generic_types`, `27_printable` | `generics.rs` |
| `32_tasks` | `dev.rs` |
| `05_fizzbuzz`, `11_enums`, `71_pattern_matching` | mirrored (Rust) in `tir.rs` |

(`tir.rs` *re-implements* 05/11/71 in hand-written Rust rather than loading the
file, so the TIR-shape coverage survives a cut — but they are pedagogically
load-bearing; see OWNER QUESTIONs.)

## Category A — examples to FOLD into canon (clean, all 4 hold)

Conservative: only files I read end-to-end and confirmed add nothing canon
lacks.

| Cut | Lines | Preserved by (canon.jet) | Proof |
|---|---|---|---|
| `10_structs.jet` + `expected/10_structs.out` | 19 | struct fields + method + static ctor + dot-brace, lines 41–55 & 116–119 | canon's `Point{dist_sq, origin}` ≡ 10's `Point{dist_sq, unit}`; same dot-brace `Point.{...}`, same field/method shape. No unique assertion. Not test-referenced. |
| `41_fan_out.jet` + `expected/41_fan_out.out` | 10 | fan-out S75 + destructure, canon line ~150 (`tripled #= triple.[1,2,3]` / `[t0,t1,t2] ::`) | identical pattern (`double.[1,2,3]` vs `triple.[1,2,3]`); destructure already in canon. No `.rs` references the fan-out example. |

That's the entire provably-clean fold list. Two files (~29 lines, ~4 test
executions each removed). Small on purpose — this is a safety net.

## Category B — partial-overlap CONSOLIDATE (move the unique bit, then fold)

`42_inline_module.jet` (14 lines) — canon already has `module math { pub fn
double }`. 42's only delta is a second fn, `add`. **Move:** add one line
`pub fn add(a: Int, b: Int) -> Int { return a + b }` to canon's `math` module
and one `math.add(...)` call to canon's `main` (update `canon.out`), then fold
42. Net: no lost assertion, one fewer file. OWNER QUESTION because it nudges
canon's pinned output.

## Category C — KEEP (fail the discriminator, named edge cases)

Each carries an assertion canon does **not** make:

- `03_values.jet` — escapes `\n \t \" \\`, brace-doubling `{{}}`, modulo,
  negative literal, float division. Canon has none of these. **KEEP.**
- `37_if_expression.jet` — `if` *as a value* (S68). Canon only uses the
  `if subject == { }` match-form, a different feature. **KEEP.**
- `38_method_chain.jet` — D-SG3 broken dot-chain layout with per-step comments.
  Canon has no multi-line chain; `fmt.rs` tests the formatter, not this runtime
  path. **KEEP.**
- `40_tuples.jet` — adds tuple equality, destructure-via-clone, a fn returning
  a tuple, and reordered-field equality. Canon only does literal + `.x`
  access. **KEEP** (or migrate these four asserts into canon first — heavier,
  not recommended now).
- `effect_caps / effect_grant / effect_higher_order / effect_levers /
  effects / effect_trait_bound` (6 files) — each demonstrates a **distinct**
  effect facet (`#Caps` block E0741, grants, higher-order, levers, `#Pure`
  inference, trait bounds). Canon has no effect syntax at all. `effects.rs`
  covers semantics but in Rust-unit form, not as runnable I5 examples. **KEEP
  all.**

## Category C (tests) — duplicate golden coverage: NO safe cuts

Asked to find golden tests that duplicate another test's coverage. Honest
finding: **none cuttable.** The per-feature `.rs` suites assert things an
output-equality golden structurally **cannot** see, so they are not duplicates:

- `effects.rs` (771) — asserts effect annotations are **erased** in generated
  Rust (I3); golden only sees stdout.
- `tir.rs` (4679) — asserts TIR node shapes; golden sees neither IR nor TIR.
- `taint.rs`, `typestate.rs`, `ownership.rs`, `single_use.rs`, `rollback.rs`,
  `arena.rs` — assert *rejection* diagnostics and soundness on programs that
  must **fail** to compile; golden only runs programs that pass.
- `ref_soundness_fuzz.rs`, `fuzz_sema.rs` — randomized soundness; not a fixed
  golden.
- `dev.rs` interpreter pass over the same examples is **intentional**
  differential coverage (interpreter vs compiled miscompile guard), not
  redundancy.

If anything, the four-pass-per-example fan-out is the only test-side
multiplier, and every pass earns its keep. **No test cut recommended.**

## OWNER QUESTIONs (borderline — coverage preserved, judgment is yours)

1. **Teaching-ladder dups.** `02_functions` (canon covers fn via greet/id/
   triple), `05_fizzbuzz`, `11_enums`, `71_pattern_matching` are all
   canon-covered (and 05/11/71 also tir-covered). Coverage is preserved if
   cut — but they are the beginner ladder. Cut, or keep as pedagogy? (My lean:
   **keep** — pedagogy is an I5 value, and the win is ~3 files.)
2. **Canon's pinned output.** Category B (`42_inline_module`) and any future
   "migrate the edge case into canon" requires re-blessing `canon.out`. OK to
   let canon grow as the consolidation sink, or keep canon frozen and leave
   small examples standalone?
3. **Tuple migration.** Move `40_tuples`' four extra asserts into canon and
   fold it? Heavier edit to canon for one fewer file — worth it?

## Impact

- Provably-clean cuts (Category A): **2 files** of 142 (~1.4%) → ~1.4% off the
  golden rustc run + 2× (4 passes) fewer test executions + 2 fewer files in
  every syntax sweep. Plus B (1 more) if approved.
- This is deliberately marginal: the example suite is **not** bloated with
  redundancy — it is mostly distinct edge cases and I5 demonstrations. Deep
  deletion would trade safety for speed, which the brief forbids.

### The real lever is policy, not deletion

The recurring fat is *minting a new tiny `features/` example every time a
syntax knob lands*, when canon already demonstrates the surface. Proposed
standing rule (owner sign-off):

- New "does this syntax compile + run" coverage goes into **`canon.jet`**, not
  a new numbered example.
- A new `features/` file is justified only when it carries a **unique
  assertion** (an edge case, error path, or codegen invariant) or a **distinct
  teaching rung** canon doesn't serve.
- This caps growth at the source instead of pruning after the fact, and keeps
  the syntax-sweep blast radius from compounding over time.

## Cut mechanics (when approved)

For each approved example: delete `examples/features/<stem>.jet` **and**
`examples/features/expected/<stem>.out` in the same change (truthfulness pairs
both ways); re-run `golden`, `dev`, `fuzz_sema`, `truthfulness`. For Category B,
edit `canon.jet` + re-bless `examples/showcase/expected/canon.out` first, then
fold.
