# Syntax wave — 16 ballots ratified 2026-07-01

**Card:** `c8syntaxwave` · **Epoch 3** · **Status:** *ready to build*
**Scope:** the 16 ballots the owner ratified 2026-07-01 under card `c8syntaxwave`.

This plan is executable as-written. Every design question is settled; do **not**
re-derive any decision. Where a ballot's illustrative code used v5 notation
(`::` bindings, `type X :: Base`), the *live* spelling is D-BIND3 (`#=`/`:=`) and
the D-DIST1/D-DIST3 `distinct` machinery — build against current law, below.

Read first (once): `CLAUDE.md` (I1–I8), `docs/spec/philosophy.md` (ranked
priorities — effort never counts), `docs/spec/architecture.md` (R1 codegen dumb,
R2 sema gatekeeper, R4 spans, R7 TIR seam, R11 generated code re-enters),
`docs/spec/diagnostics.md` (the `Error [E####]` / `Why:` / `Fix:` voice —
banned words: *token, expression, parse, syntax error, lifetime, borrow*).

Ratified option text lives in `tools/Tower/tower.json` (`decisions[]` where
`cardId=="c8syntaxwave"`) and in `docs/spec/syntax-decisions.md` (rows dated
2026-07-01, and the narrative block "Marker family, CLI flags, syntax wave").
The Syntax.rs constants for this wave are **already registered** (lowercase,
"pending D-CONTRACTCASE1"): `CONTRACT_PRE/POST/PERSIST`,
`CONTRACT_BUNDLE_{NUMERIC,COMPARABLE,PRINTABLE,CODABLE_AS_BASE}`,
`UNIT_SUFFIX_EXPONENT_RESERVED`, plus no-new-token comments for trailblock /
destruct / chaincmp (`crates/jet-foundation/src/Syntax.rs` ~lines 1853–1893).

---

## 0. Global: sequencing, shared infra, and the DO-NOT list

### 0.1 Cross-cutting gate — the `@` contract plane (BLOCKING)

Four of these ballots put a marker on the **`@` plane**: `@Pre`, `@Post`
(D-PREPOST1), `@Persist` (D-PERSIST1), and the bundle markers `@Numeric`
`@Comparable` `@Printable` `@CodableAsBase` (D-CAPBUNDLE1). The `@`-plane
grammar itself (a `@Name` / `@Name(...)` that precedes a declaration and states a
checkable contract) is owned by a **separate card, `c7markerfamily`**
(D-MARKER-FAMILY1=B). Two follow-up ballots fix the spelling:

- **D-CONTRACTCASE1 = A** — the `@` plane is **PascalCase** (confirmed by the
  D-METHODMACRO1 row: "casing per D-CONTRACTCASE1=A"). So the **final spellings
  are `@Pre`, `@Post`, `@Persist`, `@Numeric`, `@Comparable`, `@Printable`,
  `@CodableAsBase`** — not the lowercase forms the ballots drafted.
- **D-MARKERMOVE1 = B** — the exact list of existing `#` markers that move to
  `@`, and the deliberate `#Numeric` (D-DIST3/D-QUAL3) vs `@Numeric` overlap
  reconciliation.

**Sequencing rule:** every `@`-plane section here (D-PREPOST1, D-PERSIST1,
D-CAPBUNDLE1) lands **after** `c7markerfamily` has shipped the `@`-plane parser
and renamed the plane. When you reach those sections, the shared parse path
`parse_at_contract()` (a `@`-prefixed PascalCase name, optional `(args)`, bound
to the following declaration) already exists — you register new contract names
against it, you do not invent `@` parsing. Until then, **build the non-`@`
sections** (they have no such dependency). When you update Syntax.rs, flip
`CONTRACT_PRE`→`"Pre"`, `CONTRACT_POST`→`"Post"`, `CONTRACT_PERSIST`→`"Persist"`,
`CONTRACT_BUNDLE_*`→PascalCase, in the same commit as the marker-family rename.

### 0.2 Shared infra beyond the `@` plane

- **Distinct-type machinery** (D-DIST1/D-DIST3, `crates/jet-sema/src/Sema/` —
  registration + `CheckerCore`, `Bundle.rs` already present) is the substrate for
  **D-UNITLIT1**, **D-RANGETYPE1**, and **D-CAPBUNDLE1**. Build D-RANGETYPE1's
  constraint form and D-UNITLIT1's literal resolution on the same registration
  pass; D-CAPBUNDLE1 re-exposes base operations through it.
- **String interpolation lexing** (`crates/jet-lexer/src/Lexer/Strings.rs`,
  `StrTokPart::Interp`) is shared by **D-PARSESTR1** (interp literal in pattern
  position) and **D-TYPEDTEXT1** (interp literal elaborating to bound params).
