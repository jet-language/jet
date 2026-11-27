# Adversarial Design Review — One `Predicate<T>` Value and `#Check` (`P5` / Part 3.10)

- **Date:** 2026-11-26
- **Scope:** Greenfield Proposal Part 2.4 (`P5`) and Part 3.10 (`Predicates`) — unifying `validate { check(...) }`, inline/distinct refinement checks, query filters, schema constraints, integer prover facts, and property-test generators behind one `Predicate<T>` value and `#Check` marker.
- **Baseline inspected:** `dev` (`b84063a8e0f`)
- **Related audits:** `Docs/Audits/2026-11-24-greenfield-language-review.md` (`C42`, `L1106`), `Docs/Audits/2026-11-26-typed-key-paths-review.md` (`#7008`)
- **Verdict:** **NEEDS BALLOT** — the unification thesis is sound and bootstrap-safe, but four user-visible semantic choices (construction failure mode vs `D-VALIDATE1`/`?`, error path attribution and `Validate.over`, mutation of `#Check` structs, and compile-time vs runtime handling of untranslatable predicates) must be ratified before Wave 1 implementation begins.

---

## 1. Proposal Summary & Baseline Reality Check

The proposal (`Docs/Proposals/2026-11-24-greenfield-language.md`, Part 2.4 `P5` and Part 3.10) proposes replacing Jet's separate rule syntaxes with a single inspectable `Predicate<T>` value and a single `#Check` marker:

```jet
Booking: struct {
    #Check(.nights >= 1, "stay must be at least one night")
    nights: Int,
    start: Date,
    end: Date,
    #Check(.end > .start, "the stay must end after it starts")
}
```

The proposal claims one `#Check` declaration drives seven consumers automatically:
1. **Construction** (`Booking{...}` checked dynamically, or at compile time when inputs are constant).
2. **`Decode`** (`json.decode<Booking>` / streaming `__read` accumulating `[FieldError]`).
3. **HTML forms** (`Core/Web/Forms.jet` client/server field validation).
4. **Database** (`CREATE TABLE ... CHECK(...)` and `Table.where(pred)` SQL pushdown).
5. **OpenAPI / JSON Schema** (`minimum`, `maximum`, `minLength`, `pattern`, etc.).
6. **Integer prover** (`TIntegerBounds` in Sema and offline `jet prove`).
7. **Property tests** (`#Test(for_all: Booking)` generating valid instances).

### Immediate Baseline Corrections on `dev` (`b84063a8e0f`)

1. **`#Invariant` is already retired and deleted.** It has zero compiler/Core presence and is negatively tested as an unknown marker (`E0927`) in `Tests/Cases/RatifiedP14/d_type2_refine1_contract.case:2,40`, `Tests/Cases/RatifiedP14/p14_public_cutover_and_production_bypass.case:8,30,38-41`, and `Compiler/JetCLI/Tests/SyntaxExplain.jet:103`. Today's refinement surface is `Type.InlineRange` (`Int(0..100)`) and `DistinctDef.range` (`distinct Port: Int(1..65535)`), not `#Invariant`.
2. **Actual fragmentation on `dev` is four-way, not three-way:**
   - **Validation blocks + builder:** `validate { check(cond, at: field, "msg") }` (`Sema/Registration/Validate.jet`) and `Validate.over(v).check(...).finish()` (`Core/Encoding/Validate.jet`).
   - **Range refinements:** `Type.InlineRange(base, lo, hi)` and `DistinctDef.range` (`TIntegerBounds`, `ConfigExport.jet`, `FactReads.jet`, `SerdeItems.jet`).
   - **Function contracts + prover:** `#Pre(cond, "msg")` / `#Post(cond, "msg")` (`Sema/Contracts.jet`, `CLI/ProveObligations.jet`).
   - **SQL query filter lifter:** `Query.filter(fn(row) { ... })` lowered via `BuiltinRole.QueryFilter` (`Sema/Calls/DataFilterSQL.jet`, `Core/Data/Data.jet` `__query_filter`).
3. **`#Check` is completely unclaimed as a marker name.** The only `"Check"` identifier in syntax/config is `OutputKind.Check` (`Compiler/JetSema/Source/Sema/Outputs.jet:49,62`, `Compiler/JetFoundation/Data/Syntax/JetpackConfig.jet:254`).
4. **Bootstrap safety is 100% clean.** There are **zero** `validate { ... }` blocks anywhere inside `Compiler/` or `Core/`.

---

## 2. Strongest Existing Solutions & Primary Sources

