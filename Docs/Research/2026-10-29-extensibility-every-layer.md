# Extensibility of every layer of Jet: what is closed, what is open, and one design (2026-10-29)

Tower card: #6202. Ballot: `D-EXTEND-ARCH1`. Follow-up cards: #6206–#6217 (defects) and the design cards filed from this research.
The census script (`closed-lists.mjs`) and its TSV output stay outside the repository for now. They land with the guard card, which checks the script in under `Tools/`.

Code is the source of truth. All file:line facts cite dev `496e49a853`.
Run-time probes used the dev compiler built from commit `ec16c32d35`, unless a line says otherwise.
Claims about other systems cite a URL, an author and a date. Unverified reasoning is marked `[INFERENCE]`.

## Plain summary

The owner asked one question about every part of Jet. Can it grow without a compiler change?
D-CORE-BOUNDARY1 answers it for chip instructions. D-INFRA1 answers it for cloud resources. This document asks it of every other layer.

The census found 2,039 closed lists in the compiler and Core. 1,145 of them hold domain knowledge, with 30,695 members in total.
A package should be able to add to a domain list. Today it can add to almost none of them; only four enums accept a `Custom(...)` value.
The other 894 lists are kernel lists. They hold grammar, IR, soundness rules or local state, and they should stay closed.

The main finding is that ratified law is already library-first in most layers. Most of that law is not built yet.
Hook points, compiler stages, the Compiler value, service providers and package hardware kinds are all ratified. Hook points and compiler stages have no code at all today (section 2).
The real gaps are narrower. There is no law for package diagnostics, package lints, editor features beyond two stages, formatter rules for Jet code, debugger views, package providers, migrations, or new effect roots.

One pattern recurs in several systems that grew without core changes. Prettier, Kubernetes and Bevy all build their own core features with the same plug-in interface that outsiders use.
Jet already has that shape in markers, unit families, effect leaves and `CheckedText`. Core uses the same forms a package uses.
The design in `D-EXTEND-ARCH1` makes that the rule for every domain list. A guard in CI stops new closed lists from appearing.

The recommended design costs nothing at run time when no package adds a member. Compile time grows with the members of imported packages only.
Kernel lists stay closed. Each one is named in a ledger with its reason and the expert way around it.

## 1. Method

### 1.1 The census

The census script is a Node program of about 780 lines. It walks `Compiler/` and `Core/` by default and writes one TSV row per finding.
The run is deterministic. The output at `496e49a853` is 2,040 lines including the header.

It finds twelve kinds of closed list:

| kind | what it matches | findings |
|---|---|---|
| enum | an `enum` declaration | 746 |
| eq-chain | three or more `x == "name"` tests in one function | 306 |
| dispatch | an `if x == { "a" -> … }` arm table on string names | 300 |
| table | a constant array of rows | 259 |
| inline-list | a literal list of names inside a function | 100 |
| prelude-marker / fact / effect / rows | rows in `Compiler/Prelude/*.jet` | 81 / 49 / 46 / 49 |
| const-family | a family of numbered constants with one prefix | 46 |
| unit-family | a `#UnitFamily` in the Prelude (808 units) | 32 |
| core-names | inline `core.*` module names | 26 |

Each finding gets a class and a subclass from fixed rules, written in the script.
`domain` means a package should be able to add a member. `kernel` means the list should stay closed.

| class / subclass | findings | members | meaning |
|---|---|---|---|
| domain / registry | 277 | 22,350 | a table of library facts the compiler owns |
| domain / name-dispatch | 580 | 5,678 | compiler code that branches on a library name |
| domain / vocabulary | 238 | 1,507 | a set of words a domain will grow |
| domain / kernel-op | 3 | 749 | one compiler enum member per Core handle operation |
| domain / platform | 33 | 145 | targets, capabilities, chips |
| domain / reserved-names | 7 | 141 | names the compiler reserves for first-party use |
| domain / mirror | 3 | 92 | a generated copy of a declared list |
| domain / already-open | 4 | 33 | an enum that already has a `Custom(...)` escape |
| kernel / local-state | 598 | 3,938 | a state machine private to one function |
| kernel / fixed-standard | 54 | 5,797 | data fixed by an outside standard (Unicode, DWARF) |
| kernel / grammar | 72 | 1,183 | tokens, precedence, syntax kinds |
| kernel / soundness | 128 | 243 | rules that keep the type system sound |
| kernel / ir | 35 | 395 | IR and machine-code forms |
| kernel / kernel | 6 | 54 | the numbered kernel operation table |
| kernel / protocol | 1 | 5 | a wire protocol fixed by its spec |

Only 43 of the 2,039 findings are generated from a declared source. The other 1,996 are written by hand.

A supplementary run over `Jetpack/` found 734 findings: 373 domain lists (2,976 members) and 361 kernel lists (1,987 members).

The rules are heuristics. A finding is a candidate, not a verdict. The TSV `basis` and `reason` columns say which rule fired, so a reviewer can disagree row by row.
Known blind spots: lists built at run time, lists inside string templates, and lists hidden behind one generic helper. The census under-counts, it does not over-count kernel lists. `[INFERENCE]` from the rule design.

### 1.2 Defects found on the way

Twelve defects and a set of near-defects were carded as #6206–#6217. Two later probes became #6218 and #6219.
They matter here because most of them are the same bug: one list kept in two to four places that have drifted apart.

Examples: three effect enums that disagree (the leaf copy lacks `Mem.Alloc` and `Mem.Rc`); four foreign-language lists; four Jetpack provider lists; `StructureRegistry` drift in four modules.
Each duplicate is a closed list that someone had to copy to extend. Open declarations with one home remove the copying.

### 1.3 Prior art

Sources were read on 2026-10-29. Each external claim in this document cites the source list in section 10.
Where a page has no author or date, the citation names the publisher and the access date.

## 2. The extension forms Jet already has

Jet has more open forms than any single closed list suggests. This is the inventory the rest of the document reuses.

| # | form | what a package can add | evidence |
|---|---|---|---|
| F1 | `trait` + `impl` | behaviour for its own or a foreign type | ordinary language; `impl Token.CheckedText` in `Examples/features/tooling/checked_text_type.jet:6` |
| F2 | `marker Name(params, $sites…) { body }` | a compile-time rule that checks or derives | `Examples/features/reflection/user_rule_body.jet`, `derive_loop.jet`; `Core/Http/Server.jet:20-26` declares Core's own route markers |
| F3 | block marker `#Name { text }` | a small checked language in braces | `Examples/features/tooling/declared_text_block.jet` |
| F4 | `effect Root.Leaf` | a leaf under an existing effect root | `Examples/features/effects/effect_declarations.jet:1`; `JetFoundation/Data/Syntax/EffectsSurface.jet:29-30` |
| F5 | `#UnitFamily(Name, …) { units }` | a family of unit types | `Examples/features/types/unit_family.jet:7-9`; the Prelude's own 32 families in `Prelude/Units.jet` |
| F6 | `derive T.Trait`, `comptime loop field in target.$fields` | generated code from type shape | `derive_loop.jet` |
| F7 | `plugin.load<I>(path, authority, sha256)` | a sandboxed wasm component checked against a trait | D-CORE-PLUGINPATH1 (ratified) |
| F8 | `point.on(scope, f)` events | run-time listeners | D-EVENT1, D-HOOK-ONE1 |
| F9 | `Custom(String)` variants | an unlisted value of a closed enum | `Core/Http/Http.jet:65` `Method.Custom`; 4 findings of subclass `already-open` |
| F10 | `#ContextKey pub name: T{default}` | a new ambient context field | D-JAI-CONTEXT1 (ratified) |

Several forms are ratified but not yet built. They are `#On(point, priority)` hooks (D-HOOK-ONE1), the compiler and tool stage points (D-HOOK-STAGES1, D-JAI-EVENTS1) and the Compiler value (D-META-COMPILER1).
The list goes on: one provider trait per service (D-CORE-PROVIDER1), package hardware kinds (D-HOST-CAPS1) and library instruction entries (D-CORE-BOUNDARY1). Generated tables from declared twins (D-OPENTABLE1) are partly built: 43 census findings are generated.
A search of `Compiler/`, `Core/` and `Jetpack/` at this commit finds no `.jet` file that names `compiler.parsed`, `compiler.checked`, `lsp.hover`, `fmt.file` or `#On(`. The stage points exist only in specs and Tower.

The forms share one property that matters for scale. Core uses the same form a package uses.
`Prelude/Markers.jet` declares Core's markers with `marker`. `Prelude/Units.jet` declares Core's units with `#UnitFamily`. `Prelude/Effects.jet` declares Core's effects with `effect`.
That is the property Prettier, Kubernetes and Bevy describe for their own plug-in systems (section 5.2).

The gap is the other direction. The compiler often reads these declarations and then also keeps a hand-written copy, or branches on the names.
Example: `Prelude/Markers.jet` declares the function markers, and `JetParser/Source/Parser/Items.jet:1593` also switches on 17 marker names (census CL01091).

## 3. The cost rules every mechanism is judged by

Every candidate mechanism below is sized against the same rules.

1. **Zero run-time cost when unused.** A program that does not import a member pays nothing at run time. D-HOOK-ONE1 sets this bar for hooks: "a point with no attachment emits no dispatch code".
2. **Compile time.** Cost should grow with what a program imports, not with what exists in the ecosystem.
3. **Tooling.** The editor, `jet inspect`, docs and `jet fix` must see a package member exactly as they see a Core member. D-ONE-TOOLS1 makes compiler queries the one fact owner.
4. **Readability.** A beginner must not see the mechanism. A package member is used with the same spelling as a Core member.
5. **I1 safety.** A package member may not make safe code unsafe. Escape is only `#Unsafe("reason")`.
6. **I4 complete diagnostics.** A package refusal needs a code, what/why/fix text and a snapshot. Today a package can only borrow a Core code (section 4.15).
7. **I8 one mechanism.** A new extension form must replace old ones, not sit beside them.
8. **I9 one meaning across tiers.** A package member must mean the same at O0, O1, O2 and on web. Its meaning lives in Jet source, so the shared runtime runs it.

Two measured warnings from other languages bound the design.

