# syntax-decisions.md reconcile — PROPOSE-ONLY map

Status: proposal. Nothing in `docs/spec/syntax-decisions.md` changes until the owner
approves this map. A later pass does the edits.

Two ground-truth files now exist that this doc can lean on instead of re-showing syntax:
- `examples/showcase/canon.jet` — compiling, golden-tested. Every line = ratified AND shipped.
- `docs/reference/syntax-surface.jet` — full ratified surface incl. ratified-but-unbuilt
  (each cites its decision id).

The doc's job shrinks to **decision IDs + why**. It should stop re-showing syntax those
two files demonstrate, stop carrying implementation essays, and stop duplicating every
decision in both a prose entry and a log row.

---

## Biggest reconcile opportunities (ranked)

1. **Decision log (~lines 2581-2970, ~390 lines) duplicates the prose entries, and many
   rows have grown into full implementation essays.** The log was meant to be
   `Date | ID | one-line | By`. It now carries multi-hundred-word rows with embedded
   `**(impl …)**` blocks naming Rust file paths and diagnostic-code assignments
   (D-SIMD2 2596, D-PKGSIGN1 2603, D-DBG3 2604, D-LINALG1 2605, D-SUPPLY1 2606,
   D-TXN3 2607, D-NUMOPS2 2608, D-QUAL3 2609…). This is the single largest line-count
   win: collapse the log back to one chronological line per id; substance lives in the
   prose entry only.

2. **`**(impl …)**` notes interleaved into the prose entries** (c129 1827-1838,
   D-SCAP1 1946-1956, D-LIN1 1979-1993, D-STATE1 2051-2068, D-TAINT1 2085-2100,
   D-EFF2 2216-2223, plus the SIMD/linalg/unit rows in the log). These describe
   `Source/Sema/Taint.rs`, `E0712`, codegen lowering — none of it is "syntax decided."
   It belongs in an implementation log or on the Tower cards, not the owner's syntax
   control surface. Owner question on destination (don't silently delete — see below).

3. **Worked code blocks that no longer compile, shown as current** (S19, S24, S28, S30,
   S55, S57, S66, S82, S83). Each shows retired spelling (`val`/`var`, visible `;`,
   `when`, `for`, `mut`, `@`-markers). canon.jet / syntax-surface.jet now show every one
   correctly — delete the block, point to the canon line.

4. **Supersession history re-derived inline at length.** The binding-sigil saga
   (`::`→`@=`), capability words→sigils, `when`→`if` dispatch, struct-ctor dot-prefix,
   manifest-filename churn each re-explain the dead spelling across 2-4 entries. Compress
   into one **superseded-spelling ledger** table; entries keep only the live decision +
   one pointer.

5. **Jetpack/jetos cluster (~30 D-JPK + 18 U-series) is mostly PM/OS policy, not language
   syntax**, and has its own design-of-record (`unified-ecosystem.md`). Mixed: a few are
   real syntax tokens (`provider@target`, `pkg#version`, `pack.jet` struct-literal deps);
   most are CLI/store/provider policy. Candidate to relocate the pure-policy half — owner
   question, not a guess.

Counts: contradicted code blocks **9**; supersession chains carrying dead-spelling prose
**~9** (covering ~25 entries); inline `(impl …)` essays **~20**; decision-log rows
duplicating prose **~200** (≈390 lines). Net: roughly half the file is compressible
without losing one decision or one rationale.

---

## A. Contradicted code blocks — delete, point to canon

These present non-compiling syntax as current. Unique content survives in canon.jet /
syntax-surface.jet (verified line refs below).

- **line 109-128 (S19, loops)**: examples use `val line = read_line();`, `var n = 10;`,
  visible `;`, `for … in` in the prose. All retired (`@=`/`:=`, no `;`, `for`→`loop`).
  — survives in **canon.jet 162-167** (`loop i in 1..5`) + **syntax-surface.jet 107-138**
  (infinite / conditional / `step` / labeled). Keep the "one keyword, header picks mode"
  rule sentence; drop the block.
- **line 160-166 (S24, dispatch arms)**: `when x { x == 1 -> { … }; }` — `when` retired to
  `if`, the per-arm `x ==` repetition reversed by the inferred comparator, `;` retired.
  — survives in **canon.jet 60-76 / 131-135** + **syntax-surface.jet 142-153**. The whole
  S24 entry is now historical; fold its surviving arm-semantics note into the if-dispatch
  entry (D-IF1/D-IF2/D-IF3).
- **line 302-308 (S30, enum decl)**: `Circle(Float);` etc. with `;` separators.
  — survives in **canon.jet 54-58 / 79-81** + **syntax-surface.jet 195-199**.
- **line 384-398 (S28, traits/impl)**: `x: Float;` fields, `;`.
  — survives in **canon.jet 96-108** + **syntax-surface.jet 279-296**.
- **line 450-473 (S55, derives)**: `@Comparable` / `@Serialize` prefix + `;`. Double-stale:
  `@`→`#` (D-ATTR1) AND `Serialize`→`Codable`/`Encode`/`Decode` (D-SERDE4).
  — live form in **syntax-surface.jet 297-301** (`#[Comparable, Serialize]`). Note: even
  syntax-surface still shows `Serialize`, not `Codable` — flag for the canon-author pass.
- **line 541-560 (S66, map entry iteration)**: `for fruit in fruits { … };` — `for`
  retired, `;` retired, `val … = …;`. — survives in **syntax-surface.jet 327-329**
  (`loop e in fruits { print("{e.key}: {e.value}") }`). S66's actual *decision* (capitalized
  acronyms `JSON`/`IOError`/`U8`) is unrelated to the example and must be kept as one line.
