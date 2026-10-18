# Jai parity audit: how Jai works and every Jai feature mapped to Jet (2026-10-16)

## Verdict

Jet does not yet match Jai's power, but the gap is now fully mapped. All 215 rows have an exact owner record: shipped code, a ratified decision with a card, or one of twelve open ballots (D-JAI-*).

Owner direction (2026-10-16): Jet must be at least as powerful and flexible as Jai, with every Jai feature supported explicitly. Safety and auditability stay the default, experts can switch guardrails off, and the Jai author's regrets are designed out.

The cohesive answer has three parts:

- **Safe by default.** Everything Jet can check stays available to every program without a switch: compile-time execution, typed code values and hygienic macros (pending D-JAI-CODE1), hooks, reflection and build control.
- **Jai by opt-in.** Each guardrail Jai lacks becomes a named power, with a reason, in the main package's `policy: .{ powers: [...] }` list on the existing policy ladder (D-JAI-EXPERT1). Examples are full host access while compiling, unlimited steps, build-wide check removal, text-to-code, duck-typed generics and the advanced compiler API.
- **Visible.** Every expert power used, its reason and every site using it appear in the build receipt and in `jet inspect powers`.

This report merges the 2026-10-15 mining report (formerly `Docs/research/mine-for-jet-2026-10-15-jai.md`) into the 2026-10-16 parity audit. The mining report's reconstruction of Jai and its sources are kept below as the evidence base ([How Jai works](#how-jai-works-reconstructed-from-public-sources), [Scope, sources and coverage](#scope-sources-and-coverage)). It called Jet "stronger but less powerful" and judged some Jai powers refused; this audit replaces that framing and puts each refusal to the owner as a ballot instead.

**How to read the tables.** Each row gives the Jai feature, Jet's counterpart today, a state, and the record that closes it. State values:

| State | Meaning |
|---|---|
| shipped | code and an executable example or test exist |
| groundwork | code exists but is incomplete |
| ratified | an owner decision exists; implementation is owed on the named card |
| ballot | an open ballot of this run decides it (listed below) |
| card | approved work is tracked on the card |
| by design | Jet reaches the same result through a different mechanism, and no power is lost |
| not proposed | deliberately not proposed, with the reason given; the owner may add it |