- Julia's open methods let any package add a method to any function. A fresh Julia 1.5 session held about 56,000 method instances. Loading `SIMD` invalidated 2,949 of them, and `DataFrames` invalidated 4,126 (Holy, Bezanson and Nash, 2020-08-26 [S1]).
  After loading `SIMD`, the next Plots call paid its 7.2 s compile time again. Open extension with global effect costs compile time in every later step.
- Elixir protocols solve the same problem by consolidation. When the build knows every implementation, it rewrites dispatch to a fixed table. The documentation says implementations added in tests have no effect unless consolidation is turned off [S2].
  Open extension that is closed at build time keeps dispatch cheap, but late additions need care.

Jet's design should behave like Elixir's consolidation, not like Julia's global tables. Members are gathered once per build from the imported packages. Nothing is added later at run time except through explicit events.

## 4. Layer by layer

Each layer has the same parts: what is closed, what is open, the ratified law, candidates, prior art, costs and the verdict.
"Census" lines give the layer totals as findings/members. Row IDs (`CLnnnnn`) are census rows at `496e49a853`; the script that produces them lands with the guard card.

### 4.1 Lexical layer: tokens, keywords, literals

**Closed.** Census `syntax.lexical`: domain 1/15, kernel 3/241. The keyword map in `JetLexer/Data/Lexer/Keywords.jet` is kernel.
The one domain row is `LEXER_CHUNK_HEAD_WORDS` (`JetLexer/Source/Lexer/Chunks.jet:103`, CL01068, 15 words), which the chunker uses to find item starts.
Numeric suffixes are open in one place: a unit family member such as `meter` becomes a literal suffix (`3meter`, `unit_family.jet:29`).
The byte-size suffixes `kb kib mb mib gb gib bytes` are a separate closed list (`JetDriver/Source/Driver/Loader.jet:538`, CL00401).
So are the budget suffixes `ns B us ms s KiB MiB GiB` (`JetSema/Source/Sema/BudgetSpecs.jet:34`, CL01127).

**Open.** Unit suffixes through F5. Text heads `Name{"…"}` through F1 with `CheckedText` (section 4.11).

**Law.** I7 requires every keyword or sigil to be registered and ratified. D-SURFACE-BUDGET1 counts keywords, marks and markers against a ceiling.

**Candidates.** (a) Keep tokens closed. (b) Let the byte and duration suffixes become unit families declared in Core, like `meter`. (c) User-defined operators or tokens, as in Haskell or Lean.

**Prior art.** Lean 4 lets a library declare new syntax categories and notations, with hygienic expansion (Ullrich and de Moura, 2020-01-28, rev. 2022-04-12 [S12]).
Racket lets a module choose its whole reader with `#lang` (Tobin-Hochstadt et al., PLDI 2011 [S13]; Racket Guide ch. 17 [S14]).
Both buy reach at the cost of tooling that must run user code to read a file.

**Costs.** Open tokens break the rule that any tool can parse any Jet file without running package code. The formatter, the syntax highlighters and the census itself all depend on it.
Moving the two suffix lists to unit families costs nothing at run time, since a suffix is resolved at compile time.

**Verdict.** Tokens and keywords stay kernel. Grammar growth goes through block markers (F3) and text heads (F1). The two suffix lists should become Core unit families (a design card).

### 4.2 Grammar and declaration forms

**Closed.** Census `syntax.ast`: domain 18/79, kernel 63/408. `syntax.parse`: domain 16/132, kernel 7/52.
Kernel rows include the AST node kinds. Domain rows include `StrFormat` (`JetFoundation/Source/AST/Expressions.jet:272`, CL00519, 13 format selectors such as `Hex` and `PadLeft`), mirrored as `TStrFormat` (`Facts/Checked.jet:259`, CL00659).
`OutputKind` (`AST/Items.jet:1238`, CL00564, 9 kinds), `SerdeWireShape` (`AST/Items.jet:1178`, 8 shapes) and `Namespace` (`AST/Items.jet:519`, 7 build namespaces) are also domain rows.
The parser switches on marker names in four places: function markers (`Parser/Items.jet:1593`, 17 names), type markers (`Parser/DeclarationMarkers.jet:39`, 14), statement markers (`Parser/Statements.jet:1875`, 13) and marker values (`Parser/Expressions.jet:854`, 8).

**Open.** New declaration forms through F2 and F3. The contextual words `state` and `protocol` show the pattern for words that are keywords only in one place (`EffectsSurface.jet:118-131`).

**Law.** D-META-REG1 makes marker rules, knowledge planes, rights and build facts rows of one declaration kind. D-MARKER-TARGETS1 fixes nine marker sites.

**Candidates.** (a) Parser marker switches become reads of `Prelude/Markers.jet` rows, so a package marker parses like a Core one. (b) Format selectors become a trait such as `Format<Selector>`, so a package adds `{x:base58}`. (c) Open declaration keywords.

**Prior art.** Swift macros are declared like functions with a typed signature, and the expansion is type-checked against it (Gregor, SE-0382, implemented in Swift 5.9 [S5]).
Rust added a `#[diagnostic]` attribute namespace whose unknown members are hints, not errors, so new attributes do not break old compilers (Rust Release Team, 2024-05-02 [S3]).

**Costs.** (a) has no run-time cost. It removes four switches and lets the parser treat every marker alike. (b) costs one trait dispatch at compile time per selector. (c) breaks the budget rule and tool parsing, as in 4.1.

**Verdict.** (a) and (b). Declaration keywords stay kernel; new declaration kinds come from D-META-REG1 rows with marker syntax.

### 4.3 Markers and attributes

**Closed.** Census `syntax.markers`: domain 43, kernel 38. Each row is one Prelude marker. Domain examples: `Memo` (`Prelude/Markers.jet:31`), `Job` (:50), `Every` (:52), `Replayable` (:54), `WasmExport` (:56).
Two marker parameters carry closed menus. `Rename(json, cbor, csv, toml, yaml, xml, args, env, db, layout)` is at `Prelude/Markers.jet:123`. `Context(allocator, logger, deadline)` is at `Prelude/Markers.jet:166`.

**Open.** Any package can declare a marker (F2). Its body can `reject(code:, what:, why:, fix:)`, emit `impl $tname`, and loop over fields.

**Law.** D-MARKER-TARGETS1, D-META-REG1, D-META-OPTIN2, D-META-HOLD1. Open ballots D-MARKER-DIAG1, D-MARKER-LADDER1, D-MARKER-REFLECT1, D-MARKER-EXPAND1 and D-META-HASH1 sit in this layer. Ballots-4 owns their rework.

**Gaps.** The `Rename` menu means a package format cannot accept `#Rename(msgpack: "…")`. The `Context` menu means a package service cannot be a context key; D-CORE-PROVIDER1 already replaces it with a generated service-key list.
A package marker's `reject` must borrow a Core code. `declared_text_block.jet` borrows `E0927` for a meaning unrelated to that code.

**Prior art.** Rust's `#[diagnostic::on_unimplemented]` lets a library write the error text for its own trait [S3]. GHC's typechecker plugins let a library solve constraints and rewrite type families [S27].

**Costs.** Opening the `Rename` menu means the parameter becomes `Rename(format: Format, name: String)` or a per-format marker. Both are compile-time only.

**Verdict.** Markers are the most open layer already. Close the two menus by reading them from declarations: formats from the codec trait (4.27), context keys from the provider list.

### 4.4 Operators and literal forms

**Closed.** Operator hooks exist only for `Add`, `Sub`, `Mul`, `Div`, `Equatable` and `Comparable` (`JetSema/Source/Sema/Registration/Checked.jet:2729-2751`).
`JetCli/Source/Cli/Inspect.jet:1155` and `:1167` mirror four of them. The AST has 22 binary operators (`AST/Expressions.jet:152`).

**Law.** D-OP-NEG1 adds `Neg` and `Mod` traits; it is ratified and not yet built. D-INFER-LINEAR1 forbids result-type overloading. D-OP-CALL1 forbids a call hook. D-CORE-ROLE1 keeps the builtin role enum closed and Core-only.

**Candidates.** (a) Every remaining operator maps to one Core trait, declared in Jet: bit operators, shifts, indexing, range construction. (b) User-defined operator symbols.

**Prior art.** Rust maps each operator to a trait in `core::ops` and allows no new symbols. Haskell and Scala allow new symbols, which makes code hard to read for people new to a library `[INFERENCE]`.

**Costs.** (a) is free at run time, since a trait call on a concrete type is static. (b) breaks the parse-without-running rule.

**Verdict.** (a). The operator-to-trait table should be a declared Prelude twin, so sema and `jet inspect` read one list.

### 4.5 Type system

**Closed.** Census `types.core`: domain 6/62, kernel 9/61. `types.facts`: domain 14, kernel 35. `types.units`: domain 32/808.
`foundation_named_type` (`Types/Types.jet:34`, CL00904) names 16 scalar types. `type_simd_lane_arity` (`Types.jet:1222`, CL00916) names 20 lane types.
`InternalTag` (`Types.jet:88`, CL00907) gives the compiler private tags for Core types such as `DeterministicClock` and `ExpiringSecretLoan`.
Sema names 110 Core types in `sema_call_reflection_synthetic_printable` (`Calls/Reflection.jet:136`, CL01282). Further name tables sit in `TypeRelations.jet:1000`, `CoreUsage.jet:478` and `Constructors.jet:112`.
Target facts such as `TargetMemory` and `TargetMmio` are Prelude rows (`Prelude/Facts.jet:54-58`).

**Open.** Distinct types, unit families (F5), traits (F1), and `derive`.

**Law.** D-CORE-ROLE1 (roles closed, Core-only), D-CORE-NUMFAMILY1 (one numeric family), D-KEY-TRAITS1 (`KeyBy`), D-CORE-LAWS1 (`#Test` inside a trait is a law).

**Gaps.** The compiler should not know the names of Core library types. Where it needs a property, such as "printable" or "has a deterministic clock", that property should be a trait or a declared fact on the type.

**Prior art.** Rust's orphan rules decide which crate may implement which trait for which type. RFC 2451 widened them, because otherwise "a crate like Diesel will never be able to be designed in a completely extensible fashion" [S61].
Wadler named the underlying tension: adding cases and adding operations without recompiling, with static safety (Wadler, 1998-11-12 [S10]).