| System & Primary Source | What It Gets Right | Where It Breaks / Lesson for Jet |
| --- | --- | --- |
| **Liquid Haskell** (Vazou et al., *Refinement Types for Haskell*, ICFP 2014; *Refinement Reflection*, POPL 2018) | Refinements are explicitly restricted to a decidable logic fragment (QF-LIA/EUF) discharged by SMT; arbitrary pure functions require explicit reflection. | Arbitrary language expressions cannot be silently handed to a solver. Jet must define an explicit decidable `PredExpr` fragment rather than pretending any pure `Bool` expression is analyzable. |
| **F\* & Dafny** (Swamy et al., POPL 2016; Leino, *Dafny: An Automatic Program Verifier*, LPAR 2010) | Cleanly separates pure specification predicates from effectful code; proves bounds and arithmetic invariants statically. | **The mutation-invariant trap:** automatic invariant enforcement on every individual field write breaks legitimate two-field updates (`b.start = s; b.end = e`). Dafny explicitly requires either immutable value updates or explicit `Valid()` restoration at method boundaries. |
| **Ada 2012/2022 & Eiffel** (ISO/IEC 8652:2023 §3.2.4 `Static_Predicate`/`Dynamic_Predicate`, §7.3.2 `Type_Invariant`; Meyer, *OOSC* 1988) | Ada separates `Static_Predicate` (decidable constant intervals/sets usable statically) from `Dynamic_Predicate` (arbitrary pure checks on conversion/construction), and checks `Type_Invariant` only at public module boundaries, never on intermediate private field writes. | Validates Jet's need to distinguish decidable predicate structure from arbitrary pure checks, and warns against checking invariants on every mid-update field mutation. |
| **Pydantic v2 / Zod / Valibot** (Colvin; McDonnell; Hiller) | Accumulates *all* failing rules into structured errors with exact field paths (`loc` / `path`) instead of short-circuiting on the first failure; exports declarative constraints (`gt`, `minLength`, `pattern`) to JSON Schema / OpenAPI. | All three explicitly split declarative constraints (schema-exportable) from arbitrary refinements (`model_validator` / `.refine()` — runtime-only). Trying to pretend arbitrary user methods emit JSON Schema produces lying schemas. |
| **C# LINQ `Expression<Func<T, bool>>` & Swift `#Predicate<T>`** (Microsoft ECMA-334; Swift Evolution `SE-0397` *Freestanding Expression Macros* / Foundation `Predicate`) | Reifies a normal-looking predicate expression at compile time into a typed expression tree (`PredicateExpressions`) so one expression can be both evaluated natively and inspected by a query backend. | **SwiftData's runtime-crash lesson:** `SE-0397` permits arbitrary Swift expressions inside `#Predicate`, then SwiftData throws *runtime* errors when a query uses an untranslatable method. Jet must reject untranslatable SQL predicates at **compile time** (resolving `C42`). |
| **PostgreSQL `CHECK` & `WHERE`** (PostgreSQL Documentation §5.4.1 *Check Constraints*, §9.2 *Comparison Functions*) | Database enforces row invariants identically across every writer, not just one application binary. | **Three-valued `NULL` polarity inversion:** SQL `CHECK (x > 0)` **succeeds** when `x IS NULL` (`UNKNOWN` satisfies `CHECK`), whereas `WHERE (x > 0)` **rejects** `NULL`. Collation differences also make SQL `<` on `String` diverge from UTF-8 codepoint comparison. |
| **QuickCheck / Hypothesis / Bolero** (Claessen & Hughes, ICFP 2000; MacIver et al., *Hypothesis*, JOSS 2019) | Hypothesis translates structural constraints (ranges, lengths, choices) into direct constrained draws and shrinks inside the generator's choice stream so shrunk counterexamples stay valid. | Naïve rejection sampling (`suchThat`) collapses exponentially on narrow/equality predicates (`a == b`, `age in 18..21`, `end > start` over wide domains) and can hang or fail before finding 100 samples. |

---

## 3. Complete Live-Code Inventory on `dev` (`b84063a8e0f`)