- **line 666-675 (S57, comptime binding)**: `comptime x = f();` with `;`. The `=` is
  correct here (comptime keeps `=`, not `@=`); only the `;` is stale.
  — survives in **syntax-surface.jet 491** (`comptime TABLE = build_table()`).
- **line 1356-1382 (S82, attribute syntax)**: the `@`-marker table + worked block use `@`
  (reversed to `#` by D-ATTR1), `mut Player` (retired to `~Player`), and `;`. Entire S82
  surface superseded by D-ATTR1/D-ATTR2/D-ATTR3. — live form in **syntax-surface.jet
  297-301, 593-599** + canon has no attrs. Keep only the position-disambiguation rule;
  re-point spelling to `#`.
- **line 1389-1404 (S83, multi-head fns)**: `;` throughout. — survives in
  **syntax-surface.jet 95-99** (marked ratified-not-yet-implemented).

## B. Supersession chains — compress into one ledger

Each chain is *accurate* but re-explains the dead spelling across multiple entries. Replace
the scattered inline history with one table; each live entry keeps only its current decision
+ "superseded earlier spelling: see ledger."

| Concept | Dead → live spelling | Entries carrying the history |
|---|---|---|
| Immutable binding | `val` → `::` → `@=` | S2 (47-58), D-BIND1 (1650-1657), D-BIND2 (2431-2436) |
| Mutable binding / reassign | `var` → `:=` / `=` | S2, S17 (243-246), D-BIND1 |
| Access capability | `mut`/`take`/`view` → `~`/`^`/`&` | S10 (73-80), D-CAP1 (1810-1818), D-CAP7 (1853-1928) |
| Multi-way dispatch | `switch` → `when` → `if … == { }` | S24 (151-179), D-SG1 ref, S68 (1094-1126), D-IF1 (1680-1693), D-IF3 (2598) |
| Struct construction | `T { }` → `T.{ }` | S29 (291-298), S29-FLUSH (1736-1743), D-DOTCTOR refs |
| Fallback operator | `or` → `??` | S35 (484-493), S71 (1149-1163) |
| Attribute sigil | `@Marker` → `#Marker` | S82 (1327-1382), D-ATTR1 (1702-1708) |
| Harness markers | `test`/`pure`/`todo` → `#Test`/`#Pure`/`#Todo` | S43 (597-602), S60 (897-907), D-CASING1 follow-on (2167-2176) |
| Serialize derive | `derive Serialize`/`to_json` → `#[Codable/Encode/Decode]` | S55 (443-482), D-SERDE4 (2591), D-JSONOUT1 (2288-2293) |
| Manifest filename | `jet.toml` → `pack.jet` → `payload.jet` → `pkg.jet` | S52 (788-818), U1 (1420-1426), U10 (1505-1536), D-JPK-FILES (1242-1269), C-MANIFEST (2601) |
| std → Core | `std`/`std.fs` → `core`/`core.fs` | S51 (758-777), D-CASING1 (2144-2165) |