Evidence for each Jet claim comes from six read-only census lanes over `crates/`, `Compiler/`, `Core/`, `Examples/features/`, `tests/ui` and `Docs/spec/` (2026-10-16), plus Tower decisions. Jai evidence locators are in [How Jai works](#how-jai-works-reconstructed-from-public-sources).

## Ballots to vote, in order

| Order | Ballot | Card | Decides |
|---|---|---|---|
| 1 | D-JAI-EXPERT1 | #5022 | One named-powers list for switching guardrails off; amends I1 for build-wide check removal |
| 2 | D-JAI-CODE1 | #5023 | Code values, `insert`, `#Expand` macros, explicit caller access; lifts the universal macro ban |
| 3 | D-JAI-GENERIC1 | #5024 | `#Check prep { }` per-instance checks, `bake`, `prep?`, `#Open` duck-typed generics |
| 4 | D-JAI-EVENTS1 | #5009 | Missing hook stages (imported, settled, link, before_write, after_write, finished) and a read-only ordered stage view |
| 5 | D-JAI-SITE1 | #5010 | `$here`, `$caller` (SourceLocation), `.$known`, `.$callee`; resolves stale `#Caller()` |
| 6 | D-JAI-BUILDOPT1 | #5005 | Every Jai build option as typed fields and flags, `#Unchecked` per scope, pinning |
| 7 | D-JAI-CONFIG1 | #5006 | Targets that list package.jet profiles (Jai workspaces) and recorded `$host` facts |
| 8 | D-JAI-CONTEXT1 | #5025 | Readable context, `#ContextKey`, scratch memory, assertion handler, library boundary |
| 9 | D-JAI-DEFER1 | #5026 | General `defer`, replacing `scope.guard` |
| 10 | D-JAI-LAYOUT1 | #5027 | `#Layout(c, packed)`, plain-data `#Layout(overlay)`, `Rel<T, I32>` through a checked `mem.Image` |
| 11 | D-JAI-SMALL1 | #5028 | Bit-backed `Set<Enum>`, `#Through`, `#As`, `#Specified` |
| 12 | D-JAI-COMPTIME1 | #5029 | Changeable globals initialized while compiling (Jai `#run` initializers) |

Parent card: #5021. Every ballot recommends option A, the full-power form. The ballots use today's `prep` and `$` spellings; the open ballots D-COMPTIME-WORD1 and D-META-HASH1 may respell them without changing any choice.

## The Jai author's regrets and how Jet designs them out

| Regret (source) | Jet design | Record |
|---|---|---|
| Out-of-order compilation is nondeterministic and costs about half the compiler effort (KHIeP_-Kp00 01:28:00; kHmbabcL-is 01:17:51) | Hook stages run in a fixed order with order-free merges and cache keys | D-HOOK-STAGES1, D-JAI-EVENTS1 |
| The backtick is clumsy (OAIqCpqszVw 00:34:34) | Macros list caller names in `#Expand(uses: [...])`; `uses: *` is the expert form | D-JAI-CODE1 |
| Macro errors lack the call site; no macro debug info (6qkL_DmHOuo 00:35:04; QX46eLqq1ps 00:59:00) | Three-location errors, `jet inspect expand`, debugger steps into macros | D-JAI-CODE1 |
| Notes are untyped strings (LPdMNsL5MgI 00:46:04) | Typed markers with bodies (shipped) | D-MARKER-LAW1 |
| Context breaks across DLLs; context member names clash (pg_Wk2GBZDw 00:21:45; SSVHWrYG974 00:40:00) | Library root context at C boundaries; `#ContextKey` names resolve through imports | D-JAI-CONTEXT1 |
| Compile-time code may need host or target versions of per-OS code (jjCfAg_gtDw 00:43:50) | `$host` facts, recorded as build inputs and keyed by host when read | D-JAI-CONFIG1 |
| Deferred undeclared identifiers make errors poor (jjCfAg_gtDw 01:28:04) | Undeclared names are errors; `compiler.generate` publishes generated names before checking | D-META-PUBLISH2 |
| No limit on compile-time loops (b5ATngfKQP4 00:37:25) | Step budget by default; `unlimited_steps` power | D-PREP-FUEL1, D-JAI-EXPERT1 |
| Interception silently filtered declarations (KHIeP_-Kp00 00:33:00) | Each hook receives its whole stage payload; nothing is filtered silently | D-JAI-EVENTS1 |
| Polymorphs sent only when used (jjCfAg_gtDw 01:18:09) | Generic declarations delivered once; instances listed at settled | D-JAI-EVENTS1 |
| Generic duck typing gives worse errors (BqF2SbY99B8 01:54:55) | Definition-checked generics by default; `#Open` is an expert power | D-JAI-GENERIC1 |
| `#modify` errors silent during overload choice (COQKyOCAxOQ 00:47:30) | Failing `#Check prep` blocks are listed in the no-match error | D-JAI-GENERIC1 |
| Two bake directives for one idea (_tLk-JNGD60 00:12:41) | One `bake(f, ...)` | D-JAI-GENERIC1 |
| Generated code errors land in an opaque generated file (BKvQXmEmDjU 01:51:17) | Generated items carry provenance; `.jet/generated/` mirrors them | D-JAI-EVENTS1, shipped |
| `#caller_location` not constant inside `#run` (pgiVrhsGkKY 00:40:09) | `$caller` is a compile-time constant wherever legal | D-JAI-SITE1 |
| Compile-time debugging only by print (TwqXTf7VfZk 03:17:40) | Debugging compile-time code | #5008 |
| Few built-in warnings; project rules belong to metaprograms (mGe5d6dPHAU 00:04:27) | Hook findings with lint policy and deny (E1293) | D-HOOK-STAGES1 |
| Removed checks and unpinned build code are invisible in Jai builds | Build receipt and banner list every power used, with its reason | D-JAI-EXPERT1 |
| Unrestricted, unrecorded compile-time effects: windows, servers, DLLs (UTqZNujQOlA 00:44:04–00:53:31) | Tier 1 pure and locked by default; Tier 2 behind `#Impure("reason")` and a gate; full host access only as the named `comptime_host` power | D-BUILD-IO1, D-JAI-EXPERT1 |

## Where Jet is ahead by default

1. **Determinism by construction.** Jai's metaprogram sees a mutable, order-dependent stream: it was nondeterministic across runs, and out-of-order compilation is "about half the compiler effort". Jet's hooks are pure stage functions with order-free merges and cache keys (D-HOOK-STAGES1). Jai cannot adopt this without dropping its pull-loop API. *Ratified, not built.*
2. **Checked generation.** Jai splices strings and resubmits bodies with "not carefully checked" input. Jet republishes typed `Generated` items, or change sets that recheck every affected caller atomically (D-META-PUBLISH2, D-META-CONTROL2). *Ratified; registry rows only.*
3. **Bounded, recorded compile-time execution.** Jai has no step budget, and compile-time code can open windows, talk to servers and load DLLs with nothing recorded. Jet has a step budget in the cache key (D-PREP-FUEL1). It also keeps compile-time effects pure and locked by default, behind an explicit gate otherwise (`metaprogramming.md:83-129`). *Shipped.*
4. **Typed everything the metaprogram reads.** Jai notes are strings, its arguments are raw strings, and its API is learned by trial. Jet has typed markers, typed settings, and a registry-published metadata contract (#3520). *Partly shipped.*
5. **Hot swap.** Jai never shipped code hot reload; Jet's `jet dev --swap` exists. *Shipped.*

## For agent-driven development

- **Verdict fidelity.** Jai pushes most project rules into user metaprograms and keeps built-in warnings rare. Jet's sema-owned checks (I3) plus hook findings put both in one diagnostics path with registered codes (I4).
- **Repair determinism.** One mechanism per meaning (I8) beats Jai's overlapping forms:
  - `#insert` string versus `Code` versus `,scope()`;
  - `#bake_constants` versus `#bake_arguments` versus `$` versus `$$`;
  - `#modify` versus `$T/Constraint`.

  Each Jai overlap is a second valid fix an agent must choose between.
- **Context economy.** Jai's message loop forces every metaprogram to write the same filter loop (workspace check, kind switch, phase switch). A typed `#On(stage)` hook is a few lines.

## Corrections found during the census

1. **D-META-CODE1 drift.** The owner ratified both options of D-META-CODE1 ("Both option A and b. A is the simple case, b is the expert case"). Option B is first-class `code { }` values. The 2026-10-07 metaprogramming audit described B as declined, and `crates/jet-foundation/src/Syntax.rs:119-122` still says no code values exist. D-JAI-CODE1 builds on the ratified B.
2. **`#Context` defect (#5030).** `#Context(allocator:)` and `#Context(logger:)` parse and type-check, but lowering reads only `deadline` (`crates/jet-codegen/src/Codegen/TIR/tir_to_mir_stmt.rs:686-693`). `Examples/features/effects/smart_context.jet` therefore does not swap the allocator, and it has no golden output.
3. **Spec still states the older refusals.** `Docs/spec/syntax-decisions.md:5216-5225` and `Docs/spec/reference/metaprogramming.md:302-309` still reject message-loop mutation and user macros without the D-META-RECONCILE1=A amendment; #3520 criterion 19 owns the fix. D-JAI-CODE1 and D-JAI-EVENTS1 would amend these texts further.
4. **`#Caller()`** is named in `Docs/spec/syntax-decisions.md:2056` with no registry row or code (#5010, D-JAI-SITE1).
5. **No `#On` code.** The ratified hook stages (D-HOOK-STAGES1) have no lexer, parser or registry entries yet. The only shipped compiler hook is the `JET_COMPILER_EXTENSION` post-check plugin that D-HOOK-STAGES1 retires (#4760).
6. **No per-scope check removal.** `#Unsafe` keeps bounds checks, and D-OOBPROOF1 elides a check only when it is proved unneeded. Jai's `#no_abc` has no Jet form; D-JAI-BUILDOPT1 proposes `#Unchecked(...)` inside `#Unsafe`.
7. **D-HOOK-STAGES1 has no spec entry.** Tower records D-HOOK-STAGES1=A as ratified on 2026-10-07 (card #3520), but `Docs/spec/syntax-decisions.md` has no entry for it, and the 2026-10-07 metaprogramming report still calls it open. #3520 owns the spec text.
8. **`scope.guard` already runs arbitrary end-of-block work.** `core.mem.scope.guard` ships (`Examples/features/errors/scope_guard.jet`) beside `defer close(^x)`. D-JAI-DEFER1 option A retires it in favour of general `defer`, keeping one mechanism.
9. **Five Jai conveniences already had Jet forms.** `..Any` (D-ANY-JAI1), `xx` (D-SHAPE-CONVERT1), bucket arrays (`Pool<T>`/`Id<T>`), custom entry symbols (D-TARGET-SURFACE1) and C++ methods (D-FFI-CPP1) are covered; the first ballot draft re-proposed them.

## A. Compile-time execution

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| A01 | `#run f();` | `prep { … }`, `NAME :: prep { value }` (`Examples/features/comptime/prep_block.jet`) | shipped (Rust host; native build host missing) | #5003 |
| A02 | `X :: #run -> T { … }` block | `NAME :: prep { … }`; multi-statement value blocks are E0391 today | ratified (D-PREP-SURFACE2) | #3521 |
| A03 | `x := #run f();` in a procedure | value-form prep in function bodies | ratified (D-PREP-SURFACE2) | #3521 |
| A04 | `#run,stallable` | callees compile on demand (`Compiler/JetDriver/Source/Driver/Pipeline.jet:1302-1378`); preparation never observes unfinished code, so nothing must stall | by design | — |
| A05 | unrestricted compile-time effects (windows, processes, network) | Tier 2 `#Impure("reason")` + `--gate impure=allow`; unpinned network refused (E3412) | ballot | D-JAI-EXPERT1 (`comptime_host`) |
| A06 | foreign/DLL calls at compile time | none | ballot | D-JAI-EXPERT1 (`comptime_host`) |
| A07 | no step limit | 10,000,000-step budget, `#StepLimit(n)` | ballot | D-JAI-EXPERT1 (`unlimited_steps`) |
| A08 | `#compile_time` | `$phase` (registered; evaluation not found) | groundwork | #3521 |
| A09 | `is_constant(x)` | none | ballot | D-JAI-SITE1 (`.$known`) |
| A10 | `#no_reset` globals | changeable globals start from an integer, float or Bool literal | ballot | D-JAI-COMPTIME1 (compute the final value in prep; build code never mutates a run-time global) |
| A11 | compile-time heap data copied to static storage | `NAME :: prep { list }` values stored once | groundwork | #3521, D-JAI-COMPTIME1 (option B for changeable heap data) |
| A12 | `add_global_data(bytes, .READ_ONLY)` | `embed_bytes`, prep byte values; writable data via D-JAI-COMPTIME1 | shipped / ballot | D-JAI-COMPTIME1 |
| A13 | `#assert cond "msg"` | `prep { if !c -> panic("msg") }` stops the build (E0953) | shipped | — |
| A14 | `#if` | `prep if` (dropped arm still name-resolved; `Examples/features/comptime/prep_branch.jet`) | shipped, stricter | — |
| A15 | `ifx` / expression static if | `prep { if c -> a else -> b }` value form | ratified | #3521 |
| A16 | `#compiler` compile-time-only procedures | `prep fn`; `core.compiler` and `core.build` refuse runtime calls | shipped | — |
| A17 | `#dump` bytecode | none | card | #5000 |
| A18 | `-debugger` for compile-time code | `jet debug` covers runtime only | card | #5008 |
| A19 | host versus target at compile time | `$build` target facts only | ballot | D-JAI-CONFIG1 (`$host`), #5007 |
| A20 | `#run main()` (whole program while compiling) | advanced session `run_prepared` (D-META-CONTROL2) | ratified | #3526, D-JAI-EXPERT1 (`metaprogramming`) |
| A21 | compile-time string work | `prep { a + b }` | shipped | — |
| A22 | `jai f.jai -run "expr"` | `jet eval` | shipped | — |
| A23 | `jai f.jai -add "decl"` | none | ballot | D-JAI-BUILDOPT1 (`--add`) |
| A24 | per-callee compile-time checks (2014 "check call") | `#On(compiler.declared)` hooks; per-instance `#Check prep` | ratified / ballot | #4760, D-JAI-GENERIC1 |

## B. Code values, insertion, macros, specialization

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| B01 | `#code`, `Code` | `code { }` ratified as the expert case of D-META-CODE1, unbuilt | ratified / ballot | D-JAI-CODE1 |
| B02 | `#insert code` | advanced call expansion (D-META-CONTROL2), spellings only | ballot | D-JAI-CODE1 (`insert`) |
| B03 | `#insert "text"`, `#insert -> string` | `s.code.parse_items` in advanced sessions | ratified / ballot | D-JAI-CODE1 (`parse`, `raw_code`) |
| B04 | `#insert,scope()` | none | ballot | D-JAI-CODE1 (`insert c with [...]`) |
| B05 | `#expand` macros | refused by the D-META-RECONCILE1 universal limit | ballot | D-JAI-CODE1 (`#Expand fn`) |
| B06 | backtick caller access | none | ballot | D-JAI-CODE1 (`uses:`, `defines:`, `exits`) |
| B07 | `#caller_code` | advanced capture (D-META-CONTROL2) | ballot | D-JAI-CODE1, D-JAI-SITE1 (`$caller.code`) |
| B08 | `compiler_get_nodes` | front-end read APIs (stubs) | ballot | D-JAI-CODE1 (`c.nodes()`) |
| B09 | code-to-code transforms | advanced change sets | ratified / ballot | #3526, D-JAI-CODE1 (`c.map`) |
| B10 | expansion limit (100) | none | ballot | D-JAI-CODE1 (`#ExpandLimit`) |
| B11 | macro arguments implicitly generic | none | ballot | D-JAI-CODE1 |
| B12 | `#run` once per expansion | prep inside derive templates runs per expansion | shipped / ballot | D-JAI-CODE1 |
| B13 | `for_expansion` | `Iterable` + `Iterator` (`Examples/features/collections/iter_hook.jet`) | shipped / ballot | D-JAI-CODE1 (trailing-block macros) |
| B14 | named break across nested loops | named loops, `next(rows)` (`Examples/features/basics/labeled_loops.jet`) | shipped | — |
| B15 | `#modify` | bounds checked at the definition | ballot | D-JAI-GENERIC1 (`#Check prep`) |
| B16 | duck-typed `$T` bodies | refused by D-GENERIC-TYPEFN1 | ballot | D-JAI-GENERIC1 (`#Open`, `open_generics`) |
| B17 | `$T/Vector`, `$T/interface Foo` | trait bounds | shipped | — |
| B18 | `#bake_constants` | `prep T` parameters (`<prep N: Int>`) | ratified / ballot | #3505, D-JAI-GENERIC1 (`bake`) |
| B19 | `#bake_arguments` | none | ballot | D-JAI-GENERIC1 (`bake`) |
| B20 | `$` / `$$` auto-bake | `prep` parameters require a known value | ballot | D-JAI-GENERIC1 (`prep?`) |
| B21 | body text (string bodies) | typed `derive` and `b.generate` templates | by design / ballot | D-JAI-CODE1 (`parse`) |
| B22 | `#placeholder` | `compiler.generate` publishes names before checking (D-META-PUBLISH2) | ratified | #3520 |
| B23 | `#string END … END` | `"""…"""`, raw backtick strings (`Examples/features/basics/multiline_string.jet`) | shipped | — |
| B24 | types as values | `prep T: Type`, `prep fn` recipes (D-GENERIC-TYPEFN1) | ratified | #4276 |
| B25 | structs with value and type parameters | `prep fn Vector(N: Int) -> Type`, `<prep N: Int>` | ratified | #3505 |
| B26 | `#if` per polymorphic instance | `prep if` in generic bodies | shipped | — |
| B27 | `#procedure_of_call` | none | ballot | D-JAI-SITE1 (`.$callee`) |
| B28 | `#this` | `Self` in types; the function's own name | by design | — |
| B29 | generated code written for debugging | `.jet/generated/`, `--emit-generated` | shipped | — |

## C. Locations, notes, reflection

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| C01-C04 | `#file`, `#line`, `#filepath`, `#location` | none | ballot | D-JAI-SITE1 (`$here`, `code.$location`) |
| C05 | `#caller_location` | `#Caller()` named in spec only | ballot | D-JAI-SITE1 (`$caller`), #5010 |
| C06 | `#procedure_name` | none | ballot | D-JAI-SITE1 (`$here.function`) |
| C07 | `@note` strings | typed markers (`marker` declarations, `#Name`) | shipped, stronger | — |
| C08 | `type_info(T)`, `Type_Info_*` | `T.reflect()`, `reflect.of(x)`, `T.$fields` (`Examples/features/comptime/reflect.jet`) | shipped | — |
| C09 | `Any`, `..Any` | trait-bounded `...T` varargs (D-ANY-JAI1); `reflect.of(x)` handle | shipped | — |
| C10 | `#type_info_none` and related | none | ballot | D-JAI-BUILDOPT1 (`type_info`); per-type marker not proposed |
| C11 | `runtime_storageless_type_info` | none | ballot | D-JAI-BUILDOPT1 (`type_info`) |
| C12 | type equality | Type values in prep (D-GENERIC-TYPEFN1) | ratified | #4276 |
| C13 | enum names and values at run time | enum reflection | shipped | — |
| C14 | member iteration to generate code | `prep loop field in T.$fields` (`Examples/features/reflection/derive_loop.jet`) | shipped | — |

## D. Compiler message loop, workspaces, plugins

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| D01 | metaprogram drives the compiler as a library | `fn build(b: BuildContext)`; advanced sessions | groundwork / ratified | #3526 |
| D02 | `compiler_create_workspace` | `compiler.advanced.session` (spelling only) | ballot | D-JAI-CONFIG1 (profiles per target), #3526 |
| D03 | per-workspace build options | one profile per command | ballot | D-JAI-CONFIG1 (`profiles:` on targets) |
| D04 | `add_build_file`, `add_build_string` | target sources; typed `b.generate` | shipped / ballot | D-JAI-CODE1 (`parse`) |
| D05 | intercept, wait for message | typed `#On(stage)` hooks (no code yet) | ratified / ballot | #4760, D-JAI-EVENTS1 (read-only `Session.stages()`) |
| D06 | `.FILE` | `compiler.parsed` | ratified | #4760 |
| D07 | `.IMPORT` | none | ballot | D-JAI-EVENTS1 (`compiler.imported`) |
| D08 | `.TYPECHECKED` | `compiler.declared` | ratified | #4760 |
| D09 | `ALL_SOURCE_CODE_PARSED` | `compiler.parsed` per file; declared runs after parsing; code added there goes in settled | by design / ballot | #4760, D-JAI-EVENTS1 |
| D10 | `TYPECHECKED_ALL_WE_CAN` | none | ballot | D-JAI-EVENTS1 (`compiler.settled`) |
| D11 | `ALL_TARGET_CODE_BUILT` | `compiler.emitted` | ratified | #4760 |
| D12-D13 | `PRE_WRITE_EXECUTABLE`, `POST_WRITE_EXECUTABLE` | none | ballot | D-JAI-EVENTS1 (`build.before_write`, `build.after_write`) |
| D14 | `READY_FOR_CUSTOM_LINK_COMMAND` | none | ballot | D-JAI-EVENTS1 (`build.link`) |
| D15 | `.DEBUG_DUMP` | internal compiler errors exit 101 (I2) | by design | D-JAI-EVENTS1 (crash report) |
| D16 | `.COMPLETE`, `.ERROR` | none | ballot | D-JAI-EVENTS1 (`compiler.finished`) |
| D17 | `compiler_report(msg, loc, .WARNING)` | hook findings (L1401); shipped only through the retiring extension | ratified | #4760 |
| D18 | `compiler_modify_procedure` | `ChangeSet.replace_body` | ratified | #3526 |
| D19 | collect, then generate at the end | none | ballot | D-JAI-EVENTS1 (`compiler.settled`) |
| D20 | house rules (MISRA, thread safety, pointer math) | hook findings + `policy.lints.deny` | ratified | #4760 |
| D21 | `Intercept_Flags` | flags filter messages only; a hook ignores what it does not need | by design | #4760 |
| D22 | `-plug Name`, `+Name args` | `JET_COMPILER_EXTENSION` (retiring) | ballot | D-JAI-EVENTS1 (hook packages listed in the main package.jet with D-JPK-BUILDSCRIPT1 grants) |
| D23 | plugin hooks (`before_intercept` … `shutdown`) | typed stages | ratified / ballot | #4760, D-JAI-EVENTS1 |
| D24 | bundled plugins (Check, Polymorph_Report, Program_Print, IProf, Autorun, Icon) | none | ballot | D-JAI-EVENTS1 (first-party hook packages) |
| D25 | replace the default metaprogram | `fn build`, `@build.jet` override | groundwork | D-BUILDENTRY1 |
| D26 | metaprogram arguments | typed `--set key=value` | shipped / ballot | D-JAI-BUILDOPT1 (`b.args`) |
| D27 | `compiler_set_command_line_arguments` | none | ballot | D-JAI-BUILDOPT1 (`b.args`) |
| D28 | compiler version gates | package toolchain requirement (E1208) | shipped | — |
| D29 | fail a workspace | build entry failure (D-BUILD-FAIL1) | ratified | — |
| D30 | `remap_import` | overlays (D-JPK-OVERLAY1), program allocator (D-ALLOC-PROGRAM1) | ratified | — |
| D31 | `run_command` from the build | `process.cmd` under effect grants | groundwork / ballot | D-JAI-EXPERT1 (`comptime_host`) |
| D32 | DLL workspace, then executable | `b.add_library` and `b.add_executable` in one plan | groundwork / ballot | D-JAI-CONFIG1 |
| D33 | translate the typed program (shaders, Scheme) | typed program payloads | ratified / ballot | #4760, D-JAI-CODE1 (`nodes()`) |
| D34 | IProf automatic instrumentation | `jet perf` sampling; advanced change sets | groundwork / ballot | D-JAI-EVENTS1 (profiler hook package) |
| D35 | Polymorph_Report, expression statistics | none | card | #5000 |
| D36 | Program_Print round trip | `jet inspect expand` (facts) | card | #5000 |
| D37 | `#!` scripts | `#!` at byte 0 (`Examples/features/cli/direct_launch.jet`) | shipped | — |

## E. Build options and command line

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| E01 | `output_type` (5 kinds) | package output kinds; no object or check-only output | ballot | D-JAI-BUILDOPT1 (`kind`) |
| E02 | output, intermediate path | `--output` (unported in JetCli) | ballot | D-JAI-BUILDOPT1 (`output_dir`, `work_dir`) |
| E03 | `entry_point_name` | `output.entry`; freestanding target profile facts (D-TARGET-SURFACE1) | ratified | — |
| E04 | `os_target` | `--target=web`, wasm targets | ballot | D-JAI-BUILDOPT1 (`os`) |
| E05 | `cpu_target`, machine options | none | ballot | D-JAI-BUILDOPT1 (`cpu`, `cpu_features`) |
| E06 | `minimum_os_version` | none | ballot | D-JAI-BUILDOPT1 (`min_os`) |
| E07 | `backend` | implied by profile (`Compiler/JetCli/Source/Cli/Front.jet:342-346`) | ballot | D-JAI-BUILDOPT1 (`backend`) |
| E08 | `Optimization_Type` (6 presets) | `optimize: none\|basic\|full`, `small` | groundwork / ballot | D-JAI-BUILDOPT1 (`.Size`, `.MinSize`) |
| E09 | bytecode inliner | `#Inline` per function | ballot | D-JAI-BUILDOPT1 (`inline`) |
| E10 | dead-code elimination policy | none | ballot | D-JAI-BUILDOPT1 (`dead_code`) |
| E11 | `maximum_polymorph_depth` | none | ballot | D-JAI-BUILDOPT1 (`max_instance_depth`) |
| E12-E15 | bounds, cast, math, null checks | always on; no options | ballot | D-JAI-BUILDOPT1, D-JAI-EXPERT1 (`unchecked_bounds`, `unchecked_null`, `unchecked_casts`) |
| E16 | overflow checks | `#Arithmetic(.Checked\|.Wrapping\|.Saturating)` per scope (`Examples/features/lowlevel/fixed_overflow_modes.jet`) | shipped / ballot | D-JAI-BUILDOPT1 (build default) |
| E17 | `#no_abc` per scope | none (`#Unsafe` keeps checks) | ballot | D-JAI-BUILDOPT1 (`#Unchecked`) |
| E18 | `emit_debug_info` | `debug_info: Bool` | groundwork / ballot | D-JAI-BUILDOPT1 (`debug_info`, `debug_format`) |
| E19-E20 | stack traces, backtrace on crash | debugger `backtrace` verb only | ballot | D-JAI-BUILDOPT1 |
| E21 | frame pointers, red zone | none | ballot | D-JAI-BUILDOPT1 |
| E22 | `llvm_options` | none | ballot | D-JAI-BUILDOPT1 (typed fields + `backend_args`) |
| E23 | `x64_options` | none | ballot | D-JAI-BUILDOPT1 (`backend_args`) |
| E24 | `temporary_storage_size` | none | ballot | D-JAI-BUILDOPT1 (`scratch_size`), D-JAI-CONTEXT1 |
| E25 | linker arguments | `-l`/`-L` for `jet cc` only | ballot | D-JAI-BUILDOPT1 (`link_args`, `libs`) |
| E26 | `use_custom_link_command` | none | ballot | D-JAI-EVENTS1 (`build.link`) |
| E27 | `import_path` | package identity resolution | by design | — |
| E28 | diagnostics format options | `--color`, `--quiet` | ballot | D-JAI-BUILDOPT1 (`diagnostics`, `short_paths`, `timings`) |
| E29 | natvis type names | none | ballot | D-JAI-BUILDOPT1 (`debugger_names`) |
| E30 | `lazy_foreign_function_lookups` | none | ballot | D-JAI-BUILDOPT1 (`lazy_foreign`) |
| E31 | `user_data_*` | typed settings (D-CONF-KEY1) | groundwork | — |
| E32 | `do_output = false` | register no outputs in `fn build` | by design | — |
| E33 | `write_added_strings` | `--emit-generated` | groundwork / ballot | D-JAI-BUILDOPT1 (`write_generated`) |
| E34 | `-context_size` | not needed: Jet sizes context statically | by design | — |
| E35 | per-function optimization hints | `#Inline`, `#Scalar` | not proposed | Jai has only planned it; revisit if Jai ships it |
| E36 | command-line flags (26) | `--release`, `--quiet`, `--version`, `--threads`, `--profile` exist; the rest map in D-JAI-BUILDOPT1's flag list | ballot | D-JAI-BUILDOPT1 |
| E37 | program options beat CLI flags | CLI always wins (D-BUILDCTX-FLAGS1) | ballot | D-JAI-BUILDOPT1 (`b.pin`) |
| E38 | module copy policy | path and vendored dependencies | by design | — |
| E39 | `#module_parameters` | typed package settings (D-CONF-KEY1) | groundwork | — |

## F. Context and runtime

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| F01 | implicit `context` | none; `#Context(...) { }` block (D-CTX1) | ballot | D-JAI-CONTEXT1 |
| F02 | `context.allocator` | `#Context(allocator:)` accepted but ignored at run time | groundwork / card / ballot | #5030, D-JAI-CONTEXT1 |
| F03 | logger in context | `#Context(logger:)` ignored; `core.log` shipped separately | groundwork / card / ballot | #5030, D-JAI-CONTEXT1 |
| F04 | temporary storage, marks, reset | none | ballot | D-JAI-CONTEXT1 (`context.scratch`) |
| F05 | assertion handler | none | ballot | D-JAI-CONTEXT1 (`on_assert`) |
| F06-F08 | stack trace, print style, thread index in context | none | ballot | D-JAI-CONTEXT1 |
| F09 | `#add_context` | none (#3502 frozen) | ballot | D-JAI-CONTEXT1 (`#ContextKey`) |
| F10 | `push_context`, `push_allocator` | `#Context(...) { }` lexical block | shipped (deadline) / ballot | D-JAI-CONTEXT1 |
| F11 | `#no_context`, `#c_call` | C-ABI functions take no context | by design | D-JAI-CONTEXT1 (library root context) |
| F12 | allocator protocol (one procedure with modes) | `core.mem` allocator families | by design | — |
| F13 | per-thread context and temporary storage | none | ballot | D-JAI-CONTEXT1 |

## G. Language features named in the sources

| # | Jai | Jet today | State | Closes it |
|---|---|---|---|---|
| G01 | `#as` | none | ballot | D-JAI-SMALL1 (`#As`) |
| G02 | `#align` | `#Layout(c, align(N))` | shipped | — |
| G03 | `#asm` | `#FFI(asm)` inside `#Unsafe` | groundwork | — |
| G04 | `#bytes` | `#FFI(asm)` raw bytes; `embed_bytes` | by design | — |
| G05 | `#char` | `'a'` character literals | shipped | — |
| G06 | `#complete` | exhaustive matches on closed enums (D-ENUM-EVOLUTION1) | shipped, stronger | — |
| G07 | `#cpp_method`, `#cpp_return_type_is_non_pod` | `cpp.*` binder: methods become methods, generated shims (D-FFI-CPP1) | ratified | — |
| G08 | `#deprecated` | `#Deprecated(since:, use:)` | shipped, stronger | — |
| G09 | `#foreign`, `#library`, `#system_library` | `use c.<lib>`, bindgen, `c@system` | shipped | — |
| G10 | `#import,dir/file/string` | `use` paths | shipped | — |
| G11 | `#load` | modules | not proposed | Jet modules split files with explicit names (I8 organizational flex) |
| G12 | `#intrinsic` | Core-internal intrinsics | not proposed | compiler-internal in both languages |
| G13 | `#must` | `#MustUse` with `.drop("reason")` | shipped, stronger | — |
| G14 | `#no_alias` | `no_alias` unsafe obligation | groundwork | — |
| G15 | `#no_debug` | none | ballot | D-JAI-CODE1 (`#NoDebug`) |
| G16 | `#no_padding` | none | ballot | D-JAI-LAYOUT1 (`#Layout(c, packed)`) |
| G17 | `#place` | none | ballot | D-JAI-LAYOUT1 (`#Layout(overlay)` of a struct and a list) |
| G18 | `#program_export` | `#Export(c)` functions | shipped | — |
| G19 | `#runtime_support` | Core runtime internals | not proposed | compiler-internal in both languages |
| G20 | `#scope_file`, `#scope_module`, `#scope_export` | `pub`, `priv`, `#PubFile`, `pub(package)` | shipped | — |
| G21 | `#specified` | explicit discriminants allowed, not required | ballot | D-JAI-SMALL1 (`#Specified`) |
| G22 | `#symmetric` | `#Commutative` | shipped | — |
| G23 | `#through` | or-patterns only | ballot | D-JAI-SMALL1 (`#Through`) |
| G24 | `#type,distinct`, `#type,isa` | `distinct` with an explicit conversion | shipped / by design | — |
| G25 | relative pointers `*~s32` | none | ballot | D-JAI-LAYOUT1 (`Rel<T, I32>` through `mem.Image`) |
| G26 | struct-of-arrays | `#Layout(columnar)` (D-SOA1) | shipped, stronger | — |
| G27 | `using` promotion | `impl Trait using field` delegation (S62) | ratified | — |
| G28 | `enum_flags` | `Bits` (integer set), `Set<Enum>` | ballot | D-JAI-SMALL1 (bit-backed `Set<Enum>`) |
| G29 | anonymous structs and unions | `.{ }` values, `A \| B` unions | ratified | — |
| G30 | multiple return values | named tuples (`Examples/features/basics/tuples.jet`) | shipped | — |
| G31 | named and default arguments | labels and defaults (D-APILABEL1) | ratified | — |
| G32 | `..T`, `..Any` varargs | `...T`, trait-bounded `...Renderable` (D-ANY-JAI1) | shipped | — |
| G33 | `defer` any statement | `defer close(^x)`; `scope.guard(() -> ...)` for other work | shipped / ballot | D-JAI-DEFER1 (general `defer` replaces `scope.guard`) |
| G34 | operator overloading | `impl T.Add(Rhs)` and other operator traits | shipped | — |
| G35 | quick lambdas `x => x.name` | `(x) -> x.name` | by design | — |
| G36 | `xx` autocast | destination-owned checked conversion `U8.from_int(n)?` (D-SHAPE-CONVERT1) | shipped, by design | — |
| G37 | `= ---` uninitialized | `T{ uninit }` with write-before-read proof (E0420) | shipped, stronger | — |
| G38 | order-independent declarations | order-independent lookup | shipped | — |
| G39 | identifier backslashes | none | not proposed | public sources do not show the exact meaning |
| G40 | intrinsics fail on unsupported targets | declaration-time target refusal | shipped | — |
| G41 | hot reload (wanted, not built in Jai) | `jet dev --swap`, `#Persist` (`Examples/features/devloop/persist.jet`) | shipped, stronger | — |
| G42 | custom entry symbol | freestanding target profile facts (D-TARGET-SURFACE1) | ratified | — |
| G43 | bucket arrays | `Pool<T>`/`Id<T>` stable identities; raw stable addresses stay behind `#Unsafe` | shipped, by design | — |

## Coverage summary

Of 215 rows (E36 counts once and E12-E15 as one):

- **Shipped or equal today:** 70 rows.
- **Ratified, owed on a named card:** 34 rows.
- **Decided by one of the twelve open ballots:** 100 rows.
- **Not proposed, with a stated reason:** 6 rows.
- **Other cards (#5000, #5008, #5030):** 5 rows.

Counts are approximate where a row has two states; every row names its closing record.

Also worth checking beside the tables: the native compile-time host (#5003), compile-time evaluator gaps (#4867, #4887) and delivery of the hook stage table (#4760).

## How Jai works (reconstructed from public sources)

Jai runs everything through one idea. The compiler is a library, and the build is a Jai program that the compiler runs first. That program (the *metaprogram*) creates workspaces, sets their `Build_Options`, adds source files or source strings, and then pulls a stream of compiler messages. The messages are files loaded, modules imported, phases reached, declarations type-checked, and "ready to link". In that loop it can read every checked syntax tree, report errors, generate more code, rewrite procedure bodies and take over linking.

Inside ordinary code, `#run` executes any procedure at compile time. `#insert` splices `Code` values or strings. `#expand` macros, `#modify` and the `#bake_*` directives specialize code, and an implicit `context` carries the allocator, logger, temporary storage and assertion handler into every call.

### 1.1 The shape: the compiler is a library and the build is a program

- **The metaprogram runs first.** Starting the compiler runs a metaprogram before anything else. It is a complete program that can organize compilation and change the program being compiled (Blow `y-d6GXNsbX4` 00:04:45, 2018).
  - With no metaprogram of your own, `modules/Default_Metaprogram.jai` runs. It sets the working directory, turns command-line flags into `Build_Options`, picks debug or release, and loads plugins (The Way to Jai `book/30A_Integrated_build_system.md:23`).
  - You replace it by compiling a build file (`jai build.jai` or `jai first.jai`) or with `jai main.jai -- meta Build` (`30A:427-448`).
- **Workspaces.** A workspace is a separate universe of code with its own options. Workspace 1 is the metaprogram, workspace 2 is the target given on the command line, and each `compiler_create_workspace` adds another (`30A:61-67, 141-144`; Blow `OHZwYYW9koI` 00:02:01).
  - Code does not cross workspaces unless done deliberately.
  - Since 2024 there is also a hidden "host imports" workspace for compile-time code (Blow `LPdMNsL5MgI` 00:03:21).
- **The pipeline.** Declarations drain through it independently and out of order (Blow `KHIeP_-Kp00` 00:52:49, 2017; `4_ODvZ01CjU` 00:23:20, 2019):
  1. parse;
  2. resolve and type-check;
  3. compute sizes;
  4. a message pipe that waits for the metaprogram;
  5. bytecode;
  6. backend (x64 or LLVM);
  7. link.

  The 2019 rewrite cut eight stages to five. Undeclared identifiers are deferred and retried until no progress is possible (Blow `jjCfAg_gtDw` 01:28:04).
- **Libraries ship as source.** Code is not linked as machine-code libraries, because a binary blocks metaprograms from transforming it (Blow `RuNr092aXIw` 00:17:41).

### 1.2 Compile-time execution

| Construct | Exact spelling | Meaning and limits | Source |
|---|---|---|---|
| Run at compile time | `#run expr;`, `#run { … }`, `X :: #run f();`, `X :: #run -> T { … }` | Runs any procedure in the compiler's bytecode interpreter, compiling its dependencies first; the value becomes a constant. Nothing else is guaranteed compiled, and order is not frozen. | Blow `OHZwYYW9koI` 01:24:17; focus `first.jai:4-12`; wiki `Metaprogramming.md:120,341` |
| Stallable run | `#run,stallable build();` | Lets the compiler suspend and resume a run, because calls through function pointers make the call graph unanalysable. The programmer must keep runs from interfering (for example through shared globals). | Blow `jjCfAg_gtDw` 00:10:52; focus `first.jai:18` |
| Constant results | `#run` values; `#no_reset` globals | Constant results, and globals changed at compile time, are copied into the executable. Compile-time heap data is duplicated into static storage; deep-copying arbitrary pointers was an open question. | Blow `OHZwYYW9koI` 00:18:04, 00:21:48; Appendix C `#no_reset` |
| Bake raw bytes | `#run add_global_data(bytes, .READ_ONLY)` | Puts arbitrary bytes, such as a PNG, in a data segment and returns the slice. | The Way to Jai `book/30B:827-863`; Tsoding `8_5L8yVHcII` 00:41:03 |
| Phase query | `#compile_time` | True while executing at compile time. | Appendix C |
| Constant query | `is_constant(expr)` | True if the expression reduces at compile time. The compiler runs bytecode to decide, so the answer is guaranteed language semantics, not an optimizer accident. | Blow `b5ATngfKQP4` 00:16:06, 00:34:50 |
| Compile-time assert | `#assert cond "msg";` | Fails the build. In 2014 it was introduced as "a flavour of run", so the condition can call any procedure, even an interactive game. | The Way to Jai `book/20A:72-83`; Blow `UTqZNujQOlA` 00:51:16–00:52:45 (whisper) |
| Static if | `#if cond { … } else { … }`, `ifx` | Evaluated after analysis. The false branch is parsed but not type-checked, so unknown names there are not reported. Re-evaluated per polymorph instance. | Blow `iUYZNbUKVAc` 01:16:23–01:30:00 |
| Compile-time-only procedures | `#compiler` | Interfaces with the compiler. Calling one at runtime fails with "attempt to call compile-time procedure at runtime". | Appendix C; Tsoding `BKvQXmEmDjU` 00:43:33 |
| Bytecode dump | `#dump` on a procedure | Prints its bytecode and basic blocks. | `book/20A:175-182` |
| Debugging | `-debugger` flag | Drops into an interactive debugger on a crash in compile-time execution. `#run main()` runs a whole program in the bytecode debugger. | `book/02B:45`; wiki `Snippets:81` |
| Unrestricted effects | `#run` / `#assert` bodies | Compile-time code can do anything the language can: open windows, run a whole game and bake its result into the binary, call external functions by loading DLLs at compile time, "contact a build server". Nothing is sandboxed or recorded. | Blow `UTqZNujQOlA` 00:44:04–00:53:31 (whisper) |
| Per-callee call checks (2014) | a "check call" directive naming a check procedure for a callee | At every call of, for example, `printf`, the compiler runs a check procedure at compile time. It receives the argument count, argument types and values, and the file and line; returning false fails the build. This is the ancestor of today's `Check` plugin and user-level checks. | Blow `UTqZNujQOlA` 00:38:21–00:43:16, 00:53:46–00:57:13 (whisper; directive spelling not confirmed) |
| Build manifest in source | `#import`, `#load` and build options in the main file | The program lists its modules, files and linker options in ordinary syntax, replacing make files and IDE projects. Global scope is order-independent, with multi-pass resolution and no forward declarations. | Blow `UTqZNujQOlA` 00:05:43–00:07:43, 00:18:21–00:19:19 (whisper) |

**Limits the author stated:**

- No time or instruction limit; an infinite loop at compile time hangs the build (`b5ATngfKQP4` 00:37:25, 2018).
- Compile-time returns were limited to register-sized values in 2016 (`k9HlR1VjNxs` 00:32:40).
- Nested `#run` could falsely deadlock (`pgiVrhsGkKY` 00:52:05).
- Compile-time code ran in parallel threads beside the main compile, which hid its cost in profiles (`TwqXTf7VfZk` 03:17:01).
- A `#run` in a per-OS module must use the host version (`jjCfAg_gtDw` 00:43:50).

Blow's guidance: "use compile-time execution only for what cannot be done at runtime" (`pg_Wk2GBZDw` 00:10:41).

### 1.3 Code as data: code values, insertion, macros, specialization, notes

| Construct | Exact spelling | Meaning | Source |
|---|---|---|---|
| Code value | `#code expr`, `#code { … }`, type `Code` | Captures a syntax tree. Needed only when names would not resolve in the caller's scope. | Blow `6qkL_DmHOuo` 00:16:17; `QX46eLqq1ps` 00:34:40 |
| Insertion | `#insert code_or_string;`, `#insert -> string { … }`, `#insert,scope() …` | Splices code or a generated string into a procedure or struct. The `,scope()` form lets the inserted code see local names. An older "insert internal" form existed. | Appendix C; `QX46eLqq1ps` 00:32:56–00:36:14 |
| Multi-line string | `#string END … END` | Here-strings, intended for generating code. | `4h-0Sc2jK88` 00:19:08–00:21:11 |
| Macro | `name :: (…) #expand { … }` | Body inserted at each call site and hygienic. Overloads with procedures, takes named and variadic arguments, is implicitly polymorphic, and cannot recurse. Mutual insertion stops after 100 levels. | `QX46eLqq1ps` 00:13:12–00:21:40, 01:24:24 |
| Caller scope access | `` `name `` | Reads or declares a name in the caller's scope; `` `defer `` and `` `return `` act on the caller. Blow calls it clumsy and plans a version 2, possibly an explicit import list. | `QX46eLqq1ps` 00:23:40–00:28:30; `OAIqCpqszVw` 00:34:34; `pg_Wk2GBZDw` 00:29:53 |
| Caller code | `call := #caller_code` | The calling expression as `Code`. | Appendix C; The Way to Jai `26C §26.14` |
| Code transform | `f :: (code: Code) -> Code { root, expressions := compiler_get_nodes(code); … }` then `#insert` the result | A compile-time procedure takes code, edits its nodes (for example rewriting `Code_Literal` values) and returns new code to insert; `code_of` also yields `Code`. | The Way to Jai `book/26C:206-268` |
| Iteration protocol | a macro named `for_expansion` with `#expand` (exact parameter list not confirmed) | Defines `for` over a user type. Takes the object, the loop body as `Code`, and by-pointer and reverse flags; exposes `` `it `` and `` `it_index ``; named break works across nesting. | `QX46eLqq1ps` 00:44:40–00:55:02 (caption) |
| Polymorph hook | `f :: (a: $T) #modify { … return true; } { … }` | Runs compile-time code each time a call resolves. May set, default or reject type variables. Returns false, optionally with a message, to refuse. Runs silently during speculative overload resolution and is chainable. Works on procedures; on structs it was "not supported yet" in 2018. | `7Fsy2WaxLOY` 00:02:27–00:07:52; `COQKyOCAxOQ` 00:47:30; `b5ATngfKQP4` 01:13:22; wiki `Advanced.md:402-422` |
| Specialization | `#bake_constants`, `#bake_arguments` (call syntax not confirmed here), `$` / `$$` on a parameter | Generates a new compiled procedure with fixed type or value arguments (compile-time currying). `$` auto-bakes per constant value. Renamed from `#bake` and "bake values" in 2018. | Appendix C:18-20; `y-d6GXNsbX4` 00:39:55; `_tLk-JNGD60` 00:01:30–00:20:06 (caption) |
| Constraint shorthand | `$T/Vector`, `$T/interface Foo` | Structural check on a polymorph. | `2IBr0XZOPsk` 00:05:42; wiki `Advanced.md:402` |
| Placeholder | `#placeholder NAME;` | Promises a name a metaprogram will generate; an unfilled one errors. Blow proposed making undeclared names illegal unless placeholdered. | `SSVHWrYG974` 00:43:00–00:54:00; `jjCfAg_gtDw` 01:29:29 |
| Locations | `#file`, `#line`, `#filepath`, `#location`, `loc := #caller_location`, `#procedure_name` | Source location values. `#location` gives the location of a piece of `Code`. `#caller_location` as a default argument gives a logger or assert its caller's file and line. | Appendix C; `30A:90-95`; `yJukJS81o-k` 00:19:28–00:21:38 |
| Notes | `@note` on any declaration | Plain strings attached to declarations, read by metaprograms (serializers, profilers, settings versioning). Not allowed on expressions or argument lists. Typed notes planned; one 2024 answer says they stay strings. | `JoNkttD_MUs` 00:55:00; `LPdMNsL5MgI` 00:46:04 vs 02:05:02; focus `first.jai:239-322` |
| Type info | `type_info(T)`, `Type_Info_Struct.members`, `it.notes`, `Any` | Compile-time and runtime type tables. `Any` is a type-info pointer plus a value pointer. `#type_info_none`, `#type_info_procedures_are_void_pointers` and `#type_info_no_size_compliant` trim the table. | `JoNkttD_MUs` 00:10:40–00:17:58; Appendix C |
| Other directives | `#this`, `#through`, `#complete`, `#must`, `#deprecated`, `#no_abc`, `#no_context`, `#no_debug`, `#c_call`, `#intrinsic`, `#symmetric`, `#specified`, `#place`, `#align`, `#as`, `#asm`, `#bytes`, `#char`, `#type,distinct`, `#type,isa`, `#scope_file`, `#scope_module`, `#scope_export`, `#module_parameters`, `#program_export`, `#foreign`, `#library`, `#system_library`, `#import,dir`, `#import,file`, `#import,string`, `#load`, `#add_context`, `#runtime_support`, `#cpp_method`, `#cpp_return_type_is_non_pod`, `#procedure_of_call` | The full public directive index (60 as of January 2023). | The Way to Jai `Appendix C - Directives.md:8-115` |

**History worth knowing:**

- String generation came first. "Body text" returned a string that replaced a procedure or struct body (2015–2019). Generated strings were saved to a debug file so errors and the debugger point at real lines (`2IBr0XZOPsk` 00:26:50; `59lKAlb6cRg` 01:02:14).
- Pre-parsed tree insertion was wanted from 2016 and still on the roadmap in 2023 (`jjCfAg_gtDw` 00:04:36).
- Constructors and destructors were tried in 2017 and removed in 2019 in favour of `defer` and macros (`4_ODvZ01CjU` 00:10:50).

### 1.4 The compiler message loop

**Canonical loop** (focus `first.jai:78-169`, verbatim names):

```jai
w := compiler_create_workspace(options.output_executable_name);
set_build_options(options, w);
intercept_flags: Intercept_Flags;
for plugins if it.before_intercept it.before_intercept(it, *intercept_flags);
compiler_begin_intercept(w, intercept_flags);
for plugins if it.add_source it.add_source(it);
add_build_file("src/main.jai", w);
add_build_string(build_constants, w);
while true {
    message := compiler_wait_for_message();
    if message.workspace != w continue;
    for plugins if it.message it.message(it, message);
    if message.kind == {
      case .TYPECHECKED;  // inspect Message_Typechecked
      case .PHASE;        // .ALL_TARGET_CODE_BUILT, .READY_FOR_CUSTOM_LINK_COMMAND …
      case .COMPLETE;     break;
    }
}
compiler_end_intercept(w);
for plugins if it.finish   it.finish(it);
for plugins if it.shutdown it.shutdown(it);
```

**Compiler-module procedures seen in public code:**

| Procedure | Use | Source |
|---|---|---|
| `compiler_create_workspace()` or `compiler_create_workspace("name")` -> `Workspace` | 0 means failure; `-1` passed as a workspace means the current one | `30A:42-53, 227`; Tsoding `sx1cjdJIU70` 00:18:00 |
| `get_current_workspace()` | 0 at runtime | `30A:62` |
| `get_build_options()` (current workspace) or `get_build_options(w)`; `set_build_options(opts, w)` | Copy out, edit, write back | `30A:126-128`; `30.12_placeholder.jai:6`; focus `first.jai:23,79` |
| `set_build_options_dc(.{…})`, `set_build_options_dc(.{…}, w)` | Options applied during compile: `do_output`, `write_added_strings`, `append_linker_arguments` | focus `first.jai:33,83-88`; `30A:155-160` |
| `copy_commonly_propagated_fields(get_build_options(), *options)` | Copy shared settings from the metaprogram's options | infinite-pong `first.jai:9` |
| `set_optimization(*opts, .DEBUG, true)`, `set_optimization(*opts, .OPTIMIZED)` | Preset bundles (third argument's name not confirmed) | `30A:305-319` |
| `compiler_begin_intercept(w)`, `compiler_begin_intercept(w, intercept_flags)`, `compiler_end_intercept(w)` | Become the message consumer | `30.5_compiler_intercept.jai:16-19`; focus `first.jai:101,166` |
| `compiler_wait_for_message() -> *Message` | Blocks; the previous message is released at the next call | Blow `KHIeP_-Kp00` 00:50:49 |
| `add_build_file(path, w)`, `add_build_string(code, w)` | Add source; added strings go to one generated file under `.build/` | `30A:151-197`; Tsoding `BKvQXmEmDjU` 01:51:17 |
| `compiler_report(msg)`, `compiler_report(msg, location)`, `compiler_report(msg, location, .WARNING)`; `make_location(node)` | Errors and warnings from metaprograms (parameter names not confirmed) | jai-wasm `first.jai:45-55`; focus `first.jai:244`; `30.9_house_rules.jai:44-45` |
| `compiler_get_nodes(code: Code) -> (root: *Code_Node, expressions: [] *Code_Node) #compiler` | Walk a `Code` value; `expressions` is the same flattened list a typechecked message carries | The Way to Jai `book/26C:139-188` |
| `compiler_modify_procedure(w, body)` | Resubmit a changed `Code_Procedure_Body` | jai-wasm `first.jai:56-58`; Blow `jjCfAg_gtDw` 00:01:44 |
| `compiler_custom_link_command_is_complete(w)` | Ends the custom link phase | focus `first.jai:155` |
| `compiler_set_workspace_status(.FAILED)` | Fail a workspace | `book/30B:936-941` |
| `compiler_get_version_info(*info)` | Version gates | focus `first.jai:216-238` |
| `compiler_get_command_line_arguments()`, `compiler_set_command_line_arguments(args)` | Metaprogram arguments; the setter is for script wrappers | Blow `y-d6GXNsbX4` 00:05:13, 00:14:10 (caption) |
| `remap_import(w, "*", from, to)` | Replace a module for every importer | infinite-pong `first.jai:33` |
| `run_command(..args, capture_and_return_output=…)` | Process module, used for external tools | jai-wasm `first.jai:12` |

**Messages:**

| `message.kind` | Payload struct and fields seen | When |
|---|---|---|
| `.FILE` | `Message_File.fully_pathed_filename` | Each source file loaded |
| `.IMPORT` | `Message_Import.module_name`, `.module_type`, `.fully_pathed_filename` | Each module imported (once) |
| `.PHASE` | `Message_Phase.phase`, `.executable_name`, `.executable_write_failed`, `.linker_exit_code`, `.compiler_generated_object_files` | Each phase entered |
| `.TYPECHECKED` | `Message_Typechecked.declarations`, `.structs`, `.procedure_bodies`, `.procedure_headers`, `.all`; each entry has `.expression` and `.subexpressions` | Per top-level declaration, after type-checking, before code generation |
| `.DEBUG_DUMP` | `Message_Debug_Dump.dump_text` | Compiler crash dump |
| `.ERROR` | — | A compilation error |
| `.COMPLETE` | `Message_Complete.error_code` (`.NONE`, `.COMPILATION_FAILED`) | End; break the loop |

Sources: The Way to Jai `examples/30/30.5_compiler_intercept.jai:22-78`; `book/30B:113-206`; focus `first.jai:129-162`; jai-wasm `first.jai:37-73`.

**Phases** (`Message_Phase.phase`, The Way to Jai `book/30B:169-176`):

| Value | Name | Typical use |
|---|---|---|
| 0 | `ALL_SOURCE_CODE_PARSED` | Fires once per batch of added source |
| 1 | `TYPECHECKED_ALL_WE_CAN` | Last chance to generate code: `add_build_string` here; exit here for a check-only build |
| 2 | `ALL_TARGET_CODE_BUILT` | Validate settings; exit if `output_type == .NO_OUTPUT` |
| 3 | `PRE_WRITE_EXECUTABLE` | Print gathered reports without interleaving compiler output; the executable can be replaced (Blow `AdoShhhnsQA` 00:03:53) |
| 4 | `POST_WRITE_EXECUTABLE` | Linker exit code, executable size, post-build steps |
| 5 | `READY_FOR_CUSTOM_LINK_COMMAND` | With `use_custom_link_command`, link the object files yourself, then call `compiler_custom_link_command_is_complete(w)` |

**Typed tree.** `Code_Node` is the base, with `.kind`, and casts to specific structs (Blow `OHZwYYW9koI` 00:35:52; focus; jai-wasm):

- `Code_Declaration` (`.name`, `.notes`, `.type`, `.expression`);
- `Code_Procedure_Header` (`.name`, `.notes`);
- `Code_Procedure_Body` (`.header`, `.block`);
- `Code_Block` (`.statements`, block flags such as the bounds-check flag);
- `Code_Struct` (`.defined_type`);
- `Code_Literal` (`.value_type`, `.array_literal_info`, `.struct_literal_info`);
- `Code_Procedure_Call`;
- `Code_Ident`.

Node kinds include `.DECLARATION`, `.LITERAL`, `.ASM`, `.BLOCK`, `.IDENT` and binary operators. Nodes carry a location: `make_location(node)` builds one, and `expr.enclosing_load.enclosing_import.module_type` tells main-program code from module code (`30.9_house_rules.jai:58-59`). Line and column fields printed as `l0` in one stream are caption-level. Implicit casts appear as nodes.

**Rules and pitfalls the author named:**

- Hooking after type-check means the tree must already be correct; changing a signature or arity breaks callers (`iGiHa3GtQhM` 00:25:40).
- Body resubmission is a discrete call to avoid races with parallel compilation, and malformed input is not checked carefully (`jjCfAg_gtDw` 00:01:44–00:03:51).
- Polymorphs arrive only when used; generic bodies arrive first, then each specialization (`jjCfAg_gtDw` 01:18:09; `iGiHa3GtQhM` 00:01:40).
- For speed, interception once exported only declarations that had notes, so a metaprogram silently saw fewer than expected (`KHIeP_-Kp00` 00:33:00).
- Compilation was nondeterministic across runs; Blow suggested sorting files by name, and later called out-of-order compilation "about half the compiler effort" (`KHIeP_-Kp00` 01:28:00; `kHmbabcL-is` 00:22:06, 01:17:51).
- Generated code added late can hide real errors behind undeclared-identifier noise (`SSVHWrYG974` 00:44:00).
- The wait loop originally burned CPU (`OHZwYYW9koI` 00:23:00).

**Plugins.** The default metaprogram parses `-plug Name` or `+Name args…` and imports each plugin module. It calls `get_plugin() -> *Metaprogram_Plugin`, then the hooks shown in the loop above. A plugin extends `Metaprogram_Plugin` as its first member. `Intercept_Flags` members seen in a comment are `.SKIP_DECLARATIONS`, `.SKIP_STRUCTS` and `.SKIP_OTHERS` (inference from public code; kujukuju/MetaThreadSafe). Bundled plugins (The Way to Jai `book/36A:34-47`):

- `Check`, print-argument checks, on by default and disabled with `-no_check`;
- `Polymorph_Report`;
- `Program_Print`;
- `IProf`, automatic instrumentation of every procedure;
- `Autorun`;
- `Icon`.

**What metaprograms are used for, shown in public:**

- MISRA rules, recursion bans and thread-safety whitelists (Blow `OHZwYYW9koI` 00:46:17–00:55:00; `30.9_house_rules.jai`);
- forbidding entity pointers held across frames;
- auditing pointer arithmetic across a whole program (`0mQbBayzDPI`);
- collecting `@fruit`-tagged procedures and generating `main` (`30.11_using_notes.jai`);
- versioned-settings checks (focus);
- translating Jai to shaders (PixelRifts) and to Scheme (`OHZwYYW9koI` 00:33:00);
- stripping `#asm` bodies for WebAssembly (jai-wasm);
- generating bindings (`Bindings_Generator`);
- profiling;
- command-line parsing from struct type info (`TwqXTf7VfZk`);
- binding game functions to keys and console commands (Blow's game: about 1,000–1,200 metaprogram lines in a 265,000-line program, `pg_Wk2GBZDw` 00:12:13).

### 1.5 Build options (`Build_Options`)

The complete printed set from The Way to Jai (`examples/30/30.4_build_options.jai:50-123`, an older beta). Newer names from focus (2026-09) and infinite-pong (beta 0.1.096, 2024-11) are noted per row.

| Group | Field | Values seen |
|---|---|---|
| Output | `output_type` | `.EXECUTABLE` (default), `.DYNAMIC_LIBRARY`, `.STATIC_LIBRARY`, `.OBJECT_FILE`, `.NO_OUTPUT` |
| | `output_executable_name`, `output_path`, `intermediate_path` | strings; default output path `.build/` |
| | `output_flags`, `entry_point_name` | |
| Target | `os_target` | `.WINDOWS`, `.LINUX`, `.MACOS`, `.WASM` (2024) |
| | `cpu_target` (2024) or `machine_type` + `machine_options` (older) | `.CUSTOM` |
| | `minimum_macos_version` → `minimum_os_version` | `.{10, 13}` |
| | `backend` | `.LLVM` (default), `.X64` |
| Optimization | `set_optimization(*o, Optimization_Type)` | `.DEBUG`, `.VERY_DEBUG`, `.OPTIMIZED`, `.VERY_OPTIMIZED`, `.OPTIMIZED_SMALL`, `.OPTIMIZED_VERY_SMALL` (`30A:311-319`) |
| | `enable_bytecode_inliner`, `dead_code_elimination` | `.ALL`, `.NONE`, `.MODULES_ONLY` |
| | `maximum_polymorph_depth` | 100 |
| | `maximum_array_count_before_compile_time_returns_are_not_reflected_in_ast` | 5000 |
| Runtime checks | `array_bounds_check` | `.OFF`, `.ON`, `.ALWAYS` |
| | `cast_bounds_check`, `math_bounds_check` | `.FATAL`, `.NONFATAL`, `.OFF` |
| | `null_pointer_check` | `.ON`, `.OFF` |
| | `arithmetic_overflow_check` (focus) | `.FATAL`, `.OFF` |
| Debug and crash | `emit_debug_info` | `.DEFAULT`, `.NONE`, `.DWARF` |
| | `stack_trace` | bool |
| | `backtrace_on_crash` | `.ON`, `.OFF` |
| | `enable_frame_pointers`, `interactive_bytecode_debugger`, `debug_for_expansions` | bools |
| Codegen | `disable_redzone`, `enable_auto_dereference`, `lazy_foreign_function_lookups` | bools |
| | `x64_options` | 3 unnamed bools in the dump |
| | `llvm_options` | `function_sections`, `enable_tail_calls`, `enable_loop_unrolling`, `enable_slp_vectorization`, `enable_loop_vectorization`, `reroll_loops`, `verify_input`, `verify_output`, `merge_functions`, `disable_inlining`, `disable_mem2reg`, `enable_split_modules`, `output_bitcode_before_optimizations`, `output_llvm_ir_before_optimizations`, `output_bitcode`, `output_llvm_ir`, `target_system_triple`, `target_system_cpu`, `target_system_features`, `command_line`, `code_gen_optimization_level`, `code_gen_size_level` |
| Runtime | `temporary_storage_size` | 32768 default; focus uses 1 MiB per thread |
| | `runtime_storageless_type_info` | bool: drop the runtime type table |
| Linking | `additional_linker_arguments` | e.g. `.["--export-table"]` |
| | `use_custom_link_command` | bool |
| | `_dc` field `append_linker_arguments` | e.g. `/SUBSYSTEM:windows` |
| Modules | `import_path` | array of strings |
| | module policy | four modes, copy on first use by default (Blow `Wp9XD5FKZ2c` 00:04:00, caption) |
| Diagnostics | `text_output_flags` (`OUTPUT_LINK_LINE`, `OUTPUT_TIMING_INFO`), `info_flags`, `use_ansi_color`, `use_visual_studio_message_format`, `use_natvis_compatible_types`, `shorten_filenames_in_error_messages` | |
| | `_dc` field `write_added_strings` | bool |
| Metaprogram data | `compile_time_command_line` | strings after a lone `-` |
| | `user_data_u64`, `user_data_string`, `user_data_pointer`, `user_data_pointer_size` | |
| During compile | `set_build_options_dc(.{do_output, write_added_strings, append_linker_arguments}, w)` | |

**Precedence.** Program-set options win over command-line flags, and Blow wants fewer flags so builds reproduce "in everybody's environment" (`OAIqCpqszVw` 01:06:25–01:07:35). Optimization is per build, not per file; a per-function hint was planned in 2023 (`_tLk-JNGD60` 00:04:03; `jjCfAg_gtDw` 00:57:39).

### 1.6 The command line

`jai <options> program.jai - <metaprogram args> -- meta <Metaprogram> import_dir <dir> +Plugin <plugin args>`. `--` and `---` both work as the delimiter (The Way to Jai `book/02B:1-122`).

| Group | Flags |
|---|---|
| Backend | `-x64`, `-llvm` |
| Code | `-add "CODE"` (adds code to the target), `-run "EXPR"` (starts a `#run`) |
| Debugging | `-debugger`, `-debug_for`, `-very_debug`, `-msvc_format`, `-natvis`, `-verbose` |
| Output | `-output_path`, `-exe name`, `-no_color`, `-quiet` |
| Modules | `-import_dir dir` (repeatable), `-- import_dir dir` (for the metaprogram) |
| Performance | `-release`, `-release_debug`, `-no_dce`, `-no_check`, `-no_check_bindings`, `-no_split`, `-no_inline`, `-no_backtrace_on_crash`, `-context_size n` |
| Metaprogram | `-- meta Name`, `-plug Name` / `-plugin Name`, `+Name …` |
| Misc | `-no_cwd`, `-version`, `-report_poly` |
| Developer (`jai -- help`) | `import_dir`, `meta`, `-no_jobs`, `-randomize`, `-seed n`, `-extra`, `-chaos` |

A file may start with `#!` and run as a script; the compiler parses its own arguments so behaviour matches across operating systems (Blow `y-d6GXNsbX4` 00:09:27–00:14:10).

### 1.7 Context and runtime hooks

`Context_Base` (The Way to Jai `book/25A_Context.md:17-35`) holds:

- `context_info`, `thread_index`;
- `allocator := Allocator.{__default_allocator_proc, null}`;
- `logger := default_logger`, `logger_data`, `log_source_identifier`, `log_level`;
- `temporary_storage: *Temporary_Storage`;
- `dynamic_entries: [16] Dynamic_Context_Entry`, `num_dynamic_entries`;
- `stack_trace: *Stack_Trace_Node`;
- `assertion_failed := default_assertion_failed`, `handling_assertion_failure`;
- `print_style` (`default_format_int`, `default_format_float`, `default_format_struct` and others, from `book/25A` §25.8).

**How the context works:**

- **Passing.** It is passed implicitly to every procedure as the first hidden parameter; Tsoding saw `sum(a, b: s32)` compile to four wasm parameters (`sx1cjdJIU70` 01:45:10). `#no_context` and `#c_call` remove it.
- **Scoping.** `push_context new_context { … }` and `push_allocator` swap it for a scope. `#add_context name := value;` adds program-wide fields (`SSVHWrYG974` 00:37:00).
- **Why not thread-local storage.** TLS support varies by OS, has an access cost, and implicit context across callbacks is invisible; for callbacks Blow recommends `push_context` in a macro with `defer` (`jjCfAg_gtDw` 01:32:24).
- **Allocator protocol.** One procedure `(mode: Allocator_Mode, size, old_size, old_memory, allocator_data) -> *void` with modes allocate, resize and free; free-all was removed (`Wp9XD5FKZ2c` 00:25:50–00:29:44). Data structures remember the allocator they first used (`ZpgjnCgsGwk` 00:16:08).
- **Temporary storage.** A per-thread bump allocator in the context, reset per frame with a high-water mark. `get_temporary_storage_mark` / `set_temporary_storage_mark` give scoped use; overflow goes to pages that are later freed; `tprint` and `temp_c_string` allocate from it (`SSVHWrYG974` 00:02:11–00:25:00; `Wp9XD5FKZ2c` 01:41:35). Core libraries avoid both the context allocator and temporary storage because they cannot know when they run (`Wp9XD5FKZ2c` 01:43:44).
- **Known problems.** Context member names collide between libraries (`SSVHWrYG974` 00:40:00). The context does not work across DLLs, whose context ordering is nondeterministic, and DLLs are no longer recommended (`pg_Wk2GBZDw` 00:21:45; `LPdMNsL5MgI` 00:07:54, 00:10:39).
- **Entry.** `__system_entry_point` is `#program_export "main"` and calls `__jai_runtime_init` (The Way to Jai `book/04E:5-29`).

### 1.8 Worked platform patterns from public code

- **WebAssembly in 2022–2023 (Tsoding, jai-wasm `first.jai`):**
  1. build one workspace with `llvm_options.output_bitcode = true` and `enable_split_modules = false`;
  2. at `.TYPECHECKED`, find procedure bodies containing `.ASM` nodes and empty them with `compiler_modify_procedure`;
  3. at `POST_WRITE_EXECUTABLE`, take `compiler_generated_object_files[0]`;
  4. run `clang --target=wasm64`, then `wasm-ld --no-entry --export-all --allow-undefined`, then a 64-to-32 patcher (`tfME9wwdgGw`).
- **WebAssembly in 2024 (infinite-pong):** set `os_target = .WASM`, `cpu_target = .CUSTOM` and `llvm_options.target_system_features = "+bulk-memory"`; replace the allocator module with `remap_import`; pass `--export-table` to the linker.
- **Parameter hot reload (Tsoding):** one module parses `params.conf`. At compile time it emits constants via `add_build_string` in release; at runtime in debug it reloads on F5 (`tPUXbxH2AFA` 01:17:00–01:59:13; `YFtT48e9mA0` 00:29:17). Jai itself had no code hot reload; edit-and-continue was wanted but not built (`RuNr092aXIw` 00:46:21).
- **Relative pointers:** a relative pointer type stores a 32-bit self-relative offset (declared with `~` and an explicit `s32` underlying type; exact placement is caption-level). Zero is null, and the encoding XORs the high bit (`GQj8fzxpdrk` 00:03:11; `_HGkhvpCbsk` 01:36:20).

### 1.9 The author's own verdicts over ten years

**Speed.**

- About 0.75 s for a 100–150k-line game (2020).
- 250k lines in 1.36 s, peak about 270k lines/s against a 1M lines/s goal (2023).
- About 100–200k lines/s without LLVM (2024). Linkers "mostly should not exist".
- The native backend plan was dropped in 2024: LLVM is kept for optimization.

Sources: `Wp9XD5FKZ2c` 02:25:45; `jjCfAg_gtDw` 00:20:06; `LPdMNsL5MgI` 00:41:01, 01:19:45; `pg_Wk2GBZDw` 01:10:00.

**Regrets and open problems:**

- out-of-order compilation costs half the effort;
- the backtick is clumsy;
- notes as strings;
- context across DLLs;
- the `#if` host-versus-target problem;
- undeclared-identifier deferral makes errors poor;
- macro error locations are incomplete;
- no limit on compile-time loops;
- RAII "a mistake";
- few built-in warnings by design, with project rules left to metaprograms.

Sources: `kHmbabcL-is` 01:17:51; `OAIqCpqszVw` 00:34:34; `RuNr092aXIw` 00:50:01; `mGe5d6dPHAU` 00:04:27.

**Users' pain.** The compiler API is learned by trial and error. Tsoding guessed that `-1` means the current workspace, and when `do_output` takes effect (`BKvQXmEmDjU` 00:20:23, 00:29:30). The backtick rule was undocumented (`tPUXbxH2AFA` 01:26:50), and errors in generated strings land in one generated file (`BKvQXmEmDjU` 01:51:17). A production user reports drifting documentation, hiring difficulty and closed distribution ("Four Years of Jai", 2024-12-02).

**Users' praise:**

- baking an image or font into the executable;
- `::` versus `:=` as a one-character constant-to-variable edit;
- "the compiler always tells me if I'm doing something wrong";
- a whole build in one language, "a cross-platform shell" (`AwbLjR46HkY` 01:29:51, 00:31:04; `Y5liBU9f_jM` 01:45:13; `tfME9wwdgGw` 00:05:31).

## Scope, sources and coverage

**Request (2026-10-15 mining run).** The owner asked for a full reconstruction of Jai from public material, with exhaustive detail on compiler and build options, metaprograms, compile-time execution, the compiler's message loop, hooks and the build context. Owner answers before capture: Tower-enabled run; add Jonathan Blow's public demos; transcripts plus targeted frames; static Jet check, where ratified intent or groundwork counts as covered.

**Above-board rule.** Only material published openly was used: public videos, a public book, a public wiki, public user-written repositories and blog posts. No compiler binary, leaked build or copy of the proprietary beta `modules/` or `how_to/` folders was read. Where public text cites a beta file by name, the file itself was not opened.

| Source set | Identity | Size | Captured |
|---|---|---|---|
| Owner playlist, Tsoding Daily (beta user) | `PLpM-Dvs8t0Vb5bowR1en3vQPN9RfJAJAE`, 13 videos, 2022-06-21 to 2022-09-19 | 25.8 h | 13 auto-caption transcripts |
| Jonathan Blow, "A Programming Language for Games" | `PLmV5I2fxaiCKfxMBrNsU1kgKJXD3PkyxO`, 71 videos, 2014-09-19 to 2024-09-16 | about 93 h | 69 auto-caption transcripts; 2 without captions (see limits) |
| Blow, "Compiler programming livestreams" (targeted subset) | 10 streams on message/phase loop, `#modify` for structs, custom link commands, argument parsing, user-level SOA, pointer-math scan, Program Print | about 12 h | 9 transcripts; 1 without captions |
| The Way to Jai (Ivo Balbaert) | github.com/Ivo-Balbaert/The_Way_to_Jai @ `19cb4b7` (2026-06-11) | book + examples | chapters 02B, 25A, 26A–C, 30A–B, 36A, Appendix C read; others grepped |
| Jai Community wiki and library; JaiPrimer (historical) | wiki @ `80457dfd`, library @ `24bad890`, JaiPrimer @ `dd6581a2` | — | Metaprogramming, Advanced, Snippets (part) |
| focus editor | github.com/focus-editor/focus @ `c6b3ead` (2026-09-08), `first.jai` | 516 lines | read in full |
| Tsoding repos | jai-wasm @ `ee0f885` (2023-01-17), infinite-pong-jai-wasm64 @ `d3319ec` (2024-12-12), jaibreak @ `e7e206a6`, tetris-jai @ `6fd371e3`, plus 4 others | — | build files read |
| Writeups and user repos | Smári McCarthy "Four Years of Jai" (2024-12-02); PixelRifts/Jai-Shader-Transpiler @ `fbc76835`; onelivesleft/Metaprogram @ `1a230c17`; kujukuju/MetaThreadSafe @ `5b9a34dd` | — | plugin and metaprogram files |

**Capture quality.** No video had creator captions; all 91 captured transcripts are YouTube auto-captions. Captions mangle code: `#run` arrives as "hash run" or "run directive", backtick as "backslash", `$$` as "dollar dollar". Every transcript claim carries a caption-confidence label in the ledger. Spellings in this report come from the book, the wiki or public code wherever those exist; a caption-only spelling is marked "(caption)". Three videos have no captions at all:

- `UTqZNujQOlA` (2014-10-31, "Base language, compile-time execution"). A local whisper.cpp `base.en` transcription covers 00:00–00:57 of 2:07 before a one-hour limit; the remainder is a gap;
- `DN6kY5iH8ys` (Treemap Viewer part 3);
- `rzE6bz2Uf7g` ("Making a big change without breaking the program").

Three more videos had no relevant content in keyword windows: `ZHqFrNyLlpA`, `kJp95IikU-U` and `jm1QkRpEaRQ`. Targeted frames were not needed: every on-screen detail the captions pointed at was found in public code or the book. Twenty-one helper lanes produced 1,812 timestamped transcript claims; I read the partial whisper transcript myself. The ledger condenses all of this into 43 reviewed claims with Jet classifications. Capture files are deleted at close as the method requires, so the ledger lives only in this report's tables.

**Version caution.** Jai is a closed beta whose syntax changes. Field and directive names in [How Jai works](#how-jai-works-reconstructed-from-public-sources) are dated by their source. The Way to Jai's `Build_Options` dump predates the 2024–2026 fields seen in focus and infinite-pong; for example `minimum_macos_version` became `minimum_os_version`. Where two sources disagree, both are given.

The 2026-10-16 parity audit added six read-only census lanes over Jet's own tree (see [Verdict](#verdict)); it read no further Jai sources.

## Tower records

The 2026-10-15 mining run filed #5005–#5010 and logged evidence to #3520, #4760, #3526, #5000 and #4913. The 2026-10-16 parity audit widened #5005, #5006, #5009 and #5010 into ballots, and added the parent card #5021, the ballot cards #5022–#5029 and the defect card #5030.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 Expert opt-out model | card | #5022 |
| F2 Code values, insertion and macros | card | #5023 |
| F3 Per-instance generic control | card | #5024 |
| F4 Missing compiler event stages, custom link and post-link moments | card | #5009 |
| F5 Code-site facts and stale #Caller() | card | #5010 |
| F6 Build option catalog, per-scope check removal and pinning | card | #5005 |
| F7 Several profiles per build and host facts | card | #5006 |
| F8 Context parity | card | #5025 |
| F9 General defer | card | #5026 |
| F10 Layout control | card | #5027 |
| F11 Small conveniences | card | #5028 |
| F12 Changeable globals initialized while compiling | card | #5029 |
| F13 #Context allocator and logger ignored at run time | card | #5030 |
| F14 D-META-CODE1 option B ratified but contradicted by Syntax.rs | card | #3520 |
| F15 Spec still states pre-reconciliation refusals (D-META-RECONCILE1=A not rendered) | card | #3520 |
| F16 Instance report, bytecode dump and program printer | card | #5000 |
| F17 Durable form of this audit as an executable manifest | card | #5021 |
| F18 Host versus target facts for prep code | card | #5007 |
| F19 No debugger for compile-time code | card | #5008 |
| F20 Stage table must cover message kinds, polymorph delivery, determinism, IMPORT and plugin selection | card | #4760 |
| F21 Advanced suite acceptance: hygiene, captures, expansion limit, change-set recheck | card | #3526 |
| F22 Context extensions, temporary storage, FFI boundary | card | #3502 |
| F23 Eager constants and auto-bake | decision | D-COMPTIME-WORD1 |
| F24 Compile-time host not wired in native jet build | card | #5003 |
| F25 Undocumented metaprogramming API pain | card | #3520 |
| F26 Compile-speed comparison | no-action | Jai is a closed beta with no runnable binary here, so no matched same-run cell can satisfy the performance gate |
<!-- /audit-dispositions -->

## Source index

- Owner playlist (Tsoding Daily): https://www.youtube.com/playlist?list=PLpM-Dvs8t0Vb5bowR1en3vQPN9RfJAJAE
- Jonathan Blow, "A Programming Language for Games": https://www.youtube.com/playlist?list=PLmV5I2fxaiCKfxMBrNsU1kgKJXD3PkyxO
- Jonathan Blow, compiler programming livestreams (subset listed in Scope): https://www.youtube.com/playlist?list=PLmV5I2fxaiCIZVTLzofsocka2LvWBFvBa
- The Way to Jai @ 19cb4b7: https://github.com/Ivo-Balbaert/The_Way_to_Jai
- Jai Community wiki and library: https://github.com/Jai-Community/Jai-Community-Library
- JaiPrimer (historical, 2021): https://github.com/BSVino/JaiPrimer
- focus editor @ c6b3ead: https://github.com/focus-editor/focus/blob/c6b3ead7d4174527d0138e8a31f7c3c5663badec/first.jai
- tsoding/jai-wasm @ ee0f885: https://github.com/tsoding/jai-wasm
- tsoding/infinite-pong-jai-wasm64 @ d3319ec: https://github.com/tsoding/infinite-pong-jai-wasm64
- tsoding/jaibreak, tetris-jai, jaison, rope-jai, jai-ctags, twitch_irc: https://github.com/tsoding
- PixelRifts/Jai-Shader-Transpiler, onelivesleft/Metaprogram, kujukuju/MetaThreadSafe (GitHub)
- Smári McCarthy, "Four Years of Jai" (2024-12-02): https://smarimccarthy.is/posts/2024-12-02-four-years-of-jai/
- Earlier Jet research on Jai: `Docs/research/jai-vs-jet-syntax-briefing-2026-09-16.html`, `Docs/research/jai-vs-jet-metaprogramming-boundary-2026-09-16.html`, `Docs/audits/2026-10-07-ast-metaprogramming-report.md`

Strongest unverified assumption: field names from The Way to Jai's `Build_Options` dump and from auto-captions still match the current beta. Only the focus (2026-09) and infinite-pong (2024-11) build files confirm current spellings, and no Jai compiler was run.
