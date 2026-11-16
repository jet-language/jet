# Architecture review, 2026-10-22

This review covers the Jet compiler, runtime, Core library, repository and
contributor workflow at dev `45bca333d` ("#5073 JetIR: module storage cells"),
with seed `a0_010`. Paths and line numbers refer to that commit. The Prelude
sources cited under `crates/jet-codegen/src/Prelude` were being moved to
`Compiler/Prelude` while this review was written. After that move, those paths
change and the findings stay the same.

Method. Seven helper censuses (counts made with `git ls-files`, awk and node
over the tree) were each checked by the reviewer against the cited source
lines. The reviewer ran four measurements:

- the seed runtime pack provenance;
- the route census;
- a traced `jet check` of `Compiler/JetLexer`;
- the `.copy()` and `panic(` counts.

Measurements from other runs are attributed to their Tower card or audit.
Statements marked [INFERENCE] were not measured.

Each finding gives:

- what was found and the evidence;
- the impact on maintainability, reliability or performance;
- a direction;
- a rough size (S, M or L);
- how it relates to cutover A (MIR, LIR and the Jet backend replaced by
  JetIR), cutover B (TIR replaced by AST plus fact tables), and Rust removal
  (#4717).

## Correction to the review's starting premise

The shipped runtime pack is not built from the Rust `CAbi*.rs` files. The
seed's `jet-runtime/<hash>/provenance.json` records `backend "jet-native"`,
`rustObjects 0` and `llvmObjects 0`. It ships `jet_runtime.pack.o` (6.9 MB)
and `jet_runtime_jet.o` (12.8 MB), with 2841 exported symbols. The pack is
compiled from `Compiler/JetRuntime` through MIR and LIR by
`x64_runtime_library` (`Compiler/JetBackend/Source/Image/RuntimeLibrary.jet:203`,
called from `Compiler/JetCli/Source/Cli/Request.jet:126`).

`crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs` (32,447 lines,
4,203 `extern "C"` functions) is a Rust-tier mirror. The overlap between the
two:

| Set | `jet_rt_*` names |
|---|---|
| Rust `CAbi*.rs` | 2728 |
| Pack exports | 1948 |
| In both | 917 |

The transition gates are listed in
`Compiler/JetRuntime/rust-transition-gates.tsv` (260 lines).

## Size of the tree

All counts are lines from `git ls-files`.

| Area | Lines |
|---|---|
| Jet, total | 1,301,319 |
| JetRuntime | 150,431 |
| JetSema | 114,181 |
| JetBackend | 95,531 |
| JetFoundation | 73,006 |
| JetCodegen | 63,034 |
| JetCli | 43,866 |
| JetEval | 28,910 |
| JetIR | 22,418 |
| JetOptimizer | 13,430 |
| Core (own Jet) | about 68,500 |
| `Core/Vendor/Sqlite3/sqlite3.c` | 255,636 |
| Rust, total | about 1.94 million |
| Rust under `crates/` | 1.435 million |
| Rust under `Source/` | 151,000 |
| Rust under `tests/` | 324,000 |

Thirty Compiler `.jet` files exceed 3,000 lines and six exceed 10,000. The
largest are:

- `Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet`: 16,035
- `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`: 13,794
- `Compiler/JetRuntime/Source/UnicodeText1.jet`: 13,389
- `Compiler/JetEval/Source/Evaluator.jet`: 10,211

## Top 15, ranked by value for cost

| # | Finding | Size | Relation to cutovers |
|---|---|---|---|
| 1 | Comptime should run the runtime's own IR, not hand-written routes (R1) | M spike | Shapes A3 |
| 2 | The runtime allocator does not check frees (Rel1) | S–M | None |
| 3 | `jet check` runs the MIR backend (P1) | S | After A6 |
| 4 | `jet check` needs twice the memory of `jet build`, #5100 (P2) | M | None |
| 5 | Map is a sorted list, #5096 (P3) | M–L | B1/B2 for the compiler side |
| 6 | Read-only bindings copy whole rows, #5097 (P4) | M | None |
| 7 | The serial `sema.finalize` tail (P5) | M | None |
| 8 | Route meaning lives in five tables (R2) | M | Partly A |
| 9 | Generators have no single freshness gate; some still read Rust (Repo1) | M | Before #4717 |
| 10 | The Unicode tables are unrolled code, 48% of JetRuntime (P6) | M | None |
| 11 | Packages outside the build rot (L3) | M | None |
| 12 | Hosted CI never runs the Jet compiler (Repo2) | M | Owner ballot |
| 13 | Public docs describe the Rust backend (Repo3) | S | Spec text after A/B |
| 14 | Diagnostic registry drift and missing UI snapshots (Rel2) | S–M | None |
| 15 | Divergent private copies of Core and runtime helpers (R3) | M | None |

Some work is already owned and is not ranked here:

- parallel `ir.build` (#5094);
- B1/B2 interned names;
- the A and B deletions;
- the Prelude move out of `crates/`.

## Area 1: Redundancy (I8, I9)

### R1. Runtime meaning is implemented four times

Core meaning has four executors, each written separately:

- the native JetRuntime (Jet);
- the JetEval MIR evaluator (`Compiler/JetEval/Source/Evaluator.jet` and
  `Routes/*.jet`, 176 route functions);
- the JetEval IR machine (`Compiler/JetEval/Source/IR/Machine.jet` 866 lines,
  `IR/Runtime.jet` 319, `IR/Values.jet` 226);
- the Rust host evaluator (`crates/jet-codegen/src/Codegen/MIREval.rs`,
  34,063 lines).

The IR machine is a sandbox. Memory is regions addressed as
`region<<32|offset`, and Strings are text handles. It re-implements routes
by name in `IR/Runtime.jet:12-102`:

- `jet_rt_float_to_string` is formatted through Jet interpolation, not the
  runtime's formatter.
- Float32 display is marked unsupported.

The machine routes 57 symbols. The JetIR builder emits 270, so 219 are
builder-only. Of those 219:

- 111 are already runtime pack exports with Jet bodies in JetRuntime.
- About 108 are builder glue, such as `jet_iter_*` and `jet_fmt_grouped`.

Impact. Writing the remaining routes by hand makes a third Jet copy of
meaning that JetRuntime already defines. Each copy can drift: the float
display above already differs. Reliability and maintainability.

Direction. Once A2 builds the runtime pack through JetIR, the IR machine
should interpret the runtime's own IR for routes that have Jet bodies. It
should implement only leaf primitives such as allocation, raw memory, OS
calls and assembly. A spike must first settle one question: native String
and record layouts would have to run under the machine's region memory model
[INFERENCE: the two models differ today].

Size: an M spike, then M. This changes the shape of A3. A deletes the MIR
evaluator, and #4717 deletes `MIREval.rs`.

### R2. Route meaning lives in five tables

The same Core route set is listed in five places:

- `Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet`: 16,035 lines and
  1,722 `LowerRoute` mentions. Its header (`:1-4`) says it is authored
  source and the routes' only home, and that every row table and its
  KEYS/KEY_ROWS index must be kept "sorted by hand". The generator that once
  wrote it retired in `13bff4d7c`.
- `Compiler/JetEval/Source/Routes/*.jet`: 176 functions.
- `Compiler/JetIR/Source/BuildRoutes.jet`: 75 functions.
- The IR machine's route switch.
- `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet`. Its header
  (`:4`) names `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs` as
  an input.

JetIR imports the route tables from JetBackend. Its `use` lines name
`lower_runtime_route`, `LOWER_GENERATED_RECORDS`, `LOWER_RECORD_SCHEMAS` and
`runtime_abi_atom`. PLAN-A moves these into JetIR when JetBackend is deleted.

Impact. A route added to one table is silently missing from the others, and
a 16k-line table sorted by hand is a source of merge conflicts.
Maintainability and reliability.

Direction. Make one route registry, derived from the runtime's `#Export(c)`
declarations and Core's declarations, and have the builder, the machine and
the checker read it. The registry should be data with a generated sorted
index, not a hand-kept order.

Size: M. A removes the JetBackend and JetEval copies but relocates
RuntimeRoutes. The JetFoundation registry is not covered by any plan.

### R3. Divergent private copies of Core and runtime helpers

- `ascii_lower` is defined 13 times in Core: Email, Encoding/Ini,
  Http/Client, Http/Http, Log, Net/Mime, Net/Net, Net/Tls, Net/Url, Net/Ws,
  Sys, Term and `Text/Runtime/KernelUnicodeString.jet:251`. The copies
  differ: some declare `Never!` and some do not, and some convert with
  `Int.from_u8` and some do not. `KernelUnicodeString.jet:250` explains why
  the copies exist: the private-module policy "does not add a public
  core.text spelling".
- Hex digit helpers are defined at least eight times. Examples:
  `Compiler/JetRuntime/Source/CompilerNeededJSON.jet:46`,
  `CompilerNeededURL.jet:39`, `Comptime.jet:225`, `IpAddrShow.jet:19`,
  `Sentry.jet:366`, `Test.jet:305`, and
  `Compiler/JetEval/Source/Routes/Encoding.jet:42,59`.
- UTF-8 validity loops are copied into six runtime modules beside the
  canonical `TextBytes.jet:26` (`text_bytes_utf8_error`): `CodecEnv.jet:49`,
  `Scan.jet:58`, `Test.jet:182`, `VaultCodec.jet:251` and
  `HostStartupOutputFacts.jet:5`.
- JSON escaping exists in four runtime modules, and their escape sets
  differ:
  - `RouteALog.jet:198` escapes only quote, backslash, LF, CR and TAB.
  - `Observe.jet:102` also replaces control characters.
- Checked integer arithmetic has five copies with different widths:
  - `Compiler/JetFoundation/Source/Target/Layout.jet:927,933`: u64;
  - `Compiler/JetOptimizer/Source/Transforms/ScalarAndCfg.jet:696-712`: i64;
  - `Compiler/JetSema/Source/Sema/BudgetSpecs.jet:871`: u128;
  - `Compiler/JetRuntime/Source/Duration.jet:38`;
  - Rust `MathLibPure.rs`.
- The helper census found 455 Core function names defined more than once.

Impact. A fix to one copy does not reach the others, and the JSON escape
sets already differ. Maintainability and reliability.

Direction. Give Core and the runtime one internal home for byte, text and
number primitives that other Core modules can import without making them
public API. Add a census that flags new same-name private helpers. A public
`core.text` ASCII case API is a separate decision for the owner.

Size: M. No plan covers this; A only removes the optimizer and JetEval
copies.

### R4. Member-name lists are typed out five times

The same Core member-name lists appear in:

- `Compiler/JetCli/Source/Cli/StructureRegistry.jet:82-86`;
- `Compiler/JetCodegen/Source/Emit/Operations.jet:3027-3031`;
- `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet:2125-2129`;
- `Registry/CoreSignatures.jet:405-408`;
- `Registry/CoreSourceOwned.jet:74-77`.

Adding a Core member means editing all five. Size: S. A removes only the
JetCodegen copy.

## Area 2: Layering and dependencies

The production package graph has 451 edges and no cycles (Tarjan over the
graph built from `use` lines).

### L1. JetFoundation holds the whole IR vocabulary

`Compiler/JetFoundation/Source` contains `AST/`, `TIR/`, `MIR/`,
`Registry/`, `Types/` and `Diagnostics/` alongside the utilities. 21
packages depend on it:

| Edge | `use` lines |
|---|---|
| JetSema → JetFoundation | 140 |
| JetBackend → JetFoundation | 74 |
| JetCodegen → JetFoundation | 59 |

Impact. Any change to an IR type touches the base layer of every package.
Maintainability. Direction: B moves the TIR and A moves the MIR. After both,
split the AST and the Registry into their own packages so that the
foundation holds only utilities. Size: L; AST and Registry ownership are not
in either plan.

### L2. JetIR depends upward on the legacy back end

`jet_ir` imports from:

- `jet_backend`: 10 `use` lines, for example `lower_float_constant_bits`,
  `lower_runtime_route` and `LowerRecordSchema`;
- `jet_codegen`: 7 lines, for example
  `jet_codegen_failure_carrier_for_type` and
  `JET_CODEGEN_COMPILER_OWNED_CORE_RECORDS`.

The target IR therefore cannot be built or tested without the packages it
replaces. PLAN-A relocates these helpers when JetBackend is deleted, because
moving them earlier would create a cycle. Size: S once A lands.

### L3. Packages outside the build rot

`Compiler/package.jet` lists 13 members. The tool packages beside them
(JetLSP, JetDevserver, JetDebug, JetSemIndex, JetCanvas, JetRepl/Console)
are not members. Per common.md's per-member check baselines, their `jet
check` error counts are:

| Package | Errors |
|---|---|
| JetDevserver | 1633 |
| JetDebug | 184 |
| JetSemIndex | 7 |

Several files import packages that no manifest declares:

- `Compiler/JetRepl/Console/Repl/TurnFacts02.jet:16-17` uses `jet_loader`
  and `jet_comptime`. The same applies to `Console/Notebook/Eval.jet:2,8`,
  `Console/Notebook/Kernel.jet:23,25` and `Console/Sql.jet:3,5`.
- `Compiler/JetDevserver/Source/Canvas/EditActions02.jet:5` uses
  `jet_fixer`.
- `Compiler/JetLSP/Source/LSP/Hover.jet:5` uses `jetpack_provider`.

The files call these imports "pending" ports; the source is
`Hover.jet:6-8`. `Compiler/JetCli/Commands/package.jet:4-6` declares only
`jet_cli`, but its sources import five more packages: `jet_driver`,
`jet_foundation`, `jet_pkg_model`, `jetpack_foundation` and
`jetpack_trust`. Whether nested packages inherit their parent's
dependencies was not checked [INFERENCE].

JetRuntime itself checks with 3322 expected errors (E0107 and E0102 from
the runtime build mode). A real regression in the runtime package is
therefore not visible in `jet check`.

Impact. About 60k lines of tool code have no gate. Reliability.

Direction:

- Make each tool package a gated member at zero errors, or delete it.
- Give the runtime package a check mode with its build-mode facts, so that
  its baseline is zero.

Size: M. Not covered by any plan; the Rust tool crates go with #4717.

### L4. Layout is computed in three places

- Sema: `TargetLayoutEngine`, used at
  `Compiler/JetSema/Source/Sema/Registration/Checked.jet:3686` and
  `Sema/Comptime/Reflection.jet:620,671`.
- The LIR back end: `lower_repr_layout`
  (`Compiler/JetBackend/Source/Lower/Lower.jet:1604`) and `lir_layout_*`
  (`LIR/LIR.jet:234-254`).
- JetIR: `ir_struct_size` and `ir_enum_box_size`
  (`Compiler/JetIR/Source/BuildTypes.jet:1241,2286`).

Impact. Reflection's `LayoutInfo` can disagree with emitted code.
Reliability. Direction: after A, JetIR's type rows should be the only
layout, and sema reflection should read them or share one function. Size: M.
A removes the LIR copy; no plan names the sema copy.

### L5. The driver and CLI fan out widely

| Package | Fan-out (packages) | Fan-in (packages) |
|---|---|---|
| `jet_cli` | 16 | |
| `jet_devserver` | 13 | |
| `jet_driver` | 11 | 7 |

`jet_lsp` depends on `jet_devserver` (11 `use` lines). Tools rebuild with
the driver. Size: M; no plan covers this.

## Area 3: Data layout, memory and CPU

### P1. `jet check` runs the MIR back end

The reviewer ran a traced `jet check --threads 8 Compiler/JetLexer` (3.9k
lines) with seed `a0_010`. It took 14.06 s wall at 232% CPU, with a maximum
RSS of 3,173,140 KB. Nearly all spans ran on one thread. The top-level
spans:

| Span | Seconds |
|---|---|
| `sources` | 0.18 |
| `load` (113 + 16 + 37 files) | 1.53 |
| `register` | 0.51 |
| `sema.check` | 0.33 |
| `comptime` | 4.37 |
| `sema.finalize` | 2.12 |
| `driver.prelower.merge` | 0.23 |
| `lower` | 1.67 |
| `assemble` | 0.43 |
| `optimize` (lint 0.57, legality 0.24) | 1.35 |
| `driver.release` | 0.12 |

Only 0.52 s of `comptime` falls under a child span.

The `optimize.lint` pass is the MIR verifier. Its failures are internal
compiler errors, not user diagnostics
(`Compiler/JetOptimizer/Source/Verification/Lint.jet:1-9`). So about 3.45 s
(25%) of a check is back-end work that cannot produce a user report. A
separate run on JetFoundation took 12.57 s and 3.29 GB.

PLAN-A's A6 lists doc, fmt, LSP, test and queries as stopping at the
lowering inputs. It does not list `check`.

Impact. Every contributor check, and every agent check, pays for lowering.
Performance. Direction: `check` should stop after `sema.finalize`. The IR
verifier belongs to build and test. Separately, trace the 3.85 s of
`comptime` that has no child span. Size: S, once A6 exists.

### P2. `jet check` needs twice the memory of `jet build` (#5100)

Card #5100 records 20.3 GB for check against 9.3 GB for build. It has no
owner.

Impact. Check memory sets the parallelism of the proofq queue and of
contributors' machines. Performance. Size: M. Not covered by A or B; P1
removes part of it.

### P3. Map is a sorted list (#5096)

`Compiler/JetIR/Source/BuildMaps.jet:1-6` lowers Map as a sorted array, so
insertion costs O(n). Sorted iteration order is ratified
(`Docs/spec/syntax-decisions.md:4364-4367`, D-DATATREE-ORDER1: "ordinary
Maps stay sorted").

The compiler uses String-keyed maps heavily:

- 1,900 `[String:` occurrences in `Compiler/*/Source` (reviewer's count);
- 674 of them in JetSema (helper census, which also counts literals).

Keys are often composite strings, for example
`Compiler/JetBackend/Source/Lower/Mono.jet:913,932` (`"key:…"`,
`"{trait id}:{method name}"`) and `Lower/Lower.jet:255`
(`LowerTypeCache.sizes: [String:LowerSize]`).

Impact. Quadratic insertion and per-lookup string hashing and copying.
Performance. Direction:

- Keep the ratified ordering and change the representation: a B-tree, or a
  hash index over an ordered entry array.
- Inside the compiler, move hot tables to dense integer ids (B1/B2).

An unordered map type would be new public API and needs the owner. Size:
M–L. B1/B2 cover only the compiler side.

### P4. Read-only bindings copy whole rows (#5097)

Explicit `.copy()` calls, counted by the reviewer, excluding comments:

| Package | `.copy()` calls |
|---|---|
| JetSema | 807 |
| JetBackend | 546 |
| JetCodegen | 337 |
| JetIR | 141 |
| JetDriver | 114 |
| JetParser | 75 |

#5097 makes read-only place bindings borrow. That removes implicit copies,
which this count does not include. Measuring the change after #5097 lands
needs the profile's allocation counts, not a recount of `.copy()`. Size: M.

### P5. The serial `sema.finalize` tail

After the body checks, several whole-program passes run on one thread over
String-keyed maps:

- `sema_failure_solve` (`Compiler/JetSema/Source/Sema/CheckProgram.jet:6753`,
  `address_taken: [String:Bool]`);
- `sema_failure_recheck_modules` (`:7310`);
- the obligation settlement in `sema_finalize_graph` (`:7434`, `:7502`).

Main's s3 profile on card 5094 shows `sema.finalize` at 23.2 s of 78 s.
About 10 s of that is not body checking, according to the helper's
breakdown in `evidence/area3-dod.md`.

Impact. A serial floor for every build and check. Performance. Direction:
key these passes by function ids over dense arrays, and run the per-module
rechecks on the compiler threads. Size: M; not covered by B, which changes
storage but not this scheduling.

### P6. The Unicode tables are unrolled code

`Compiler/JetRuntime/Source/Unicode*.jet` holds 68,117 of the 141,421 lines
in JetRuntime's Source (48%). `CompilerNeededText*.jet` adds 5,551 more.
`Tools/agent/gen-unicode-tables.mjs` emits each lookup as a binary search
unrolled into nested `if { (cp < I64{…}) -> … }` code; see
`UnicodeText1.jet:10-30`, where 8,920 of 13,389 lines are branch lines.
Normalization is split across 33 shards of about 787 lines each.

Impact. The runtime pack's compile time and memory scale with these lines,
and every table change rebuilds and re-reviews code. Performance and
maintainability.

Direction. Emit read-only data arrays (a two-stage table or a sorted range
array) and one shared lookup function. If the IR cannot place static
read-only data without allocating, that is the prerequisite to add.

Size: M. No plan covers this.

### P7. Type interning and handoffs use strings and whole rows

- IR type interning keys on strings (`Compiler/JetIR/Source/IR.jet:269-277`).
- `ir_enum_box_size` builds `"{row}|{identity}"` keys and searches them for
  `"SharedGuard<"` (`BuildTypes.jet:2288-2289`).

Direction: intern on structural ids. Size: M. Partly in #5073's IR work.

## Area 4: Reliability

### Rel1. The runtime allocator does not check frees

`jet_rt_free` (`Compiler/JetRuntime/Source/Alloc.jet:762-773`) frees
unconditionally:

- `alloc_untrack` returns 0 both when tracking is off and when a pointer is
  unknown.
- `alloc_free_validated` trusts the header words at pointer−16 and
  pointer−8.
- Its unsafe reasons state that "the lowering frees exactly once".

Nothing detects a double free or a foreign pointer.

Impact. A lowering ownership bug becomes silent heap corruption that
surfaces far from the cause, as in the open card #5095. Reliability.

Direction. Add a checked allocator mode for the self-host chain and the
goldens:

- a header canary;
- poisoning of freed blocks;
- a trap on double free or an unknown pointer, naming the call site.

Size: S–M. No plan covers this.

### Rel2. Diagnostic registry drift and missing UI snapshots

`Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet` has 862 rows:

- 21 have the placeholder why "The registered jet rule applies here".
- 313 have a why identical to the meaning.

The helper census in `evidence/area4-reliability.md` compares the Prelude's
`Diagnostics.jet` (989 rows) with the Jet registry (774 codes):

- 89 active codes exist only in the Prelude.
- 283 registered codes have no `tests/ui` snapshot.

AGENTS.md I4 requires a snapshot for every diagnostic.

Impact. Users see placeholder text, and codes ship untested. Reliability.
Direction: one registry, a lint that rejects placeholder and duplicate
text, and the snapshot gap closed or the codes retired. Size: S to
classify, M to fix. Not in any plan.

### Rel3. Keep-going is a second control flow in every phase

`JET_KEEP_GOING` is the compiler-internal all-phases mode
(`Compiler/JetDriver/Source/Driver/Identity.jet:640-643`). Code paths that
branch on it:

- sema: `CheckProgram.jet:2521`, `Registration/Items.jet:669`;
- comptime: `Pipeline.jet:3534,3885,3909,3985`;
- lowering: `Lower/Lower.jet:481,866,999`, `Lower/Workers.jet:25,256`;
- optimization: `Opt/Opt.jet:93,352`;
- Rust emission: `Emit/Program.jet:398`.

Normal mode stops at the first failed unit; keep-going traps the unit and
continues. The thread schedules also differ: with keep-going the calling
thread does not claim work (`Workers.jet:256-259`, `Opt.jet:352-355`).

Impact. Each phase has two failure paths to test, and the default path does
not exercise the collector. Maintainability and reliability.

Direction. Use one failure-capture path per phase, the task join with a
typed trap. Normal mode then stops after the phase that reported. Size: M.
A deletes the LIR and Rust-emit copies.

### Rel4. Determinism has narrow coverage

Determinism is tested on 6 examples plus the s2 == s3 fixed point. Parallel
`ir.build` (#5094) and parallel finalize work add new ordering risk.
Direction: compare the compiler's own build byte for byte at `--threads 1`
and `--threads N`. Size: S.

### Rel5. Panics in user-facing packages

`panic("` appears 1,686 times in `Compiler/*/Source`, of which 881 are in
JetCodegen and 242 in JetBackend (reviewer's count). The helper
classification found 137 panics that are not invariant checks in packages
that serve user requests. Direction: those become diagnostics, under I6.
Size: M; A deletes the JetCodegen share.

## Area 5: Runtime and Core

### RC1. The Core↔runtime C-ABI boundary is left over from the Rust design

The pack exports 2841 symbols. About 1,347 are handle or record shims that
marshal values between Core's Jet types and C-ABI carriers, per the
helper's route census in `evidence/area5-routes.tsv`. Both sides are now
Jet.

Impact. Every Core runtime call crosses a marshalling layer that exists
because the runtime used to be Rust, and the route tables of R2 exist to
describe that layer. Performance and maintainability.

Direction. Analyze which routes still need a C symbol (FFI, assembly, OS
entry) and which could be ordinary Jet calls across packages. I9 and
D-EXEC1's single shared runtime favor fewer boundaries. Size: L. This
changes product architecture and needs the owner.

### RC2. Compiler sources of truth live in, or read from, Rust

- The Prelude sources `Core.jet`, `Diagnostics.jet`, `Effects.jet` and
  `CoreSourceParts.tsv` live in `crates/jet-codegen/src/Prelude`. They are
  being moved to `Compiler/Prelude`.
- `Tools/agent/gen-core-tables.mjs:29-39,432-436` reads
  `crates/jet-foundation/src/Effects.rs`, `BuildEffects.rs`,
  `Collections.rs` and `Syntax/*.rs` constants.
- `gen-unicode-tables.mjs:884-885` uses Rust tables as its oracle.
- `gen-runtime-host-carriers.mjs:424` writes `CAbiHost.rs`.
- `Compiler/JetFoundation/Source/Registry/RegistrationTruths.jet:16-19`
  points at Rust homes and `tests/*.rs`.

Impact. Deleting `crates/` breaks these generators and registries.
Reliability. Direction: re-home each input in Jet or TSV before #4717.
Size: M. These blockers are not in PLAN-A or PLAN-B.

### RC3. Core size and vendoring

Core's own Jet is about 68.5k lines. `Core/Time` has 607 files and
`Core/Encoding` 13,210 lines. `Core/Vendor/Sqlite3` (269,765 lines) is
third-party C. This is informational: the size of Core is not a defect.
The vendored C should stay outside line-count and audit gates.

## Area 6: Repository and process

### Repo1. Generators have no single freshness gate

The helper census in `evidence/area6-repo-process.md` counts 27
generators. About 8 have no `--check` mode, and the Node and Jet ports of
the runtime generators coexist (`RuntimeGenerators.jet`, 2,153 lines,
added in `4a84a5a38`). Card #5099 records the lander's RuntimeGenerators
step failing with E2105 on every landing, so generated tables are not
verified at landing. RC2 lists the generators that read Rust.

Impact. Generated files drift silently. Reliability. Direction: one
generator registry, each generator with `--check`, run as a lander gate,
and one implementation per generator. Size: M.

### Repo2. Hosted CI never runs the Jet compiler

`.github/workflows/ci.yml:36` runs
`cargo check --workspace --all-targets --locked`. No hosted job runs `jet
check`, `jet build` or the tests on the Jet compiler, which is the product.

Impact. Hosted CI validates code that #4717 removes. Reliability.
Direction: a seed-based `jet check` per member against zero-error
baselines. A per-member check fits in about 3.3 GB, per P1. How CI obtains
the seed is an owner decision. Size: M.

### Repo3. Public docs describe the Rust backend

- `README.md:149-150` says "Native builds emit Rust and compile it with
  rustc and LLVM", and `:158` says the Rust-emission and rustc/LLVM
  backends stay.
- `Docs/spec/architecture.md:257-260` says native release does not invoke
  rustc. `:1655-1656` says both profiles "invoke the transitional rustc AOT
  build".
- AGENTS.md's layout table describes `Compiler/` as a staged port.
- The helper count found 310 mentions of TIR, MIR or rustc in the spec.
- `Docs/audits/2026-10-03-compiler-scaling-audit.md` has a header and no
  findings.

Impact. New contributors and agents start from false premises.
Maintainability.

Direction:

- Correct the README and AGENTS.md now.
- Rewrite the spec text about TIR, MIR and LIR as A and B delete them.
- Complete or remove the empty audit.

Size: S, then M with the cutovers.

## Area 7: Contributor build and test speed

Measured costs:

| Run | Wall time | Memory | Source |
|---|---|---|---|
| Self-host s1 | 2:17 to 2:56 | about 17 GB | Helper runs in `evidence/area7-build-speed.md` |
| s2 | about 1:21 | 6 to 6.7 GB | Same |
| kseed s1 | 1:30 | 9.3 GB | Same |
| Golden suite | 11,297 s | 13 timeouts | `Docs/audits/gate-timing-2026-10-08.md` |
| `jet check` of one small member | 14 s | 3.2 GB | P1 |
| s3 profile | 78 s | 12.8 GB | Card 5094 |

The s3 profile breaks down as:

| Phase | Seconds |
|---|---|
| `ir.build` | 35.7 (single-threaded) |
| `sema.finalize` | 23.2 |
| `sema.bodies` | 13.3 |
| `comptime` | 4.7 |

The largest levers are the following findings:

- P1, the back end inside `check`;
- #5094, parallel `ir.build`;
- P5, the finalize tail;
- P2, check memory;
- P6, the Unicode tables, for runtime pack builds.

The golden suite's runtime is dominated by per-test compiles. Sharing one
loaded Core and runtime across a test batch is the structural fix
[INFERENCE: not measured here].