- **Expected-type elaboration** (the flow that already powers `.{ }` construction
  and `.Variant` shorthand, in sema's expression checker) is shared by
  **D-LAMBDAINFER1**, **D-DESTRUCT1**, and **D-TYPEDTEXT1**.
- **Taint/sanitizer model** (`crates/jet-sema/src/Sema/Taint.rs`, E0721 family) is
  the enforcement engine for **D-TYPEDTEXT1**.
- **E3 runtime** (JIT hot-reload + structured-concurrency/coroutine machinery, an
  E3 exit criterion not yet built) is the runtime for **D-PERSIST1**,
  **D-STREAMYIELD1**, and the trace half of **D-ERRCTX1**. Their *surface* is
  fixed now; wire surface + sema + the dumb lowering, and land the runtime piece
  against the E3 machinery as it arrives (name that gate in the section, per
  philosophy "do it right / name the gate").

### 0.3 Build order (DAG)

```
c7markerfamily (@-plane parser + rename)  ─────────────┐   [external gate]
                                                       │
Tier 1  no @-plane, pure parser/sema/desugar:          │
  D-CHAINCMP1 ──► D-TRAILBLOCK1 ──► D-LAMBDAINFER1      │
  D-DESTRUCT1 (also: migration pass over examples)     │
                                                       │
Tier 2  lexer + distinct-type machinery:               │
  D-UNITLIT1 ──► D-RANGETYPE1                           │
                                                       ▼
Tier 3  @-plane (needs c7markerfamily):
  D-CAPBUNDLE1 ──► D-PREPOST1 ──► D-PERSIST1(*E3 runtime)

Tier 4  string/parse + taint:
  D-PARSESTR1 ──► D-TYPEDTEXT1

Tier 5  manifest / no-grammar / runtime:
  D-EFFBUDGET1 (pkg.jet)   D-ERRCTX1 (stdlib + *E3 trace)   D-STREAMYIELD1 (*E3 coroutine)

Tier 0  status-quo, docs only, no code path to build:
  D-UFCS1 = B
```
Within a tier the arrow is "cheapest/least-risky first," not a hard dependency.
Rationale is one line per section below. Ship each ballot fully (parser → sema →
codegen → diagnostics → example → tests → docs) before the next — no stubs
(philosophy: do it right the first time; never "milestone-pending").

### 0.4 DO NOT (I8 traps + scope fences)

- **D-UFCS1 = B — no UFCS.** A free function never becomes a method. Do **not**
  add `a.f(b)` → `f(a, b)` rewriting, and do **not** add a `|>` pipeline operator
  (both rejected as second/third call spellings). The one method spelling stays
  `fn Type.name(self)` (D-EXTMETH1). No code; optional teaching guard only (§16).
- **D-TRAILBLOCK1 — zero-parameter only.** A trailing `{ }` block is a zero-arg
  lambda. A lambda that binds parameters stays inside the parens as `(x) => …`.
  Do **not** invent Kotlin-`it` / Swift-`$0` implicit parameter names.
- **D-DESTRUCT1 — `..` mandatory on partials, one nesting level.** Do not allow a
  partial destructure without `..`; do not build deep nested destructure towers
  (formatter discourages; one level only).
- **D-CHAINCMP1 — same-direction, no `==`/`!=`.** Only `<`/`<=`/`>`/`>=` chains,
  one direction per chain. `==`/`!=` chains are excluded (use table dispatch).
- **D-UNITLIT1 — no implicit cross-unit conversion.** `1s + 500ms` compiles only
  if the `#UnitFamily` declares the conversion; otherwise it's a diagnostic. A
  suffix shaped `e`+digits is always a float exponent, never a unit.
- **D-RANGETYPE1 — literal-range constraint only.** Ship `Int(0..10)`-style
  bounds. Do **not** build arbitrary predicate constraints
  (`Int where (self % 2 == 0)` = ballot option B, explicitly deferred).
- **D-TYPEDTEXT1 = D — two typed values only (`Sql`, `Html`).** Do **not** ship
  user-definable text prefixes (`css"…"`, `re"…"` = reader macros, deferred to
  E4). `Sql.raw(...)` is the only audited String→Sql escape.
- **D-CAPBUNDLE1 — four fixed bundles, no per-op grants.** `@Numeric`,
  `@Comparable`, `@Printable`, `@CodableAsBase`. Do **not** add a per-operator
  grant list (`with (+, -, <)` = option B, rejected). Leave the `#Numeric`
  overlap alone — it rides D-MARKERMOVE1.
- **D-EFFBUDGET1 = D — manifest keys, not grammar.** `effects:`/`allow:`/`deny:`/
  `grants:` are `pkg.jet` fields. Add no language token. The always-on report is
  zero-config and prints on every `jet build`.
- **D-ERRCTX1 = D — no grammar.** `.context("…")` is a stdlib method; the trace is
  runtime rendering. Do **not** add a context operator on `?` (option B rejected).
- **D-PREPOST1 — checked in every build by default.** Not a debug/release split.
  Conditions are pure (same checker as `#Pure`). The per-module strip is an
  explicit build-policy opt-out (see §11 ambiguity note — surface rides open
  D-BUILDPOLICY1).

---

## 1. D-CHAINCMP1 — chained comparisons `0 <= sev < 10`  *(Tier 1, first: pure desugar, zero type surface)*

**Ratified (A).** `0 <= sev < 10` desugars to `0 <= sev && sev < 10` with the
shared middle operand `sev` evaluated **exactly once**. Only same-direction
chains of `<`/`<=`/`>`/`>=`, any length, are legal; a mixed-direction chain
(`a < b > c`) is a compile error naming the direction break. `a < b < c` is a
type error in Jet today, so no existing program changes meaning. `==`/`!=`
excluded. No new token.

- **Grammar/lexer:** none new. `crates/jet-parser/src/Parser/Expressions.rs`
  `expr_cmp` (line ~368) **currently rejects any chaining** ("comparisons can't
  be chained", ~line 407). Replace that rejection: accept a run of same-direction
  relational operators into a new `Expr::CompareChain { operands, ops }` node
  (add to `crates/jet-foundation/src/AST.rs`, with spans, R4). A mixed direction
  in the run raises the error below.
- **Sema:** each adjacent pair type-checks as the existing binary comparison
  (E0109 operand mismatch still applies per pair). No new *type* rule.
- **Codegen:** dumb lowering in the TIR lower/emit
  (`crates/jet-codegen/src/Codegen/TIR/`): bind the shared middle operands to
  temps once, emit `t0 op0 t1 && t1 op1 t2 && …`. Single evaluation is a lowering
  fact, resolved at lower time (R1). If simpler, desugar in the parser to nested
  `&&` over let-bound temps — but a dedicated TIR node keeps spans clean.
- **New diagnostic (I4):** `E0333` (parse) — mixed-direction comparison chain.
  - what: `` this comparison chain changes direction ``
  - why: `a chain like `0 <= sev < 10` reads in one direction; mixing `<` and `>` in one chain is almost always a mistake and has no single meaning`
  - fix: `split it into two comparisons joined with `&&``
  - fixture: `tests/ui/chained_comparison_mixed_direction.{jet,stderr}`
- **Formatter:** emit `a <= b < c` with single spaces; add a fmt STABILITY test
  in `tests/fmt.rs` (`fmt_preserves_chained_comparison`) — assert the chain
  survives round-trip (idempotence alone misses dropped operands).
- **Example (I5):** `examples/features/operators/chained_comparison.jet` — a
  range guard `if 0 <= sev < 10 { print("in range") }`; expected out one line
  `in range` in `examples/features/expected/`.
- **Tests:** `cargo test --test fmt`, `cargo test --test ui` (or the ui harness),
  `cargo test --test golden`.
- **Exit:** [ ] AST node + spans [ ] same-direction chain of length ≥3 desugars,
  middle operands evaluated once (assert via a side-effecting counter in a test)
  [ ] E0333 fixture [ ] fmt stability test [ ] example + expected out [ ] docs row
  in syntax-decisions.md marked implemented.

## 2. D-TRAILBLOCK1 — trailing block argument `ui.button("Save") { save() }`  *(Tier 1: parser postfix, no type surface)*

**Ratified (A).** When a call's final parameter is a function type, the call may
end with a bare `{ }` block standing in for that last **zero-parameter** lambda
argument. v1 is zero-parameter only. No new sigil: a `{` directly after a call's
`)` is the trailing zero-arg lambda (Jet has no bare-block statements, so the
slot is free).

- **Grammar:** `crates/jet-parser/src/Parser/Expressions.rs`, in the call/postfix
  layer: after parsing `callee(args)`, if the next token is `{` on the same
  logical line, parse a block and append it as a final argument
  `Expr::Lambda { params: [], body }` (reuse the existing lambda AST). Guard: only
  after a `)` call tail; do not trigger for `if`/`loop`/struct-literal `{`.
- **Sema:** the appended lambda type-checks against the callee's last parameter
  (must be a function type taking zero params). Two new errors:
  - `E0334` (sema) — the call's last parameter is not a zero-parameter function.
    what: `` `NAME` doesn't take a trailing block ``; why: `a trailing `{ }`
    block fills a last argument that is a function taking no parameters; this
    call's last parameter is `T``; fix: `pass it inside the parentheses, or give
    the function a zero-parameter last argument`.
  - `E0335` (parse) — more than one trailing block, or a trailing block on
    something that isn't a call. what/why/fix in voice; fixture each.
  - fixtures: `tests/ui/trailing_block_not_function.{jet,stderr}`,
    `tests/ui/trailing_block_double.{jet,stderr}`.
- **Codegen:** none special — the appended lambda lowers exactly like an
  in-parens `() => …` argument (R1; the two spellings are identical post-parse).
- **Formatter:** canonical layout — `callee(args) { … }`, space before `{`, block
  body indented; if the call has only the block argument, emit `callee { … }`.
  fmt STABILITY test `fmt_preserves_trailing_block` (assert the block does not
  migrate back inside the parens and args are not dropped).
- **Example:** `examples/features/syntax/trailing_block.jet` — define
  `fn twice(f: fn())` calling `f()` twice; call `twice { print("hi") }`; expected
  out two `hi` lines.
- **Exit:** [ ] parser appends zero-arg lambda [ ] E0334/E0335 fixtures
  [ ] fmt stability test [ ] byte-identical lowering to in-parens form
  (add a codegen-parity assertion) [ ] example + expected [ ] docs.

## 3. D-LAMBDAINFER1 — expected-type lambda parameter inference  *(Tier 1: expected-type elaboration)*

**Ratified (A).** A lambda parameter's type may be omitted where the expected type
already fixes it; required elsewhere. Inference is local and one-directional
(expected type flows **in**) — the same expected-type elaboration that powers
`.{ }` construction and `.Variant` shorthand. `(i: Incident) => …` stays legal;
`(i) => …` is now legal when the callee's parameter type is known
(e.g. `filter(fn(Incident) -> Bool)`).

- **Grammar:** `crates/jet-parser/src/Parser/Expressions.rs` lambda parse — make
  the per-parameter type annotation optional (parse `Ident` with optional
  `: Type`). AST `LambdaParam.ty` becomes `Option<Type>` (`AST.rs`).
- **Sema:** in the expression checker, when a lambda is checked against an expected
  function type, bind each un-annotated parameter to the expected type's
  corresponding parameter type before checking the body. This is exactly the
  existing expected-type push used for `.{ }`/`.Variant` — extend it to lambda
  params. When there is **no** expected function type and a param is
  un-annotated, reuse the existing **E0801** ("lambda parameter type unknown") —
  no new code. Do not attempt bidirectional/return-type-driven inference.
- **Codegen:** none — types are resolved by sema; TIR carries the resolved param
  types (R1). The Rust closure emits the same as an annotated lambda.
- **Formatter:** do **not** re-insert inferred annotations (they'd be reading
  noise the ballot explicitly rejects). Preserve whatever the user wrote. fmt
  STABILITY test `fmt_preserves_bare_lambda_params` — assert `(i) => …` does not
  gain a type and does not drop the param.
- **Example:** `examples/features/functions/lambda_inference.jet` — a
  `list.filter((x) => x > 3)` where the element type fixes `x: Int`; expected out
  the filtered list.
- **Exit:** [ ] optional param type in AST/parser [ ] expected-type push binds
  bare params [ ] E0801 still fires with no expected type (fixture already exists;
  add a positive example) [ ] fmt stability [ ] example + expected [ ] docs.

## 4. D-DESTRUCT1 — struct destructuring + mandatory `..`  *(Tier 1: reuses `.{ }`; carries a breaking migration)*

**Ratified (A).** The dot-construction shape (D-DOTCTOR1/2) runs backwards. In
binding position, `.{ id, severity: sev } #= incident` binds `id` and `severity`
(renamed `sev`). The same shape is an if-table pattern head —
`.{ kind: "page", target, .. } -> page(target)` — matching named field values and
binding the rest. **`..` is mandatory whenever the pattern does not name every
field.** This **amends S74** (shipped irrefutable, no partiality marker):
existing partial destructures need `..` added — a **breaking spelling change**
requiring a migration pass across examples/tests. One nesting level only.

- **Grammar:** `crates/jet-parser/src/Parser/Expressions.rs` — the `.{ }` pattern
  parser (S74 path; see `enum_lit_named_fields` for the sibling construction
  parser). Accept `field` (bind same name), `field: name` (rename), and a
  trailing `..` rest token (reuse `OP_RANGE` `..`). Both in binding position
  (`.{ … } #= expr`) and in if-table arm heads.
- **Sema:** reuse existing S74 checks — `E0313` (shape mismatch), `E0315`
  (list-pattern arity). New:
  - `E0326` (parse) — a partial struct destructure with no `..`.
    what: `` this pattern leaves out fields of `Incident` ``; why: `a
    destructure that doesn't name every field must end with `..` so the skipped
    fields are visible at a glance`; fix: `add `, ..` before the closing `}`, or
    name the remaining fields`. fixture
    `tests/ui/destructure_partial_missing_rest.{jet,stderr}`.
  - `E0327` (parse) — a redundant `..` on a pattern that already names every
    field. what/why/fix; fixture `tests/ui/destructure_redundant_rest.…`.
- **Codegen:** dumb — lower to the equivalent field reads / Rust pattern match in
  TIR (R1). Rename maps field→local.
- **Formatter:** emit `.{ id, severity: sev, .. }`; preserve `..`. fmt STABILITY
  test `fmt_preserves_destructure_rest` (assert `..` and renames survive — this is
  exactly the class of drop that idempotence misses).
- **Migration (breaking):** run a pass over `examples/` and `tests/` adding `..`
  to every existing partial `.{ }` destructure; re-bless affected snapshots. This
  is in-scope for the card, not a follow-up.
- **Example:** `examples/features/patterns/struct_destructure.jet` — bind two of
  three fields with `..`, and one if-table arm matching `.{ kind: "page", .. }`;
  expected out.
- **Exit:** [ ] `..` parse in both positions [ ] E0326/E0327 fixtures
  [ ] migration pass done + snapshots re-blessed [ ] fmt stability [ ] example +
  expected [ ] S74 row amended in syntax-decisions.md.

## 5. D-UNITLIT1 — unit-suffix numeric literals `500ms`, `12.50usd`  *(Tier 2: lexer + distinct-type machinery)*

**Ratified (A).** A numeric literal may carry a unit suffix resolving to a
`#UnitFamily` member (D-QUAL3/`ATTR_UNIT_FAMILY`) imported into scope. Sugar over
the existing distinct-type path — **not a new type**. Suffix resolution is
import-scoped. **No implicit cross-unit conversion**: `1s + 500ms` compiles only
if the family declares the conversion, else a diagnostic names the missing
conversion fn. A suffix shaped `e`+digits stays float-exponent notation
(`UNIT_SUFFIX_EXPONENT_RESERVED`) and may never be a unit. Supersedes
D-LITSUFFIX-SCOPE (the `px.{100}` dot-construction stays valid; this adds the
literal spelling).

- **Lexer:** `crates/jet-lexer/src/Lexer/Scan.rs` (numeric scan) — after a numeric
  literal (int or float, including `_` separators and base prefixes per S67),
  greedily read a trailing identifier run as a *unit suffix candidate* and attach
  it to the number token (`NumberLit { value, unit_suffix: Option<String> }`).
  **Guard:** if the suffix is exactly `e`/`E` followed by digits, it is the float
  exponent — do not treat as a unit (this is the `UNIT_SUFFIX_EXPONENT_RESERVED`
  rule; the exponent scan already exists, run it first). The lexer does **not**
  resolve the family — it only carries the string; resolution is sema's job
  (imports aren't known to the lexer).
- **Sema:** resolve the suffix against `#UnitFamily` members in scope; a match
  elaborates the literal to that member's distinct `#Numeric` type
  (`crates/jet-sema/src/Sema/` registration + core checker). Cross-unit arithmetic
  reuses the existing **E0127** (cross-unit mixing on distinct types) unless the
  family declares a conversion. New:
  - `E0134` (sema) — a unit suffix that is not a `#UnitFamily` member in scope.
    what: `` `ms` isn't a unit in scope ``; why: `a unit suffix names a member of
    a `#UnitFamily` you've imported; `ms` isn't one here`; fix: `import the family
    that defines `ms`, or write the number without a suffix`. fixture
    `tests/ui/unit_literal_unknown_suffix.{jet,stderr}`.
- **Codegen:** none new — the elaborated value is an ordinary distinct-type value;
  TIR lowers it like any `#Numeric` distinct construction (R1).
- **Formatter:** emit `500ms` (no space between number and suffix). fmt STABILITY
  test `fmt_preserves_unit_literal` (assert the suffix is not dropped and no space
  is inserted).
- **Example:** `examples/features/types/unit_literals.jet` — a `#UnitFamily(time)`
  with `ms`/`s`, add two `ms` values, print; expected out. (Reuse/extend the
  existing `112_unit_family.jet` domain but under the new topic dir.)
- **Exit:** [ ] lexer carries suffix, `e`-exponent guard [ ] sema resolves to
  family member [ ] E0134 fixture [ ] cross-unit still E0127 (fixture) [ ] fmt
  stability [ ] example + expected [ ] docs row implemented.

## 6. D-RANGETYPE1 — range-constrained types `Severity #= distinct Int(0..10)`  *(Tier 2: distinct-type machinery)*

**Ratified (A).** Extend the nominal `distinct Base` form with a literal range
constraint: `Severity #= distinct Int(0..10)` is an `Int` that provably holds
0–10 (`..` is inclusive, S22). Construction from a **literal** is checked at
compile time; from a **runtime value** it is **fallible** (`Severity(raw)?`).
Arithmetic **widens to the base** (`Int`); converting back re-checks.
"Parse, don't validate" as a type. (Ballot's `type X :: Int(0..10)` is v5
illustration; live form is `distinct Int(0..10)` via D-DIST1.)

- **Grammar:** `crates/jet-parser/src/Parser/Types.rs` (or the distinct-type
  parse site) — after `distinct Int`, accept an optional `( lo .. hi )` range
  constraint. Store on the type-decl AST (`AST.rs`, with spans).
- **Sema:** `crates/jet-sema/src/Sema/` (distinct registration + core checker):
  - literal construction `Severity(3)` checks `lo <= n <= hi` at compile time.
  - runtime construction `Severity(raw)` is fallible — it must be spelled
    `Severity(raw)?` (yields the constrained type or the fallible failure).
  - arithmetic on `Severity` **widens to `Int`** (result is base `Int`, not
    `Severity`); assigning back into a `Severity` re-checks (compile-time if
    literal-foldable, else fallible).
  - New diagnostics:
    - `E0135` (sema) — a compile-time literal outside the declared bounds.
      what: `` `12` is outside `Severity`'s range 0..10 ``; why: `a range type
      only holds values inside its bounds; `12` can never be a `Severity``; fix:
      `use a value in `0..10`, or widen the type's range`. fixture
      `tests/ui/range_type_literal_out_of_bounds.…`.
    - `E0136` (sema) — a runtime value constructed into a range type without `?`.
      what: `` making a `Severity` from a runtime value can fail ``; why: `only a
      literal is checked at compile time; a runtime number needs the fallible
      form so a bad value is handled`; fix: `write `Severity(raw)?` and handle the
      failure`. fixture `tests/ui/range_type_runtime_needs_try.…`.
    - `E0137` (parse/sema) — an empty/reversed declared range (`lo > hi`).
      Reuse the E0316 wording style ("empty range"). fixture.
- **Codegen:** dumb — the constrained type is a base-`Int` newtype; the fallible
  runtime construction lowers to a bounds check returning the fallible value
  (R1). No new runtime concept.
- **Formatter:** emit `distinct Int(0..10)`; fmt STABILITY test
  `fmt_preserves_range_constraint` (assert the `(0..10)` is not dropped).
- **Example:** `examples/features/types/range_types.jet` — declare
  `Severity #= distinct Int(0..10)`, construct one from a literal, one fallibly
  from input; expected out.
- **Exit:** [ ] range constraint parses + stores [ ] literal check E0135
  [ ] fallible runtime E0136 [ ] empty range E0137 [ ] widen-to-base arithmetic
  test [ ] fmt stability [ ] example + expected [ ] docs.

## 7. D-CAPBUNDLE1 — capability bundles on nominal types  *(Tier 3: @-plane — needs c7markerfamily)*

**Ratified (A).** A nominal `distinct` type starts **inert** (no inherited
operators). Tagging it with a capability bundle re-exposes a curated slice of the
base type's operations **while preserving nominal identity** (`Usd + Usd -> Usd`;
`Usd + Eur` still an error). **Four fixed bundles ship**, spelled on the `@`
plane (D-CONTRACTCASE1=A → PascalCase):

| Marker | Grants |
|--------|--------|
| `@Numeric` | `+ - * /`, ordering — **same-type only** |
| `@Comparable` | `==` / `<`, hash / sort |
| `@Printable` | interpolation / display |
| `@CodableAsBase` | encode / decode via the base's wire representation |

Bundles **stack**. Per-operator grant lists (option B) and hand-written operators
(option C) rejected. **The `#Numeric` (D-DIST3/D-QUAL3) overlap is deliberately
left unresolved — it rides D-MARKERMOVE1.** Do not touch `#Numeric` here.

- **Sequencing:** land after `c7markerfamily` ships the `@`-plane parser. Register
  the four bundle names against `parse_at_contract()`; flip
  `CONTRACT_BUNDLE_*` in Syntax.rs to PascalCase in the marker-family rename
  commit.
- **Grammar:** a `@Numeric`/`@Comparable`/`@Printable`/`@CodableAsBase` marker
  preceding a `distinct` type declaration, stackable. Store the granted-bundle
  set on the type-decl AST.
- **Sema:** `crates/jet-sema/src/Sema/Bundle.rs` (already present — this is its
  purpose) + core checker. Each bundle enables its operation set on the nominal
  type, results typed as the **nominal type** (identity preserved), rejecting
  cross-type mixing. New:
  - `E0138` (sema) — an operation used on a nominal type whose bundles don't grant
    it. what: `` `Usd` doesn't support `*` ``; why: `a nominal type only gets the
    operations its capability bundles grant; `Usd` has `@Numeric`… `` (name the
    granted bundles); fix: `add the bundle that grants it, or convert to the base
    type first`. fixture `tests/ui/capbundle_operation_not_granted.…`.
  - Note in the code comment: relationship to E0127 (`#Numeric` arithmetic gate)
    is reconciled by D-MARKERMOVE1; until then both paths coexist.
- **Codegen:** dumb — a granted operation lowers to the base operation on the
  newtype (R1). `@CodableAsBase` lowers encode/decode through the base wire form
  (re-enters the front end per R11 if it emits a derive fragment).
- **Formatter:** emit each `@Bundle` on its own line before the declaration
  (marker-family formatter convention). fmt STABILITY test
  `fmt_preserves_capbundle_markers`.
- **Example:** `examples/features/types/capability_bundles.jet` — `@Numeric`
  `Usd`, `@Comparable` `CustomerId`; show `Usd + Usd`, and that `CustomerId * 2`
  is rejected (as a commented expected-error or a separate ui fixture); expected
  out for the passing part.
- **Exit:** [ ] four bundles register on `@` plane [ ] identity preserved
  (`Usd+Usd->Usd`, `Usd+Eur` error) [ ] stacking works [ ] E0138 fixture
  [ ] fmt stability [ ] example + expected [ ] `#Numeric` overlap left untouched,
  comment references D-MARKERMOVE1 [ ] docs.

## 8. D-PREPOST1 — `@Pre` / `@Post` function contracts  *(Tier 3: @-plane — needs c7markerfamily)*

**Ratified (A).** A signature may carry `@Pre(condition, "message")` (a claim
about the arguments) and/or `@Post(condition, "message")` (a claim about
`result`, the return value). Conditions are **pure** (no effects — enforced by
the same purity checker as `#Pure`). **Checked in every build by default**; a
per-module build-policy strip is the only opt-out (not a debug/release split — the
C-assert tradition was rejected). A violated clause raises a diagnostic quoting
the clause **at the call site**. Final spelling PascalCase (`@Pre`/`@Post`) per
D-CONTRACTCASE1=A. (Complements D-RANGETYPE1: a range type states one value's
shape; pre/post state a relation between arguments and result.)

- **Sequencing:** after `c7markerfamily`; register `@Pre`/`@Post` against
  `parse_at_contract()`; flip `CONTRACT_PRE`/`CONTRACT_POST` to `"Pre"`/`"Post"`.
- **Grammar:** `@Pre(expr, "msg")` / `@Post(expr, "msg")` on a function signature,
  repeatable. `result` is a bound name inside a `@Post` condition. Store clauses
  on the function AST (with spans, R4).
- **Sema:**
  - each condition runs through the **purity checker** (`Sema/Purity.rs`) — an
    effect in a clause is an error:
    - `E0139` (sema) — a contract condition uses an effect. what: `` a `@Pre`
      condition can't do I/O ``; why: `a contract is checked at every call and
      must be a pure claim about values`; fix: `move the effect out; keep only a
      pure test`. fixture `tests/ui/contract_condition_impure.…`.
  - `result` used in a `@Pre`, or a bare `result` outside any `@Post`:
    - `E0144` (sema). fixture `tests/ui/contract_result_misuse.…`.
- **Codegen:** dumb — lower `@Pre` to a checked guard at function entry and
  `@Post` to a checked guard before each return, both raising the runtime
  diagnostic below with the clause text and the caller's location. The strip
  opt-out (see §11 ambiguity) simply omits the guards for that module.
  - `E3005` (runtime) — a contract clause failed. Renders the **clause message**
    plus the call-site location (pinned to the caller, not the callee body).
    Register in diagnostics.md runtime table alongside E3001/E3002.
- **Formatter:** each `@Pre`/`@Post` on its own line above the `fn`. fmt STABILITY
  test `fmt_preserves_contracts` (assert clause text + message string survive).
- **Example:** `examples/features/contracts/pre_post.jet` — `@Pre(cents > 0,
  "cents must be positive")` on a function; a passing call; expected out.
  Optionally a failing-call ui/runtime fixture showing E3005.
- **Exit:** [ ] `@Pre`/`@Post` parse + `result` binding [ ] purity check E0139
  [ ] `result` misuse E0144 [ ] entry/return guards emit E3005 at call site
  [ ] checked in a default build (test) [ ] fmt stability [ ] example + expected
  [ ] docs.

## 9. D-PERSIST1 — `@Persist` dev-hot-reload state survival  *(Tier 3: @-plane + E3 runtime gate)*

**Ratified (A).** A module-level binding marked `@Persist` survives a `jet dev`
hot reload instead of resetting; unmarked state resets as today. Identity =
module path + binding name (rename ⇒ fresh state). On a reload that changes the
value's type layout, the dev runtime attempts a Codable-style re-decode, falling
back to reinit + a **one-line warning** — never a crash. **Purely dev-tier:**
release semantics are identical with or without the marker. Persist-by-default
(option B) rejected as a footgun. PascalCase `@Persist` per D-CONTRACTCASE1=A.

- **Sequencing:** after `c7markerfamily` (register `@Persist`, flip
  `CONTRACT_PERSIST`→`"Persist"`). The **runtime carry-across** rides the **E3 JIT
  hot-reload runtime** (an E3 exit criterion). Wire the surface + sema now; land
  the carry-across against the JIT reload machinery as it lands — **name that
  gate** in the docs row.
- **Grammar:** `@Persist` before a module-level binding. Store a persist flag on
  the binding AST.
- **Sema:**
  - `E0145` (sema/parse) — `@Persist` on a non-module-level binding. what: `` only
    module-level state can persist across reloads ``; why: `persistence is keyed
    by module + name; a local has no stable identity across a reload`; fix: `move
    it to module level, or drop `@Persist``. fixture
    `tests/ui/persist_not_module_level.…`.
- **Codegen / dev runtime:** in a **release** build `@Persist` is inert (no
  codegen difference — assert this). In `jet dev`, register the binding in the
  reload state table keyed by module+name; on reload, re-decode (Codable path) or
  reinit + warn. The reinit warning is a dev-runtime message (not a sema
  diagnostic) — one line, plain voice.
- **Formatter:** `@Persist` on its own line above the binding. fmt STABILITY test
  `fmt_preserves_persist`.
- **Example:** `examples/features/devloop/persist.jet` — a module-level counter
  `@Persist`; documents that it survives reload. Golden example asserts normal
  (non-dev) run output; the reload behavior is covered by a dev-runtime test once
  the E3 machinery lands.
- **Exit:** [ ] `@Persist` parses, module-level only (E0145) [ ] inert in release
  (codegen-parity test) [ ] dev-runtime carry-across wired to E3 reload (gated;
  named) [ ] fmt stability [ ] example + expected [ ] docs row names the E3 gate.

## 10. D-PARSESTR1 — interpolation as a parse pattern  *(Tier 4: reuses interp lexing)*

**Ratified (A).** The same interpolation literal that *formats* a string may sit
in **pattern position** and *match*: it matches the fixed text and binds each
`{hole}` to a name. A typed hole `{id:Int}` matches only what that type accepts
and binds an `Int` (a **fallible** hole). Holes are **non-greedy, anchored by the
literal text** between them; an `else` arm catches non-matches. No new sigils —
reuses interpolation syntax and if-tables.

- **Grammar/lexer:** interpolation is already lexed
  (`crates/jet-lexer/src/Lexer/Strings.rs`, `StrTokPart::Interp`). The new work is
  **parser** (`Parser/Expressions.rs`): accept a string-interpolation literal as a
  **pattern** in an if-table arm head and in a binding-pattern position. A hole
  `{name}` binds `String`; `{name:Type}` is a typed fallible hole. The literal
  segments become anchors.
- **Sema:** each typed hole elaborates to the type's fallible parse (reuse the
  same type-directed parse the language already has for typed input). The match is
  refutable ⇒ requires an `else` arm (if-table) or the fallible form in binding
  position. New:
  - `E0147` (parse) — two holes with no literal text between them (can't anchor).
    what: `` these two `{}` holes have nothing between them to split on ``; why:
    `a pattern splits the text at the fixed characters between holes; back-to-back
    holes are ambiguous`; fix: `put literal text between them, or type them so the
    boundary is unambiguous`. fixture `tests/ui/parse_pattern_adjacent_holes.…`.
  - `E0148` (sema) — a refutable interpolation pattern with no `else`/fallible
    handling. what/why/fix in voice; fixture.
- **Codegen:** dumb — lower to a scan/split against the anchors + per-hole
  fallible parse, producing the bindings (R1). No regex engine.
- **Formatter:** the pattern literal formats identically to a format literal. fmt
  STABILITY test `fmt_preserves_parse_pattern` (assert holes + types survive).
- **Example:** `examples/features/parsing/parse_interpolation.jet` — parse
  `"inc-{id:Int}"` out of `"inc-42"`, print `id`; an `else` arm for a non-match;
  expected out.
- **Exit:** [ ] interp literal parses in pattern position [ ] typed holes fallible
  [ ] non-greedy anchoring [ ] E0147/E0148 fixtures [ ] fmt stability [ ] example
  + expected [ ] docs. **See §17 — the adjacent-hole + bare-binding refutability
  behavior was under-specified; confirm the two rules I chose before shipping.**

## 11. D-TYPEDTEXT1 — typed text `Sql`/`Html` via expected-type elaboration  *(Tier 4: taint model)*

**Ratified (D).** `db.query` takes `Sql`; a **string literal with interpolations**
in that position elaborates to *template + typed bound params* — the **same
expected-type law as `.{ }` construction**. No prefix to learn for the common
call. A runtime `String` value, or concatenation, into a `Sql` position stays a
**type error naming the fix**. `Sql.raw("…")` is the audited expert escape
(greppable, lint-gateable). The prefixes `sql"…"` / `html"…"` remain **only** for
bindings without an expected type. **Only `Sql` and `Html` ship** (extensibility
= E4). The enforcement machinery is the ratified taint/sanitizer model.

- **Grammar:** an optional string prefix `sql`/`html` before a `"…"` literal
  (register `TEXT_PREFIX_SQL`/`TEXT_PREFIX_HTML` in Syntax.rs under D-TYPEDTEXT1 —
  new constants). Lexer: recognize an identifier immediately before an opening
  quote as a text prefix. The common path uses **no prefix** — expected-type
  elaboration at the call site.
- **Sema:** `crates/jet-sema/src/Sema/Taint.rs` + expression checker:
  - a string literal (with or without interpolations) in a position whose expected
    type is `Sql`/`Html` elaborates to that typed value; interpolation holes
    become **typed bound parameters** (Sql) or **escaped insertions** (Html).
  - a runtime `String` (or a concatenation `String + String`) reaching a
    `Sql`/`Html` position is rejected:
    - `E0149` (sema) — a plain `String` can't fill a `Sql`/`Html` position.
      what: `` a runtime `String` can't be used as `Sql` ``; why: `interpolating
      untrusted text into a query is how injection happens; only a checked literal
      or bound parameters may build a `Sql``; fix: `write the query as a literal
      with `{value}` holes (they become bound parameters), or use `Sql.raw("…")`
      if you have audited the text`. fixture `tests/ui/typed_text_string_into_sql.…`.
  - `Sql.raw("…")` is the sole escape — taint-clears by contract (reuse the
    sanitizer mechanism); it is greppable and lint-gateable.
- **Codegen:** dumb — lower `Sql` to (template string, `[params]`) and `Html` to
  the escaped-on-the-way-in string; `db.query(Sql)` passes params separately (R1).
- **Formatter:** preserve the prefix when present; preserve the no-prefix literal.
  fmt STABILITY test `fmt_preserves_typed_text` (assert prefix + holes survive).
- **Example:** `examples/features/safety/typed_sql.jet` — build a `Sql` from a
  literal with an `{id}` hole, show it becomes a bound param, show a `String`
  concat into the query position is a type error (ui fixture); expected out.
- **Exit:** [ ] no-prefix expected-type elaboration to `Sql`/`Html` [ ] typed
  bound params (Sql) / escaping (Html) [ ] `String`→`Sql` E0149 [ ] `Sql.raw`
  escape taint-clears [ ] prefixes work only without expected type [ ] fmt
  stability [ ] example + expected [ ] docs. **See §17 — the "plain literal
  always elaborates?" trigger and prefix-scope need a confirm.**

## 12. D-EFFBUDGET1 — package effect budget  *(Tier 5: pkg.jet manifest, no grammar)*

**Ratified (D).** **Zero config:** every `jet build` prints a **one-line summary**
of the effects the dependency graph uses and records **per-dependency effect
provenance in the lock file**. An `effects: { allow: […], deny: […] }` block in
`pkg.jet` turns on **whole-graph enforcement** — the build fails naming the exact
dependency path and function when a transitive dep needs an effect outside the
list. `grants: { "dep": [Effect] }` is the audited per-dependency escape, recorded
in the lock so exceptions are a diff. Shares the closed **ten-effect vocabulary
(D-EFF4)**. Manifest keys, **not** grammar. (Distinct from open D-BUILDPOLICY1.)

- **Grammar:** none. New `pkg.jet` blocks parsed in
  `crates/jet-driver/src/Jetpack/PackageManifest/ParseBlocks.rs` (see
  `parse_packages`/`parse_target` for the block-parse pattern): `effects { allow:
  […] deny: […] }` and `grants { "dep": [Effect] }`. Effect names validate against
  the D-EFF4 vocabulary (`crates/jet-sema/src/Sema/Effects.rs` — `Effect` enum;
  `Browser` added by D-WASM1).
- **Always-on report:** the driver (`crates/jet-driver/src/Driver/`) aggregates
  each dependency's inferred effect set (sema already types effects per function)
  and prints the one-line summary on every `jet build`; write per-dep provenance
  into the lock file.
- **Enforcement (opt-in):** when `effects:` is present, fail the build if any
  transitive dep uses an effect outside `allow`/inside `deny` and not covered by a
  `grant`. New:
  - `E1220` (jet/jetpack) — a transitive dependency uses an effect outside the
    budget. what: `` `pdf-lib` uses the `Net` effect, which this package's budget
    doesn't allow ``; why: `an `effects:` budget fails the build when any
    dependency reaches an effect you didn't list — supply-chain review as a
    compile error`; fix: `add `Net` to `allow`, or grant it to `pdf-lib` in
    `grants:`, or drop the dependency`. Include the dependency path + offending
    function. fixture under the CLI/manifest ui harness.
  - `E1221` (jet) — malformed `effects:`/`grants:` block (unknown effect name,
    bad shape). Reuse the manifest-error rendering (E1214/E1215 style).
- **Codegen:** none.
- **Example:** `examples/features/packages/effect_budget/` — a `pkg.jet` with an
  `effects: { allow: [Fs] }` budget and a small graph; document the summary line
  and an E1220 rejection. (Manifest example dir, like `50_publishable_pkg`.)
- **Exit:** [ ] always-on summary on every build [ ] per-dep provenance in lock
  [ ] `effects:` enforcement + E1220 (with dep path) [ ] `grants:` escape recorded
  in lock [ ] E1221 malformed-block [ ] effect names validate against D-EFF4
  [ ] example dir + expected [ ] docs.

## 13. D-ERRCTX1 — automatic `?` trace + `.context()`  *(Tier 5: stdlib + E3 trace gate)*

**Ratified (D).** Every `?` crossing is **recorded in dev/debug builds** (cheap
span push) so an unhandled failure prints the **full propagation path with
file:line** — zero user code (Zig-style, but with Jet spans/rendering).
`.context("loading config {path}")` adds a **lazily-evaluated** human boundary
message; the renderer shows both in one chain. **Release** keeps origin + explicit
contexts only (policy may retain full traces). Stdlib method — **no grammar**.
Context-operator-on-`?` (option B) rejected.

- **Grammar:** none.
- **Stdlib:** add `.context(message)` on the fallible/error value (the `Error`
  surface — `core` prelude, `crates/jet-codegen/src/Prelude/CoreLib.rs`). The
  message is a lazily-evaluated interpolation (only formatted if the error
  actually propagates). It attaches a context frame to the error chain.
- **Auto `?` trace:** extend the existing **error-return trace** machinery
  (**E3002**, "error-return trace entry on a `?`-propagated failure, Zig-style")
  — this already exists. Ensure every `?` crossing pushes a span frame in
  dev/debug builds, and the renderer walks the chain (origin → contexts) with
  file:line. **The full trace runtime rides the E3 observability/runtime work** —
  E3002 is already registered; wire `.context()` frames into the same chain and
  the renderer. Name the E3 gate for any piece not yet present.
- **Sema/Codegen:** dumb — `.context()` lowers to an error-map that captures the
  lazy message closure; the trace is a build-mode-gated span push (R1). Release
  drops the per-`?` push, keeps origin + explicit contexts.
- **Diagnostics:** no new sema code (`.context` is an ordinary method; arity
  errors go through the normal call-arity path). The rendered chain reuses the
  E3002 format.
- **Formatter:** `.context("…")` formats as an ordinary method call; no special
  work. (Still add a fmt round-trip check via the example.)
- **Example:** `examples/features/errors/error_context.jet` — a 3-layer `?`
  propagation with a `.context("loading config {path}")` at the boundary; expected
  out shows the chain (dev build) with the context sentence.
- **Exit:** [ ] `.context()` stdlib method, lazy message [ ] auto per-`?` frame in
  dev/debug (E3002 chain) [ ] release keeps origin + explicit contexts
  [ ] renderer shows the chain with file:line [ ] example + expected [ ] docs row
  names any E3 runtime gate.

## 14. D-STREAMYIELD1 — generators `fn ticks() -> Stream<Int>` with `yield`  *(Tier 5: E3 coroutine gate)*

**Ratified (A).** A generator function returns `Stream<T>` and uses `yield` to
hand a value to the consumer and resume where it left off on the next demand.
**One keyword (`yield`), one type (`Stream<T>`); consumers are ordinary loops; no
async/await coloring anywhere.** The runtime is the **E3 structured-concurrency /
coroutine machinery** — this decision fixes only the spelling.

- **Grammar/Syntax.rs:** register `KW_YIELD = "yield"` (new constant, D-STREAMYIELD1)
  and `TYPE_STREAM = "Stream"` (new constant). Parser
  (`Parser/Statements.rs`/`Expressions.rs`): `yield expr` statement, legal only in
  a function whose return type is `Stream<T>`. Add to the keyword/reserved-name
  set.
- **Sema:** a `fn` returning `Stream<T>` is a generator; `yield e` requires `e: T`.
  Consumers iterate a `Stream<T>` with the ordinary `loop x in stream { }`. New:
  - `E0805` (sema) — `yield` outside a `Stream<T>`-returning function. what/why/fix
    in voice; fixture `tests/ui/yield_outside_generator.…`.
  - `E0806` (sema) — a generator mixes `return value` with `yield` (a generator
    body yields; it doesn't return a value). fixture.
  - `E0807` (sema) — a yielded value's type doesn't match `Stream<T>`. fixture.
- **Codegen:** dumb — lower the generator to the E3 coroutine state machine (R7
  TIR seam). **Gate:** the coroutine runtime is E3 work; wire surface + sema + the
  TIR lowering shape now, land the state-machine lowering against the E3 coroutine
  machinery as it arrives. Name the gate in the docs row.
- **Formatter:** `yield expr` on its own line. fmt STABILITY test
  `fmt_preserves_yield`.
- **Example:** `examples/features/streams/generators.jet` — `fn count(n) ->
  Stream<Int>` yielding `0..n`; consume with `loop x in count(3) { print("{x}") }`;
  expected out `0\n1\n2`.
- **Exit:** [ ] `yield` + `Stream<T>` register [ ] generator sema (E0805/E0806/
  E0807) [ ] consumer loop works [ ] TIR lowering to E3 coroutine (gated; named)
  [ ] fmt stability [ ] example + expected [ ] docs.

## 15. (implicit) — the `@`-plane rename touch-points

When you flip the four `@`-plane sections' spellings to PascalCase, update in one
commit with the marker-family rename: `crates/jet-foundation/src/Syntax.rs`
(`CONTRACT_PRE/POST/PERSIST`, `CONTRACT_BUNDLE_*`), any `@`-plane parser table in
`crates/jet-parser`, the formatter emission, all `tests/ui/*.stderr` that mention
these markers, and the `syntax-decisions.md` rows. Re-bless snapshots
(`env UPDATE_EXPECT=1 cargo test`).

## 16. D-UFCS1 = B — no code

Status quo: `fn Type.name(self)` extensions stay the one method spelling. No
parser/sema/codegen change. Optional (nice-to-have, not required by the card): if
a user writes an obvious UFCS chain that fails resolution, the existing "unknown
method" path already teaches. Do **not** add UFCS or `|>`. Record the ratified
row in `syntax-decisions.md` (already present) and move on.

---

## 17. Ambiguities in the ratified text (confirm before shipping the affected ballot)

These are the only places the ratified option text left a behavior genuinely
under-specified. I chose a default for each (used above); flag for owner confirm.

1. **D-PARSESTR1** — option A says holes are *"non-greedy holes anchored by
   literal text; else arm catches non-matches."* It does **not** define (a) two
   holes with **no literal text between them** (`"{a}{b}"`), nor (b) whether a
   **bare binding** use `.{…} #= s` (outside an if-table) is refutable/fallible or
   a hard error on non-match. **My defaults:** (a) adjacent untyped holes = compile
   error `E0147` (unanchorable); (b) a refutable interpolation pattern needs an
   `else` (if-table) or fallible form (`E0148`). Confirm both.

2. **D-TYPEDTEXT1** — option D says *"a string LITERAL with interpolations in that
   position elaborates to template + typed bound params — the same expected-type
   law as `.{ } construction`"* and *"Prefixes `sql"…"`/`html"…"` remain only for
   bindings without an expected type."* Two gaps: (a) does a **plain literal with
   no interpolations** in a `Sql` position also silently elaborate to `Sql` (the
   text says "with interpolations")? (b) Under option D, do the `sql`/`html`
   **prefixes ship at all in v1**, given option A "deferred extensibility to E4"?
   **My defaults:** (a) yes — any string literal (interp or not) in a `Sql`/`Html`
   expected position elaborates; (b) yes — the two fixed prefixes ship (for
   no-expected-type bindings only); user-defined prefixes do not. Confirm.

3. **D-PREPOST1** — option A names *"per-module build-policy strip is the explicit
   opt-out"* but the **build-policy surface is the open ballot D-BUILDPOLICY1** —
   there is no ratified spelling for how a module opts out today. **My default:**
   ship contracts checked-always; leave the strip opt-out unimplemented and gated
   on D-BUILDPOLICY1 (named as a gate), rather than invent a strip spelling.
   Confirm the gate is acceptable (contracts ship enforced-everywhere first).

Not ambiguities, but named gates carried above: **D-CAPBUNDLE1** `#Numeric` vs
`@Numeric` reconciliation rides open **D-MARKERMOVE1** (leave `#Numeric` untouched);
**D-PERSIST1 / D-ERRCTX1 / D-STREAMYIELD1** runtime pieces ride the **E3**
hot-reload / observability / coroutine machinery (surface + sema + dumb-lowering
shape land now).

## 18. Global exit criteria

- [ ] All 15 code-bearing ballots: parser → sema → codegen → diagnostics →
  example → tests → docs, no stubs (E3-gated runtime pieces excepted, each named).
- [ ] Every new keyword/sigil/marker registered in `Syntax.rs` with its decision
  ID (I7).
- [ ] Every new diagnostic has a code + what/why/fix in `docs/spec/diagnostics.md`
  **and** a `tests/ui/` snapshot (I4).
- [ ] Every new syntax has formatter emission **and** a fmt STABILITY test in
  `tests/fmt.rs` (idempotence alone is insufficient — it misses dropped tokens).
- [ ] Every feature ships an example under `examples/features/<topic>/` with an
  expected-output entry the golden tests enforce (I5).
- [ ] The `@`-plane four (D-PREPOST1/D-PERSIST1/D-CAPBUNDLE1 markers) landed after
  `c7markerfamily`, spellings flipped to PascalCase in that rename commit.
- [ ] `nix develop -c cargo build` clean; `nix develop -c cargo test` green
  (snapshots re-blessed only where output genuinely drifted, checked against
  diagnostics.md format).
- [ ] No `unsafe` in any generated example output (golden.rs greps the substring).
- [ ] `docs/spec/syntax-decisions.md` rows for all 16 ballots marked implemented.