| Area | File & Line Ranges | Live Symbols & Current Behavior | Treewide Count / Notes |
| --- | --- | --- | --- |
| **Lexer & Syntax Registry** | `Compiler/JetFoundation/Data/Syntax/PackageFiles.jet:192-215`, `Highlights.jet`, `Jetpack/Foundation/Source/SyntaxRegistry.jet:1227-1240`, `Tools/Editors/TreeSitter/grammar.js:18,23` | `KW_VALIDATE_BLOCK = "validate"`, `VALIDATE_CHECK_FN = "check"`. Neither is a hard keyword in `Compiler/JetLexer/Data/Lexer/Keywords.jet`; both are contextual identifiers. | 0 hard lexer keywords to remove; 4 registry/grammar rows to update. |
| **AST & Codec** | `Compiler/JetFoundation/Source/AST/Items.jet:1088-1089,1633-1634`; `Compiler/JetFoundation/Source/Facts/SyntaxCodec.jet:5248,5294` | `StructDef.validate_block: [Stmt]`, `StructDef.validate_span: Span?`. `DistinctDef.range: DistinctRange?` at `AST/Items.jet:1164`. | `validate_block` exists only on `StructDef` (not enums or distincts). |
| **Parser & Formatter** | `Compiler/JetParser/Source/Parser/Items.jet:2205-2211,2626`; `Declarations/Markers.jet:483`; `Formatter/Items.jet:80,128,2352-2358,2607-2638,2762` | Parses contextual `validate { ... }` block inside struct bodies and formats it back. | 0 parser conflicts with `#Check(...)` (standard marker syntax already parsed on types & fields). |
| **Sema `validate` Lowering** | `Compiler/JetSema/Source/Sema/Registration/Validate.jet:1-263`; `Items.jet:1926,4326-4327`; `Expressions.jet:4905-4919`; `Checked.jet:2497`; `Derives.jet:1788`; `CoreUsage.jet:417,992`; `Casing.jet:258`; `GrammarItems.jet:340` | `sema_validate_blocks` synthesizes pure `impl T { validate: fn(value: T) -> T [FieldError]! }`, accumulating all failing `check(cond, at: field, "msg")` rules. `sema_validate_over_rule` rewrites bare `at: field` on `Validate.over(v).check(...)` into a string literal. | 263 LOC in `Validate.jet`. Diagnostics: `E0353` (bad rule shape), `E0354` (unknown `at:` field) in `Diagnostics.jet:5756-5790` & `DiagnosticRows.jet:5103-5135`. |
| **Serde Decode Hook** | `Compiler/JetSema/Source/Sema/Registration/SerdeItems.jet:550-565,1011-1021,1149-1156` | Tree decode (`sema_serde_items_struct_decode_body`) and streaming `__read` (`sema_serde_items_struct_read_body`) construct `decoded: T{...}` and call `T.validate(decoded)` if `validate_block.len() > 0`. Distinct decode checks `definition.range`. | Decode only runs `T.validate` after field-level decode succeeds (`jet_serde_errors.is_empty()`). |
| **Core Validation Builder** | `Core/Encoding/Validate.jet:1-32` | `Validate<T>` struct with `Validate.over(value)`, `.check(cond, at: String, reason: String)`, `.finish() -> T [FieldError]!`. | Used in `Examples/Data/Validate.jet:31` and `Tests/UI/validate_over_unknown_field.jet:7`. |
| **SQL Query Filter Lifter** | `Compiler/JetSema/Source/Sema/Calls/DataFilterSQL.jet:1-298`; `Core/Data/Data.jet:1606`; `Core/DB/DB.jet:1032,1443,1554,1673-1773,1873,2156`; `Core/DB/Engine/Parse.jet:72,535,611`; `Compiler/JetCLI/Source/CLI/InspectChoices.jet:43,256-260` | Translates `Query.filter(fn(row) { ... })` into `SQL` over `"jet_row.<field>"` columns + bound `[String]` values + fallback closure via `BuiltinRole.QueryFilter`. SQL parser already parses `CHECK (...)`. | **Critical gap:** `DataFilterSQL.jet` only handles `field <op> non_row_val` on unrenamed `Int`/`String`/`Bool` fields (cannot handle `.end > .start`, `!`, `Option`, nested fields), and silently emits `""` (full table scan) otherwise. |
| **Contracts & Integer Prover** | `Compiler/JetSema/Source/Sema/Contracts.jet:1-1322`; `Scopes.jet:124` (`TIntegerBounds`); `Statements/Refutable.jet:1411-1537`; `CostTree.jet:31-90`; `Compiler/JetCLI/Source/CLI/ProveObligations.jet:1-598` | `#Pre`/`#Post` emit runtime checks (or comptime `#run` checks). `TIntegerBounds` tracks scalar `Int`/`IntN`/`InlineRange` intervals. `ProveObligations.jet` solves straight-line affine integer formulas (`fixed_index_bounds`, `call_precondition`, `function_postcondition`) and marks non-affine formulas `unsupported_formula`. | `ProveObligations.jet` currently has **no** struct `.field` term support (`L157`) and no type-invariant assumptions. |
| **JSON Schema / OpenAPI / Forms** | `Compiler/JetCLI/Source/CLI/ConfigExport.jet:528-698`; `Core/Web/Forms.jet:122,438,652`; `Core/Web/Web.jet:3982-4045`; `Compiler/JetSema/Source/Sema/Expressions/FactReads.jet:344,1566` | `ConfigExport.jet` emits JSON Schema `minimum`/`maximum` from `Type.InlineRange` (`L544-548`) and `DistinctDef.range` (`L626-634`). `WebFormTyped<Model>.validate` takes `FieldRef<Model, T>` and a manual `fn(String) E!`. `web.openapi(...)` renders route contracts. | None of `ConfigExport.jet`, `Forms.jet`, or `web.openapi` read `validate { }` today. |
| **Testing / Property Generation** | `Core/Testing/Testing.jet` | Exports `Fake` (`fake_data(seed)`), `HistoryStrategy<Command>` with `HistoryPrecondition` + `testing_history_shrink`, `corpus`, `snap`, `golden`. | No `#Test(for_all: T)` generator exists on `dev` yet. |
| **In-Repo Call Sites (`validate {`)** | `Examples/Data/Validate.jet:12`, `Examples/Data/Serde/CodecFieldErrors.jet:67`, `Tests/UI/validate_{bad_rule_shape,impure_rule,unknown_field,over_unknown_field}.{jet,stderr}`, `Tests/Cases/UILintStyle/RedundantFinalReturnSkipsValidateBlock.fixture/main.jet:6`, `Tests/Cases/CoreLib/Fixtures/GeneratedStructDecodeAccumulatesNestedErrorsAndValidation/fixture.jet:20`, `Docs/Spec/spec.md:1300-1306`, `Compiler/JetCLI/hot-text.list:1651,2715` | All live uses of `validate { }` / `Validate.over` across the entire repository. | **0** in `Compiler/`, **0** in `Core/`, **0** in `Jetpack/`, **2** examples, **4** UI tests, **2** case fixtures, **1** spec section. |

---

## 4. Adversarial Attacks & Edge-Case Analysis

### Attack 1 — `#Invariant` Is Already Gone, While `Validate.over(value)` Is Overlooked
The card title says "`#Check` replaces `validate`, `check`, and `#Invariant`". On `dev`, `#Invariant` is already deleted and negatively tested (`d_type2_refine1_contract.case`). Conversely, `Core/Encoding/Validate.jet` (`Validate.over(value).check(cond, at: field, "msg").finish()`, exercised in `Examples/Data/Validate.jet:31` and `Tests/UI/validate_over_unknown_field.jet:7`) **is** live and solves a real case that type-attached `#Check` cannot express: **context-dependent validation** (e.g., `allow_short || signup.password.len() >= 12` depending on a runtime caller flag or database lookup).
- **Failure if ignored:** Deleting `validate` without deciding `Validate.over(value)` leaves behind a stray special-cased Sema hook (`Expressions.jet:4905-4919` rewriting bare `at: field` on `Validate.over`) that contradicts the "one predicate mechanism" invariant.
- **Fix:** Either (a) keep `Validate.over(value)` as a pure library builder migrated from bare `at: field` magic to typed key paths (`at: .password` once `#7008` lands, or `Predicate<T>` combinators `.check(pred)`), or (b) replace it with explicit `Predicate<T>` composition (`check_all(signup, [pred1, pred2])`). This belongs in Ballot Q2.