**Costs.** Replacing a name table with a trait or fact costs one lookup per use at compile time and nothing at run time.

**Verdict.** Types stay open through traits. Every name table about Core types becomes a trait bound or a declared fact. Roles stay kernel.

### 4.6 Effects

**Closed.** Census `semantics.effects`: domain 28, kernel 18. Each row is one Prelude effect, such as `DB`, `DB.Read`, `DB.Write` (`Prelude/Effects.jet:18-20`), `Log` (:29) and `GPU` (:30).
`EFFECT_VOCABULARY_ROWS` is a generated mirror (`Registry/EffectVocabulary.jet:10`, CL00833, 46 rows).
`Core/Compiler/Lang.jet:25` has a third copy, `Effect` (CL01783, 12 members). DEFECT 1 (#6206) records that these copies disagree.
`authority_effect_action` (`Policy/Authority.jet:376`, CL00743) switches on 15 root names.

**Open.** A package may declare a leaf under an existing root (F4). `EffectsSurface.jet:29-30` says "A declared leaf makes its closed root checked in the package view".

**Law and ballots.** Effect roots are closed by law today. The open ballots D-EFFTYPE-KERNEL1, D-EFFTYPE-MODEL1, D-EFFTYPE-USES1 and D-EFFTYPE-GRANT1 would reduce the compiler's roots to about ten machine primitives and move `Log`, `DB`, `GPU`, `Browser` and `Secret` into Core.

**Prior art.** Koka types effects and lets libraries define new effects with handlers [S48].

**Costs.** A package root costs nothing at run time; effects are erased after checking. It costs policy work: grants and authority need a rule for a root the policy file has never seen.

**Verdict.** The effects ballots already move in this direction. This document adds one requirement: the authority switch at `Authority.jet:376` must read declared roots, not names.

### 4.7 Memory and ownership

**Closed.** `Context(allocator, logger, deadline)` (`Prelude/Markers.jet:166`). `PolicySetting` (`Core/Compiler/Lang.jet:111`, CL01792) includes `GC` and `ExplicitUnits`.
The allocator byte suffixes are closed (4.1).

**Open.** D-CORE-PROVIDER1 makes `Allocator` a trait; boards, `#Context` blocks and tests name a value of it.
D-HOOK-RUNTIME1 adds watch-only memory hooks that are free when unused.

**Kernel.** Ownership rules, drop order and layout are soundness and stay closed. `Layout` and `ABI` are kernel.

**Verdict.** Allocators are open by law; building D-CORE-PROVIDER1 opens them. Nothing more is needed here.

### 4.8 Errors

**Open.** Error types are ordinary types marked `#Error`. A package defines its own. Error unions are open.

**Closed.** Only the diagnostic side is closed; see 4.15.

**Verdict.** Already open.

### 4.9 Concurrency and scheduling

**Closed.** Census `stdlib.jobs` kernel 2/14 and `stdlib.sync` kernel 2/11. `ServiceRestart` (`Core/Service/Service.jet:529`, 3 strategies) and `ServiceDelivery` (:535, 2 guarantees) are domain vocabularies.

**Law.** D-HOOK-LIFECYCLE1 gives `core.jobs` hook points. D-HOOK-RUNTIME1 gives watch-only scheduler hooks. The scheduler itself is not a provider.

**Candidates.** (a) A scheduler provider trait, chosen per target like an allocator. (b) Restart strategies as a trait with Core's three as instances.

**Prior art.** Rust's async ecosystem lets a library pick the executor; the cost is several executors that cannot share tasks `[INFERENCE]` from common experience, not from a cited measurement.
Erlang/OTP's supervisor strategies are the source of `OneForOne`, `OneForAll` and `RestForOne`, and OTP keeps them closed `[INFERENCE]`.

**Costs.** A scheduler provider is resolved at link time, like an allocator, so it costs nothing per call.

**Verdict.** Keep the restart and delivery lists closed. They are a small fixed vocabulary with clear meaning. Make the scheduler a target-profile provider when D-CORE-PROVIDER1 lands, so embedded and JetOS boards can supply one.

### 4.10 Evaluation and compile-time code

**Closed.** Census `semantics.comptime`: domain 15/88, kernel 4/18. `CompilerValueMethods.jet:20-38` (CL00417–CL00426) lists by name which records and fields the compile-time evaluator exposes, such as `CompilerManifest` and `ProgramInfo`.
`COMPILER_STAGE_ALL` (`AST/Comptime.jet:13`, CL00503) lists five stages.

**Law.** D-META-COMPILER1 gives one Compiler value in the build root, with `meta.events(watch:)`, `meta.add`, `meta.rewrite_body` and `#code {}` quotes. D-HOOK-STAGES1 and D-JAI-EVENTS1 list the stage points.

**Gaps.** The evaluator's method list should be generated from the Core declarations of those records, so a new field appears without an evaluator change.

**Prior art.** Zig runs ordinary code at compile time with the same language [S60].
Roslyn replaced its first generator interface with incremental generators "to allow users to specify generation strategies that can be applied in a high performance way", and caches each step [S4].

**Costs.** Stage hooks cost compile time only when attached. Roslyn replaced its first generator interface to scale to very large projects, which suggests caching per step from the start `[INFERENCE]` from [S4].

**Verdict.** The law is right. Build it with per-step caching, and generate the evaluator's record list.

### 4.11 Checked text and embedded languages

**Closed.** `TypedHeadKind` names the text heads the compiler knows. DEFECT 3 (#6208) records that it disagrees with `CheckedText`.

**Open.** `trait CheckedText` (`Prelude/core/prelude.jet:19-23`) has `check(text)` and `encode_hole(value)`. A package type implements it (F1).
The example `Examples/features/safety/checked_text_sql.jet:1-4` says SQL and HTML are "library-defined, nominal String-backed types with ordinary `CheckedText` implementations".

**Probe (ec16c32d35).** A package text head with a constant hole works. A package text head whose hole is a run-time local stops the compiler with an internal error: "comptime evaluator can't build `…Token checked text literal` … read of unknown local name".
The repro is a package `Token` text head used as `accept(Token{"hello {name}"})` with a local `name`; it is carded as #6218. Core's `SQL{"… {id}"}` with a local works, so Core and packages do not take the same path.

**Gap.** `encode_hole` returns text. SQL needs bound parameters, not text. So Core's SQL cannot be an ordinary `CheckedText` until the trait can lower a hole to a parameter.

**Candidates.** (a) Add one method that lowers a hole to a typed value, with text encoding as the default. (b) Keep SQL special.

**Prior art.** Tree-sitter injections let a grammar say "this string is SQL" through a query file, without changing the host grammar [S32].

**Costs.** (a) is one trait method and one compile-time call per hole. The emitted code is the same as Core's SQL path today `[INFERENCE]`.

**Verdict.** (a). It closes DEFECT 3 and #6218 at the root, and it is the model case for the design in section 6.

### 4.12 Parse and sema pipeline

**Closed.** Census `pipeline.sema`: domain 314/3,177, kernel 65/406. This is the largest layer by findings.
Sema names Core APIs directly: web app methods (`Sema/Calls/Methods.jet:882`), browser argument types (`Calls/UiHandles.jet:406`, CL01306, 80 names), browser effects (`UiHandles.jet:488`, 42), stream handles (`Calls/StreamHandles.jet:269`), fixed Core records (`Calls/Core.jet:1150-1182`).
`THandleOp` (`Sema/Tree.jet:456`, CL01489) has 270 members, one per Core handle operation. `SEMA_CASING_ACRONYMS` (`Sema/Casing.jet:66`, CL01321, 99 acronyms) duplicates `Data/Syntax/Acronyms.jet:10` (CL00432).

**Law.** D-CORE-BOUNDARY1 and D-CORE-BUILTINS1 move Core meaning into Core source behind one builtins door. D-OPENTABLE1 generates compiler tables from declared twins.

**Verdict.** The sema name tables are the main migration work. Each should become either a generated twin of a Core declaration, or a property read from the type. Section 7 gives the rule.

### 4.13 IR and optimisation

**Closed.** Census `pipeline.ir`: domain 109/6,674, kernel 33/2,827.
`IR_RUNTIME_ROUTES` (`JetIR/Source/RuntimeRoutes.jet:49`, CL01031) has 1,618 hand-sorted rows, with key, handle, field and native-type tables after it (:1687, :4946, :5594, :7171).
`IR_LINT_OPS` (`JetIR/Source/Lint.jet:196`, CL00976, 49 ops) and `IR_LINT_HANDLE_VERBS` (`Lint.jet:1155`, 6 verbs) are closed.
`BodyHandleOp` (`Facts/Body.jet:417`, CL00643, 270, generated) and `TBuiltinOp` (`Facts/Checked.jet:523`, CL00680, 209) mirror `THandleOp`.

**Kernel.** The IR op set is kernel by law (D-ARCH-IR1). D-CORE-BOUNDARY1 keeps the numbered kernel table closed.

**Candidates.** (a) Optimisation passes as packages, like LLVM passes [S29] or MLIR dialects [S11, S30]. (b) Rewrite rules declared in Jet, checked against a plain body.

**Prior art.** MLIR was built "for extension and evolution", with dialects as the unit of growth (Lattner et al., 2020-02-25 [S11]).
LLVM documents how to write passes for its new pass manager [S29]. Both systems assume pass authors are compiler engineers `[INFERENCE]`.

**Costs.** User passes would break I9 unless every pass is proven to keep the meaning of the plain body. D-CORE-BOUNDARY1 already requires that check for instruction entries.

**Verdict.** No user passes. The route tables are domain data and should be generated from Core declarations (near-defect: hand-sorted). Instruction entries (D-CORE-BOUNDARY1) are the only library-supplied lowering, and they carry a plain-body proof.

### 4.14 Back ends, targets and ABI

**Closed.** Census `pipeline.backend`: domain 8/69, kernel 15/345. `deploy.targets`: domain 6/65, kernel 30/182.
`TargetCapability` (`Target/TargetMachine.jet:5`, CL00894, 14 capabilities) disagrees with the target facts (DEFECT 4, #6209).
`jet_target_capabilities_for_core_usage` (`TargetMachine.jet:695-705`, CL00896/CL00898) maps Core module and function names to capabilities.
The driver picks target machines by name (`JetDriver/Source/Pipeline.jet:616`). `Core/Compiler/Lang.jet:161` lists six targets (CL01795).

**Law.** D-TARGET-ONE1 fixes nine output kinds. D-CORE-BOUNDARY1 says a new target, register kind or calling convention needs a compiler change. D-HOST-CAPS1 makes hardware kinds package-defined.

**Prior art.** rustc reads custom targets from a JSON specification without a compiler change [S52]. OpenXLA's PJRT is a C API that each device implements as a plug-in, so frameworks never see device code [S15].

**Verdict.** New instruction sets and boards are library changes by law. New code generators stay compiler changes. Capability lists should be read from Core's declared facts, not names; this is DEFECT 4.

### 4.15 Diagnostics and lints

**Closed.** Census `process.diagnostics`: domain 23/1,704. `DIAGNOSTIC_ROWS` (`Registry/DiagnosticRows.jet:1`, CL00830) has 863 rows. `Prelude/Diagnostics.jet` has 992 rows: 841 active, 135 retired and 16 reserved.
`pipeline.diagnostics`: kernel 6/29, which includes stage names that drifted (DEFECT 9, #6214).
The formatter has its own list of 23 teaching parse codes (`JetParser/Source/Formatter/Formatter.jet:1449`, CL01073).

**Open.** Nothing. A package marker can only borrow a Core code.

**Law and ballots.** I4 requires a code, text and snapshot for every diagnostic. D-MARKER-DIAG1 (open) proposes `diagnostic Name(params) { what:, why:, fix: }` printed as `package.Name`, with Jet's own codes in the same form.

**Prior art.** Go's analysis framework lets any package ship an `Analyzer` and pass typed facts between packages [S38]. ESLint and Dylint load rules from plug-ins [S53, S54].
Rust's diagnostic attributes let a library write the message for its own trait [S3].

**Costs.** A package diagnostic is data. It costs nothing at run time. Lint rules cost compile time per attached rule.

**Verdict.** D-MARKER-DIAG1 is the right shape. A lint is then a package hook on `compiler.checked` that reports a declared diagnostic. That needs no separate lint form.

### 4.16 Runtime

**Closed.** Census `runtime`: domain 1/1,812. `TRAP_DEBUG_RANGES` (`JetRuntime/Source/TrapMessages.jet:1009`, CL01098) is generated.
`RUNTIME_STOP_EXPORTS` (`Registry/RuntimeStopExports.jet:6`, CL00842) has 2,475 generated rows.

**Verdict.** These tables are generated from Core. They are the model for the route tables in 4.13. No package extension point is needed in the runtime beyond providers (4.7, 4.9) and watch-only hooks (D-HOOK-RUNTIME1).

### 4.17 Language server and editor

**Closed.** Census `tooling.lsp`: domain 6/81, kernel 2/14. `lsp_handle_request` switches on 35 method names (`JetLSP/Source/LSP/Requests.jet:14`, CL01064) and `lsp_handle_notification` on 9 (`Requests.jet:87`, CL01065).
The reasoning view has 9 fixed panels (`LSP/Reasoning.jet:262`, CL01063). Navigation target kinds are fixed at 18 (`LSP/TypeReferences.jet:47`, CL01066).

**Law.** D-HOOK-STAGES1 publishes two editor stages: `lsp.code_actions` and `lsp.hover`. D-ONE-TOOLS1 makes compiler queries the one fact owner.

**Gaps.** No stage exists for completion, semantic tokens, inlay hints, code lenses, rename or diagnostics-on-type. A package that adds a marker or text head cannot teach the editor to complete it or colour it.

**Prior art.** The Language Server Protocol lets a server register capabilities dynamically per client [S31]. VS Code declares extension features in a `contributes` manifest and starts an extension only on its activation event, such as `onLanguage:python` [S20, S21].
Tree-sitter colours embedded languages through injection queries in a file next to the grammar [S32].

**Costs.** An editor stage costs nothing until a package attaches to it. Each attached hook adds latency to the request it serves. VS Code's lazy activation is the answer to that cost: run a package's editor hook only when the file uses that package.

**Verdict.** Add stages for completion, semantic tokens and inlay hints, with the same merge identities as D-HOOK-STAGES1. Run a package's hook only for files that import the package. Package markers and text heads then get editor support without an LSP change.

### 4.18 Formatter

**Closed.** The formatter keeps its own list of teaching parse codes (`Formatter/Formatter.jet:1449`, CL01073, 23 codes). Formatting rules for Jet syntax are hand-written in the formatter.

**Law.** D-HOOK-STAGES1's `fmt.file` stage covers only declared non-Jet file kinds.

**Gap.** A block marker body (F3) is a small language inside Jet. Nothing formats it.

**Prior art.** Prettier says "Prettier's own implementations of all languages are expressed using the plugin API" [S46]. Topiary lets a formatter author write rules as Tree-sitter queries, without writing an engine [S44].

**Costs.** A block formatter hook runs per block, only where the marker is used.

**Verdict.** Extend `fmt.file` with a `fmt.block` point keyed by marker. Jet's own syntax stays in the formatter. The teaching-code list should read a declared diagnostic property instead of a list.

### 4.19 Testing, laws and conformance kits

**Closed.** `jet_cli_comparison_terminal` (`JetCli/Source/Cli/TestCompare.jet:200`, CL00233, 7 states). `stdlib.testing`: domain 2/6, kernel 8/32. Fake host lists such as `FAKE_EN_HOSTS` (`Core/Testing/Testing.jet:307`).

**Open.** `#Test` blocks, `comptime loop` over tests, `#Test(faults: [.FS])`, and D-CORE-LAWS1 laws: a `#Test` inside a trait runs against every implementation, and `#Waive(law, "reason")` skips one with a reason.

**Law.** D-CORE-LAWS1, D-ACCEL-LIB1 (a public device interface plus a CPU reference suite), D-REGISTRY-EXAMPLE1 (publishing runs every doc example), D-CORE-PROVIDER1 (fakes become provider types), D-HOOK-STAGES1 `test.finished`.

**Prior art.** Go's `fstest.TestFS` checks any file-system implementation against the `fs.FS` contract with one call (Go Authors, `testing/fstest/testfs.go` [S9]).
embedded-hal 1.0 defines peripheral traits so that drivers "work on any microcontroller with an embedded-hal implementation without modifying them" (Rust Embedded WG, 2024-01-09 [S18]).

**Verdict.** Laws are already the conformance-kit mechanism. Every open vocabulary in section 6 should ship its laws in the trait. A kit then needs no separate tool: implementing the trait pulls in its tests.

### 4.20 Debugger

**Closed.** Census `tooling.debugger`: domain 9/165, kernel 11/46. The DAP handler switches on 29 commands (`JetDebug/Source/Dap02.jet:111`, CL00244).
Scalar display hard-codes type names three times: `debug_scalar_class` (`Debug/Snapshot.jet:311`, 31 names), `safe_value` (`Inferior.jet:520`, 30) and `jet_type_name` (`Inferior.jet:533`, 26).

**Gap.** A package type cannot say how it should look in the debugger. A `Map` or a ring buffer shows its raw fields.

**Prior art.** Rust RFC 3191 lets a crate ship debugger visualizer scripts with its source (start date 2021-11-01 [S8]). GDB loads Python pretty-printers [S50]. LLDB has type summaries and synthetic children [S50].

**Costs.** A visualizer runs only while debugging. It costs nothing in the program.

**Candidates.** (a) A Core `DebugView` trait with a pure method that returns a tree of labelled values. The debugger calls it in the inferior or the evaluator. (b) Scripts in a debugger language, as in GDB.

**Verdict.** (a). It keeps one language and reuses `Debug`. The three scalar name lists should become one read of declared type facts.

### 4.21 Profiling, tracing and performance budgets

**Closed.** Perf report fields are fixed in `Performance/BudgetReport.jet:607-716` (CL00735–CL00738). Budget suffixes are fixed (4.1).

**Law.** D-HOOK-RUNTIME1 adds watch-only scheduler and memory hooks, free when unused.

**Prior art.** DTrace places probes that cost nothing when disabled (Cantrill, Shapiro and Leventhal, USENIX 2004 [S58]). Perfetto groups track events into categories that are enabled one by one [S59]. Tracy is a frame profiler built around zones in user code [S59].

**Verdict.** Let a package declare its own trace categories as typed events (D-EVENT1). Keep the report format closed, since it is a stable interchange file.

### 4.22 Inspect, docs and the symbol index

**Closed.** Census `tooling.index`: domain 9/63, kernel 16/80. `doc_kind_rank` (`JetSemIndex/Source/Docs/Graph.jet:283`, CL01099) and `jet_cli_doc_kind_rank` (`JetCli/Source/Cli/Docs.jet:284`, CL00053) are the same 15-kind list in two places.
`semantic_builtin_special_docs` (`Symbols/BuiltinDocs.jet:88-105`) hand-writes docs for Core methods. `jet_cli_inspect_expand_report` (`Inspect.jet:1736`) has 8 fixed report kinds.
`InspectShapes.jet:239` fixes output formats.

**Law.** D-HOOK-STAGES1 `doc.page`; D-ONE-TOOLS1.

**Verdict.** Docs for Core methods should come from doc comments in Core source, like any package. Doc kinds should come from the declaration table (D-META-REG1). `doc.page` covers package additions.

### 4.23 Command line

**Closed.** Census `tooling.cli`: domain 130/2,053, kernel 77/472. `JET_CLI_SYNTAX_ROWS` (`Cli/SyntaxDictionaryData.jet:32`, CL00231, 298 rows) and `JET_CLI_FIXED_FLAGS` (`Registry/CLICommands.jet:698`, CL00779, 236 flags).
`JET_CLI_CORE_DECLARATIONS` (`Cli/StructureRegistry.jet:25`, CL00228, 116 Core names) has drifted in four modules (DEFECT 6, #6211).
`JET_CLI_FIRST_PARTY_RESERVED` (`InspectB.jet:142`) and `JET_DRIVER_FIRST_PARTY_RESERVED` (`Driver/Loader.jet:106`, CL00399, 28 names such as `core jet c rust py js swift go`) reserve package names.

**Law.** D-SURFACE-BUDGET1 counts commands. D-BUILD-CORE1 makes `Build`, `Dev` and `Test` Core types; `b.action(...)` declares a project action.

**Candidates.** (a) Package commands as `jet <pkg>:<action>`, from `b.action`. (b) Git-style external commands on `PATH`.

**Prior art.** Cargo runs any `cargo-<name>` binary on `PATH` as a subcommand `[INFERENCE]` from common use; not cited. Bazel exposes user rules as build targets, not new verbs [S36].

**Verdict.** (a) through `b.action`, already ratified. The CLI's own verbs stay counted and closed. The syntax dictionary should be generated from the syntax registry; it is a mirror today.

### 4.24 Build: steps, toolchains, caches, lockfiles

**Closed.** Census `build.session`: domain 11/76, kernel 3/60. Build capability names `net fs io db time rand env exec` appear three times (`JetBuildSession/Source/Session.jet:296`, `:299`, `:305`).
`build_session_add_target` (`Declarations.jet:136`, 8 target methods) and `build_key_target_kind` (`Keys.jet:92`) fix the target kinds. `Session.jet:664` names foreign build tools: cmake, make, gradle, npm and cargo.
`jet_cli_lock_load` (`InspectProvenance.jet:332`, `:380`) fixes lockfile sections and keys.

**Law.** D-BUILD-CORE1: `b.action`, `b.toolchain`, `b.probe`, `plugin`, `contribute`. D-HOOK-STAGES1: `build.planned`, `build.asset`. D-JAI-EVENTS1: `build.link`, `before_write`, `after_write`. D-REAPI1 and D-CORE-HEADER1 cover remote execution and headers.

**Prior art.** Bazel separates rule logic from tool choice with toolchains, resolved per platform [S36]. Bazel module extensions read data from every module in the dependency graph and create repositories [S36].
Gradle and Bazel both treat a foreign build tool as a rule written in the build language `[INFERENCE]`.

**Verdict.** Foreign build tools should be toolchain providers (`b.toolchain`) in packages, not names in the session. The capability list should be the effect roots (4.6). Lockfile keys are a file format and stay closed, with a schema version.

### 4.25 Packages, registries and Jetpack

**Closed.** Jetpack has 373 domain lists. The largest: `ServicePresetKind` (`Jetpack/Services/ServiceCatalog.jet:31`, 43 presets) and `JETPACK_VERBS` (`Foundation/Syntax.jet:625`, 28 verbs).
Provider lists sit in four places that disagree (DEFECT 12, #6217): `RefSpec.jet:130`, `ProviderFamily.jet:7`, `Syntax.jet:52` and `ProviderFacts.jet:577`.
`CranProvider.jet:258`, `NixEval/Builtins.jet:261` (37 Nix builtins), `PackageParse.jet:168/236/460`, `InlinePackage.jet:13` (`PKG_MANIFEST_BLOCKS`), `Variant.jet:194` and `JetOS/OptionsRendering.jet:117` are further domain lists.
In the compiler, `jet_driver_parse_package_root` (`Driver/Loader.jet:1058`, CL00402) fixes 12 package root fields.

**Law.** D-CORE-PROVIDER1 for services; D-ADOPT-TIER1 for adoption tiers; D-MANIFEST-ONE1 is open.

**Prior art.** Terraform providers are separate binaries behind a versioned gRPC protocol; "minor versions of the protocol are additive" [S19]. NixOS modules declare options and merge definitions from many modules with `mkMerge` [S33].
Kubernetes custom resources extend the API, and "many core Kubernetes functions are now built using custom resources" [S35].

**Costs.** A provider package costs nothing in programs; it runs only in Jetpack.

**Verdict.** A package source (a registry, a language ecosystem, a store) should be a provider trait in Jet, with Jetpack's own providers as instances. Service presets should be package rows. Closing DEFECT 12 is the first step: one declared provider list.

### 4.26 Foreign code (FFI and binders)

**Closed.** Four foreign-language lists disagree (DEFECT 2, #6207). `Registration/Items.jet:989-996` builds an effect name `FFI.[language]` from a closed enum.
Driver tables name types per language: `jet_driver_rust_foreign_record_type` (`Driver/RustForeign.jet:255`, 14 names), `jet_driver_foreign_parse_provenance` (`ForeignProviders.jet:723`, 20).
Jetpack binders repeat this per schema: `SqlSchemaBinder.jet:320`, `ProtoSchemaBinder.jet:283`, `HeaderParser.jet:509/529`, `BinderRender.jet:176`.
`jet_cli_import_map_javascript_doc_type` (`ImportBindings.jet:140`) maps JavaScript doc types.

**Law.** D-FFI-ONE1, D-CIMPORT1. The REWORK idea under discussion makes a binder one Jet metaprogram package, removing the closed language enum, the reserved roots and the per-language error codes.

**Prior art.** F# type providers generate types from an outside schema at compile time [S47]. The WebAssembly Component Model describes interfaces in WIT, independent of source language [S49].

**Verdict.** A binder is a package that implements a binder trait and runs at compile time under grants. The effect for foreign calls should be declared by the binder package, not built from a name.

### 4.27 Standard library domains

The census shows that Core itself is mostly open by construction. Its domain lists are small vocabularies. The closures that matter sit in the compiler, which names Core's types.

| domain | Core domain lists (findings/members) | examples | what should be open |
|---|---|---|---|
| encoding and serialization | 16/70 | `EncodingFormat` (`Core/Encoding/Encoding.jet:77`, 7 formats) copied as `format_row` (`Prelude/Core.jet:274`); `DataEvent` (`Encoding.jet:61`) | the format list; the event model stays closed |
| data | 13/49 | `DataFormat` (`Core/Data/Data.jet:33`); plot channels, aggregates, scales (`Data.jet:385-495`) | formats and aggregates |
| database | 4/18 | `DBValue` (`Core/Db/Db.jet:27`); `JetSQLAggregate` (`Core/Db/Sql.jet:34`) | drivers and aggregates |
| HTTP | 5/22 | `Method` already has `Custom` (`Core/Http/Http.jet:55-65`); `Version` (`Http.jet:68`) | nothing more; versions are a standard |
| web | 6/23 | `WebFormControl` (`Core/Web/Forms.jet:23`, 8 controls); `WebRouterValueType` (`Router.jet:16`) | form controls and route value types |
| UI | 10/35 | `UICapability` (`Core/Ui/Host.jet:9`); `UINodeKind` has `Custom` (`Ui.jet:44`) | capabilities and roles |
| terminal UI | 4/19 | `TUIEvent`, `TUIColorProfile` (`Core/Tui/Tui.jet:22-50`) | small; keep |
| net and TLS | 2/5 | `TLSVersion` (`Core/Net/Tls.jet:57`) | a standard; keep |
| crypto | 0 domain, 22/677 kernel | round constants such as `C_SHA256_K` (`Crypto.jet:1371`) | algorithms as functions are already open |
| compiler | 9/50 | `Effect` (`Core/Compiler/Lang.jet:25`), `Target` (:161), `TaintKind` (:153) | read from declarations |
| files | 6/21 | Windows path prefix kinds (`KernelPathComponents.jet:9`) | a standard; keep |
| services | 2/5 | `ServiceRestart`, `ServiceDelivery` | keep (4.9) |

**Encoding is the clearest case.** A package that adds MessagePack today can write its own encoder. It cannot appear in `EncodingFormat`, in `#Rename(msgpack: …)` or in the data loaders.
The serde data model shows the alternative: 29 types that every format maps to, so formats and types grow independently (serde.rs [S39]). Jet's `DataEvent` already plays that role.

**Prior art for registries.** Go's `database/sql.Register` and `crypto.RegisterHash` let a package add a driver or hash at init time [S40, S41]. Java's JCA is provider-based [S42]. Python's `codecs` module keeps a codec registry [S51].
All four use a global run-time table. Jet should get the same openness at compile time, with no global state, through traits and declared rows.

**Verdict.** Format, driver and aggregate lists become traits with Core's members as instances. Closed vocabularies that a standard fixes stay closed. The compiler must not name Core types; that is section 4.12's work.

### 4.28 Hardware, accelerators and embedded

**Law.** D-CORE-BOUNDARY1 (instruction entries in Core), D-HOST-CAPS1 (package hardware kinds), D-ACCEL-LIB1 (device interface plus CPU reference suite), D-SIMD-NATIVE1.
`core_value_fixed_signature` (`Registry/CoreValueSignatures.jet:155`, CL00822) names devices `device_cpu device_auto device_metal device_cuda device_vulkan…`.

**Verdict.** The law is library-first. The device name table should become the provider list from D-ACCEL-LIB1.

### 4.29 Infrastructure, services and JetOS

**Law.** D-INFRA1: typed Jet infrastructure with Terraform providers under `Net` and `Secret` grants. Jetpack `ServicePresetKind` (43 presets) and `JetOS/OptionsRendering.jet:117` are closed.

**Prior art.** NixOS modules [S33]; Kubernetes operators [S35].

**Verdict.** Service presets and JetOS options should be module declarations in packages, merged by a declared merge law, as NixOS does.

### 4.30 Deployment and output kinds

**Closed.** `OutputKind` (`AST/Items.jet:1238`, 9 kinds) is fixed by D-TARGET-ONE1.

**Verdict.** Kernel by law. New deployment shapes ride on `Image`, `Bundle` and `System` outputs plus build actions.

### 4.31 Policy, authority and trust

**Closed.** Census `semantics.policy`: domain 15/169, kernel 5/33. `gate_kind_parse` (`Policy/GateLedger.jet:52`, CL00746, 38 gate kinds), `policy_key_parse` (`Policy/Policy.jet:35`, 20 keys), `POLICY_RULES` (`Policy.jet:122`, 20 rules), `MARKER_SOURCE_SITES` (`MarkerSource.jet:9`).
`TaintKind` (`Core/Compiler/Lang.jet:153`) has four kinds; taint tags exist in three copies (near-defect).

**Gap.** A package cannot declare a new taint kind or policy key. A health-data package cannot mark values as `PHI`.

**Verdict.** Taint kinds should be declared rows with sinks declared by the packages that own them. Policy keys stay closed, since a project's policy file must mean the same across packages.

### 4.32 Naming, casing and acronyms

**Closed.** `SEMA_CASING_ACRONYMS` (99, `Sema/Casing.jet:66`) and `ACRONYMS` (`Data/Syntax/Acronyms.jet:10`) are the same list twice.

**Verdict.** One list, read by both. A package may add acronyms for its own names through a row. This is small but it is the clearest single example of the drift problem.

### 4.33 Evolution: versions, editions, deprecation, migrations

**Today.** `Docs/spec/release-policy.md:23-86` sets SemVer for the compiler and `edition: "2026"` in `package.jet`; an unknown edition is E2001.
`#Deprecated(since:, use:, removed_in:)` gives L2001 and becomes E2002 after the removal edition. `jet fix` performs only plain renames. D-REL5 allows rewrites only on explicit request.

**Gaps.** A package cannot ship a migration richer than a rename. Adding a member to a public enum breaks every exhaustive match in dependents.

**Prior art.**
- Go 1.26's `go fix` runs analyzers that modernise code (Donovan, 2026-02-17 [S17]). `//go:fix inline` lets a library mark a deprecated function so that `go fix` inlines its body at call sites, which "enables self-service API migration" (Donovan, 2026-03-10 [S16]).
- Rust editions require that "crates in one edition must seamlessly interoperate with those compiled with other editions" [S26].
- Swift SE-0192 split enums into frozen and non-frozen. Clients must handle future cases with `@unknown default`; in Swift 5 the check is a warning (Rose, implemented in Swift 5.0 [S6]).
- Rust's `#[non_exhaustive]` marks enums and structs that may grow (RFC 2008, 2017-05-24 [S7]).
- Unison names every definition by a hash of its syntax tree, so a rename never breaks a dependent [S55].

**Verdict.** Two additions. First, `#Deprecated` should accept a replacement body, so `jet fix` can inline it, as `//go:fix inline` does. Second, a public vocabulary that may grow should be a trait (open) or be marked as growing, so that matches need an `else` arm.

### 4.34 Process: syntax registry, diagnostics registry, Core surface ledger

**Closed.** Census `process.core-ledger`: domain 47/6,876. `CORE_CALLS` (`Registry/CoreCallRows.jet:10`, 1,990, generated), `dispatcher_row` (`Prelude/Core.jet:47`, 1,960, generated), `CORE_RECORD_FIELD_ROWS` (`Registry/CoreRecordRows.jet:7`, 1,046, hand-written), `CORE_SOURCE_OWNED_TYPES` (`CoreEnumRows.jet:185`, 200, generated).
`process.syntax-registry`: domain 68/1,126, including `LEXICAL_LEDGER` (`Data/Syntax/Spellings.jet:11`, 53) and `NAME_CASE_CATEGORIES` (`Spellings.jet:277`, 36).
`process.registry`: domain 12/2,932.

**Law.** I7, D-SURFACE-BUDGET1, D-OPENTABLE1, D-ONCE-LAW1 (one home and a guard).

**Gap.** D-OPENTABLE1 generates compiler tables from declared twins. It does not say that packages may add rows to those twins. Generated tables are frozen when the compiler is built.
DEFECT 8 (#6213) found decision citations in 883 lines of 77 files, against I7.

**Verdict.** The registries are the right shape. Section 6 extends D-OPENTABLE1 one step: Core's rows are generated into the compiler as a cache, and package rows join them per program.

### 4.35 Layers this survey added to the taxonomy

The brief's taxonomy missed five layers. Each has closed lists in the census.

| added layer | where | evidence |
|---|---|---|
| policy, authority and trust | 4.31 | `GateLedger.jet:52`, `Policy.jet:35/122`, `TaintKind` |
| naming, casing and acronyms | 4.32 | two acronym lists, 99 entries each |
| core trait vocabulary | below | the protocol traits are compiler names, not Jet declarations |
| target capability mapping | 4.14 | `TargetMachine.jet:695-705` maps Core names to capabilities |
| developer server and canvas | below | `JetDevserver`: domain 40/351, kernel 94/507 |

**Core trait vocabulary.** Only one trait in `Compiler/Prelude` is declared in Jet source: `CheckedText` (`Prelude/core/prelude.jet:19`). In `Core/`, only `GeometryPoint` (`Core/Math/Math.jet`) is.
The protocol traits `Display`, `Debug`, `Encode`, `Decode`, `Equatable`, `Comparable`, `Add`, `Sub`, `Mul`, `Div` and `Iterable` are names in sema. `sema_raw_protocol_trait` (`Sema/Declarations.jet:40`, CL01348) and the synthetic list in `Sema/Calls/Methods.jet:319` hold them. 33 compiler files name `Comparable`.
Their derives are already Jet (`Prelude/Derives.jet:13` derives `Equatable` in Jet). The trait declarations themselves should be Jet too, carrying their roles (D-CORE-ROLE1) and their laws (D-CORE-LAWS1).
This matters for extension. A package trait and a Core trait should differ only in where they live.

**Developer server and canvas.** `apply_transaction_json_on_compiler_stack` (`JetDevserver/Source/Canvas/EditActions.jet:42`, CL00275) switches on 35 edit operations. `validate_debug_command` (`DebugSourceGit.jet:380`) on 33 verbs. `decode_devtools_body` (`WebHost.jet:209`) on 20 panel kinds.
Canvas edit operations are a protocol and can stay closed. Devtools panels are a domain list: a package that adds a store or a queue should be able to add a panel. D-HOOK-STAGES1 has no devtools stage yet.

## 5. The cross-cutting recipe

### 5.1 The recipe

Every layer that is open today, or open by law, uses the same five parts. Writing them down once lets a reviewer check a new domain against them.

1. **An interface in Jet.** A trait, or a declaration kind from D-META-REG1 (marker, fact, effect, diagnostic, unit family). Core declares its own members with it.
2. **Providers.** Core's members and package members are instances of the interface. The compiler reads instances from the checked program, never by name.
3. **A conformance kit.** Law tests live inside the trait (D-CORE-LAWS1). `jet test` runs them against every implementation. `#Waive(law, "reason")` skips one; it is a gate kind (D-GATE-LAW1), so audits show it and policy may forbid it.
4. **Declared effects and declared diagnostics.** A member says what it touches (effects, grants) and how it refuses (a declared diagnostic printed as `package.Name`, per D-MARKER-DIAG1).
5. **Registry-driven tooling.** The editor, `jet inspect`, docs and `jet fix` read members through compiler queries (D-ONE-TOOLS1). No tool keeps its own list.

### 5.2 Evidence that the recipe scales

- Prettier: "Prettier's own implementations of all languages are expressed using the plugin API" [S46].
- Kubernetes: custom resources extend the API, and "many core Kubernetes functions are now built using custom resources, making Kubernetes more modular" [S35].
- Bevy: "All Bevy engine features are implemented as plugins … This includes internal features like the renderer" [S43].
- Go: `testing/fstest.TestFS` is a conformance kit for any `fs.FS` [S9]; embedded-hal is a trait set that every board implements [S18].
- Elixir: protocol consolidation turns an open set into a fixed dispatch table at build time [S2].

The common thread is that the platform owner pays the same price as an outside author. That keeps the extension interface honest, because the owner feels every gap in it.

Other systems show single parts of the recipe:
- Lua was built as an extension language from the start; its authors describe that history in HOPL III [S28].
- Emacs hooks are variables that hold functions to call on a given occasion [S34]. Jet's `#On` points (D-HOOK-ONE1) are the typed version.
- pytest plugins are sets of hook functions, and pytest's own features use the same hooks [S45] `[INFERENCE]` from the plugin guide.
- Kotlin replaced annotation processing through kapt with KSP, an API over the compiler's symbols [S37]. Jet's compiler stages (D-HOOK-STAGES1) aim at the same layer.
- Julia's multiple dispatch lets any package add methods to any function [S56]. Its cost appears in 5.3.
- Elixir macros receive and return quoted code at compile time [S57]. Jet's markers do the same under a sandbox.
- uom checks units at compile time as a plain Rust library; F# builds units of measure into the language [S62]. Jet's `#UnitFamily` follows the library route.

### 5.3 Evidence of where open extension went wrong

- **Global run-time registries.** Go's `sql.Register`, `crypto.RegisterHash` and Python's codec registry are global tables filled at start-up [S40, S41, S51]. Order of imports changes behaviour, and unused members still load `[INFERENCE]`.
- **Open methods without consolidation.** Julia's invalidations: loading one package recompiled thousands of method instances [S1].
- **Compiler plug-ins with full access.** Rust procedural macros "have the same resources that the compiler has", including standard input and output [S25].
  When `serde_derive` shipped a precompiled binary, Fedora's packager reported it could not be shipped under distribution rules (decathorpe, serde issue #2538, 2023-07-27 [S23]). dtolnay's `watt` runs procedural macros as WebAssembly instead [S24].
- **Implicit resolution.** Scala 3's reference describes Scala 2 implicits as overused, with "inscrutable type errors that go away with the right import incantation" [S22]. Scala 3 replaced them with explicit `given` and `using`.

Jet's design must avoid all four: no global run-time tables, consolidation per build, compile-time code in a sandbox with grants, and explicit imports for every member.

## 6. The design: open declarations, a closed kernel, and a guard

This section is the long form of ballot `D-EXTEND-ARCH1`. It extends D-OPENTABLE1 and D-META-REG1. It does not replace any ratified decision.

### 6.1 The rules

1. **One home in Jet.** Every domain list is declared in Jet with a form from section 2: a trait with impls, a declaration row, or a hook attachment. Core declares its members with the same form a package uses.
2. **No library names in the compiler.** Compiler and tool code read properties of declarations. They never branch on the name of a Core type, function, format, effect root or target.
   Core's rows may be generated into compiler tables as a cache (D-OPENTABLE1). Package rows join them per program, read from each package's interface record.
3. **Laws travel with the interface.** An open trait carries its law tests. `jet test` runs them against every package impl (D-CORE-LAWS1 keeps them out of builds), and a failure names the law.
4. **Members are named by package.** A package member is imported like any name. Two members with one name are told apart by package, and the root package may alias one.
5. **The kernel is a ledger.** The lists that stay closed are listed in one ledger, each with a reason class and an expert way out. D-SURFACE-BUDGET1 counts them.
6. **A guard keeps it that way.** A CI check runs the census on every change (section 8).

### 6.2 What a package can add, by layer

| layer | form | example member |
|---|---|---|
| text heads | `impl T.CheckedText` (+ companions) | `Cypher{"…"}` |
| units and suffixes | `#UnitFamily` | `hop`, `kib` |
| effects | `effect Root.Leaf`; roots per the effects ballots | `DB.Graph` |
| diagnostics and lints | `diagnostic` + `#On(compiler.checked)` | `routes.BadPath` |
| formats | a codec trait; `#Rename` reads it | `msgpack` |
| providers and services | provider traits (D-CORE-PROVIDER1) | a clock, an allocator, a scheduler |
| hardware | hardware kinds (D-HOST-CAPS1), instruction entries (D-CORE-BOUNDARY1, Core and admitted providers only) | an NPU |
| foreign code | a binder trait | a Kotlin binder |
| build | `b.toolchain`, `b.action` | a CMake toolchain |
| editor | `#On(lsp.…)` stages | completion for a marker |
| formatter | `#On(fmt.block)` | a SQL block formatter |
| debugger | a `DebugView` trait | a ring buffer view |
| docs | `#On(doc.page)` | a schema diagram |
| taint | declared taint kinds and sinks | `PHI` |
| package sources | a Jetpack provider trait | a new registry |
| migrations | `#Deprecated(…)` with a replacement body | inline-on-fix |

### 6.3 Worked example: the checked-text row

The ruling on #6208 (C1) needs `CheckedText` to cover SQL, HTML, Sh, URL, Path and DateTime as ordinary implementations. This amends D-TEXTHEAD-TYPE1.
The trait today is `{ type Error; check(text); encode_hole(value) -> String }` (`Prelude/core/prelude.jet:19-23`).

Three needs are missing. SQL holes must become bound `DBValue` parameters, not text. HTML, Sh, URL and Path need a carrier that is not a plain string. DateTime takes no holes.

A carrier hook that takes the finished text, such as `build(text: String)`, is wrong for two heads. In `Sh`, each hole must stay one argv item (`Docs/spec/spec.md:82-84`); flat text loses the boundaries.
In HTML, the right escape depends on where the hole sits: text, attribute, URL or script. Go's `html/template` escapes by that place [S63]. `encode_hole(value)` sees no place.

Today's Core constructors already have the right shape. `__sql_literal(literals, values: [DBValue])` (`Core/Db/Db.jet:259`), `__sh_literal(literals, holes)` (`Core/Process/Process.jet:64`) and `__html_literal(literals, holes, trusted)` (`Core/Text/Html.jet:110`) each take the literal parts and the holes.
The compiler picks the constructor by name (`Facts/Checked.jet:133-141`) and converts SQL hole types by name (`Sema/Expressions/Constructors.jet:1632-1656`). Both are closed lists.

Probes at `ec16c32d35` set three limits on the spelling:
- associated-type defaults do not parse (`type Part = String` in a trait gives E0003);
- `-> Self` in a static trait method is refused (E0108);
- default method bodies parse and run.

So the design keeps the beginner's three items and adds two things:

- `fn holes_allowed() -[]> Bool { return true }` on `CheckedText`, with a default body. DateTime returns `false`.
- A companion trait for experts:

```jet
trait TextBuilder {
    type Hole
    type Carrier
    fn hole<T: Printable>(value: T) -[]> Hole
    fn build(literals: [String], holes: [Hole]) -[]> Carrier
}
```

- SQL sets `Hole = DBValue`. Its `hole` uses the ratified type table `if T == { Int -> DBValue.Int(value) … }` (D-TYPESWITCH1), replacing the compiler's switch. Holes never enter the text.
- Sh sets `Hole = String`. `build` splits literal words and pushes each hole as one argv item.
- HTML's `build` scans the literal parts, so it knows each hole's place and escapes for it.
- URL and Path encode each hole as one component. DateTime has no holes and builds its record.

A probe program with stand-in names runs at `ec16c32d35`. It declares the trait above and three heads: SQL-like, Sh-like and HTML-like.
It prints `3` for the argv of `grep -n {file}` where `file` is `my notes.txt`. It prints `SELECT * FROM t WHERE id = ?` for SQL, and `<a title="a&quot;b">&lt;b></a>` for an attribute hole and a text hole.
Adding the SQL type table makes the same program stop with E0107, because D-TYPESWITCH1 is ratified but not built.

The check rule, per the ruling on #6218:
- Constant text is checked at compile time.
- A head without a builder whose holes hold run-time values checks the complete text at run time. A rejection stops the program with the check error.
- A head with a builder checks its template at compile time, with each hole written as `encode_hole` gives it. Its holes then go through `hole` and `build`, which stop the program if they reject a value.

After this, `TypedHeadKind` has no reason to exist. Core's six heads become impls in Core, and the compiler reads them like any package's.

**Cost against goal G4 (self-compile in about 50 ms).** The compiler imports no third-party package, so it reads no package rows. Core rows stay pre-generated, as today.
The compiler has 4 checked-text literals in 585,740 lines of `.jet` source. Each becomes one impl lookup instead of a chain of name comparisons. So the self-compile cost does not grow.
A program that imports packages pays one table insert per declared row in their interface records, once per build.

### 6.4 Costs of the design

| cost | size | why |
|---|---|---|
| run time when unused | zero | members are compile-time declarations; unused impls are not emitted; hook points with no attachment emit nothing (D-HOOK-ONE1) |
| run time when used | same as Core today | a trait call on a known type is static; Core's own members take the same path |
| compile time, Core only | unchanged | Core rows stay generated into the compiler as a cache |
| compile time, with packages | grows with rows in imported packages | rows are indexed once per package build in its interface record |
| tooling | one query per vocabulary | the editor and `jet inspect` read members through compiler queries |
| readability | unchanged for beginners | a package member is spelled like a Core member |
| I1 | unchanged | members run in safe Jet; unsafe needs `#Unsafe` |
| I4 | met | package refusals use declared diagnostics |
| I8 | improved | one form per vocabulary replaces duplicate lists |
| I9 | met | members are Jet source, run by the shared runtime |

### 6.5 The alternatives the ballot weighs

**B, compiler extension components.** Each open vocabulary gets a component interface, loaded by the build with `plugin.load<I>(path, authority, sha256)` (D-CORE-PLUGINPATH1). This is the Terraform provider model [S19] and close to Roslyn analyzers [S4].
Its real strength is reach: a component can be written in any language and can touch grammar, which Jet code cannot.
Its cost is a sandbox call at each use and members that tools cannot see without running the component. It is also a second extension system beside traits and markers, against I8.

**C, one reviewed list per vocabulary, in Core.** D-OPENTABLE1 completed: every domain list gets one declared home in Core, tables are generated from it, and the same guard blocks copies.
Its real strength is review. Each vocabulary stays one list that the Core owners read, with no name clashes, no coherence questions and exhaustive matches. It is also the cheapest to compile.
Its cost is that a third party waits for a Core release, or forks Core. For a database, shell or file format that is the gap this research set out to close.

A is recommended because it keeps C's single form and guard, and adds package rows read once per build. The ballot's `designAway` shows how each loss of A was removed.

## 7. Where the recipe fails, and what replaces it

| area | why the recipe fails | what replaces it | expert way out | cost of the way out |
|---|---|---|---|---|
| tokens and keywords | every tool must parse a file without running package code | block markers and text heads | `#Name { text }` with a package checker | braces form only; no new operators |
| soundness rules | ownership, effect checking and inference must hold for all code | kernel, closed | `#Unsafe("reason")` | audited block; policy may deny |
| IR ops and code generators | I9 and speed need one proven meaning | instruction entries with plain-body tests (D-CORE-BOUNDARY1) | FFI or inline assembly in `#Unsafe` | audited; not portable |
| output kinds, ABI, layout | every package must agree | kernel, versioned | `System`/`Image` outputs plus build actions | more build code |
| policy keys, lockfile format | must mean the same in every project | kernel, versioned schema | project-level rules in Jet build code | not portable between projects |
| run-time registries | global tables break consolidation | compile-time traits and rows | `point.on(scope, f)` events | explicit run-time cost |
| two impls for one foreign type | coherence | orphan rule | a distinct wrapper type | one wrapper type |
| dependency compile-time code | trust | sandbox plus grants (D-META-OPTIN2, D-META-HOLD1) | `Meta.Code` hold in `package.jet` | one authority line; dependents see a trust prompt |
| hook order | merge conflicts | declared merge identities (D-HOOK-STAGES1) | `priority:` on `#On` | visible in `jet inspect hooks` |

The kernel ledger lists every closed list with one of these reasons. Today that is 894 findings, of which 598 are function-local state machines that need no review.

## 8. The "no closed lists" guard

**Tool.** The census script, checked in under `Tools/`, run by the existing guard harness. It takes 4.2 s on the full `Compiler/` and `Core/` tree and its output is byte-stable (sha256 `5d72eb6d…77b2b` on two runs).

**Inputs.** `Compiler/`, `Core/` and `Jetpack/`.

**Ledger files.** A kernel ledger (one row per kept-closed list: file, name, reason class, way out) and a migration baseline (one row per domain list not yet moved). Rows carry no decision IDs, per I7.

**Rules.**
1. A new domain finding fails, unless it is generated from a declared twin and says so.
2. A baseline row may be removed and never added. The baseline count is printed on every run, like D-SURFACE-BUDGET1's library counts.
3. A new kernel finding needs a ledger row with a reason class: grammar, soundness, ir, kernel, protocol or fixed-standard. Function-local state machines pass without a row.
4. Two lists in different files whose members overlap by 80% or more fail as a duplicate. This catches the drift behind DEFECTs 1, 2, 4, 6, 11 and 12.
5. Findings are keyed by file and name, not line number, so edits do not churn the ledger.
6. The failure message names the declaration form to use instead, from the table in 6.2.

**Cost.** One more guard test of about 4 s. No effect on compiled programs.

## 9. Order of work

1. Ratify `D-EXTEND-ARCH1`. It is one package: the six rules, the checked-text row, and the guard.
2. Land the guard with today's baseline, so nothing new is added while the old lists move.
3. Fix the drift defects (#6206–#6217) by giving each list one declared home. Each fix shrinks the baseline.
4. Build the ratified forms that have no code yet, in this order: package diagnostics (D-MARKER-DIAG1), `#On` and the compiler stages, providers, the checked-text row.
5. Move the sema and IR name tables (4.12, 4.13) to generated twins. This is the largest step: 314 sema findings and 109 IR findings.
6. Open the remaining vocabularies in section 6.2, one design card each.

## 10. Sources

Read on 2026-10-29 unless a date is given. URLs were checked for HTTP 200 on that date.

- [S1] Tim Holy, Jeff Bezanson, Jameson Nash. "Analyzing sources of compiler latency in Julia: method invalidations." 2020-08-26. https://julialang.org/blog/2020/08/invalidations/
- [S2] Elixir team. "Protocol" (Elixir v1.20.4 docs). https://hexdocs.pm/elixir/Protocol.html
- [S3] The Rust Release Team. "Announcing Rust 1.78.0." 2024-05-02. https://blog.rust-lang.org/2024/05/02/Rust-1.78.0/ ; Rust Reference, "Diagnostics attributes". https://doc.rust-lang.org/reference/attributes/diagnostics.html
- [S4] dotnet/roslyn. "Incremental Generators." https://github.com/dotnet/roslyn/blob/main/docs/features/incremental-generators.md ; Microsoft Learn, "Source Generators". https://learn.microsoft.com/en-us/dotnet/csharp/roslyn-sdk/source-generators-overview
- [S5] Doug Gregor. SE-0382 "Expression Macros", implemented in Swift 5.9. https://github.com/swiftlang/swift-evolution/blob/main/proposals/0382-expression-macros.md ; The Swift Programming Language, "Macros". https://docs.swift.org/swift-book/documentation/the-swift-programming-language/macros/
- [S6] Jordan Rose. SE-0192 "Handling Future Enum Cases", implemented in Swift 5.0. https://github.com/swiftlang/swift-evolution/blob/main/proposals/0192-non-exhaustive-enums.md
- [S7] Rust RFC 2008 "non_exhaustive", start date 2017-05-24. https://rust-lang.github.io/rfcs/2008-non-exhaustive.html
- [S8] Rust RFC 3191 "debugger_visualizer", start date 2021-11-01. https://github.com/rust-lang/rfcs/blob/master/text/3191-debugger-visualizer.md
- [S9] The Go Authors. `testing/fstest/testfs.go`, copyright 2020. https://github.com/golang/go/blob/master/src/testing/fstest/testfs.go
- [S10] Philip Wadler. "The Expression Problem." Email, 1998-11-12. https://homepages.inf.ed.ac.uk/wadler/papers/expression/expression.txt
- [S11] Chris Lattner, Mehdi Amini, Uday Bondhugula, Albert Cohen, Andy Davis, Jacques Pienaar, River Riddle, Tatiana Shpeisman, Nicolas Vasilache, Oleksandr Zinenko. "MLIR: A Compiler Infrastructure for the End of Moore's Law." arXiv:2002.11054, 2020-02-25. https://arxiv.org/abs/2002.11054
- [S12] Sebastian Ullrich, Leonardo de Moura. "Beyond Notations: Hygienic Macro Expansion for Theorem Proving Languages." arXiv:2001.10490, 2020-01-28, rev. 2022-04-12. https://arxiv.org/abs/2001.10490
- [S13] Sam Tobin-Hochstadt, Vincent St-Amour, Ryan Culpepper, Matthew Flatt, Matthias Felleisen. "Languages as Libraries." PLDI 2011. https://www2.ccs.neu.edu/racket/pubs/pldi11-thacff.pdf
- [S14] Racket Guide, ch. 17 "Creating Languages." https://docs.racket-lang.org/guide/languages.html
- [S15] OpenXLA project. "PJRT – Uniform Device API." https://openxla.org/xla/pjrt
- [S16] Alan Donovan. "//go:fix inline and the source-level inliner." The Go Blog, 2026-03-10. https://go.dev/blog/inliner
- [S17] Alan Donovan. "Using go fix to modernize Go code." The Go Blog, 2026-02-17. https://go.dev/blog/gofix
- [S18] Rust Embedded Working Group. "embedded-hal v1.0 now released!" 2024-01-09. https://blog.rust-embedded.org/embedded-hal-v1/
- [S19] HashiCorp. "Terraform plugin protocol." https://developer.hashicorp.com/terraform/plugin/terraform-plugin-protocol
- [S20] Microsoft. VS Code "Activation Events." https://code.visualstudio.com/api/references/activation-events
- [S21] Microsoft. VS Code "Contribution Points." https://code.visualstudio.com/api/references/contribution-points
- [S22] LAMP/EPFL. Scala 3 Reference, "Contextual Abstractions." https://docs.scala-lang.org/scala3/reference/contextual/index.html
- [S23] decathorpe. "using serde_derive without precompiled binary." serde issue #2538, 2023-07-27. https://github.com/serde-rs/serde/issues/2538
- [S24] David Tolnay. "watt: Runtime for executing procedural macros as WebAssembly." https://github.com/dtolnay/watt
- [S25] Rust Reference, "Procedural macros." https://doc.rust-lang.org/reference/procedural-macros.html
- [S26] Rust Edition Guide, "What are editions?" https://doc.rust-lang.org/edition-guide/editions/index.html
- [S27] GHC User's Guide, ch. 7 "Extending and using GHC as a Library" (compiler and typechecker plugins). https://downloads.haskell.org/ghc/latest/docs/users_guide/extending_ghc.html
- [S28] Roberto Ierusalimschy, Luiz Henrique de Figueiredo, Waldemar Celes. "The Evolution of Lua." HOPL III, 2007. https://www.lua.org/doc/hopl.pdf ; and "The evolution of an extension language: a history of Lua", SBLP 2001. https://www.lua.org/history.html
- [S29] LLVM Project. "Writing an LLVM Pass" (new pass manager). https://llvm.org/docs/WritingAnLLVMNewPMPass.html
- [S30] MLIR. "Defining Dialects." https://mlir.llvm.org/docs/DefiningDialects/
- [S31] Microsoft. Language Server Protocol 3.17 specification. https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
- [S32] Tree-sitter. "Syntax Highlighting" (injection queries). https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html
- [S33] NixOS Manual, "Writing NixOS Modules." https://nixos.org/manual/nixos/stable/#sec-writing-modules
- [S34] GNU Emacs Lisp Reference Manual, "Hooks" and "Advising Functions." https://www.gnu.org/software/emacs/manual/html_node/elisp/Hooks.html
- [S35] Kubernetes. "Custom Resources" and "Operator pattern." https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/custom-resources/
- [S36] Bazel. "Rules", "Toolchains", "Module extensions." https://bazel.build/extending/rules ; https://bazel.build/extending/toolchains ; https://bazel.build/external/extension
- [S37] JetBrains. "Kotlin Symbol Processing API." https://kotlinlang.org/docs/ksp-overview.html
- [S38] Go team. `golang.org/x/tools/go/analysis` (Analyzer, Fact). https://pkg.go.dev/golang.org/x/tools/go/analysis
- [S39] Serde. "Serde data model" (29 types). https://serde.rs/data-model.html
- [S40] Go standard library, `database/sql` (`Register`). https://pkg.go.dev/database/sql
- [S41] Go standard library, `crypto` (`RegisterHash`). https://pkg.go.dev/crypto
- [S42] Oracle. "Java Cryptography Architecture (JCA) Reference Guide," Java SE 21. https://docs.oracle.com/en/java/javase/21/security/java-cryptography-architecture-jca-reference-guide.html
- [S43] Bevy. "Plugins." https://bevy.org/learn/quick-start/getting-started/plugins/
- [S44] Tweag. "Topiary." https://topiary.tweag.io/
- [S45] pytest. "Writing plugins." https://docs.pytest.org/en/stable/how-to/writing_plugins.html
- [S46] Prettier. "Plugins." https://prettier.io/docs/plugins
- [S47] Microsoft Learn. "Type Providers" (F#). https://learn.microsoft.com/en-us/dotnet/fsharp/tutorials/type-providers/
- [S48] Daan Leijen et al. "The Koka Programming Language." https://koka-lang.github.io/koka/doc/book.html
- [S49] Bytecode Alliance. "The WebAssembly Component Model" and WIT. https://component-model.bytecodealliance.org/ ; https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md
- [S50] GDB manual, "Pretty Printing." https://sourceware.org/gdb/current/onlinedocs/gdb.html/Pretty-Printing.html ; LLDB, "Variable Formatting." https://lldb.llvm.org/use/variable.html
- [S51] Python. "codecs — Codec registry and base classes." https://docs.python.org/3/library/codecs.html
- [S52] The rustc book, "Custom Targets." https://doc.rust-lang.org/rustc/targets/custom.html
- [S53] Trail of Bits. "Dylint: Run Rust lints from dynamic libraries." https://github.com/trailofbits/dylint
- [S54] ESLint. "Create Plugins." https://eslint.org/docs/latest/extend/plugins
- [S55] Unison. "The big idea." https://www.unison-lang.org/docs/the-big-idea/
- [S56] Julia manual, "Methods." https://docs.julialang.org/en/v1/manual/methods/
- [S57] Elixir. "Macros" guide. https://hexdocs.pm/elixir/macros.html
- [S58] Bryan Cantrill, Michael Shapiro, Adam Leventhal. "Dynamic Instrumentation of Production Systems." USENIX 2004. https://www.usenix.org/legacy/event/usenix04/tech/general/full_papers/cantrill/cantrill.pdf
- [S59] Perfetto. "Track events." https://perfetto.dev/docs/instrumentation/track-events ; Bartosz Taudul, "Tracy." https://github.com/wolfpld/tracy
- [S60] Zig Software Foundation. Zig language reference (comptime). https://ziglang.org/documentation/master/
- [S61] Rust RFC 2451 "Re-rebalancing coherence." https://rust-lang.github.io/rfcs/2451-re-rebalancing-coherence.html
- [S62] uom crate (units of measurement). https://docs.rs/uom/latest/uom/ ; Microsoft Learn, F# "Units of Measure." https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/units-of-measure
- [S63] The Go Authors. `html/template` package documentation: "The escaping is contextual, so actions can appear within JavaScript, CSS, and URI contexts." https://pkg.go.dev/html/template
