# Adversarial Design Review: Clause Sublanguages, Marker Vocabularies, and `Emit<T>` (`#7003` / Proposal Item P1)

- **Date:** 2026-11-26
- **Card:** `#7003` (Proposal item P1 — first-tier clause sublanguages + ratified second-tier ballots + ballot-independent defects `c11`/`c12`)
- **Base Commit Audited:** `dev` (`b84063a8e0f181f70c46325f5dcc55d15a6a0640`)
- **Explicit Out-of-Scope Bracket:** Criterion `c8` (structured concurrency `task` vs. `#Tasks { }` / `spawn`) remains **open on Tower** and is explicitly excluded from this review, its cutover list, and its wave plan. Do not touch `task` syntax or semantics under `#7003` until that ballot settles.
- **Verdict:** **PASS WITH 3 BOUNDED SUB-DECISIONS & BOOTSTRAP PROTOCOL** — **Wave A (`c7`, `c9`, `c10`, `c11`, `c12`) is 100% unblocked and immediately buildable on `dev`**. Wave B (`x.freeze()`, `#Defer { }`, `Distinct<T>`) requires a mandatory two-stage bootstrap cutover for `defer` (43 live compiler/Core/Jetpack source sites) and integrates `#Check` invariants via `#7006`. Wave C (`#States`, `#Protocol`, `#Migration`, and final keyword/registry retirement) requires confirming three small surface questions (**Q1–Q3**) and wiring `#Migration` key paths to `#7008`.

---

## 1. Executive Summary

Proposal item P1 (`Docs/Proposals/2026-11-24-greenfield-language.md`, §2.2) eliminates dedicated contextual-keyword sub-grammars in favor of Jet's single marker/method/type vocabulary, and three ratified second-tier ballots extend that cleanup to three active lexer keywords:

1. **First tier (`c2`–`c6`):**
   - `state { A -> B }` -> `#States({…})`
   - `protocol Name { client: … server: … }` -> `#Protocol([…])`
   - `migration Type { rename / add / remove / change / via }` -> `#Migration(from:, steps: […])` (coupled with `#7008` typed key paths)
   - `defer …` -> `#Defer { … }`
   - `freeze(x)` -> `x.freeze()` (beside `x.copy()` and `x.move()`)
   - `distinct T` -> `Distinct<T>` (with `#Check` invariants via `#7006`)
2. **Ratified second tier (`c7`, `c9`, `c10`):**
   - Hand-written Rust rows (`c7`): retire `extern rust "crate" { … }` in favor of unified `#Import <lang>.<lib>: module { … }` (merging `Item.CModule` and `Item.ExternRust` into one AST node, moving versions to `deps: { <lib>: rust@"<crate>" }`, and enforcing `FFI.Rust("<crate>")` grants on parity with `FFI.C`).
   - Generators (`c9`): retire `yield` in favor of generic `#Effect Emit: trait<T>` with `-> Stream<T>` auto-handling `Emit<T>` over the existing task-backed generator runtime (`JetRuntime/Source/Generator.jet` + `JetIR/Source/BuildGenerator.jet`), plus zero-fiber synchronous collection via `#Context(Emit<T>: ^buf)` built directly on `#7030`.
   - Implicit subject `it` (`c10`): delete `KW_IT` from the lexer and retire `it` from both value tables and bindingless loops.
3. **Ballot-independent defects (`c11`, `c12`):**
   - `c11`: close the built-but-rejected `loop nums -> it * 2` hole and remove `Compiler/JetSema/Source/Sema/Diagnostics.jet:386-388` teaching it.
   - `c12`: fix `E0805`'s reversed arrow (`Stream<T> ->` -> `-> Stream<T>`) in `Compiler/JetSema/Source/Sema/Statements/Loops.jet:195` (and its UI snapshot `Tests/UI/yield_outside_generator.stderr:6-7`), and fix the inaccurate "reserved word" comments for contextual `task` at `Compiler/JetFoundation/Data/Syntax/Spellings.jet:157` and `Compiler/JetFoundation/Data/Syntax/PackageFiles.jet:461`.

### Seven Critical Findings from Live `dev` (`b84063a8e0f`) Inspection

Auditing the live compiler source uncovered **seven concrete traps** that naive implementation would hit immediately:

1. **`KW_VIA` has a second live syntactic position outside `migration { }` (`-[via act]>`).** `Compiler/JetParser/Source/Parser/Items.jet:1424` parses `-[via param]>` effect pass-through arrows, formatted at `Formatter/Items.jet:1963` and used in `Examples/ErrorsAndEffects/Effects/EffectLevers.jet:15` plus 7 test suites. Retiring `migration` does **not** remove `via` from the language unless `-[via f]>` is separately retired.
2. **Bootstrap self-hosting trap on `defer` (43 non-test source sites in `Compiler/`, `Core/`, and `Jetpack/`).** Unlike `state`, `protocol`, `migration`, `freeze`, and `extern rust` (which have **0** non-test source sites in `Compiler/`), `defer` is used in **6** `Compiler/` source files (`JetCLI`, `JetIR`, `JetSema`), **16** `Core/` lines, and **21** `Jetpack/` lines; `distinct` has **2** live `Core/` declarations (`Core/App/App.jet:8150` `BlobID`, `Core/Auth/Auth.jet:91` `AccountID`); and `yield` has **10** live `Core/` sites (`Core/App/App.jet`, `Core/Data/Data.jet`). Deleting `defer` in the same commit that rewrites `Compiler/` breaks `S1 -> S2` self-hosting.
3. **`Stmt.DeferClose` resource-close-duty split (`Parser/Statements.jet:2047-2058`).** Today `defer close(f)` (unbraced) parses to `Stmt.DeferClose`, which satisfies resource-close ownership obligations and lowers via `ir_defer_close` (`BuildExpr.jet:3181`), whereas braced `defer { close(f) }` parses to plain `Stmt.Defer`. Migrating `defer close(f)` to `#Defer { close(f) }` will break resource-close checking unless `#Defer { }` recognizes a single `close(local)` body (or unifies `DeferClose` cleanly into `Defer`).
4. **Collecting loops (`loop x in xs -> expr`) internally desugar through `Stmt.Yield` (`Sema/Statements/Loops.jet:181-187`).** Deleting `Stmt.Yield` when retiring the `yield` keyword without first giving collecting-loop items their own AST/TStmt carrier breaks every collecting loop in Jet.
5. **Hidden `TOKEN_KW_IT` import in `Compiler/Migrations/BitSigils1.jet:22, 267`.** Deleting `KW_IT` from `Keywords.jet` and `TOKEN_KW_IT` from `Tokens.jet` breaks `Compiler/Migrations/BitSigils1.jet` unless updated in the same commit.
6. **`#States` and `#Protocol` have unaddressed structural mismatches with live features.** Live `state { … }` supports **named state groups** (`Open: Draft | Confirmed`, used in `Examples/Types/StateLists.jet:35` and `StateMachine.jet:12-13`), **terminal states** (`Shipped`), and **enum state lists** (`Door: enum`). Live `protocol Name { … }` is a standalone namespace declaration with no host struct, expanding into `Name.Client` and `Name.Server` (`Sema/Registration/Protocols.jet:104-157`). Furthermore, `#States({…})` written *inside* a struct body with parentheses collides with field markers (`#As(...) field: T`), whereas Jet's in-body marker syntax (`InBodyMarkers.jet:52-61`) uses `#Name: …`.
7. **`#Migration` old/removed field names cannot be typed key paths on the current struct.** In `rename("cost", to: .total)` and `remove("legacy_id")`, `"cost"` and `"legacy_id"` no longer exist on the current struct, so they cannot be `KeyPath<Order, _>` values (Jet pins historical shapes in `.jet/cache/schema/*.snapshot` JSON, not as live `V3` structs). Only *current* fields (`.total`, `.active`, `.score`) can be typed key paths.

---

## 2. Strongest Prior Art & Contrast with Jet