### Attack 2 — Construction Semantics Collide Directly With `D-VALIDATE1` and Unconditional `?` (`#7034`)
Proposal Part 3.10 states: *"construction: `Booking{…}` checks it; when the inputs are constants, the compiler checks it instead."*
This sentence hides a massive semantic fork that collides with both `D-VALIDATE1` (accumulated `[FieldError]` on decode/validation) and `#7034` (unconditional failure marks `?` on every fallible expression):
- **Option A — Fallible construction (`Booking{...}` returns `Booking [FieldError]!`):** Every struct literal of a `#Check` type becomes a fallible expression requiring `Booking{...}?` or `Booking{...} ?? ...` (even inside tests, fixtures, and `SerdeItems.jet` generated decode!). Worse: if a struct has `#Check` added later, every construction site in the codebase changes type from `Booking` to `Booking [FieldError]!`. On the upside, it makes invalid `Booking` values **unrepresentable in safe code** — the core promise of refinement types — and matches `T.new(...)` / `Port(80)?` ergonomics.
- **Option B — Invariant construction (`Booking{...}` returns `Booking`, panics on dynamic violation; compile-time error on constant violation; `Booking.check(v)` / `Decode` return `Booking [FieldError]!`):** Keeps `Booking{...}` ergonomic and infallible in types, guarantees no invalid `Booking` ever exists at runtime, AND lets constant literals like `Booking{nights: 0, ...}` fail at compile time. **Critical trap:** `SerdeItems.jet` (`L1009`, `L1147`) currently builds `decoded: T{...}` *before* calling `T.validate(decoded)`. Under Option B, generated `Decode` MUST validate fields/checks *before* constructing `T` (or invoke an internal compiler-only unchecked constructor only after all checks pass) so malformed JSON returns `Err([FieldError])` instead of panicking!
- **Option C — Construction stays unchecked; `#Check` runs only on `Decode`, `T.check(v)`, Forms, SQL `CHECK`, and `#Test`:** Preserves `D-VALIDATE1` (`Examples/Data/Validate.jet:51` constructs `Signup{email: "", password: "short"}` to pass to `Signup.validate(signup)`), zero surprise panics, zero `?` churn — **but** completely breaks the proposal's own claim that construction and the integer prover can trust every `Booking` value satisfies `#Check`! (If any caller can construct `Booking{nights: -5}`, the integer prover **cannot** assume `b.nights >= 1` for arbitrary `b: Booking` — soundness violation!)

**Soundness rule:** Only Options A and B allow `TIntegerBounds` and `jet prove` to assume `#Check` holds on every parameter `b: Booking`. Under Option C, `#Check` is a validation schema, NOT a type invariant, and the prover MUST NOT assume it holds on arbitrary values. This is **Ballot Q1**.

### Attack 3 — Mutation After Construction (`^b.end = earlier_date`)
Suppose `Booking` carries `#Check(.end > .start, ...)` and a caller holds a mutable `^b: Booking` and shifts a trip earlier by one week:
```jet
^b.start = b.start.minus_days(7)  // OK
^b.end = b.end.minus_days(7)      // OK
```
Now shift the trip *later* by one week in the same order:
```jet
^b.start = b.start.plus_days(7)   // INTERMEDIATE STATE VIOLATES .end > .start!
^b.end = b.end.plus_days(7)       // Valid again, if we got here
```
If `#Check` is enforced on every field assignment, the first line fails even though the logical update is valid. If `#Check` is *not* enforced on field assignment, any caller can break the invariant via `^b.nights = 0` and violate soundness for the integer prover (`b.nights >= 1` becomes a lie!).
- **How prior art solves this:**
  1. **Whole-value update / functional copy (`b = Booking{..b, start: s, end: e}` or `b.with(...)`)**: single atomic check at construction; direct mutation of `#Check`-constrained fields is either forbidden (`E03xx`) outside the defining module or requires an atomic `update ^b { ... }` / struct-update expression.
  2. **Boundary checking (Ada `Type_Invariant`)**: check at public method exits; breaks down in Jet where structs have public fields (`b.start`) mutable by callers.
  3. **Per-field check only for single-field `#Check`, atomic update required for multi-field `#Check`**: cleanly handles `^b.nights = 3` (checked immediately) while preventing half-updated cross-field breaks on `.start`/`.end`!
This is **Ballot Q3**.

### Attack 4 — Multi-Field `#Check` Error Path Attribution & Accumulation
In `validate { check(password != email, at: password, "password can't be the email") }` (`Examples/Data/Validate.jet:15`), the author explicitly declared `at: password` so `FieldError.path == "password"` and HTML forms highlight the password input.
In the proposal:
```jet
Booking: struct {
    #Check(.nights >= 1, "stay must be at least one night")
    nights: Int,
    start: Date,
    end: Date,
    #Check(.end > .start, "the stay must end after it starts")
}
```
- **Field-level `#Check`** on `nights` unambiguously attributes to `path: "nights"` (and in nested structs `stay.nights` via `SerdeItems` path prefixing, preserving `GeneratedStructDecodeAccumulatesNestedErrorsAndValidation`).
- **Type-level `#Check(.end > .start, ...)`** has *no* `at:` argument. What is `FieldError.path`?
  - `""` (form/object-level error, matching `WebFormErrorState.form: [String]` in `Core/Web/Forms.jet:188`)?
  - `"end"` (heuristic: first field path on the left-hand side — brittle if written `.start < .end`!)?
  - Optional `at: .end` keyword argument (`#Check(.end > .start, at: .end, "...")`), defaulting to `""` (root object) when omitted?
