# Rust → Jet compiler parity ledger (2026-09-30)

Owner goal: before the Rust compiler freezes, the Jet compiler in `Compiler/`
must do everything the Rust compiler does (checker, TIR/MIR lowering,
codegen/emission, comptime evaluation, driver), so the freeze loses no
feature. Compiler policy (owner, 2026-09-30 evening): bug fixes land in both
compilers, new features that do not gate self-hosting land only in `Compiler/`,
and every Rust-only change is a parity gap to find, card and port.

This note records what was compared, how, and the gaps found. Plans and state
belong on Tower cards; the machine-readable gap list for filing them is
`~/.cache/jet-dev/p0/ParityAudit.json` (70 entries, ids P1…I3, used below).

## Evidence base

- Tree: master `118e7ac0c` plus the uncommitted main checkout as of
  2026-09-30. Uncommitted Rust: 273 files under `crates/` and `Source/`
  (+14,051 / −5,314) and 10 new files; uncommitted `Compiler/`: 144 files
  (+6,373 / −4,686) and 3 new files.
- Worker reports read: all 30 files in `~/.cache/jet-dev/p0/` and the 30
  closer/verify reports in `~/.cache/jet-dev/closer/`.
- Commits read: every commit since 2026-09-28 (43; 8 touch compiler code:
  `542012fe9`, `39b713f09`, `f5c00c314`, `de1e1da15`, `8d16171ec`,
  `23b44a407`, `c0c5493a9`, `118e7ac0c` is Jetpack only).
- Diagnostic-code census: a read-only script collected every quoted
  `"E####"`/`"L####"` literal in Rust compiler sources (`jet-lexer`,
  `jet-parser`, `jet-sema`, `jet-comptime`, `jet-codegen/src/Codegen`,
  `jet-driver`, `jet-foundation`, `Source/`; `#[cfg(test)]` modules and test
  files excluded) and in `Compiler/**/*.jet` (the generated
  `DiagnosticRows.jet` and `Registry/Diagnostics.jet` excluded), then joined
  them with the registry rows in `crates/jet-codegen/src/Prelude/Diagnostics.jet`.
- Structural comparison: file-level line counts per area, keyword searches in
  `Compiler/` for each Rust feature, and reading the Rust and Jet sites named
  in the tables.

Limits of the census: it compares codes, not behaviour. The Jet compiler
sometimes reports the same situation under another code (for example an
import cycle is `E0603` in JetDriver and `E0604` in Rust), and a code present
in both may still differ in coverage. Rows marked "unmeasured" need a
differential run before porting.

### Census results

| Measure | Count |
|---|---|
| Distinct codes emitted by the Rust compiler sources | 740 |
| Distinct codes emitted by `Compiler/` | 271 |
| Emitted by both | 267 |
| Emitted only by Rust (any registry status) | 473 |
| Emitted only by Rust, registry status `active` | 394 |
| … of those, emitted outside host-only `Source/` CLI code | 313 |
| Active lints emitted by Rust / by Jet | 58 / 4 (`L0101 L0102 L0303 L3102`) |
| Emitted only by Jet | 4 (`E2001 E3204 E3205 E3207`) |

The 62 active Rust-only codes that occur only in `Source/` (registry,
publish, prove replay, db, budgets, CLI flag parsing) belong to the host CLI,
not the compiler, and are not counted as gaps. The Rust parser also still
names 49 retired-status codes in its teaching table; those are listed under P2
as optional.

## Part 1 — Rust changes since 2026-09-28 with no `Compiler/` counterpart

Sources: worker reports (R), uncommitted diff (U), commits (C). "Mirrored"
means the report or diff shows a matching `Compiler/` change.