| Mechanism | Strongest Prior Art & Primary Citation | What It Does Best | Concrete Lesson for Jet `#7003` |
| :--- | :--- | :--- | :--- |
| **Scope-exit cleanup (`#Defer { }`)** | **Zig `defer` / `errdefer`** (*Zig Language Reference*, "defer"); **Swift `defer`** (*The Swift Programming Language*, "Early Exit / Specifying Cleanup Actions") | Strictly lexical LIFO execution on every exit (`return`, `break`, error propagation); forbids control flow (`return`/`break`) from escaping a defer body. | Jet's existing `ir_defer` (`BuildTransact.jet:258-279`) already matches Zig/Swift LIFO semantics and forbids escaping exits (`E0509`) and error propagation (`E0512`). Moving the syntax to `#Defer { }` preserves 100% of those semantics while eliminating a contextual statement keyword—provided `close(resource)` recognition is unified inside `#Defer { }`. |
| **Algebraic-effect generators (`Emit<T>`)** | **Koka** (*Leijen, "Algebraic Effects for Functional Programming"*, MSR-TR-2016-29); **OCaml 5** (*Sivaramakrishnan et al., PLDI 2021, "Retrofitting Effect Handlers onto OCaml"*); **Kotlin `sequence`/`flow`** (*Kotlin Docs*, "Sequences" & "Asynchronous Flow") | Separates *producing* values (`emit(x)`) from *consuming* them (lazy stream fiber vs. eager sink). Recursive helper functions emit directly into the caller's handler in $O(n)$ instead of re-yielding through $O(n^2)$ nested generators (unlike Python PEP 380 / JS `yield*`, where each value traverses every frame). | Jet already has task-backed generator suspension (`Generator.jet`) AND `#Effect` traits + `#Context(Effect: handler)` (`#7030`). Making `Emit<T>` a Prelude `#Effect` trait with `-> Stream<T>` auto-handling `Emit<T>` gives $O(n)$ recursive helpers, zero-fiber `#Context(Emit<T>: ^buf)` collection, and eliminates the `yield` keyword. |
| **Nominal newtypes (`Distinct<T>`)** | **Rust `#[repr(transparent)] struct UserId(i64)`** (*Rust Reference*, "Type Layout — The `transparent` representation"); **Haskell `newtype`** (*Haskell 2010 Report*, §4.2.3); **Nim `distinct`** (*Nim Manual*, "Distinct type") | Guaranteed zero-cost ABI-identical representation to `T` (including C FFI structs/arguments), strict nominal separation, explicit unwrap, opt-in trait forwarding (`#Numeric`, `#Comparable`, `#Codable`). | Making `Distinct<T>` a Prelude compiler-recognized nominal type removes `distinct` from the grammar while keeping guaranteed transparent representation, `.raw()` unwrap, `#UnitFamily` lowering, C-ABI transparency, and `#Check` invariants (`#7006`). |
| **Typestate & Session Types (`#States`, `#Protocol`)** | **Rust typestate crates & `typestate-rs` / `session-types`** (*Jespersen, Munksgaard, Larsen, WGP 2015, "Session Types for Rust"*); **Plaid / Obsidian** (*Coblenz et al., OOPSLA 2019*) | Models state transitions and dual client/server protocols through linear/affine ownership and nominal state types with zero runtime tag overhead. | Jet's `Typestate.jet` + `StateFacts.jet` + `#SingleUse` already deliver compile-time typestate and dual protocol handles without runtime tags. Moving the declarations to `#States({…})` and `#Protocol([…])` keeps the entire static checker intact if state groups, terminal states, and protocol module attachment are preserved cleanly. |
| **Explicit match subjects (retiring `it`)** | **Kotlin `when (val x = f())`** (*Kotlin Language Specification*, "When expressions"); **Swift `case let x`** (*The Swift Programming Language*, "Control Flow"); **Rust `match expr { x => … }`** (*The Rust Reference*) | None of Kotlin, Swift, or Rust reserves an implicit `it` for match/switch subjects (Kotlin limits `it` strictly to single-parameter lambdas, and Groovy to closures). Nested matches never silently shadow an implicit name. | Naming a computed subject on the preceding line (`decade: age /% 10`) costs 1 line at 3 repository sites, eliminates shadowing in nested tables, aligns the compiler with the August bindingless-loop ruling, and frees an active lexer keyword. |
| **Manifest-owned FFI (`#Import <lang>.<lib>`)** | **Swift Package Manager** (`Package.swift`); **Cargo** (`Cargo.toml`); **Rust `extern "C"`** (*Rust Reference*, "External blocks", which names ABIs, never crate versions) | Keeps every dependency version in one manifest/lockfile so dependency tooling never parses source files; unifies foreign declarations under one shape across languages. | Jet already parses `lang@"ref"` in `Jetpack/PackageModel/Source/Manifest/Blocks/Dependencies.jet:177-192` and locks `LockSource.Foreign` (`BridgeCache.jet:600`). Merging `Item.ExternRust` into `#Import <lang>.<lib>: module` achieves I8 single-mechanism parity with C imports and frees `extern`. |
| **Data-driven schema migrations (`#Migration`)** | **Rails ActiveRecord / Ecto / Prisma / Avro schema evolution** (*Apache Avro Specification*, "Schema Resolution"); **SwiftData `SchemaMigrationPlan`** | Represents schema evolution steps as composable, inspectable values tied to checked field references rather than ad-hoc parser grammars. | Stepping into `#Migration(from:, steps: […])` lets Jet's existing `Sema/Migrations.jet` canonical-shape calculator consume ordinary typed step values (`rename`, `add`, `remove`, `change`) once historical vs. current field references are cleanly distinguished. |

---

## 3. Complete Live-Code Inventory on `dev` (`b84063a8e0f`)

### 3.1 Keyword, Lexer, and Surface-Budget Accounting

`Tools/Agent/SurfaceBudget.jet:4-12` derives the five gated surface counts from their canonical sources:
- `keywords`: active (`retired: false`) rows in `Compiler/JetLexer/Data/Lexer/Keywords.jet` (currently **21** active + **4** retired-but-reserved = 25 rows).
- `markers`: `impl Marker` declarations in `Compiler/Prelude/Markers.jet` (currently **80**).
- `lexical-forms`: rows in `LEXICAL_LEDGER` in `Compiler/JetFoundation/Data/Syntax/Spellings.jet` (currently **88**).
- `JET_KEYWORD_LIST` (`Compiler/JetFoundation/Data/Syntax/PackageFiles.jet:417-494`) is the separate LSP completion/rename/syntax-dictionary registry (checked against `Keywords.jet` by `Toolchain/LexerKeywordRows.jet`).

**Crucial distinction:** Zero of the 13 Tier-1 words are in `Keywords.jet` (all 13 are contextual words parsed from `TOKEN_IDENT` or direct calls). All 3 ratified Tier-2 words (`extern`, `yield`, `it`) **are** active rows in `Keywords.jet`—and per `Keywords.jet:2` ("A retired word stays reserved in the lexer"), freeing them requires **deleting their lexer rows** (not marking `retired: true`).

| Word | Registry Constant & File | In `Keywords.jet`? | In `JET_KEYWORD_LIST`? | Live Non-Test Source Uses (`Compiler/` / `Core/` / `Jetpack/`) | All Repo Uses (`.jet`) | Disposition in `#7003` |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `state` | `KW_STATE_DECL` (`EffectsSurface.jet:69`) | No | Yes (`:478`) | 0 / 0 / 0 | 88 (`Tests: 69, Examples: 13, Compiler tests: 5, Docs: 1`) | **Remove** (`#States({…})`) |
| `protocol` | `KW_PROTOCOL` (`EffectsSurface.jet:76`) | No | Yes (`:480`) | 0 / 0 / 0 | 6 (`Tests: 5, Examples: 1`) | **Remove** (`#Protocol([…])`) |
| `client` | `PROTO_CLIENT` (`EffectsSurface.jet:79`) | No | Yes (`:481`) | 0 / 0 / 0 | inside 6 `protocol` blocks | **Remove** (ordinary `Client(...)` step value) |
| `server` | `PROTO_SERVER` (`EffectsSurface.jet:81`) | No | Yes (`:482`) | 0 / 0 / 0 | inside 6 `protocol` blocks | **Remove** (ordinary `Server(...)` step value) |
| `migration` | `KW_MIGRATION` (`PackageFiles.jet:216`) | No | Yes (`:445`) | 0 / 0 / 0 | 19 (`Tests: 13, Examples: 5, Docs: 1`) | **Remove** (`#Migration(from:, steps: […])`) |
| `rename` | `KW_RENAME` (`PackageFiles.jet:220`) | No | Yes (`:446`) | 0 / 0 / 0 | inside 19 `migration` blocks | **Remove** (step constructor fn) |
| `add` | `KW_ADD` (`PackageFiles.jet:224`) | No | Yes (`:447`) | 0 / 0 / 0 | inside 19 `migration` blocks | **Remove** (step constructor fn) |
| `remove` | `KW_REMOVE` (`PackageFiles.jet:228`) | No | Yes (`:448`) | 0 / 0 / 0 | inside 19 `migration` blocks | **Remove** (step constructor fn) |
| `change` | `KW_CHANGE` (`PackageFiles.jet:233`) | No | Yes (`:449`) | 0 / 0 / 0 | inside 19 `migration` blocks | **Remove** (step constructor fn) |
| `via` | `KW_VIA` (`PackageFiles.jet:243`) | No | Yes (`:450`) | 0 / 0 / 0 (in migrations); **active in `-[via f]>`** (`Parser/Items.jet:1424`) | 1 `Examples/` + 7 `Tests/` for `-[via f]>` | **Q3: Stays active for `-[via f]>`** unless effect pass-through retires separately |
| `defer` | `KW_DEFER` (`CoreSurface.jet:499`) | No | Yes (`:435`) | **6 / 16 / 21** (**43 bootstrap-sensitive!**) | 159 (`Tests: 69, Compiler: 39, Jetpack: 22, Core: 16, Examples: 12, Tools: 1`) | **Remove** (`#Defer { }`, **two-stage bootstrap cutover**) |
| `freeze` | `KW_FREEZE` (`Spellings.jet:162`) | No | Yes (`:464`) | 0 / 0 / 0 | 45 (`Tests: 40, Examples: 3, Compiler tests: 2`) | **Remove** (`x.freeze()` method) |
| `distinct` | `KW_DISTINCT` (`PackageFiles.jet:77`) | No | Yes (`:443`) | 0 / **2** (`BlobID`, `AccountID`) / 0 | 171 decls (`Tests: 144, Examples: 23, Core: 2, Compiler tests: 2`) | **Remove** (`Distinct<T>`) |
| `extern` | `KW_EXTERN` (`MathLayout.jet:566`) | **Yes (`:36`, `TOKEN_KW_EXTERN`)** | Yes (`:423`) | 0 / 0 / 0 | 54 `.jet` (`Tests: 51, Examples: 1, Tools: 1, Docs: 1`) + 16 `.case` | **Delete lexer row & remove** (`#Import <lang>.<lib>: module`) |
| `rust` | `KW_RUST` (`MathLayout.jet:568`) | No (import root) | Yes (`:425`) | 0 / 0 / 0 | paired with `extern rust` + `use rust.*` | **Remove from `JET_KEYWORD_LIST`** (match `c`); keep reserved import root |
| `yield` | `KW_YIELD` (`CoreSurface.jet:510`) | **Yes (`:34`, `TOKEN_KW_YIELD`)** | Yes (`:434`) | 0 / **10** (`Core/App`, `Core/Data`) / 0 | 108 (`Tests: 70, Tools: 20, Core: 10, Examples: 7, Docs: 1`) | **Delete lexer row & remove** (`Emit<T>` + `emit(v)`) |
| `it` | `KW_IT` (`EffectsSurface.jet:219`) | **Yes (`:31`, `TOKEN_KW_IT`)** | Yes (`:492`) | Internal compiler names only (`Switches.jet`, `Loops.jet`, `Expressions.jet`) | 3 user sites (`Examples/Basics/ValueDispatch.jet:23,49` + 1 Test) | **Delete lexer row & remove** (bind name above table; close loop hole) |
| `task` | `KW_CONC_TASK` (`Spellings.jet:158`) | No (contextual) | Yes (`:462`) | **OUT OF SCOPE (`c8`)** | **OUT OF SCOPE (`c8`)** | **Untouched** except fixing 2 comments (`c12`) |