- **Accumulation invariant:** Multiple `#Check` markers on fields and the enclosing struct MUST evaluate in deterministic declaration order (field checks in field order, then struct-level checks in declaration order) and accumulate *every* failing `FieldError` into `[FieldError]`, never short-circuiting on the first failure (preserving `D-VALIDATE1`).
- **Single-field syntax sugar:** On a field `#Check`, should the receiver be `.` (the field value itself, e.g. `#Check(. >= 1, "...")`) or `.nights` (the enclosing struct, as in the proposal's `#Check(.nights >= 1, ...)` on `nights`)? Notice the proposal wrote `#Check(.nights >= 1, ...)` *above* `nights: Int`, which treats `#Check` uniformly as `Predicate<Booking>` everywhere — meaning a reusable predicate `POSITIVE: Predicate<Int>` (`#Check(POSITIVE, ...)` on `nights: Int`) wouldn't typecheck unless field-attached `#Check` expects `Predicate<FieldType>`! That's a real type mismatch in the proposal's own example (`nights: Int` with `#Check(.nights >= 1)` has type `Predicate<Booking>`, not `Predicate<Int>`).
This is **Ballot Q2**.

### Attack 5 — Dual `Predicate<T>` Representation & I9 Parity (`O0`/`O1`/`O2` / Comptime / Web)
The proposal says a predicate is *"one value, `Predicate<T>` — a small inspectable expression tree, not an opaque closure"* yet also gives examples like `#Check(.address.country.ships_to(), "we do not ship to this country")` which invokes an arbitrary user method `ships_to()`.

You **cannot** turn arbitrary Jet method calls into a small inspectable AST without either (a) inventing a whole second interpreter/IR inside `Core` (violating I9 parity and duplicating codegen), or (b) making `Predicate<T>` fail to compile on `.ships_to()`.

**Clean, I9-preserving architecture — Dual Representation (`PredExpr` + `eval`):**
Every `#Check` or `Predicate<T>` literal compiles to a single immutable value with two synchronized faces:
1. **`expr: PredExpr`** — a closed, serializable algebraic AST for the **decidable fragment**:
   - Boolean connectives: `And(PredExpr, PredExpr)`, `Or(PredExpr, PredExpr)`, `Not(PredExpr)`
   - Comparisons: `Cmp(PredCmpOp, PredTerm, PredTerm)` (`Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`)
   - Range / membership: `InRange(PredTerm, lo: PredConst, hi: PredConst, inclusive_hi: Bool)`, `InSet(PredTerm, [PredConst])`
   - String / collection structure: `StrLen(PredTerm, PredCmpOp, PredTerm)`, `StrContains` / `StrStartsWith` / `StrEndsWith`, `StrMatches(PredTerm, pattern: String)` (regex literal), `ListLen(PredTerm, PredCmpOp, PredTerm)`
   - Optional presence: `IsSome(PredTerm)`, `IsNone(PredTerm)`
   - Terms: `Self`, `Field(PredPath)`, `Const(PredConst)`, `Param(index: Int)` (captured runtime values), `Add`/`Sub`/`MulConst` (affine integer terms)
   - Explicit escape hatch: **`Opaque(label: String)`** — represents any valid pure Jet `Bool` subexpression outside the decidable fragment (e.g., `.address.country.ships_to()`).
2. **`eval: fn(T) -> Bool Never!`** (or a captured-env pure thunk) — compiled by the normal Jet compiler pipeline (`JetSema` -> `JetIR` -> `JetCodegen` / Cranelift / LLVM / Wasm / comptime VM).

**Why this guarantees I9 parity and zero silent drift:**
- Runtime evaluation (`Decode`, `T.check`, construction, Forms fallback) **always** runs `eval` — identical behavior across `O0`, `O1`, `O2`, comptime `#run`, and Web/Wasm.
- Static/external consumers inspect `expr: PredExpr` with **explicit, honest capability contracts** (zero silent fallback, zero unsound approximations):
  - **SQL `Table.where(pred)` / `CREATE TABLE CHECK`:** Requires `expr` to contain **zero `Opaque` nodes** and only SQL-supported operators/types; otherwise emits a **compile-time error** explaining exactly which subexpression cannot lower to SQL (eliminating `DataFilterSQL.jet:16`'s silent full-table scan and directly resolving `C42`).
  - **JSON Schema / OpenAPI (`ConfigExport.jet`, `web.openapi`):** Emits standard JSON Schema keywords (`minimum`, `maximum`, `exclusiveMinimum`, `minLength`, `maxLength`, `pattern`, `enum`) for decidable field predicates; emits non-enforcing `"x-jet-check": [...]` metadata for cross-field or `Opaque` predicates so schemas never lie.
  - **Integer Prover (`TIntegerBounds` & `ProveObligations.jet`):** Consumes affine/interval `PredExpr` nodes; explicitly marks `Opaque` or nonlinear nodes `unsupported_formula` (matching `ProveObligations.jet`'s existing honesty contract).
  - **Property Generator (`#Test(for_all: T)`):** Solves decidable `PredExpr` domains constructively (ranges, lengths, linear orderings) and falls back to bounded rejection sampling (with explicit discard limit and clear failure diagnostic) only for `Opaque` / complex clauses.

Whether `Opaque` is allowed inside `#Check` (accepted by runtime/`Decode`, rejected at compile time by SQL `where`/`CHECK`) or forbidden in `Predicate<T>` altogether is **Ballot Q4**.

### Attack 6 — SQL Translation Traps (3-Valued `NULL`, Collation, Captures, Renames & Migrations)
Inspecting `Compiler/JetSema/Source/Sema/Calls/DataFilterSQL.jet` and `Core/DB/DB.jet` reveals five concrete traps if `Predicate<T>` drives SQL `CHECK` and `WHERE`:
1. **3-Valued `NULL` Polarity Inversion:** In SQL (SQLite and PostgreSQL alike), `NULL > 0` evaluates to `UNKNOWN`. A `CHECK (x > 0)` constraint **accepts** `UNKNOWN` (so `NULL` slips past `CHECK`!), whereas `WHERE (x > 0)` **rejects** `UNKNOWN`. For any `Option<T>` (`T?`) field, `PredExpr` MUST emit explicit `IS NOT NULL` / `IS NULL` guards matching Jet's `.Val(x)` / `None` semantics — never bare SQL `col = ?` or `col <> ?` on nullable columns!
2. **String Ordering & Collation:** `DataFilterSQL.jet:46,190` intentionally forbids `<`, `<=`, `>`, `>=` on `String` (`ordered: false`) because SQL collation can differ from Jet's UTF-8 byte/codepoint order. SQL `WHERE`/`CHECK` lowering must either restrict string comparisons to `=`/`<>`/`LIKE`/`LENGTH` or emit explicit `COLLATE BINARY` where supported and verify parity in DB tests.
3. **Cross-Field Predicates & Unary `!`:** `DataFilterSQL.jet:162-175` currently forbids comparing two row fields (`.end > .start` returns `None`!) and does not handle `UnOp.Not`. Lowering from `PredExpr` instead of ad-hoc lambda pattern matching lifts both cleanly to `"jet_row.end" > "jet_row.start"` and `NOT (...)` (with NULL guards).
4. **Captured Locals & SQL Injection:** Dynamic predicates like `Predicate<Booking>(.nights >= min_nights)` must bind `min_nights` via `PredTerm.Param(i)` -> `?` SQL bound parameters (extending `DataFilterSQL.jet`'s `[Expr]` parameter array), NEVER string interpolation.
5. **`#Rename` and `migration` Blocks (`DataFilterSQL.jet:91-99,130-141`):** A migrated table can hold rows encoded under older field shapes where `"jet_row.<field>"` reflects a previous schema version. SQL pushdown MUST either resolve `#Rename` wire column names from `field.shape_names` and refuse pushdown (or require migrated storage columns) on active `migration` tables — never silently return wrong rows.

### Attack 7 — Integer Prover, `InlineRange` (`Int(0..150)`), and `Distinct<T>` (`#7003`) Unification
The first greenfield review (`Docs/Audits/2026-11-24-greenfield-language-review.md:1106`) left open a duplication risk: *"Range types and `Distinct<T>` with `#Check`: two ways to say 'an `Int` with a rule'."*
With `PredExpr`, the relationship becomes completely clean and non-duplicative:
- **`Int(0..150)` (`Type.InlineRange`)**: anonymous **structural** range refinement on `Int` — no nominal wrapper, compatible with `Int` arithmetic and bounds tracking, desugars to `PredExpr.InRange(Self, 0, 150, true)`.
- **`Distinct<Int>` + `#Check`** (replacing `distinct Port: Int(1..65535)` alongside `#7003`): **nominal** newtype with zero implicit conversion to/from `Int`, carrying the exact same `PredExpr.InRange(Self, 1, 65535, true)`.
- **Unified downstream pipeline:** `TIntegerBounds` (`Scopes.jet:124`), `ConfigExport.jet` (`minimum`/`maximum`), `FactReads.jet` (`T.reflect().range`), and `ProveObligations.jet` all read `PredExpr` from one shared helper instead of maintaining three parallel code paths (`InlineRange` vs `DistinctDef.range` vs `validate_block`).
- **Prover extension required:** `ProveObligations.jet:157` currently only maps local scalar variable names to affine variables. To prove `#Check(.end > .start)` or eliminate array bounds checks from `#Check(.index < .len)`, Wave 2 must add immutable field-path variables (`booking.start`, `booking.end`) to `ProveObligations.jet` and inject `#Check` assumptions on function entry (only sound if Q1 chooses Option A or B and Q3 prevents unchecked mutation!).

### Attack 8 — Property Testing (`#Test(for_all: T)`) Generator Narrowing & Shrinking
The proposal claims `#Test(for_all: Booking)` automatically generates valid `Booking` values.
- **Why naïve rejection sampling fails:** Generating random `Int`s (`i64`) and filtering by `#Check(.nights >= 1 && .nights <= 30)` succeeds with probability $\approx 1.6 \times 10^{-18}$ — it will hang forever. Even `#Check(.end > .start)` wastes 50% of draws per clause; 10 clauses reject 99.9% of draws.
- **Required two-tier generator strategy (Wave 3):**
  1. **Constructive domain narrowing from `PredExpr`:** Extract per-field bounds (`InRange`, `Cmp` against constants -> `[lo, hi]`), string length/prefix/regex bounds (`StrLen`, `InSet`), and topological DAG order for cross-field inequalities (`start` sampled first, then `end` sampled from `(start + 1)..max_date`).
  2. **Bounded rejection fallback for `Opaque` / cyclic clauses:** Apply `eval(candidate)` with a deterministic discard budget (e.g., `max_discards = 10 * target_samples`); if exhausted, fail the test loudly with a diagnostic pointing at the unsatisfiable or too-narrow `#Check` predicate — never loop infinitely.
  3. **Invariant-preserving shrinking:** Every shrink step MUST re-verify `eval(shrunk)` before accepting the smaller counterexample, so a failing test never reports a shrunk `Booking{nights: 0}` that violates `#Check` and could never occur in production.

### Attack 9 — Cross-Card Seams (`#7008` `KeyPath`, `#Secret` Taint, Purity & `FieldError`)
1. **Decoupling from `#7008` (`KeyPath<T, V>`):** `#7008` is currently `NEEDS BALLOT` (Q1 `Path` vs `KeyPath`, Q2 read-only vs writable, Q3 literal spelling). `#7006` Wave 1 should NOT block on `#7008`: define `PredPath` (`segments: [String]`, resolved & typechecked against struct fields in Sema) internally inside `Predicate<T>`, and bind `.field` inside `#Check(...)` directly in Sema. Once `#7008` is ratified, `PredPath` becomes a zero-cost view over `KeyPath<T, V>` and `Predicate<T>` accepts `KeyPath<T, V>` operands directly.
2. **Purity (`E3401`):** Every `#Check` and `Predicate<T>` MUST be strictly pure (`Never!`, no IO/Clock/Net/State effects), preserving `Tests/UI/validate_impure_rule.jet`. Nondeterministic checks (e.g., `.start >= Clock.now()`) are forbidden in `#Check` — they belong in request-time handlers or context builders (`Validate.over`), because a value valid at `t0` would silently become invalid at `t1` and break SQL `CHECK`, caching, and prover soundness.
3. **`#Secret` Taint Preservation:** `Examples/Data/Validate.jet:14` validates a password (`password.len() >= 12`). A `#Check` reading a `#Secret` field MUST only emit its static message string into `FieldError.reason` (never interpolate field values into auto-generated messages or property-test failure dumps), and SQL `WHERE`/`CHECK` generation must refuse pushing `#Secret` fields into plaintext index/filter columns unless explicitly opted in.
4. **`FieldError` Wire Compatibility:** `FieldError{path: String, reason: String}` stays unchanged, so existing JSON/HTTP/Form error consumers keep working without wire churn.

---

## 5. Open Surface Questions Requiring Owner Ballot

### Q1 — Construction Semantics of `#Check` Types (Soundness vs `D-VALIDATE1` / `#7034` `?`)
When a struct `Booking` has `#Check(...)`, what does a struct literal `Booking{nights: n, start: s, end: e}` evaluate to?
- **Option A — Fallible construction (`Booking [FieldError]!`)**: `Booking{...}` returns `Booking [FieldError]!` (requiring `?` or `??` on dynamic inputs; constant inputs checked at compile time and infallible, or `Booking.new(...)` fallible). Guarantees invalid `Booking` values are unrepresentable *and* never panic; prover can assume `#Check` holds on every `Booking`. Tradeoff: adding `#Check` to a struct makes every non-constant struct literal return a `Result` and require `?`/`??`.
- **Option B — Invariant construction (panics on dynamic violation; compile-time error on constant violation) + fallible `Booking.check(...)` / `Decode`**: `Booking{...}` stays typed `Booking` (no `?` churn on literals), checks constants at compile time, panics (`Jet.InvariantViolation`) on dynamic violation, while `json.decode<Booking>` and `Booking.try_new(...)` / `Booking.check(...)` return `Booking [FieldError]!` without panicking. Guarantees invalid `Booking` values cannot exist, so the integer prover can trust `#Check`. Tradeoff: dynamic struct literals can panic if caller didn't validate untrusted input first.
- **Option C — Unchecked construction (`Booking` infallible); `#Check` runs only on `Decode`, explicit `Booking.check(v)`, Forms, SQL `CHECK`, and `#Test`**: Preserves `D-VALIDATE1` (`Signup{email: "", ...}` allowed as an unvalidated draft struct) with zero panics and zero `?` churn. Tradeoff: **breaks soundness for `TIntegerBounds` / `jet prove`** (prover cannot assume arbitrary `b: Booking` satisfies `#Check`) and breaks the proposal's construction claim.

**Recommendation:** **Option A** (or Option A with compile-time-verified constant literals eliding the `Result` wrapper / returning `Booking` directly when statically proven). Under Jet's explicit-failure philosophy (`#7034`), silent runtime panics on user data (Option B) undermine `?`, whereas unchecked construction (Option C) breaks the prover and makes invalid states representable. If constant-proven literals remain infallible, tests and fixtures stay ergonomic while dynamic construction stays honest and prover-sound.

### Q2 — Field-Level `#Check` Receiver, Multi-Field `at:` Path Attribution, and `Validate.over`
1. **Field-level `#Check` receiver:** Should `#Check` placed on field `nights: Int` operate on the **field value** (`#Check(. >= 1, "...")`, typed `Predicate<Int>`, enabling reusable `Predicate<Int>` constants) or on the **enclosing struct** (`#Check(.nights >= 1, "...")`, typed `Predicate<Booking>`, matching the proposal's literal snippet)?
2. **Type-level `#Check` error path:** For `#Check(.end > .start, "...")`, should `FieldError.path` default to `""` (object-level error) with an optional `at: .end` parameter to target a specific form field?
3. **`Validate.over(value)` (`Core/Encoding/Validate.jet`):** Keep as a library builder for context-dependent rules (migrating `at: field` to `.field` key path once `#7008` lands), or retire it alongside `validate { }`?

**Recommendation:** Field-level `#Check` takes `Predicate<FieldType>` (`#Check(. >= 1, ...)` on `nights: Int`), struct-level `#Check` takes `Predicate<StructType>` (`#Check(.end > .start, at: .end, ...)` with optional `at:`, defaulting to `""`), and keep `Validate.over(value)` migrated to typed paths/predicates for caller-context rules (like `allow_short`).

### Q3 — Mutation of Structs Carrying `#Check` (`^b.start = ...`)
If Q1 chooses Option A or B (so `#Check` is a true type invariant):
- **Option A — Single-field `#Check` re-checked on field write; fields participating in a multi-field `#Check` cannot be mutated individually** (must update atomically via struct update / copy construction `b = Booking{..b, start: s, end: e}?`, or inside a `b.update { ... }` block checked once on exit).
- **Option B — Any struct with `#Check` is immutable-by-field outside its defining module** (callers construct updated values via struct-update syntax `Booking{..b, ...}?`).
- **Option C — Mutation does not re-check `#Check`** (only checked on construction/decode — note: this immediately invalidates prover soundness on any value reachable through `^`!).

**Recommendation:** **Option A** (or **Option B** for simplicity in Wave 1, relaxing to atomic update blocks in Wave 2). Never Option C if the integer prover assumes `#Check`.

### Q4 — Untranslatable Predicates (`Opaque` Subexpressions) in `Predicate<T>`
When a predicate invokes an arbitrary pure Jet method (like `.address.country.ships_to()`):
- **Option A (Two-tier, strict per consumer — Recommended):** Allow arbitrary pure `Bool` expressions in `#Check` / `Predicate<T>` (stored as `PredExpr.Opaque` + compiled `eval` thunk). Runtime checks, `Decode`, Forms, and bounded-rejection property tests accept them; **SQL `Table.where(p)` and SQL `CHECK` emit a compile-time error** pointing at the untranslatable subexpression (zero silent full-table scan); JSON Schema emits `"x-jet-check"` metadata; `jet prove` reports `unsupported_formula`.
- **Option B (Strict decidable-only `Predicate<T>`):** Reject arbitrary method calls inside `#Check` / `Predicate<T>` at compile time everywhere; complex rules must be ordinary functions.

**Recommendation:** **Option A** — it keeps real-world domain checks like `.ships_to()` and custom checksums ergonomic in `#Check` and `Decode`, while strictly preventing silent SQL full-table scans (`C42`) and lying schemas.

---

## 6. Binding Cutover List & Phased Implementation Plan

### Binding Cutover List (Delete / Migrate on Cutover — Zero Dual Surface)

| Category | Exact Files on `dev` (`b84063a8e0f`) | Action |
| --- | --- | --- |
| **AST & Serialization** | `Compiler/JetFoundation/Source/AST/Items.jet:1088-1089,1633-1634`; `Facts/SyntaxCodec.jet:5248,5294` | Remove `StructDef.validate_block` / `validate_span`; represent `#Check` predicates cleanly in AST + codec. |
| **Parser & Formatter** | `Compiler/JetParser/Source/Parser/Items.jet:2205-2211,2626`; `Declarations/Markers.jet:483`; `Formatter/Items.jet:80,128,2352-2358,2607-2638,2762` | Delete contextual `validate { }` parsing and formatting. |
| **Sema & Registration** | `Compiler/JetSema/Source/Sema/Registration/Validate.jet` (replace/rewrite), `Items.jet:1926,4326-4327`, `SerdeItems.jet:1011-1021,1149-1156`, `Checked.jet:2497`, `Derives.jet:1788`, `CoreUsage.jet:417,992`, `Casing.jet:258`, `GrammarItems.jet:340`, `Expressions.jet:4905-4919` | Replace `sema_validate_blocks` with `#Check` / `Predicate<T>` lowering; wire `SerdeItems` tree + streaming decode to `#Check` validation; migrate or retire `Validate.over` hook per Q2. |
| **Diagnostics & Syntax Registries** | `Compiler/Prelude/Diagnostics.jet:5756-5790`, `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet:5103-5135`, `Data/Syntax/PackageFiles.jet:192-215`, `Data/Syntax/Highlights.jet`, `Jetpack/Foundation/Source/SyntaxRegistry.jet:1227-1240`, `Tools/Editors/TreeSitter/grammar.js:18,23`, `Compiler/JetCLI/hot-text.list:1651` | Replace `E0353`/`E0354` (or migrate messages to `#Check` diagnostics) and remove `validate`/`check` contextual syntax entries; register `#Check` marker in `MarkerNames.jet` / `MarkerHooks.jet` / `PreludeMarkers.jet`. |
| **SQL Query Filter Lifter** | `Compiler/JetSema/Source/Sema/Calls/DataFilterSQL.jet` | Unify under `PredExpr -> SQL` compiler (Wave 2) and eliminate silent empty-condition fallback on `Table.where(Predicate<T>)`. |
| **Core Library** | `Core/Encoding/Validate.jet`, `Core/Data/Data.jet`, `Core/DB/DB.jet`, `Core/Web/Forms.jet`, `Core/Web/Web.jet`, `Core/Testing/Testing.jet` | Add `Core` `Predicate<T>` / `PredExpr` types; migrate `Validate.over` per Q2; wire `DB`/`Forms`/`OpenAPI`/`Testing` across Waves 1–3. |
| **Examples, Tests & Spec** | `Examples/Data/Validate.jet`, `Examples/Data/Serde/CodecFieldErrors.jet`, `Tests/UI/validate_{bad_rule_shape,impure_rule,unknown_field,over_unknown_field}.{jet,stderr}`, `Tests/Cases/UILintStyle/RedundantFinalReturnSkipsValidateBlock.fixture/main.jet`, `Tests/Cases/CoreLib/Fixtures/GeneratedStructDecodeAccumulatesNestedErrorsAndValidation/fixture.jet`, `Docs/Spec/spec.md:1300-1306` | Migrate all live examples, UI tests, CoreLib fixtures, and spec sections to `#Check` + `Predicate<T>` in Wave 1; add negative test confirming `validate { }` is cleanly rejected. |

### Clean Wave Plan (Post-Ballot)

- **Wave 1 — Core `Predicate<T>`, `#Check` Cutover, Construction/Decode & JSON Schema (`ConfigExport.jet`)**
  - Define `Predicate<T>` + `PredExpr` + `PredPath` (decoupled from `#7008`'s pending ballot).
  - Register `#Check` marker on structs, fields, and `Distinct<T>` (enforcing purity `E3401`, comptime constant evaluation, and Q1/Q2/Q3 semantics).
  - Wire `SerdeItems.jet` (tree + streaming decode) and `ConfigExport.jet` (JSON Schema `minimum`/`maximum`/`minLength`/`maxLength`/`pattern`/`enum` + `x-jet-check`).
  - **Complete single-shot cutover:** delete `validate { }` / `check(...)` syntax, update all registries/grammars/examples/tests/spec, verify I9 parity across `O0`/`O1`/`O2`/comptime.
- **Wave 2 — SQL `CHECK`/`WHERE`, Integer Prover, Forms & OpenAPI**
  - Lower `PredExpr` to SQL `CHECK (...)` (with explicit `IS NOT NULL` polarity handling for `Option<T>`) and `Table.where(Predicate<T>)` (replacing `DataFilterSQL.jet` with compile-time error on `Opaque`/unsupported predicates and bound `?` parameters).
  - Feed `PredExpr` interval/affine constraints into `TIntegerBounds` and `ProveObligations.jet` (adding struct-field affine terms).
  - Wire `Core/Web/Forms.jet` and `web.openapi` to read `#Check` metadata directly instead of duplicating per-field validators.
- **Wave 3 — Property Testing (`#Test(for_all: T)`) & `#7008` `KeyPath<T, V>` Binding**
  - Implement constructive domain-narrowing generators + invariant-preserving shrinking in `Core/Testing` driven by `PredExpr`, with bounded rejection sampling for `Opaque` clauses.
  - Bind `PredPath` directly to `#7008`'s ratified `KeyPath<T, V>` type for first-class predicate composition (`Predicate.and`, `Predicate.or`, `Predicate.not`).