A reader who wants "what's the binding sigil" should hit one live entry, not reconstruct
three dates of token-spending. The ledger preserves the audit trail in 11 rows.

## C. Shipped + duplicated — collapse prose↔log to one home

Every owner-batch decision appears **twice**: a full prose entry AND a full log row, often
both carrying the same `(impl …)` essay. Representative (not exhaustive):

- **Migration verbs** D-MIGRATE2A/D/E/F/B/C — prose 2255-2286, log 2623-2628. Identical
  content twice.
- **Serde/encoding** D-SERDE1-8, D-ENC1, D-JSONVERB1 — prose 2448-2471, 2588-2595; log
  2610-2611, 2648. The serde *naming* (Codable/Encode/Decode, DataTree, RenameAll menu) is
  the only owner-facing syntax; the rest is library/API description.
- **Math** D-SIMD1/2, D-LINALG1, D-MATHLIB1 — prose + log 2596, 2605, 2631-2632. D-SIMD2
  (2596) and D-LINALG1 (2605) log rows are ~200-word impl essays with file paths.
- **Targets** D-TGT1-5 — prose 1775-1808, log 2682-2686. Capability D-CAP1-10 — prose
  1810-1928, log 2613-2622, 2687-2690.
- **Tags/typestate/taint/linear** D-QUAL2/3, D-STATE1, D-TAINT1, D-LIN1, D-SCAP1 — prose
  2029-2100, 2609, log 2664-2681, each with an `(impl)` block.

Resolution: keep the rationale in **one** place (the grouped prose section), shrink the log
row to one chronological line. The `(impl …)` content exits the doc entirely (owner Q1).

## D. Non-syntax PM/OS policy (jetpack/jetos)