**Net `Tools/Agent/surface_budget.tsv` delta for `#7003` (excluding open `c8`):**
- `keywords`: **21 -> 18** (`-3`: `KW_EXTERN`, `KW_YIELD`, `KW_IT` deleted from `Keywords.jet`).
- `markers`: **80 -> 83** (or **84** if retired `State` in `Markers.jet:225` stays temporarily for `E0175`; `+4` new Prelude markers: `#States`, `#Protocol`, `#Migration`, `#Defer`, `-1` if retired `State` is deleted). Note: `#Import` already exists, `Emit` is an `#Effect` trait (not a marker), `x.freeze()` is a method, `Distinct<T>` is a Prelude type, and `#Check` is accounted under `#7006`.
- `lexical-forms`: updates `LEXICAL_LEDGER` row `state { … }` -> `#States({…})`, updates `yield`/`Stream<T>` guidance, and retires `freeze`/`distinct`/`migration`/`protocol`/`defer`/`it`/`extern rust` rows cleanly.
- `JET_KEYWORD_LIST`: shrinks by **16 words** (12 Tier-1 words excluding `via` + `KW_STATE` retired `#State` if retired + `KW_EXTERN` + `KW_RUST` + `KW_YIELD` + `KW_IT`), or **17 words** if `-[via f]>` is also retired.

### 3.2 Exact Compiler & Runtime Implementation Map

| Mechanism | AST / Foundation | Parser & Formatter | Sema & Ownership | IR / Runtime / Tools |
| :--- | :--- | :--- | :--- | :--- |
| **`state { … }`** | `AST/Items.jet` (`StateDecl`, `StateArrow`, `StateGroup`, `StructDef.state`, `EnumDef.state`), `SyntaxCodec.jet`, `EffectsSurface.jet:60-69` | `Parser/Declarations/States.jet:1-119`, `StateMarkers.jet` (retired `#State` -> `E0175`), `Parser/Items.jet`, `Formatter/Items.jet` | `Sema/Registration/StateTypes.jet`, `StateFacts.jet:1-432`, `Sema/Typestate.jet:1-1008`, `Comptime/Reflection.jet`, `GateLedger.jet`, `NameFacts.jet` | Erased in IR (compile-time only). Reflected via `T.reflect().states`. `SyntaxDictionaryData.jet:76`, `Highlights.jet:217`, `grammar.js:18,275`. |
| **`protocol { … }`** | `AST/Items.jet` (`Item.ProtocolDecl`, `ProtocolDecl`, `ProtocolMessage`, `ProtocolDirection`), `SyntaxCodec.jet`, `EffectsSurface.jet:71-82` | `Parser/Declarations/Protocols.jet:1-101`, `Parser/Items.jet`, `Formatter/Items.jet` | `Sema/Registration/Protocols.jet:1-158` (expands `protocol Name` into `#SingleUse Name.Client`/`Name.Server` structs + `state { S0 -> S1 }` + impls), exhaustive `Item.ProtocolDecl` match arms in **141** files | Lowered as ordinary generated structs/impls via `Protocols.jet` expansion. `ArrowSurface/arrow_surface.jet:158,429-431` tests retired `client -> server:`. |
| **`migration { … }`** | `AST/Items.jet` (`Item.Migration`, `MigrationDecl`, `MigrationOp`), `SyntaxCodec.jet`, `PackageFiles.jet:216-251` | `Parser/Declarations/Migrations.jet:1-110`, `Parser/Items.jet`, `Formatter/Items.jet` | `Sema/Migrations.jet:1-196` (`sema_migration_canonical_shape`, default & converter fn synthesis), `Sema/Registration/SerdeItems.jet:1270-1381` | `SerdeItems.jet` synthesizes codec migration decoders; `JetCLI/Source/CLI/EmitAST.jet`. `SyntaxDictionaryData.jet`, `grammar.js:18,276`. |
| **`defer`** | `AST/Statements.jet` (`Stmt.Defer`, `Stmt.DeferClose`), `SyntaxCodec.jet`, `CoreSurface.jet:493-501`, `Registry/SyntaxRegistry.jet:42` | `Parser/Statements.jet:593-596, 2039-2059` (`parser_parse_defer_statement`, `parser_defer_close_call`), `Parser/Habits.jet:10,199`, `Formatter/Statements.jet` | `Sema/Statements.jet` (`E0509` control escape, `E0512` `?` propagation, `DeferClose` resource duty), `Sema/Statements/Assignments.jet`, `Sema/Ownership/`, `Sema/Taint.jet`, `Sema/Typestate.jet` | `JetIR/Source/BuildTransact.jet:258-279` (`ir_defer`, `ir_run_defer` LIFO on `b.bindings`), `BuildExpr.jet:863,976,3181-3205` (`ir_defer_close`), `Build.jet:3217-3227`, `BuildExpr.jet:6826` (NRVO/tail-return interaction). |
| **`freeze(x)`** | `Spellings.jet:162`, `PackageFiles.jet:464`, `Highlights.jet:75` (no dedicated `Expr.Freeze` AST node—parsed as ordinary `Expr.Call`) | Ordinary `Expr.Call` today (`Formatter` unaffected) | `Sema/Calls/Direct.jet:371, 2189-2235` (`sema_call_check_freeze`, `E1114`), `Sema/Ownership/Checks.jet:7749-7791` (`sema_ownership_frozen_expr_site`, `E1113`), `Ownership/Crossings.jet`, `TaskGroups.jet` | Lowers to `TExprKind.Clone{fact: Explicit}` or `TExprKind.MaterializeView` (or no-op if already frozen). `SyntaxDictionaryData.jet:50`, `CLI/Tests/SyntaxExplain.jet:86`, `Diagnostics.jet:24276`. |
| **`distinct T`** | `AST/Items.jet` (`Item.Distinct`, `DistinctDef`), `SyntaxCodec.jet`, `PackageFiles.jet:73-82, 141-156` (`#UnitFamily` also creates `DistinctDef`s!) | `Parser/Declarations/Types.jet:165-250` (`parser_parse_distinct`, `InlineRange`, bundle derives), `Formatter/Items.jet:1553` | `Sema/Registration/Items.jet` (registration, `E0324`, forwarding), `Sema/Calls/Methods.jet:88` (`.raw()` `E0311`), `Sema/Calls/Static.jet`, `Sema/Expressions.jet:1907`, `Sema/Ownership/Copy.jet`, `Sema/Units/Prelude.jet:342`, `Comptime/Reflection.jet` | `JetIR/Source/BuildTypes.jet:309,5295` (transparent base layout + range check `ir_distinct_in_range`), `BuildExpr.jet:2440` (`DistinctCtor`, `DistinctConvert`, `DistinctRaw`), `JetSemIndex/Symbols.jet:995`, `CLI/SemIndexJSON.jet:411`. |
| **`extern rust`** | `AST/Items.jet:428` (`CModule`) vs. `Items.jet:831` (`ExternRustBlock`), `MathLayout.jet:565-626`, `Keywords.jet:36`, `Tokens.jet:334`, `KeywordSpans.jet:74` | `Parser/Declarations/Foreign.jet:37-120`, `ItemHeads.jet:38`, `Entry.jet:2197`, `Items.jet:343,1865`, `ColonDeclarations.jet:59,121,138`, `Formatter/Items.jet` | **124 Compiler files** matching `Item.ExternRust` (362 lines); `Sema/Calls/Direct.jet:1470`, `Sema/Effects/Checks.jet:2062` (currently charges **no** `FFI.Rust` leaf!), `Compiler/Prelude/Effects.jet:73-98` (24 FFI language roots) | `JetDriver/Source/Driver/ForeignProviders.jet`, **7 Jetpack files** (`RustBridge/BridgeCache.jet:600`, `Manifest/Blocks/Dependencies.jet:177-192`), `JetEval/CompilerServices.jet:77,452`, `Docs/Spec/spec.md:5190-5271`. |
| **`yield` / `Stream<T>`** | `AST/Statements.jet` (`Stmt.Yield`), `Keywords.jet:34`, `Tokens.jet:330`, `KeywordSpans.jet:69`, `CoreSurface.jet:507-513` | `Parser/Statements.jet:75,115,570-575,2176`, `ColonDeclarations.jet:72,121`, `Parser/ExpressionForms/Loops.jet` (collecting loops emit synthetic `Stmt.Yield`!) | `Sema/Statements/Loops.jet:181-230` (`sema_stmt_check_yield`, collecting-loop branch `:182`, `E0805` reversed arrow `:195`, `E0807` `:229`, `THostCall.YieldSend`), `Sema/Registration/TraitInstances.jet` & `Effects/Library.jet` (`#7030` Wave 1!) | `JetIR/Source/BuildGenerator.jet:1-236` (`ir_build_generator`, `ir_generator_producer`, `ir_generator_yield`), `JetRuntime/Source/Generator.jet:1-117` (`jet_rt_native_generator*`), `JetIR/Source/BuildTransact.jet` (`#Context(Emit<T>: ...)`), `JetEval/CompilerServices.jet`. |
| **`it`** | `Keywords.jet:31`, `Tokens.jet:322`, `KeywordSpans.jet:18`, `EffectsSurface.jet:219` | `Parser/ExpressionForms/Loops.jet:31,360,366,375` (`var: "it"`!), `Parser/Expressions.jet:90,345,1190`, `Parser/Statements.jet:71,2203`, `ColonDeclarations.jet:64,121`, `Compiler/Migrations/BitSigils1.jet:22,267` | `Sema/Statements/Switches.jet:222-270` (binds internal local `"it"`), `Sema/Expressions.jet:8164-8177` (bindingless loop `.field` reads `"it"`), `Sema/Expressions/HoleHeaders.jet:108-135` (`E0442` naming helper), `Sema/Diagnostics.jet:377-388` (teaches loop `it`!), `Sema/Expressions/FieldRefs.jet:113` | `JetEval/CompilerServices.jet:82,446`, `JetCLI/Source/CLI/EditorGrammars.jet`, `SyntaxDictionaryData.jet:95`, `Docs/Spec/spec.md:711-713,4174`, `Examples/Basics/ValueDispatch.jet:23,49`. |