| Change | Source | Rust sites | Jet status | Gap |
|---|---|---|---|---|
| #3708 failure-set solve (`solve_inferred_failure`) and cross-module failure obligations | R RatDiscard, TierLeftovers #5; U | `Sema/mod.rs`, `Bundle/Pipeline/Completion.rs`, `CheckerInfer/fallible.rs` | none ("JetSema has no #3708 solve") | S1 |
| D-FAIL-INFER-UNION1 / D-ERR-CASES1 inferred failure unions | R RatFailUnion; U | new `Bundle/Pipeline/FailureUnion.rs` | none | S2 |
| D-DISCARD1: E0113 value-focused return mismatch | R RatDiscard | `CheckerCore/statements.rs` | not mirrored; JetSema does not return-check tails | S3 |
| D-DISCARD1: E0433 for inferred-infallible callees | R RatDiscard | `Effects.rs` | partial: only declared `Never!` callees | S4 |
| D-DBG1 `debug()` trace and E3405 | R RatDbg; U | `CheckerInfer/calls/direct_calls.rs`, `TIR/lower/expressions.rs` | registry row only | S5 |
| #3738 `copies: .Explicit` E0120 copy sites | R P0Diag | `CheckerItems.rs` | none ("JetSema copies policy port missing") | S19 |
| #3857 `assert_eq(x, None)`; #3858 L0528 on generated bodies | C `39b713f09` | `CheckerInfer/binary.rs`, `CheckerItems.rs`, `loop_lints.rs` | none (Jet requires identical arg types; no L0528) | S20 |
| #3740 plain-return ABI for `Never!` functions | R P0PlainReturn; U | `TIR/mod.rs`, `lower/functions.rs`, `mir.rs`, `tir_to_mir_expr.rs` | mirrored (#3936: JetCodegen `jet_codegen_plain_return` and call/`Try`/return/fn-value reconciliation) | — |
| #3713 journey origin, column, `jet run --json` | R P0Journey; U | `Outcome.rs`, `routes.rs`, `tir_to_mir_expr.rs`, `MIREval.rs`, JIT host, `RuntimeStop.js` | noteless hops mirrored; origin/column not | T2 |
| #3713 Effect-ABI Prelude rows never pure | R P0Journey | `MIROptimization.rs` | mirrored (`JetOptimizer/.../DeadValues.jet`) | — |
| #3856 Equatable fact → AOT `PartialEq` derive | C `542012fe9` | `tir_to_mir_types.rs`, `MIRRust.rs`, `routes.rs` | unaudited | T3 |
| #3855 bare `None` elements in `[T?]` literals | C `39b713f09` | `TIR/lower/expressions.rs` | unaudited | T3 |
| Cleanup live flags written false in the entry block | C `de1e1da15` | `TIR/mir.rs` | unaudited | T3 |
| `?? { }` fallback `err` gets its own cleanup frame | C `8d16171ec` | `tir_to_mir_expr.rs` | unaudited | T3 |
| Resource locals move into closures instead of cloning; DB begin/commit borrow | R TierLeftovers #3 | `lower/lambdas.rs`, `env.rs`, `routes.rs` | partial (`HandleMethods.jet`) | T3 |
| #3675 AOT assert helper shape; #3726 shift count width | C `f5c00c314` | `MIRRust.rs`, `Prelude/Core.rs` | unaudited in Jet Emit | EM2 |
| Sept-29 tier-parity fixes (interpreter E0956 gaps, AOT Int mapping, closure `:=` captured writes, recursive enum boxing, generator raw Stream) | C `23b44a407`, `c0c5493a9` | `MIREval.rs`, `MIRRust.rs`, TIR | never audited against Jet | T4 |
| MIREval task pause/resume/cancel arm | R TierLeftovers #1; U | `MIREval.rs` | none seen in JetEval | EV1 |
| #3862 package mechanism: transitive path deps, `package_run_member`, E0626 dependency summary, `bind_package_namespaces` (stash), D-MOD-CYCLE1 naming | R ModularizeCompiler, ModularizeCompiler2; U | `Loader.rs`, `Bundle/Pipeline.rs`, `CheckInner.rs`, `Names.rs` | E0626 registry row only | B1 |
| Impl-head alias use (false L0103) | R ModuleDefects2 | new `Bundle/Pipeline/ImplHeads.rs` | Jet has no L0103 at all | LN2 |
| #3718 package-default authority floor | R P0Diag | `jet-foundation/src/Authority.rs`, `jet-pkg-model/src/EffectBudget.rs` | none | D2 |
| D-NAME-SPLICE1 `$` splices in templates, MIR bridge and TIR | R RatSpliceReact; U | `Comptime/Template.rs`, `MirBridge.rs`, `lower/call_args.rs`, `Prelude/Derives.jet` | lexer teaching mirrored; no template expansion in Jet | C1 |
| #3662 `prep` fragments call Core source functions; literal-constant fast path | R PrepConstFix; U | `Registration/Items.rs`, `Comptime/Purity.rs` | unverified | C5 |
| #2517 S1/S1b: `CheckReads`, read audit, receipts, records, package rows | R IncrS1b, PersistCheckCache, IncrDesign; U | `CheckReads.rs`, `Source/CheckReceipt.rs`, `jet-store` | codec + identity (S5a) and receipt closure only | I1, I2, I3 |
| #3661 checker-speed changes (`Units.rs`, `blocks.rs`, `Context.rs`, `Purity.rs`) | U | as listed | Jet checker speed is its own card | not a feature gap |
| #3724 / #3722 CLI help order and `jet new` templates | R P0Cli | `jet-cli`, `Source/main.rs` | host CLI | not compiler scope |
| #3870 U64 word lists, JIT hosts; #3854 JIT list render | R CryptoU64; C `39b713f09` | `jet-rt`, `jet-jit` | host runtime/JIT | EM4 ruling |