`unified-ecosystem.md` is the ratified design-of-record. These entries are CLI/store/provider
policy, not language syntax, and bloat a *syntax* doc: D-JPK1/2/4/5/6/9/11/12/14/15/16
(909-1059), D-DEV4 (1061-1083), U5/U11-U17 merge/system/image/service fields
(1454-1613). Real syntax tokens that should **stay**: `provider@target` (U6), `pkg#version`
(VERSION-#, 1228-1240), `pack.jet` inline-struct git deps (D-JPK23, 1022-1044), kebab names
(S84), `module`/`env`/`system`/`image` keywords (U3). Owner Q3 below.

---

## Proposed target structure

Goal: decisions + rationale only; canon.jet is the worked-example surface. ~half the lines.

```
# Syntax Decisions (the owner's control surface)

1. Protocol            — ratify→build rule (keep 1-19, tighten)
2. How to read this    — NEW: canon.jet = compiling surface; syntax-surface.jet =
                         full ratified incl. unbuilt. This doc = decision IDs + why.
                         It does not re-show syntax those files demonstrate.
3. Ratified decisions  — grouped by area, each entry = ID · one-line decision ·
                         rationale (only the part not obvious from the example) ·
                         "superseded: see ledger" pointer. NO code blocks.
     3.1  Bindings, types, literals      (S2/4/11/17/21/41/42/67, D-BIND2, D-NUMOPS1/2)
     3.2  Functions & generics           (S1/12/18/45/47/61/83, D-NARG1/2, D-CTOR1)
     3.3  Control flow & dispatch        (S19/22/23/68/72/79, D-IF1/2/3, D-LABEL1)
     3.4  Structs, enums, tuples, destructure (S27/29/30/73/74/77, S29-FLUSH, D-DOTCTOR)
     3.5  Errors, Option, Result         (S7/32/34/35/71/80/81, D-ERR2)
     3.6  Traits, impl, dispatch, derives (S28/48/55/62, D-ATTR1/2/3, D-SERDE4)
     3.7  Collections, strings, iterators (S37-41/64/65/66/70/75/76/78, D-ITER1, S69)
     3.8  Closures & fan-out             (S46/47/75)
     3.9  Modules, imports, packaging-syntax (S16/51/84, U1/2/3/6, VERSION-#, D-JPK23)
     3.10 Comptime                       (S26/57, D-CTCORE1, D-CTIO1, D-STRPARSE1)
     3.11 Effects & capabilities         (S60, D-EFF1-5, D-CAP7/8/9/10, D-SCAP1)
     3.12 Low-level / unsafe tier        (S58, D-UNSAFE2, D-UNINIT1, D-ALLOC1/2, D-REGION1)
     3.13 Tags, typestate, units, linear, taint (D-QUAL2/3, D-STATE1, D-LIN1, D-TAINT1)
     3.14 Memory layout & SIMD/math      (D-SOA1, D-REPRC1, D-SIMD1/2, D-LINALG1)
     3.15 Tests, docs, migrations        (S43/49, D-TEST1/4, D-MIGRATE1/2*)
     3.16 FFI                            (S50/59, D-CFFI2*, D-CBIND*)
     3.17 Tooling verbs (syntax-bearing only) (D-DBG3 surface, D-ARGS1, D-CASING1)
4. Superseded-spelling ledger  — the §B table (one row per concept)
5. Open / deferred             — S56, and anything still gated
6. Decision log                — date · id · one-line · by. Pure chronology, no prose.
7. Enforcement                 — tests/decisions.rs (keep ~2485-2496)
```

Implementation notes (`(impl …)`) leave the doc → owner Q1 picks the destination.

Section 3 entries shrink to this shape (example, S75):

> **S75 — Fan-out `f.[a,b,c]`** *(ratified 2026-06-16)*: applies one callable to several
> typed inputs, result `[T#N]`. Type-directed authoring (Blueprint north-star). Shown:
> canon.jet 154-157. Diagnostics E0961/E0962.

No re-shown grammar, no re-derived rejection list unless the *reason* is the decision.

---

## Owner questions (genuine forks — not guessed)

**Q1 — Where do the `**(impl …)**` notes go?** They carry real information (file paths,
assigned diagnostic codes, what shipped vs. deferred) that isn't recorded elsewhere. Options:
(a) delete — content lives in git/cards already; (b) move to a new
`docs/spec/implementation-log.md`; (c) move onto the matching Tower card. Risk of (a):
the "deferred fork" sub-notes (e.g. D-LIN1-DROP, D-TXN-ROLLBACK, D-DET-CAPAPI,
D-TAINT-SAN, D-STATE-* — drafted but un-balloted) would lose their only home. Recommend
(b) for the deferred-fork pointers, (a) for the Rust-file-path prose. Owner decides.

**Q2 — Should the decision log collapse to pure chronology?** Prose + log both list every
decision today. Proposal: log becomes `date · id · one-line · by` (substance only in the
grouped prose). Confirm that's the wanted split, vs. keeping the log as the substantive
record and thinning the prose instead.

**Q3 — Relocate the PM/OS-policy half of the jetpack/jetos cluster to
`unified-ecosystem.md`?** Keep the syntax tokens here (`provider@target`, `pkg#version`,
pack.jet deps, kebab names, `module`/`env`/`system`/`image`); move the CLI/store/provider
*policy* entries (§D list) out. Or keep everything here for one-stop lookup.

**Q4 — Stale spelling inside the *reference* files themselves.** syntax-surface.jet 297-301
still shows `#[Comparable, Serialize]`, but D-SERDE4 renamed `Serialize`→`Codable`. Before
this doc points at canon/surface as ground truth, those two files need a spelling pass so the
pointer isn't pointing at a second stale copy. (Out of scope for *this* doc's reconcile, but
gates it.)

**Q5 — S24 fate.** S24 (151-179) is now wholly historical (its keyword, its `;`, and its
no-bare-value rule are all reversed). Delete the entry and keep only a ledger row, or retain
a one-line "superseded by D-IF1/2/3" stub for id-searchers?