---

## 4. Adversarial Attacks & Technical Resolution

### Attack 1: Self-Hosting Bootstrap Deadlock (`S1 -> S2 -> S3`) on `defer`, `distinct`, and `yield`
- **Claim attacked:** Criterion `c5` calls for "one greenfield cutover" deleting the old spellings and migrating every in-repository use simultaneously.
- **Evidence from live code:**
  - `defer` appears in **6** `Compiler/` source lines (`Compiler/JetCLI`, `Compiler/JetIR`, `Compiler/JetSema`), **16** `Core/` source lines (`Core/App`, `Core/DB`, `Core/Tasks`, `Core/Web`), and **21** `Jetpack/` source lines (`CLI`, `Integrations`, `PackageModel`, `Provider`, `Store`) — **43 non-test bootstrap/Core/Jetpack source sites**.
  - `distinct` appears in **2** `Core/` source declarations (`Core/App/App.jet:8150` `BlobID: distinct String`, `Core/Auth/Auth.jet:91` `AccountID: distinct String`).
  - `yield` appears in **10** `Core/` source lines (`Core/App/App.jet:6539-6656`, `Core/Data/Data.jet:1504-1555`).
  - By contrast, `state`, `protocol`, `migration`, `freeze(x)`, `extern rust`, and `it` have **0** non-test source sites in `Compiler/`, `Core/`, and `Jetpack/`.
- **Failure mode:** Under Jet's `S1` (seed compiler) -> `S2` (new compiler built by `S1`) -> `S3` (new compiler built by `S2`) bootstrap:
  - If commit 1 rewrites `Compiler/`'s 6 `defer` lines to `#Defer { }`, `S1` fails to parse `Compiler/` because `S1` does not know `#Defer { }` yet!
  - If commit 1 deletes `defer` from `JetParser` while leaving `Compiler/`'s 6 `defer` lines intact, `S2` fails to compile `Compiler/` during `S2 -> S3`!
- **Required resolution (zero-deadlock two-commit cutover for `defer` only):**
  1. **Step 1 (seed `s1`):** Add `#Defer { }` marker support (sharing `Stmt.Defer` / `Stmt.DeferClose`) while temporarily retaining the `defer` parser branch so `S1` compiles `Compiler/` and `S2` understands BOTH `#Defer { }` and `defer`.
  2. **Step 2 (next commit, after seed advance):** Migrate all 159 `defer` sites (`Compiler/`, `Core/`, `Jetpack/`, `Examples/`, `Tests/`, `Tools/`) to `#Defer { }`, delete `parser_parse_defer_statement` / `KW_DEFER`, and register the `jet fix` rewrite.
  3. Because `Core/` is compiled by `S2` (not `S1`), `Distinct<T>` (2 `Core/` sites) and `Emit<T>` (10 `Core/` sites) can cut over cleanly in **single commits** without a bootstrap bridge—provided `Compiler/` itself uses neither `distinct` nor `yield` (verified: 0 real `Compiler/` source sites!).

---

### Attack 2: `#Defer { }` vs. `Stmt.DeferClose`, `#Shield`, Early `return`, `?`, and `BuildOwn.jet` LIFO Order
- **Claim attacked:** Replacing `defer stmt` / `defer { … }` with `#Defer { … }` is a parser-only swap.
- **Hidden traps uncovered in live code:**
  1. **`Stmt.DeferClose` resource-close duty (`Parser/Statements.jet:2047-2058`, `BuildExpr.jet:3181-3205`):** Today `defer close(f)` (unbraced) lowers to `Stmt.DeferClose{close: Expr.Call("close", [f])}`, which marks `f`'s resource close duty as satisfied in `Sema/Ownership` and arms `ir_close_defer(b, binding.slot)` in IR. Today's braced `defer { close(f) }` produces `Stmt.Defer`, **not** `Stmt.DeferClose`! Under `#Defer { close(f) }`, the braces are mandatory, so migrating `defer close(f)` -> `#Defer { close(f) }` would **break every resource-close obligation** unless `parser_parse_marked_statement` (or Sema) inspects a single-statement `#Defer { close(ident) }` block and produces `Stmt.DeferClose` (or, cleaner under I8, teaches `Stmt.Defer` + ownership checking to recognize `close(ident)` inside any `#Defer { … }` block and retires the duplicate `Stmt.DeferClose` node!).
  2. **`#Defer` + `#Shield` + `return` + `?` + `break`/`next` LIFO ordering (`BuildTransact.jet:240-279`):**
     - `ir_defer` pushes an `IRBinding{name: "", slot: -1, cleanup: b.defers.len()}` directly onto `b.bindings` (`BuildTransact.jet:260`), so `ir_drop_bindings` runs defers in exact reverse binding order (after locals bound *after* `#Defer` drop, before locals bound *before* `#Defer` drop) on fallthrough, early `return`, `?` error propagation, `break`/`next`, and task cancellation/panic unwind.
     - `ir_run_defer` sets `b.stopping = true` (`BuildTransact.jet:271`), so an outer cancellation never aborts a cleanup block mid-flight, and `#Shield { … }` (`BuildTransact.jet:240-250`) opens a nested `ir_scope_open` inside the shield frame—so `#Defer { … }` inside `#Shield { … }` runs **before** `jet_rt_session_shield_leave`, while `#Defer { #Shield { … } }` shields the cleanup itself.
     - `BuildExpr.jet:6826` (`!ir_defers_open(b)`) disables direct return-slot aliasing whenever a defer is live so deferred statements reading a returned local never see an already-moved slot.
  3. **Sema restrictions (`E0509` / `E0512`) and value-tail prohibition:** `#Defer { … }` MUST keep `E0509` (no `return`/`break`/`next` escaping the defer body), `E0512` (no `?` error propagation out of a defer body), and MUST NOT be accepted in value-expression position (`x: #Defer { … }` is an error because `#Defer` schedules a statement rather than evaluating to a value).
  4. **Block-marker stacking interaction (`#7011`):** In `Parser/Statements.jet:2526-2590`, `#Defer` attaches directly to `Stmt.Defer` (sites: `[.Block]`, `Markers.jet`).

---