Rust changes confirmed mirrored in `Compiler/` (no gap): D-OUTCOME-SHAPE1
three-state patterns E0392/E0310 (#3838), L0303 unreachable `else` (#3716),
`++`/`--` retirement (#3727), `&&`/`||` mix E0082 (#3725), unclosed-delimiter
E0083 (#3719), typed-literal whole value (#3739), E0393 typed binding
(#3743), D-TIME-INSTANT1 E0302, D-CRYPTO-ERRFAM1, D-INTERP-BUDGET1 fuel,
D-PAT-NAMED-NEST1 named patterns (parser and sema), D-OPT-LIFT1 optional
lifting, `pub NAME :: value` exports, loose-file `..` imports, scope-end
`close`, core error family Display (TierLeftovers #4), RecordCodec and
PackageIdentity byte format (S5a), `$` fact respelling teaching in the lexer.

Closer and verify reports (2026-09-29/30) record Core-library and tier
defects, not Rust-only compiler changes. The two compiler-relevant findings
(`core.compiler` facade has no loaded signature; `DBLease` exposes only
`close`) are missing in both compilers, so they are not parity gaps.

## Part 2 — structural parity by area

Sizes: S is under a day, M is a few days, L is a week or more. "Blocks
self-hosting" means the Jet compiler cannot check, build or run its own
`Compiler/` packages without the item; checks that only reject invalid
programs do not block.

Line counts (2026-09-30): Rust `jet-lexer` 3.3k, `jet-parser` 43.6k,
`jet-sema` 154.9k, `jet-comptime` 85.5k, `jet-codegen` 427.9k (Codegen 200k,
Prelude runtime 225k), `jet-driver` 20.8k, `jet-foundation` 145k, `jet-jit`
161k. Jet: JetLexer 2.7k, JetParser 8.9k, JetSema 57.3k, JetCodegen 30.2k,
JetEval 19.9k, JetOptimizer 6.2k, JetDriver 6.1k, JetFoundation 41.0k
(15.5k of it generated diagnostic rows).

### Lexer (`jet-lexer` → `JetLexer`)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Scanner, strings with nested interpolation, terminator insertion, `$` splice/fact teaching (E0388, E0003) | Same; `Terminators.jet` is already the policy owner the Rust adapter calls; nested interpolation spans rebased recursively | none found; no Rust-only lexer codes | — | n |

### Parser (`jet-parser` → `JetParser`)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| 38 grammar-rule diagnostics (visibility/#PubFile, #Context fields, parameter zones and labels, arm tables and ranges, #Off/#Meta/#Shield/#Persist, rest params, `#Import(c)`) | Parses the constructs; reports generic `E0003` or nothing | P1 | M | n |
| Foreign-syntax and retired-spelling teaching (14 active parse codes + 5 sema) | Few teaching rows | P2 | M | n |
| Formatter, 10.1k lines (`jet fmt`, fix edits) | none | P3 | L | n |

### Checker core (`CheckerCore`, `CheckerInfer`, `CheckerItems`, `Registration` → JetSema)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| #3708 failure-set solve | lambda-local failure inference only | S1 | L | n (y if Core drops explicit `Never!`) |
| Inferred failure unions | none | S2 | M | n |
| Tail value checked against written return; E0113 value wording | tails not return-checked (RatDiscard) | S3 | M | n |
| E0433 via inferred-infallible set | declared `Never!` only | S4 | S | n |
| `debug()` (E3405) | none | S5 | M | n |
| Definite assignment, `uninit` (E0420, E0423, E0424) | none | S6 | M | n |
| Typestate and protocols (State.rs 1.4k, Protocol.rs) | parse only | S7 | L | n |
| Distinct/range/checked-text rules | partial registration | S8 | M | n |
| Collection typing rules (E0501, E0502, E0506, E0507, E0964, E1108, E0364) | none | S9 | M | n |
| Call-shape and callable-value rules (10 codes) | none | S10 | M | n |
| Generic modules (GenericModules 4.3k, E0850–E0857) | parse only | S11 | L | n |
| Impl coherence, tags vs traits (E0902, E0908, E0909, E0731, E0732, E2711, E2401) | none | S12 | M | n |
| Display/Debug interpolation (E0914–E0916) | none | S13 | S | n |
| Marker validations (#Memo, #Inline(Always), #Deprecated, #MustUse, #Patchable, computed fields, validate{}, #Meta) | lowering/transform only; no checks | S14 | M | n |
| Item rules (E0035, E0106, E0221, E0332, E0350, E0361–E0365, E0148, E0380, E2421) | none | S15 | M | n |
| #Test scope members (E0613–E0618, L2901) | partial ScopeMembers | S16 | S | n |
| Task detach/freeze (E1103, E1106, E1113, E1114) | task groups, sendability | S17 | S | n |
| Special forms (Layout{}, swizzle, matmul, line stream, tables, UI, #Context types) | typing only | S18 | M | n |
| `copies: .Explicit` | none | S19 | M | n |
| 2026-09-30 fixes #3857, #3858 | none | S20 | S | n |
| Nested multi-slot payload coverage | single-slot only | S21 | S | n |
| Doctest E2901 | none | S22 | S | n |

### Core library typing (`CheckerCoreLib` → JetSema `Calls/*`, generated registries)

Core call signatures are generated into both compilers from the same Prelude
tables (`CoreCallRows.jet`, `CoreModuleExports.rs`), so signature coverage
tracks automatically.

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Unknown core module/item E1001/E1004 with retirement teaching; #NoPrelude E0429; E0608 | no such codes | CL1 | M | n |
| Serde declaration checks E2408/E2414/E2415 | codec reachability only | CL2 | S | n |

### Modules and packages (`Bundle`, `jet-driver` Loader → JetDriver, JetSema)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| #3862 packages: package namespaces, E0623–E0626, transitive path deps, ruling B entries | ported (#3908): JetDriver `jet_driver_bind_package_namespaces`, E0623/E0624/E0625, `use dep.[names]` binding, E0626 summary, root path reach (E1206); JetSema namespace visibility, member owner lookup, sibling impls. Sibling-aware L0103/L0104 waits on the liveness lints themselves (#3920) | B1 | L | **y** |
| Inline `package {}` header E1362/E1363 | manifest `package.jet` only | B2 | M | n |
| Module-graph diagnostics (E0604, E0606, E0607, E0610, E0612, E0619, L0619, E0620, E0621, E1002) | cycles/ambiguity reported as E0603/E0105 | B3 | M | n |
| Outputs validation E1321, named executables | one entry; differing entries rejected | B4 | M | n |
| Workspace members and Jetpack package kinds | none | B5 | M | n |

### Effects (`Effects.rs`, `Purity.rs`, `MemoryFacts.rs`, `UnsafeObligations.rs` → JetSema `Effects/Checks.jet`)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Effect rules E0711, E0742, E0743, E0750, E2960, E3402–E3404 | effect graph solve, Secret (E1264), E0433 | EF1 | L | n |
| Memory rights denial E0921 | FunctionMemory facts | EF2 | M | n |
| Unsafe obligations and gate ledger E3105–E3108, E3415 | none | EF3 | M | n |

### Ownership (`CheckerOwnership.rs` 7.6k → `Ownership/*.jet` 3.7k)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| #SingleUse E0140–E0143 | none | O1 | M | n |
| Pins, write windows, partial moves, static borrows (E0217–E0223) | core move/borrow/last-use rules | O2 | M | n |
| String-view lifetime E2307; #MustUse E0419 | none | O3 | S | n |

### Lints

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| 26 smell/style lints L0501–L0531, L2510 | none | LN1 | L | n |
| Liveness L0103–L0105 | none | LN2 | M | n |
| Arm-table lints L0301, L0302, L0513, L0514, L0517 | L0303 only | LN3 | M | n |
| Concurrency/compute lints L0202, L0206, L0207, L1101, L1141 | none | LN4 | M | n |
| API/edition lints and lint policy (L0601, L2001, L2401, L2421, L0510, E1293) | none | LN5 | M | n |

### Domain checkers (single-purpose `jet-sema` modules)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| #CLI derive (CheckerCli) | `core.args` builder arity only | SD1 | M | n |
| App convention routes (App.rs 2k), web partition | HTTP routing calls; web emission | SD2 | L | n |
| Devtools panels E1404–E1414 | none | SD3 | M | n |
| FFI, target surface, no-OS target facts (13 codes) | target machine selection, foreign type checks | SD4 | L | n |
| Performance budgets E2904/E2905 | none | SD5 | M | n |
| API freeze, schema, published-schema migration (3.2k) | migrations parse/lowering | SD6 | L | n |
| Hot swap, knowledge loss, cognitive complexity, resource schedule, env presets, guest artifacts, data plans | scheduling frames only | SD7 | L | n |

### Comptime (`jet-comptime` → JetDriver `BodyComptime`/`run_comptime`, JetEval, JetSema `Comptime/`)

The Rust compiler evaluates compile-time code through its own AST evaluator
plus a MIR bridge and Rust "Lite" ports of Core services; the Jet compiler
lowers comptime bodies to MIR and runs them in JetEval. `prep` constants,
reflection facts and the interpreter budget already work on the Jet side.

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Template expansion with `$` splices; user rule bodies; Prelude derive templates | lexer teaching; rule sites/signatures; separate AutoDerives path | C1 | L | n |
| embed_file/find, fetch, #Impure gate, net/vault refusals, Tier-2 effects, prep-if condition | none | C2 | M | n |
| Programmable builds (`fn build`, 17k + driver 8.6k) | none | C3 | L | n |
| Lite service ports, DataPipeline, Core pure parity | JetEval over Jet-source Core; coverage unmeasured | C4 | L | n |
| prep fragments calling Core functions; literal fast path | unverified | C5 | S | n |

### TIR/MIR lowering (`Codegen/TIR` 109k → JetCodegen `Codegen/` ~20k) and optimizer

`JetOptimizer` runs the same seven passes in the same order as
`MIROptimization.rs` (legality, unreachable blocks, CFG simplification, exact
constant folding, bounds-check elimination, dead pure values, canonical loop
facts); no optimizer capability gap was found.

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Plain-return ABI (#3740) | mirrored in JetCodegen (#3936) | — | M | n |
| Journey origin/column/JSON (#3713) | noteless hop frames | T2 | M | n |
| 2026-09-28..30 lowering fixes | unaudited, one partial | T3 | M | n |
| Sept-29 tier-parity fixes | unaudited | T4 | M | n |

### Emission (`MIRRust`, `MIRWeb`, JIT → JetCodegen `Emit/`)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Full artifact: `fn main`, env init, jobs, hardware host, Prelude embedding, web ABI shims, crate packaging | `Emit/Entry.jet` (native/no-OS/Wasm entry, CLI spec/decode/commands, jobs + queue + dispatch, native and Wasm exports), `Emit/Hardware.jet` (facts statics, register helpers, interrupt runtime, setups), `Emit/Harness.jet` (test/property/fuzz wrappers, output checks, coverage branch probes and registration, harness runner and main) and `Emit/Linkage.jet` (link closure, callback trampolines, period anchors). The Jet emitter produces the user-item suffix; the host prepends `MIRRust::emit_mir_runtime_text` (same build-fact runtime/Core text, so `jet_store::runtime::prepare` links the one cached `jet_runtime` rlib) in `Compiler/Bootstrap/Runner.rs`. Missing: history strategies (fail closed), model adapters, native fix-and-continue dispatch (debug-linemap only); Driver artifact plan derives CLI only for `#CLI` record entries (no parameter-list CLI, exports, harness) | EM1 | M | **y** |
| MIRRust 27k lines of operation/type emission | Emit ~10k lines; all 62 `MirOperation` variants and 44/44 semantic ops dispatched; RequireStop, OverflowOption, HTTP router, CoreClosureCall, text/binary pattern match and Cursor/Reader `take_pattern` use the MIRRust adapters (`Emit/Semantic.jet`). Missing: vector/acceleration block emission (performance only: loops take the scalar path), history capture, partial-move parity audit | EM2 | L | **y** |
| MIRWeb 8.6k | Emit/Web ~3.7k | EM3 | L | n |
| Cranelift JIT (161k) | none; the Jet driver hands MIR to the host JIT | EM4 (owner ruling) | L | n |

### Evaluation (`MIREval` 31.5k → JetEval 19.9k)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Runtime stops E3001/E3012, task control, journey origin, TTL/range values, U64 display, closed E0956 routes | evaluator, fuel budget, E3010; comptime for the Jet compiler | EV1 | L | n |
| `jet dev` step budget, `jet debug`, REPL services | none | EV2 | M | n |

### Driver (`jet-driver` → JetDriver)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Library/plugin export, compiler-extension hooks, migrations | artifact plan, migration lowering | D1 | L | n |
| Package-default authority floor | authority block parse | D2 | S | n |

### Incremental checking (#2517: `jet-store`, `Source/CheckReceipt.rs` → JetDriver `Identity.jet`, JetFoundation `Record/`)

| Rust has | Jet has | Gap | Size | Blocks self-hosting |
|---|---|---|---|---|
| Record codec, package identity, records store, package fingerprints | codec and identity with byte conformance (S5a) | I1 (interface records, red/green) | L | n |
| Sealed check reads with audit and fault injection | reads confined to the authorized snapshot by construction | I2 | S | n |
| Build/check receipt replay, explain-build package rows | receipt closure/answer helpers; `record_store = None` until #3874 | I3 | M | n |

## Summary

70 gaps: parser 3, checker core 22, core library 2, modules/packages 5,
effects 3, ownership 3, lints 5, domain checkers 7, comptime 5, lowering 4,
emission 4, evaluation 2, driver 2, incremental 3; lexer and optimizer none.
By size: 21 L, 37 M, 12 S. Three block self-hosting: B1 (packages), EM1
(artifact assembly) and EM2 (emission coverage). S1 becomes blocking if the
#3708 plan removes Core's explicit `Never!` markers.

Largest gaps by the amount of Rust behaviour to port:

1. EV1 — MIREval features absent from JetEval (31.5k vs 19.9k lines).
2. EM2 — MIRRust emission coverage (27k vs ~6.7k lines).
3. C4 — comptime Core-call coverage (Lite ports and DataPipeline, ~25k lines; unmeasured).
4. C3 — programmable builds (~17k in `Comptime/Build` plus 8.6k driver).
5. P3 — formatter (10.1k lines).
6. EM3 — web emission (8.6k vs ~3.7k lines).
7. EM1 — artifact assembly (self-hosting blocker).
8. B1 — package mechanism (self-hosting blocker).
9. S11 — generic modules (4.3k lines).
10. LN1 — smell/style lint family (26 lints).

EM4 (the Cranelift JIT, 161k lines) is larger than all of these but is host
code that consumes Jet-produced MIR in the current bootstrap design; it needs
an owner ruling before it counts as a port.

## Open questions for the owner

- Does the host keep the JIT, runtime (`jet-rt`, Prelude) and CLI tooling
  after the freeze, with the Jet compiler supplying MIR (EM4, EV2, host
  `Source/` codes)?
- Must `jet fmt` (P3) and the dev-loop analyses (SD7) move before the freeze?
- Should the 49 retired-status teaching rows the Rust parser still lists be
  ported or dropped (P2)?