### Attack 3: `x.freeze()` Ownership, Deep Immutability, Task Crossing, and Diagnostic Provenance
- **Claim attacked:** Moving `freeze(x)` to `x.freeze()` is purely a method rename.
- **Hidden traps uncovered in live code:**
  1. **Dual recognition sites in Sema:** `freeze` is NOT an AST node—it is currently intercepted in TWO separate places by name:
     - Type/copy/crossing checking in `Compiler/JetSema/Source/Sema/Calls/Direct.jet:371, 2189-2235` (`sema_call_check_freeze`: enforces `E1114` against shared mutation / resources / closures / mutable views, reuses `SemaSendCrossing.FrozenSnapshot`, elides the clone when `already_frozen` is true, and records a `SemaCopyEvent`).
     - Provenance tracking in `Compiler/JetSema/Source/Sema/Ownership/Checks.jet:7749-7791` (`sema_ownership_frozen_expr_site` matches `.Call(call)` with `call.name == "freeze" && call.args.len() == 1` to propagate `frozen_site` through bindings, field reads, indexing, slices, and `if` merges, and `sema_ownership_reject_frozen_write` emits `E1113` on any write through a frozen snapshot!).
  2. **Receiver-method migration requirements:**
     - Move `sema_call_check_freeze` from `Calls/Direct.jet` to `Calls/Methods.jet` right beside `.copy()` and `.move()` (taking 0 arguments: `x.freeze()`, rejecting `x.freeze(arg)` with `E0104` and rejecting any user/Core method named `freeze` from shadowing the compiler-owned verb—matching `.copy()`/`.move()`/`.in_place()`).
     - Update `Ownership/Checks.jet:7756-7764` to match `.MethodCall(receiver, method, args, ...)` with `method == "freeze" && args.len() == 0` AND ensure idempotent chained calls (`x.freeze().freeze()`, tested by `Examples/Concurrency/FreezeCapture.jet:15`) still hit `already_frozen` and skip the second clone!
     - Update `E1113` / `E1114` diagnostic messages and hints (`DiagnosticRows.jet:19657`, `Diagnostics.jet:24276`, `Checks.jet:7786`) from `` `freeze(...)` `` to `` `.freeze()` ``, and check `E1102`/task-crossing fix hints so they suggest `x.freeze()`, never `freeze(x)`.
     - Update `SyntaxDictionaryData.jet:50` and `Compiler/JetCLI/Tests/SyntaxExplain.jet:86`.

---

### Attack 4: `Distinct<T>` Representation, C-ABI Transparency, Range Types, `#UnitFamily`, and `#7006` `#Check` Ordering
- **Claim attacked:** `Name: distinct Base` becomes `Name: Distinct<Base>` with its invariant written as `#Check` (`#7006`).
- **Hidden traps uncovered in live code:**
  1. **Nominal identity of `Name: Distinct<Base>` vs. generic type aliases (`E0324`):**
     - In Jet today, `Name: OtherType` without type parameters is rejected by `E0324` (`Sema/Registration/Items.jet:3642`: "a transparent type alias is only for shortening generic spellings"), whereas `Name: SomeGeneric<Int>` creates a *transparent alias* (so two aliases `UserId: Alias<Int>` and `OrderId: Alias<Int>` would be the **same** type!).
     - Therefore `Distinct<T>` MUST be a **compiler-owned nominal constructor form** in `Sema/Registration/Items.jet` (and `AliasClassify.jet`): every declaration `UserId: Distinct<Int>` and `OrderId: Distinct<Int>` MUST mint a **fresh nominal `DistinctDef`** (so `UserId != OrderId != Int`), NOT a transparent alias to one shared generic struct `Distinct<Int>`!
     - **Reject bare `Distinct<Int>` in value/field/parameter positions** (e.g. `f: fn(x: Distinct<Int>)`) with a clear diagnostic directing `Name: Distinct<Int>`—otherwise two anonymous `Distinct<Int>` annotations would either be accidentally identical (defeating distinctness) or unnameably incompatible!
  2. **Zero-cost layout & C-ABI transparency (`BuildTypes.jet:309, 5295`, `#6947`):**
     - `BuildTypes.jet:309` already lowers every `DistinctDef` to its exact base type's ABI row (size, alignment, register class, C-struct layout, `#Export(c)` / `#Import c.<lib>` signature, and callback parameter passing under `#6947`), and lowers `DistinctCtor`/`DistinctRaw` to zero-cost identity moves across `O0`, `O1`, `O2`, Comptime, and WebAssembly (100% I9 parity).
     - Preserving the internal `DistinctDef` representation (changing only the surface declaration from `Name: distinct Base` to `Name: Distinct<Base>`) preserves **100%** of C-ABI transparency, `.raw()`, `#Numeric`/`#Comparable`/`#Printable`/`#Codable` forwarding (`parser_distinct_marker_derives`), `#UnitFamily` unit-type injection (`Sema/Units/Prelude.jet:342`), and checked-text heads (`Pattern: Distinct<String>` in `Examples/Text/CheckedText/`) with zero runtime regression.
  3. **Range types (`Percent: Distinct<Int(0..100)>`) and `#7006` `#Check` decoupling:**
     - Today `Percent: distinct Int(0..100)` (`Examples/Data/Serde/CodecFieldErrors.jet:11`, `Parser/Declarations/Types.jet:199-217`) parses `Int(0..100)` as `Type.InlineRange` and extracts `IntegerRange{lo, hi}` onto `DistinctDef.range`.
     - Writing `Percent: Distinct<Int(0..100)>` parses `Int(0..100)` as the type argument to `Distinct<…>` automatically with **zero special range-postfix grammar**!
     - **Decoupling from `#7006`:** `Name: Distinct<Base>` and `Percent: Distinct<Int(0..100)>` can ship **before** `#7006` lands; when `#7006` lands `#Check(pred, "msg")`, `#Check` attaches directly to any `Name: Distinct<Base>` declaration (retiring `#Invariant`) without churning `Distinct<T>` syntax or layout.

---

### Attack 5: `#States({…})` — Declaration Site, Terminal States, Named State Groups, and Enum States
- **Claim attacked:** `state { A -> B }` becomes `#States({…})` taking a record of transitions, keeping today's generated types and diagnostics.
- **Gaps uncovered in live code (`Parser/Declarations/States.jet:56-100`, `Examples/Types/StateLists.jet:13-80`):**
  1. **Named state groups (`Open: Draft | Confirmed`) have no slot in a plain transition record.** Live `StateLists.jet:35` (`Open: Draft | Confirmed`), `StateMachine.jet:12-13` (`Active: Queued | Running`, `Settled: Done | Failed`), and `Tests/UI/Typestate/` actively use named state groups (`StateGroup` in `AST/Items.jet`, `States.jet:71-80`, `StateTypes.jet`, `Typestate.jet`). A record `{Draft: [.Confirmed, .Canceled], Confirmed: [.Shipped, .Canceled]}` only encodes transitions—it has nowhere to express `Open: Draft | Confirmed` without a second field/argument.
  2. **Terminal states (`Shipped`, `Canceled`):** In `Order`, `Shipped` and `Canceled` have no outgoing transitions. Under `#States({Draft: [.Confirmed, .Canceled], Confirmed: [.Shipped, .Canceled]})`, every target state named in a transition list SHOULD be automatically included in the state set (and explicit `Shipped: []` accepted for documentation or isolated states like `Shut: []` in `Parser/Declarations.jet:476`).
  3. **Above-declaration vs. in-body marker grammar (`InBodyMarkers.jet` collision):** Writing `#States({…})` *inside* a `struct { … }` body (as sketched in the proposal's §2.2 example) collides directly with **field markers** (`#As("id") id: String`, `#Redact token: String`): `Items.jet:2271` parses any `#Name(…)` inside a struct body as attaching to the next field! Meanwhile, Jet *already* has a uniform in-body declaration-marker syntax in `Compiler/JetParser/Source/Parser/Declarations/InBodyMarkers.jet:52-120`: `#Name: (args)`.
  4. **Enum state lists (`Door: enum`):** `StateLists.jet:57-66` attaches `state { Shut -> Open, Locked … }` to an `enum`, where `StateFacts.jet` verifies every state name matches an enum variant. `#States` must declare `sites: [.Type]` so it attaches identically to both `struct` and `enum`.
- **Bounded Sub-Decision Q1 (Recommended):**
  - Place `#States` at the standard declaration-marker position (`sites: [.Type]`):
    ```jet
    #States({
        Draft: [.Confirmed, .Canceled],
        Confirmed: [.Shipped, .Canceled],
        Shipped: [],
        Canceled: [],
    }, groups: {
        Open: [.Draft, .Confirmed],
    })
    Order: struct {
        id: Int,
        ...
    }
    ```
  - Auto-include target-only states so `Shipped: []` is optional (required only for states with neither incoming nor outgoing edges).
  - In-body `#States: ({…})` works automatically through existing `InBodyMarkers.jet` with zero new struct-body grammar!
  - All downstream passes (`StateTypes.jet`, `StateFacts.jet`, `Typestate.jet`, `T.reflect().states`, `E0150`, `E0151`, `E0174`, `L0152`) consume the exact same `StateDecl` data populated from `#States`—zero loss of span precision (`StateArrow`/`StateGroup` keep exact element spans from the record literal!).
  - Delete standalone `state TypeName { … }` (`States.jet:25-40`, already retired by `Tests/UI/Typestate/retired_top_level_state_companion_teaches_nested_owner.jet`).

---

### Attack 6: `#Protocol([…])` — Attachment Target & Message Step Representation
- **Claim attacked:** `protocol Name { client: … server: … }` becomes `#Protocol([…])` taking a list of message steps, keeping today's generated types and diagnostics.
- **Gaps uncovered in live code (`Parser/Declarations/Protocols.jet:28-96`, `Sema/Registration/Protocols.jet:104-158`, `Tests/UI/protocol_out_of_order.jet:7-19`):**
  1. **No host declaration exists today:** `protocol Payment { … }` is a top-level declaration that expands into `#SingleUse Payment.Client` and `#SingleUse Payment.Server` plus `impl Payment.Client` and `impl Payment.Server` (`Sema/Registration/Protocols.jet:115-134`). A marker `#Protocol([…])` MUST attach to a declaration. Attaching it to `Payment: struct {}` would falsely declare an instantiable empty `Payment` struct.
  2. **Message payloads vs. ordinary values:** Today `client: Hello(version: Int)` declares an ad-hoc inline field list `(version: Int)` and generates `Hello: fn(self: ^^Payment.Client.S0, version: Int) -> Payment.Client.S1 Err!` (`Protocols.jet:144`). If `#Protocol([…])` takes an ordinary `[ProtocolStep]` list, `Hello(version: Int)` is NOT a valid Jet expression—either message payloads are ordinary structs (`Hello: struct { version: Int }`, step `Client(Hello)` or `.Client(TypeId{Hello})`) OR step entries carry explicit names + payload types.
  3. **141-file `Item.ProtocolDecl` cleanup:** Merging protocol expansion onto `#Protocol` attached to a module (`ModuleDef`) lets us delete `Item.ProtocolDecl` and **all 141 match arms** across `Compiler/` (massive I8 simplification!).
- **Bounded Sub-Decision Q2 (Recommended):**
  - Attach `#Protocol([…])` (`sites: [.Module]`) to an empty/hosting module declaration:
    ```jet
    Hello: struct { version: Int }
    Ack: struct { session: Int }
    Charge: struct { cents: Int }
    Receipt: struct { id: Int }

    #Protocol([
        .Client(Hello),
        .Server(Ack),
        .Client(Charge),
        .Server(Receipt),
    ])
    Payment: module {}
    ```
  - Expansion generates `Client` and `Server` inside `Payment` (so `Payment.Client.client()`, `Payment.Server.server()`, `#SingleUse`, state transitions `S0 -> S1 -> …`, and `E0150` out-of-order diagnostics stay identical!).
  - Sending methods accept the typed message struct (`h.hello(Hello{version: 1})`, or keep field unpacking from `Hello`'s fields for `h.hello(version: 1)`—recommended: pass `Hello` directly as one typed value so protocol messages compose with `#Codable` wire framing automatically!).
  - Keep `ArrowSurface` guard clean (delete the now-obsolete `p: protocol { … }` rows in `Tests/Guards/ArrowSurface/arrow_surface.jet:158,429-431`).

---

### Attack 7: `#Migration(from:, steps: […])` — Historical Field Names, Type Arguments, and `#7008` Coupling
- **Claim attacked:** `migration Type { rename / add / remove / change / via }` becomes `#Migration(from:, steps: […])` whose steps are ordinary values built with typed key paths (`#7008`).
- **Gaps uncovered in live code (`Sema/Migrations.jet:75-196`, `Examples/Data/Migrations/run.jet:13-50`, `Docs/Audits/2026-11-26-typed-key-paths-review.md`):**
  1. **Historical/deleted fields do NOT exist on the current struct!**
     - Look at `Examples/Data/Migrations/run.jet:37-49`: `Profile` has fields `{title, id, score, active}`—its old fields `legacy_id` (removed) and `name` (renamed to `title`) are **gone** from `Profile`.
     - Therefore `legacy_id` and `name` **cannot** be typed key paths `KeyPath<Profile, _>` (no `V3` struct exists in Jet—pinned historical shapes live in `.jet/cache/schema/*.snapshot` JSON files verified by `E0910`).
     - Meanwhile, every *target/current* field (`.title`, `.active`, `.score`) **is** a real field on `Profile` and IS a typed key path!
  2. **Multiple historical releases on one struct:** `Sema/Migrations.jet:92-113` walks `[MigrationDecl]` in chronological release order. `#Migration` must be **stackable** (multiple `#Migration(from: 1, steps: […])` / `#Migration(from: 2, steps: […])` markers on one struct, ordered by `from:`), preserving `sema_migration_canonical_shape` identically.
  3. **Type arguments in `change` as ordinary values:** `change score: Int -> MemberRank via { fn(n) { MemberRank{value: n} } }` passes the old type `Int` and new type `MemberRank`. With typed key paths (`#7008`), `.score` already carries `New = MemberRank`, and `via: fn(n: Int) -> MemberRank { … }` (or `from: TypeId{Int}`) supplies `Old = Int` cleanly as an ordinary value using Jet's existing `TypeId{T}` (`Spellings.jet:57`)!
  4. **Clean `#7008` decoupling & `KW_VIA` second live use:**
     - Using Prelude step constructors whose field parameter accepts `#7008`'s canonical key-path type (`KeyPath<M, T>` or whichever name `#7008`'s Q1 ballot ratifies):
       ```jet
       #Codable(published)
       #Migration(from: 1, steps: [
           remove("legacy_id"),
           rename("name", to: .title),
           change(.score, from: Int, via: fn(n) { MemberRank{value: n} }),
           add(.active, default: true),
       ])
       Profile: struct {
           title: String,
           id: Int,
           score: MemberRank,
           active: Bool,
       }
       ```
     - Notice `add(.active, default: true)` infers `T = Bool` from `.active` (checking `true: Bool` at compile time without repeating `: Bool`), and `change(.score, from: Int, via: fn(n) { … })` infers the converter's input `Int` from `from:` (`TypeId{Int}`) and output `MemberRank` from `.score`!
- **Bounded Sub-Decision Q3 (`KW_VIA`):**
  - `Compiler/JetParser/Source/Parser/Items.jet:1424` uses `via` in `-[via act]>` effect pass-through arrows (`Examples/ErrorsAndEffects/Effects/EffectLevers.jet:15` + 7 test suites).
  - **Recommended:** Keep `KW_VIA` for `-[via f]>` in `#7003` (so **12** Tier-1 words leave `JET_KEYWORD_LIST` rather than breaking `EffectLevers.jet`), and track `-[via f]>` retirement separately under the effect-row track (`-[..E]>`).

---

### Attack 8: Hand-Written Rust Rows (`c7`) — Unified AST Item, `FFI.Rust` Grants, and 24-Root Parsing
- **Ratified outcome (`c7`):** Build `#Import <lang>.<lib>: module { … }` for hand-written Rust rows, delete `extern rust "crate" { … }`, move crate versions into `package: { deps: { <lib>: rust@"crate@ver" } }`, and delete `KW_EXTERN` from `Keywords.jet`.
- **Concrete architectural invariants enforced by live code (`Parser/Declarations/Foreign.jet:55-120`, `AST/Items.jet:428, 831`, `Jetpack/PackageModel/Source/Manifest/Blocks/Dependencies.jet:177-192`):**
  1. **True I8 single AST item:** Merge `Item.CModule` (`Items.jet:428`) and `Item.ExternRust` (`Items.jet:831`) into one unified `Item.ForeignImport` (or generalized `CModule` renamed `ForeignModule{lang: String, lib: String, kind, path_span, functions: [ExternFn], span}`). Delete `Item.ExternRust` and its **362 lines across 124 Compiler files** plus **7 Jetpack files** (`RustBridge/BridgeCache.jet` etc.) so there is literally one foreign-import representation.
  2. **All 24 FFI roots accepted in head (`Compiler/Prelude/Effects.jet:73-98`):** `#Import <lang>.<lib>: module` accepts any registered FFI language root; roots without a hand-written row bridge emit a clear registered diagnostic naming the supported roots (`c`, `rust`) instead of "A C module path starts with `c.`".
  3. **Grant parity (`FFI.Rust("<crate>")`):** Live `JetSema` currently charges **no** `FFI.Rust` effect leaf on `extern rust` calls. Under the unified `#Import` rule (`Docs/Spec/spec.md:5250-5271`), calling a Rust row records `FFI.Rust("<crate>")`, and a package without a row descriptor must grant `FFI.Rust("<crate>")` in `authority: { allow: […, FFI.Rust("base64")] }` (added automatically by the `jet fix` rewrite across all 54 `.jet` blocks and 16 `.case` files).
  4. **Bridge version lookup from `deps:`:** Wire `JetDriver/Source/Driver/ForeignProviders.jet` and `Jetpack/Integrations/Source/Foreign/RustBridge/` to resolve `rust.<lib>`'s crate spec from `DepSource.Foreign{language: "rust", reference: ...}` (which `Dependencies.jet:177-192` already parses and `BridgeCache.jet:600` already locks!), moving `E0701` (invalid crate spec) onto the `deps:` row.
  5. **Lexer & registry:** Delete `KW_EXTERN` from `Keywords.jet:36`, `Tokens.jet:334`, `KeywordSpans.jet:74`, `JetEval/CompilerServices.jet:77,452`, and `JET_KEYWORD_LIST:423`. Remove `KW_RUST` from `JET_KEYWORD_LIST:425` (parity with `C_MODULE_ROOT`, while keeping `"rust"` in the reserved foreign import-root table). Update `#6951` criteria `c2`/`c4` note in cutover tracking.

---

### Attack 9: `Emit<T>` Effect + `-> Stream<T>` Auto-Handle + `E0805` (`c9`, `c12`) — Zero-Fiber Sync vs. Suspended Stream & I9 Parity
- **Ratified outcome (`c9`):** Retire `yield` (`KW_YIELD` deleted from `Keywords.jet`) in favor of generic `#Effect Emit: trait<T>` with `emit(value)` and `-> Stream<T>` auto-handling `Emit<T>`.
- **Synergy with `#7030` (Effect-as-Trait):** `#7030` Wave 1 **already built** generic `#Effect` trait registration (`Sema/Registration/TraitInstances.jet`, `Sema/Effects/Library.jet`), `#Context(Effect: handler)` discharge & ownership (`E0782`/`E0783`), and uniform `ir_effect_call` lowering (`BuildTransact.jet`, `BuildTraits.jet`)! `#7003`'s `Emit<T>` plugs **directly** into that exact mechanism rather than building a second effect dispatch path (I8).
- **Five hard technical traps solved up front:**
  1. **Collecting loops MUST be decoupled from `Stmt.Yield` first (`Sema/Statements/Loops.jet:181-187`).** Today `Parser/ExpressionForms/Loops.jet` desugars `loop x in xs -> expr` into a zero-width synthetic `Stmt.Yield`, which `Loops.jet:182` intercepts via `span.start == span.end && check.context.collect_item_types.len() > 0`. Replace that synthetic node with a dedicated `Stmt.CollectItem{value, span}` (or handle collecting directly in `LoopValues.jet`) **before** deleting `Stmt.Yield` and `TOKEN_KW_YIELD`!
  2. **Two execution modes, ONE `Emit<T>` trait, 100% I9 parity:**
     - **Mode 1 — Lazy `-> Stream<T>` auto-handle (`BuildGenerator.jet` + `Generator.jet`):** Any function returning `Stream<T>` (or `Stream<T> E!`) automatically installs a compiler-synthesized `Emit<T>` handler in its generator producer (`ir_generator_producer`, `BuildGenerator.jet:119-167`) whose `emit(v)` calls `jet_rt_native_generator_yield(state, storage)` and **discharges `Emit<T>`** from the function's published effect row (leaving any other effects, e.g., `FS.Read` or an open row `..E`, intact!). Because the handler lives in the task context slot for `Emit<T>`, any recursive helper `walk: fn(node: Tree) -[Emit<Int>]>` calls `emit(node.val)` directly into the outermost generator's `state` pointer—turning depth-3000 recursive re-yield from **$O(n^2)$ (~5,000 ms)** into **$O(n)$ (~100 ms, identical to a flat loop)**!
     - **Mode 2 — Synchronous `#Context(Emit<T>: ^buf)` (zero fiber/scheduler overhead):** Calling `walk(tree)` inside `#Context(Emit<Int>: ^collector) { walk(tree) }` dispatches `emit(v)` synchronously through `#7030`'s `ir_effect_call` directly into `collector.emit(v)` (e.g. `ListCollector` pushing to `[Int]`)—**zero heap fiber, zero scheduler park/wake**, working identically on `O0`, `O1`, `O2`, Comptime (`#run`), and WebAssembly (`JetWeb`)!
     - **I9 Comptime & Web parity for `-> Stream<T>`:** Verify `JetEval` (comptime) and `JetWeb` (WASM) route `Emit<T>` inside `-> Stream<T>` through the same `Emit<T>` handler slot used by `ir_generator_producer`, so recursive `Emit<T>` helpers work identically across Native (`O0`/`O1`/`O2`), Comptime, and Web.
  3. **Nested generators & type-parameterized `Emit<T>` collision:**
     - If an `-> Stream<Int>` generator calls an inner `-> Stream<String>` generator (or another `-> Stream<Int>` generator), each generator's `Emit<T>` handler slot is **keyed by the concrete instantiated `Emit<T>` type** AND pushed/popped lexically per generator producer task (since each `Stream<T>` producer runs in its own child task with its own task context!). Even if a `Stream<Int>` generator pulls from another `Stream<Int>` generator, the two run in separate child tasks, so their `Emit<Int>` slots never clobber each other!
     - Inside a generic function `fn<T>(xs: [T]) -[Emit<T>]>`, `Emit<T>` uses the same per-instance substitution rule as `Stream<T>` (lifting `E0104` for `#Effect` trait type arguments).
  4. **Ownership & move semantics of `emit(value)`:** Preserve `sema_stmt_move_direct` (`Loops.jet:223`), guard-owned copy checks (`Loops.jet:209`), and numeric literal widening (`Loops.jet:213`) so `emit(value)` moves `value` (`^^T`) with zero unexpected copies and preserves `E0807` element-type mismatch diagnostics.
  5. **Criterion `c12` (`E0805` & `task` comments):**
     - Rename/update `E0805` (`Jet.YieldOutsideStream` -> `Jet.EmitOutsideStream` or `Emit<T>` missing handler) with the **corrected arrow order** (`-> Stream<T>`, NEVER `Stream<T> ->`) in `Sema/Statements/Loops.jet:195`, `DiagnosticRows.jet`, `Diagnostics.jet`, and regenerate `Tests/UI/yield_outside_generator.stderr` (or `emit_outside_generator.stderr`).
     - Fix the two inaccurate "reserved word" comments for contextual `task` at `Compiler/JetFoundation/Data/Syntax/Spellings.jet:157` and `Compiler/JetFoundation/Data/Syntax/PackageFiles.jet:461`.
  6. **Paired benchmark proof owed by build:** Pair depth-3000 recursive tree walk vs. 3000-element flat loop (before vs. after `Emit<T>`) proving linear $O(n)$ scaling.

---

### Attack 10: Retiring `it` (`c10`) and Closing `loop nums -> it * 2` (`c11`)
- **Ratified outcome (`c10`) + ballot-independent bug (`c11`):** Delete `KW_IT` from `Keywords.jet`, free `it` as an ordinary identifier (`fn(it: Int)`, `loop it in nums`), retire table-subject `it`, and refuse `loop nums -> it * 2` with a registered diagnostic and UI snapshot.
- **Every hidden compiler site verified on live `dev` (`b84063a8e0f`):**
  1. **Internal unspellable binder name (MANDATORY):**
     - `Compiler/JetParser/Source/Parser/ExpressionForms/Loops.jet:375` sets `var: "it"` on bindingless loops (`loop nums -> .len()`). Once `it` is a normal identifier, (a) `loop nums -> it * 2` MUST NOT resolve `it` to the loop item (`c11`), and (b) `loop it in nums` MUST NOT be misclassified as bindingless (`Loops.jet:360, 366`)!
     - `Compiler/JetSema/Source/Sema/Statements/Switches.jet:224, 252-265` binds computed switch subjects to a local literally named `"it"`. Once `it` is a normal identifier, a user local `it: 10` outside a computed `if f() == { … }` table would be **silently shadowed** inside the table's arms!
     - **Fix:** Replace BOTH internal names with `\0`-prefixed unspellable constants in `EffectsSurface.jet` (matching `INTERNAL_TASK_RECEIVER` at `EffectsSurface.jet:102` and `INTERNAL_VALIDATE_OVER` at `PackageFiles.jet:202`): e.g. `INTERNAL_SWITCH_SUBJECT` (`\0jet.subject`) and `INTERNAL_LOOP_ITEM` (`\0jet.loop_item`). Update `Sema/Expressions.jet:8164-8177` (bare `.member` lookup) and `Sema/Expressions/FieldRefs.jet:113` to read `INTERNAL_LOOP_ITEM`.
  2. **Closing `c11` cleanly:**
     - With `INTERNAL_LOOP_ITEM`, `loop nums -> it * 2` immediately refuses `it` with `E0107` (unknown name) instead of silently computing `[2, 4, 6]`!
     - Delete `Compiler/JetSema/Source/Sema/Diagnostics.jet:386-388` (which currently prints `` "`it` is the item only inside a bindingless loop such as `loop items -> it * 2`" ``) and add a UI snapshot proving `loop nums -> it * 2` is refused and no diagnostic text teaches loop `it`.
  3. **Hidden `TOKEN_KW_IT` dependency in `Compiler/Migrations/BitSigils1.jet:22, 267`:**
     - `BitSigils1.jet` imports `TOKEN_KW_IT` from `jet_lexer` directly. Remove `TOKEN_KW_IT` from `BitSigils1.jet:22, 267` in the same commit that deletes `TOKEN_KW_IT` from `Tokens.jet:322`, `Keywords.jet:31`, `KeywordSpans.jet:17-18`, `JetEval/CompilerServices.jet:82,446`, `Parser/ColonDeclarations.jet`, `Parser/Expressions.jet`, `Parser/Statements.jet`, and `Parser/ExpressionForms/Loops.jet`.
  4. **User migration & `jet fix`:** Add a registered `jet fix` rewrite reusing `HoleHeaders.jet:108-135`'s `E0442` naming helper for statement-leading computed tables, migrate the 3 live sites (`Examples/Basics/ValueDispatch.jet:23, 49` + 1 test), and update `Docs/Spec/spec.md:711-713, 4174`.

---

## 5. Binding Cutover List

`#7003` (excluding open `c8`) cannot close until **every** item below is delivered and verified:

### A. Lexer, Syntax Registry, Surface Budget & Migrations
1. **`Compiler/JetLexer/Data/Lexer/Keywords.jet`, `Tokens.jet`, `KeywordSpans.jet`:** Delete `KW_EXTERN`, `KW_YIELD`, `KW_IT` rows (freeing all three as ordinary identifiers); update `JetEval/Source/CompilerServices.jet` and `Compiler/Migrations/BitSigils1.jet:22, 267`.
2. **`Compiler/JetFoundation/Data/Syntax/` (`PackageFiles.jet`, `CoreSurface.jet`, `EffectsSurface.jet`, `MathLayout.jet`, `Highlights.jet`, `Spellings.jet`, `Registry/SyntaxRegistry.jet`):**
   - Remove retired words from `JET_KEYWORD_LIST` (`state`, `protocol`, `client`, `server`, `migration`, `rename`, `add`, `remove`, `change`, `defer`, `freeze`, `distinct`, `extern`, `rust`, `yield`, `it`; keep `via` per Q3 unless `-[via f]>` retires separately).
   - Fix `Spellings.jet:157` and `PackageFiles.jet:461` (`task` contextual comment, `c12`).
   - Update `LEXICAL_LEDGER` and register `LANGUAGE_REWRITES` rows (`Spellings.jet:114-126`) with matching `Compiler/Migrations/` rewrites & fixtures (`Tests/Guards/syntax_reconciliation.jet`).
3. **`Compiler/Prelude/Markers.jet` & `Tools/Agent/surface_budget.tsv`:**
   - Declare `#States`, `#Protocol`, `#Migration`, `#Defer` markers in `Markers.jet`; declare `#Effect Emit: trait<T>` + `emit` in `Core/Prelude`.
   - Append exact `surface_budget.tsv` ceiling rows (`keywords` down `21 -> 18`, `markers` updated to match exact `impl Marker` count, `lexical-forms` matched). Run `Tools/Agent/SurfaceBudget.jet --check`.

### B. Compiler Front End, Sema, Ownership, IR & Jetpack
4. **`it` retirement & `c11`:** Move `Switches.jet` and `ExpressionForms/Loops.jet` internal binders to `\0`-prefixed constants; delete `Diagnostics.jet:386-388`; migrate `ValueDispatch.jet` + tests; add UI snapshot refusing `loop nums -> it * 2` and allowing `fn(it: Int)` / `loop it in nums`.
5. **`Emit<T>` & `c12`:** Decouple collecting loops from `Stmt.Yield`; wire `Emit<T>` auto-handle into `BuildGenerator.jet` / `Generator.jet` (lazy `-> Stream<T>`) and `#Context(Emit<T>: ^buf)` (zero-fiber sync); enforce I9 parity on `O0`/`O1`/`O2`/Comptime/Web; fix `E0805` arrow order (`-> Stream<T>`) & snapshot; migrate all ~108 `yield` sites (`Core/` 10, `Examples/` 7, `Tests/` 70, `Tools/` 20, `Docs/` 1); land paired depth-3000 benchmark proof.
6. **`#Import <lang>.<lib>: module` (`c7`):** Merge `Item.CModule` + `Item.ExternRust` into one AST item across 124 Compiler files + 7 Jetpack files; accept all 24 FFI roots in `#Import` head; resolve Rust crate spec from `deps:` (`DepSource.Foreign`); enforce `FFI.Rust("<crate>")` grant parity; migrate all 54 `.jet` blocks + 16 `.case` files.
7. **`x.freeze()`:** Move `sema_call_check_freeze` from `Calls/Direct.jet` to `Calls/Methods.jet`; update `Ownership/Checks.jet:7756` (`MethodCall`), `E1113`/`E1114` texts, `SyntaxDictionaryData.jet:50`, `SyntaxExplain.jet:86`; migrate all 45 sites.
8. **`#Defer { }` (two-stage bootstrap):** Add `#Defer { }` unifying `close(local)` resource duty (`DeferClose`), preserve `E0509`/`E0512`/`#Shield`/LIFO semantics; after seed advance, migrate all 159 `defer` sites (6 `Compiler/`, 16 `Core/`, 21 `Jetpack/`, 12 `Examples/`, 69 `Tests/`, 1 `Tools/`) and delete `parser_parse_defer_statement` + `Parser/Habits.jet` entries.
9. **`Distinct<T>`:** Accept `Name: Distinct<Base>` (including `Distinct<Int(0..100)>`) as nominal `DistinctDef` preserving C-ABI transparency, `.raw()`, bundle derives, `#UnitFamily`, and `#Check` (`#7006`) attachment; reject bare anonymous `Distinct<T>` in value/param positions; migrate all 171 declarations (`Core/App/App.jet:8150`, `Core/Auth/Auth.jet:91`, 23 `Examples/`, 144 `Tests/`, 2 Compiler tests).
10. **`#States({…})`, `#Protocol([…])`, `#Migration(from:, steps: […])`:**
    - `#States` (`sites: [.Type]`): parse record transitions + optional `groups:` record onto `StateDecl` for both structs and enums; delete `Parser/Declarations/States.jet` and migrate 88 sites.
    - `#Protocol` (`sites: [.Module]`): expand step list onto `ModuleDef` (`Client`/`Server` handles); delete `Item.ProtocolDecl` (141 match arms) and `Parser/Declarations/Protocols.jet`; migrate 6 sites + `ArrowSurface` guard.
    - `#Migration` (`sites: [.Type]`, stackable, coupled with `#7008`): consume `remove(String)`, `rename(String, to: KeyPath)`, `add(KeyPath, default:)`, `change(KeyPath, from:, via:)` into `Sema/Migrations.jet`; delete `Item.Migration` and `Parser/Declarations/Migrations.jet`; migrate 19 sites.

### C. Tooling, Editors, Specs, Golden Examples & Guards
11. **`Compiler/JetCLI/Source/CLI/SyntaxDictionaryData.jet`, `Formatter/`, `SemIndex/Symbols.jet`, `SemIndexJSON.jet`:** Update formatting, semantic index signatures (`Distinct<Base>`), and CLI syntax explanations.
12. **Editor grammars:** Update `Tools/Editors/TreeSitter/grammar.js` (`JET_HIGHLIGHT_*` lists and keyword choices) and `JetCLI/Source/CLI/EditorGrammars.jet`; verify `Tests/Guards/grammar.jet` parses every `Examples/**/*.jet` with zero `ERROR`/`MISSING` nodes.
13. **Golden examples & docs (`c6`):** Add/update executable examples with `.out` goldens proving every wave across `O0`, `O1`, `O2`, Comptime, and Web; update `Docs/Spec/spec.md` and `Docs/Spec/Reference/` pages; run `surface_budget`, `syntax_reconciliation`, `diagnostics_coverage`, `diagnostic_text_retired_spellings`, `american_spelling`, `grammar`, and `copy_ratchet` guards.

---

## 6. Independent Wave Plan (Clean, Decoupled Execution)

Every wave below is independently verifiable, lands cleanly without breaking bootstrap or leaving dead code, and respects external dependencies (`#7006`, `#7008`, `#7030`) while leaving open `c8` completely untouched:

| Wave | Scope | Dependencies | Bootstrap / Seed | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Wave A1** | `c10` + `c11`: Retire `it` (`Keywords.jet` deletion, `\0`-prefixed internal binders in `Switches.jet` & `Loops.jet`, `BitSigils1.jet`, close `loop nums -> it * 2`, `Diagnostics.jet:386-388`, `ValueDispatch.jet`, `jet fix` rewrite, UI snapshots) + `c12` comment fixes (`Spellings.jet:157`, `PackageFiles.jet:461`). | None | `seed: s1` (single commit) | **Ready immediately** |
| **Wave A2** | `c9` + `c12`: `Emit<T>` `#Effect` trait + `-> Stream<T>` auto-handle (`BuildGenerator.jet` / `Generator.jet`) + sync `#Context(Emit<T>: ^buf)` + decouple collecting loops from `Stmt.Yield` + delete `KW_YIELD` lexer row + fix `E0805` arrow order + migrate ~108 sites + depth-3000 benchmark proof. | `#7030` Wave 1 (`#Effect` trait + `#Context` dispatch) | `seed: s1` (single commit; 0 `Compiler/` `yield` sites) | **Ready immediately after `#7030` Wave 1 lands** |
| **Wave A3** | `c7`: Unified `#Import <lang>.<lib>: module` merging `Item.CModule` + `Item.ExternRust` (124 Compiler files + 7 Jetpack files) + 24 FFI roots + `deps:` crate resolution + `FFI.Rust` grant enforcement + delete `KW_EXTERN` lexer row + migrate 54 `.jet` & 16 `.case` files. | None | `seed: s1` (single commit; 0 `Compiler/` `extern rust` sites) | **Ready immediately** |
| **Wave B1** | `x.freeze()` method cutover (`Direct.jet` -> `Methods.jet`, `Ownership/Checks.jet:7756`, `E1113`/`E1114`, migrate 45 sites) + `#Defer { }` **Stage 1** (add `#Defer { }` with unified `DeferClose` resource duty while keeping `defer` temporarily for `S1 -> S2`). | None | `seed: s1` (Stage 1) | **Ready immediately** |
| **Wave B2** | `#Defer { }` **Stage 2** (migrate all 159 `defer` sites incl. 43 `Compiler/`/`Core/`/`Jetpack/` sites; delete `defer` parser & `KW_DEFER`) + `Distinct<T>` cutover (nominal `Distinct<Base>` & `Distinct<Int(0..100)>`, migrate 171 declarations; `#Check` invariant hook attaches cleanly when `#7006` lands). | Wave B1 seed advance (`#Check` invariant hook waits on `#7006`) | `seed: s1` (Stage 2) | **Ready after Wave B1 seed advance** |
| **Wave C** | `#States({…}, groups: {…})` (`c2`, Q1) + `#Protocol([…])` (`c2`, Q2) + `#Migration(from:, steps: […])` (`c3`, Q3) + final `JET_KEYWORD_LIST` / `grammar.js` / `surface_budget.tsv` / spec & reference closure (`c5`, `c6`). | Confirm Q1–Q3; `#Migration` step key-path type waits on `#7008` | `seed: s1` (single commit; 0 `Compiler/` source sites) | **Ready after Q1–Q3 & `#7008`** |
| *(Bracketed Out)* | `c8`: Structured concurrency (`task` vs. `#Tasks { }` / `spawn`). | Open ballot on Tower | Untouched | **Explicitly out of scope** |
