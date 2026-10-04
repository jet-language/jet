# Fast compiler architecture for the self-hosted Jet compiler (2026-09-30)

Owner ruling, 2026-09-30: design the self-hosted compiler (`Compiler/`) for
speed from the ground up. The bar is Jai-class throughput (about 300k lines
recompiled in about one second) and Go/Zig parity on check, build, and run,
including a first build on a fresh machine. The Rust compiler receives only
bottleneck fixes until the freeze. The first priority stays unchanged: make
the self-hosted compiler usable end to end. This note guides the rebuild that
follows; nothing here asks for a refactor before `Compiler/` runs.

Related: `Docs/research/incremental-checking-design-2026-09-30.md` (item-level
incremental checking, interface records, red/green with early cutoff).

## 1. Where the pipeline stands today

The self-hosted compiler cannot yet run end to end, so the measurements below
profile the Rust pipeline, which `Compiler/` mirrors structure for structure
(the same phases, the same string-keyed symbols, the same comptime fragment
path). They show which structures cost the most; section 3 finds the same
structures in `Compiler/`.

Method: release binary snapshot67 (`~/.cache/jet-dev/scratch/jet-release-snapshot67`),
32-thread machine, `JET_STORE_DIR=` and `JET_RECEIPT_BYPASS=1` (no caches),
one process at a time. The binary has no phase timers, and `perf` could not
unwind the compiler thread, so phase shares come from a gdb sampler that
stops the process at a fixed interval and records every thread's stack
(`~/.cache/jet-dev/scratch/SpeedArch/pmp.py`, classified by
`classify.mjs`). Phase CPU seconds = busy thread-stacks in the phase ÷ sample
rounds × unsampled wall time; lines/s = input lines ÷ phase CPU seconds.
Phases below about 0.1 s are within sampling noise.

Inputs: `~/.cache/jetbench2/big/main.jet` (5,135 lines) and the sema slice
`~/.cache/jet-dev/scratch/ParallelCheck/sema/unit.jet` (116,571 lines).

| Command | Wall | CPU | Lines/s end to end |
|---|---|---|---|
| `jet check main.jet` | 4.2 s (6.7 s first run) | 4.5 s | 1,200 |
| `jet run main.jet` (JIT) | 14.5 s | 14.9 s | 350 |
| `jet build main.jet` (rustc, store off) | 118 s | 124 s | 44 |
| `jet check unit.jet` (1,180 errors) | 39.5 s | 52.4 s | 2,950 |
| `go build` same program, 5,859 lines | 0.1–0.3 s warm, 1.9 s empty cache | | |
| `zig build-exe` same program, 5,321 lines | 0.25–0.5 s | | |

The Go and Zig times include about 0.3 s of `nix shell` start-up.

Per-phase throughput (CPU seconds and lines per CPU second):

| Phase | check main.jet | run main.jet | build main.jet | check unit.jet |
|---|---|---|---|---|
| Lex | 0.07 s · 72k | 0.04 s · 120k | < 0.05 s | 1.0 s · 112k |
| Parse | 0.08 s · 61k | 0.13 s · 40k | 0.18 s · 28k | 1.9 s · 62k |
| Load, resolve packages | 0.31 s · 17k | 0.38 s · 13k | 0.49 s · 10k | 2.5 s · 47k |
| Registration | < 0.05 s | 0.06 s | 0.05 s | 1.6 s · 75k |
| Derive expansion | 0.14 s · 36k | 0.21 s · 24k | 0.29 s · 18k | 2.0 s · 59k |
| Comptime | < 0.05 s | < 0.05 s | 1.6 s · 3.2k | 16.3 s · 7.2k |
| Check bodies | 0.54 s · 9.5k | 0.64 s · 8.0k | 0.75 s · 6.8k | 13.7 s · 8.5k |
| Inference solves | 0.04 s | 0.34 s · 15k | 0.05 s | 0.9 s · 125k |
| Other sema (liveness, merge, completion) | 0.37 s · 14k | 0.06 s | 0.73 s · 7.1k | 14.1 s · 8.3k |
| Semantic index | 2.49 s · 2.1k | — | 4.3 s · 1.2k | — |
| TIR cost report | 0.66 s · 7.8k | — | — | — |
| Lower TIR and MIR | — | (in JIT) | 2.3 s · 2.2k | (in comptime) |
| MIR legality verification | — | 3.3 s · 1.6k | 2.7 s · 1.9k | (in comptime) |
| MIR digest (canonical bytes) | — | 4.7 s · 1.1k | 1.8 s · 2.9k | (in comptime) |
| Optimize | 0.04 s | 0.41 s · 13k | 0.52 s · 9.9k | (in comptime) |
| JIT (Cranelift) | — | 5.1 s · 1.0k | — | — |
| Rust emission | — | — | 0.9 s · 5.5k | — |
| rustc (program and runtime) | — | — | 101 s | — |
| Diagnostic rendering | 0.04 s | — | 0.42 s | 1.8 s |

What the samples say:

- **Off-path work dominates `jet check`.** The semantic index (52% of
  `check main.jet`) and the TIR cost report (14%) are not needed to check
  anything. The index cost is quadratic:
  `crates/jet-semindex/src/Symbols.rs:1006-1069` (`lexical_scope_for_def`)
  scans every structural node with a module-path string compare for every
  definition; `memcmp` alone is 27.5% of the samples.
- **Comptime re-lowers per evaluation.** In `check unit.jet`, comptime is the
  largest phase (31%), yet the interpreter itself (`MIREval::Machine::run`)
  is only 6% of busy samples. The rest is building a fresh MIR program for
  each evaluation: `lower_mir_fragment` 20%, `optimize_mir_program` 10%,
  legality verification 7%, digest 3%.
- **Clones and allocation are everywhere.** AST and comptime value clones and
  drops (`Func`, `Stmt`, `Expr`, `CtValue`, `SwitchArm`) are 20.5% of busy
  samples on the slice and 7.8% on `check main.jet`; `malloc`/`free` frames
  are 19–28%.
- **The JIT path spends more time proving and hashing MIR than compiling
  it.** On `run main.jet`, the MIR digest (5 s, with `Debug` formatting and
  `format!` at 9–10% of samples) and legality verification (3.3 s) together
  exceed Cranelift (5.1 s).
- **Parallel checking barely scales.** `check unit.jet` uses 1.33 cores on
  average (52.4 s CPU over 39.5 s wall) although body checks run through
  `jet_sema::Sema::Bundle::Parallel::map_checked`; comptime evaluation and
  bundle completion (liveness with a linear `find_function_by_name_span`)
  run serially.
- **Builds wait on rustc.** 85% of `jet build` wall time is the rustc child
  compiling the emitted program and the runtime from source.
- Lex and parse are the fastest phases but still 90× and 16× below budget.

## 2. Budgets

The target is a 300k-line build in one second on an 8-core machine. Budgets are per
core; serial phases must be fast on their own, and the heavy phases
(checking, lowering, code generation) must scale across cores. "Today" is
the best rate measured in section 1.

| Phase | Budget per core | Wall share, 300k lines on 8 cores | Today | Gap |
|---|---|---|---|---|
| Startup (process, Core interface record) | ≤ 20 ms total | 0.02 s | not measured | — |
| Lex | ≥ 10M lines/s | 0.03 s | 112k | 90× |
| Parse to flat AST | ≥ 1M lines/s, parallel per file | 0.04 s | 62k | 16× |
| Load, resolve, register | ≥ 2M lines/s | 0.15 s | 47k / 75k | 25–40× |
| Derive expansion | ≥ 2M lines/s | < 0.01 s | 59k | 35× |
| Check bodies, solves included | ≥ 300k lines/s | 0.13 s | 9.5k | 32× |
| Comptime | each distinct call once; ≤ 5% of check | < 0.05 s | 7.2k | — |
| Lower to TIR and MIR | ≥ 500k lines/s | 0.08 s | 2.2k | 230× |
| MIR verify and digest | off the dev path, or ≥ 2M lines/s | < 0.02 s | 1.1k–1.9k | > 1000× |
| Optimize (dev) | ≥ 1M lines/s | 0.04 s | 13k | 77× |
| Code generation, dev (Cranelift) | ≥ 300k lines/s | 0.13 s | 1.0k | 300× |
| Link with a prebuilt runtime | ≤ 100 ms | 0.10 s | 101 s (rustc) | 1000× |
| Release (rustc/LLVM) | Go-class, not Jai-class | — | — | — |

The shares sum to about 0.75 s and leave headroom for I/O and diagnostics.
At these rates, `jet check` takes about 0.3 s on the 116k-line slice and
about 20 ms on the 5,135-line program.

## 3. The rules and where `Compiler/` breaks them today

Each rule names the structure in `Compiler/` that violates it. File paths are
relative to `Compiler/`.

**R1. Names are interned once; identities are integers.** The lexer interns
every identifier and literal spelling into one table and hands out a 32-bit
`Name`. Symbols, types, items, and modules are addressed by dense integer IDs.
A string appears only at the edges (diagnostic text, emitted symbols).
Violations:

- `JetLexer/Source/Lexer/Tokens.jet:8-22`: every `Token` carries a
  `TokenPayload` with three `String`s, an `Int?`, a `Float?`, and a
  `[StringPart]`, so each token is several heap objects.
- `JetFoundation/Source/AST/Expressions.jet:4-55`: `Ident(name: String)`,
  `Field(member: String)`, `MethodCall(method: String, recv_type: String?)`,
  `StructLit(type_name: String)`, `EnumLit(type_name, variant: String)`.
- `JetFoundation/Source/Types/Types.jet:326-343`: `SymbolID{key: String}` is
  a formatted `"kind:module:owner:name"` string, and every `Symbol` also
  carries `name`, `module_name`, `owner` strings and a full `Type`.

**R2. Types are hash-consed in an arena; equality is an integer compare.**
`TypeId` indexes one type table per compilation. Constructing `List(Int)`
twice returns the same ID. Substitution and unification work on IDs.
Violation: `JetFoundation/Source/Types/Types.jet:4-28` defines `Type` as a
recursive tree with `Named(name: String)`, `Apply(name: String, args)`, and a
`Fn` variant that carries effect rows, contracts, and call metadata inline,
so every comparison is a deep walk with string compares and every stored type
is a copy.

**R3. The AST is a flat, immutable arena; later phases write side tables.**
The parser emits node arrays (kind, span, first child, child count) per file.
The checker records results in tables keyed by node ID (type of expression,
resolved symbol of a name, chosen overload) and never rewrites or copies the
tree. Violations:

- `JetFoundation/Source/AST/Expressions.jet:32`: `MethodCall` stores checker
  output (`recv_type`, `resolved_ret`, `operator_rhs`, `checked_widen`) in the
  AST, so checking produces a rewritten copy of the tree.
- `JetSema/Source/Sema/CheckProgram.jet:937,962-966`: the function memory plan
  produces a rewritten `Func` per function (`memory_func = row.function`)
  before the checker walks it; the checker then builds a second typed tree
  (`TFunc`) that lowering walks a third time.
- `JetSema/Source/Sema/Scopes.jet:75-78`: `SemaScopeDeclaration` stores a
  whole `Item` by value; `Scopes.jet:938-953` returns declarations by value.

**R4. Lookups are hashed or indexed, never linear string scans.** Scopes are
a stack of small hash maps (or one flat name→slot table with a shadow stack);
Core calls, registered items, and checked functions are indexed by ID.
Violations:

- `JetSema/Source/Sema/Scopes.jet:885-911`: `sema_scope_resolve_index`
  rescans every binding once per enclosing frame with a string compare, so a
  lookup costs O(depth × bindings) string compares.
- `JetSema/Source/Sema/Scopes.jet:931-953`: symbol-by-ID and declaration
  lookups scan every module, function, nominal, and declaration.
- `JetFoundation/Source/Registry/CoreCalls.jet:525-545`: `core_call_in` and
  `core_receiver_method_in` scan all of `CORE_CALLS` with string compares for
  every Core call the checker resolves.
- `JetSema/Source/Sema/SemanticIndex.jet:1586-1589`: dedup with
  `seen.contains(key)` on a `[String]` plus a scan of every checked function
  per definition (O(definitions × functions)); `CheckProgram.jet:993-998`
  finds checked comptime functions by linear string search.

**R5. One walk per phase, data-oriented, nothing off the critical path.**
Each phase makes one pass over flat arrays and produces flat arrays. Work that
serves tools (semantic index, cost reports, lint projections) runs only when a
tool asks for it. Violations:

- `JetSema/Source/Sema/SemanticIndex.jet:1619-1728`: the check result builds
  a full semantic index (every definition, reference, local, call, and state
  graph, stored as dynamic `TComptimeValue`s), and
  `JetDriver/Source/Driver/CompilerQueries.jet:130` makes check completeness
  depend on it. The Rust twin of this pass is the largest single cost of
  `jet check` today (section 1).
- Diagnostic source locations are attached eagerly
  (`CheckProgram.jet:989`); line and column should come from one per-file
  line-start table, looked up only when a diagnostic renders.
- `JetOptimizer/Source/Pipeline.jet:192,199` hashes the whole MIR program
  twice per optimization (once to stamp, once to confirm the stamp) and
  `JetDriver/Source/Driver/Pipeline.jet:2403` hashes it again;
  `JetFoundation/Source/MIR/Digest/Digest.jet:16-29` builds the canonical
  bytes one `push` per byte. In the Rust twin, digest plus legality
  verification cost more than Cranelift on `jet run`. Digest per function,
  once, only where a cache key needs it; run full legality verification in
  compiler test builds, not on every user compile.

**R6. Checking is parallel per function by default.** Registration produces
an immutable, shared symbol and type table. Each function body is then an
independent job that reads the shared tables and writes its own result
tables; results merge in a fixed order so diagnostics stay deterministic.
Violation: `JetSema/Source/Sema/CheckProgram.jet:955-988` checks functions
in one sequential loop, passes the scope table and environment by value into
every function (`sema_function_check(..., ~environment, scopes, ...)`), and
appends per-function facts into shared growing lists. Nothing in `JetSema`
or `JetDriver` runs work in parallel.

**R7. Comptime code is compiled once and values are cached.** Every function
reachable from comptime lowers to MIR once per compilation into one shared
program. The evaluator borrows that program. A comptime value is cached by
(item ID, argument digest) in memory and, through the incremental store,
across runs. Violations:

- `JetDriver/Source/Driver/Pipeline.jet:1686-1733`: for every pending comptime
  item, inside a fixpoint loop, the driver rebuilds all modules with current
  values (`sema_comptime_modules_with_values`), re-lowers the function
  closure (`lower_functions`), lowers the item, recomputes package facts, and
  assembles a temporary MIR program. Cost is O(items × program).
- `JetEval/Source/EvalValues.jet:528-561`: `JetEvalOwnedRoot` and
  `JetEvalResumeRequest` hold a `MIRProgram` by value.
- `JetCodegen/Source/Codegen/Expressions.jet:1458-1512`: every use of a
  comptime constant converts the value tree to a MIR constant again
  (`Functions.jet:771`, `PreparedConstants.jet:82`); lower each constant once
  and refer to it by ID.

**R8. Core is a precompiled interface record.** The toolchain ships a binary
interface record for Core (signatures, Core call rows, effects, capability
contracts, diagnostic registry), memory-mapped at startup and indexed by ID.
The same record format serves packages (`JetFoundation/Source/Record/`).
Violations: `JetFoundation/Source/Registry/` holds about 1.5 MB of Jet row
tables (`CoreCallRows.jet` 330 KB, `DiagnosticRows.jet` 809 KB, signature
tables about 280 KB) that are built as values at run time, and
`CoreCalls.jet:282-315` computes each row's effect and capability through
chains of string comparisons (`core_call_effect_for`, `core_call_one_of`).
Diagnostic rows should load only when a diagnostic renders.

**R9. Dev builds use Cranelift and a prebuilt runtime.** `jet build` in dev
mode lowers MIR to Cranelift per function in parallel, links against a
runtime archive shipped with the toolchain, and links with a fast linker.
rustc/LLVM runs only for release. The Jet driver must consume the same
prebuilt runtime artifact the Rust driver is moving to (PrebuiltRuntime
work); section 1 shows what compiling the runtime costs today.

**R10. Item-level incremental sits on top.** With stable integer item IDs
(R1), interned types (R2), and side tables keyed by node (R3), the
incremental design's per-item fingerprints and early cutoff are cheap: an
item's interface hash is a hash over its signature row, and a body edit
rechecks one function job (R6). Building incremental reuse on today's string
keys and cloned trees would make the cache itself slow.

**R11. Allocation discipline.** Each phase allocates from per-file or
per-function arenas freed in bulk: no per-token or per-node heap objects, no
`[String]` path lists as keys (`Scopes.jet:91,104`: `slot: [String]`,
`output_path: [String]`), and no string formatting on the hot path
(`Types.jet:342`, `CoreCalls.jet:511-514`).

## 4. Staged refactor plan

The order puts the biggest measured wins first. Stage 0 is limited to the
Rust bottleneck fixes the owner allows before the freeze. Stages 1 and 2
start once `Compiler/` runs end to end; they are local changes that do not
alter data shapes across packages. Stages 3 to 5 change shared
representations and start only after `Compiler/` compiles itself, so each
step can be proven by diffing the self-hosted compiler's output before and
after.

| Stage | When | Work | Expected win |
|---|---|---|---|
| 0 | now, Rust bottleneck fixes, each with its `Compiler/` mirror from stage 1 | (a) Take the semantic index and the TIR cost report off the `jet check` path, or make the index linear by indexing `db.nodes` by module and span (`crates/jet-semindex/src/Symbols.rs:1006-1069`). (b) Lower each comptime closure to MIR once per compilation and reuse it across evaluations instead of `lower_mir_fragment` plus optimize, verify, and digest per evaluation. (c) Skip whole-program MIR digest and legality verification on dev `run` and `build`, or compute the digest per function without `Debug` formatting. | (a) about two thirds of `check main.jet` (4.2 s to about 1.3 s); (b) most of the 31% comptime share of `check unit.jet`; (c) about 8 of the 14.5 s of `run main.jet`. |
| 1 | once `Compiler/` runs end to end | Hash-indexed scope table (R4); a map from Core call key to row, built once (R4, R8); drop per-function copies of scopes and environment (R6); build the semantic index only on request (R5); lower comptime closures once per compilation and cache values by item ID (R7); digest once per function (R5). | Removes the quadratic terms; each change is a few functions. |
| 2 | once `Compiler/` runs end to end | Per-function parallel checking over an immutable registration graph, deterministic merge (R6); parallel lowering per function. | Near-linear scaling on the heavy phases. |
| 3 | after self-compile | Interning: `Name` for identifiers, integer `SymbolID`, hash-consed `TypeId` (R1, R2). Touches every package, so it lands as one cutover per package, lexer outward. | The largest constant-factor win: string compares and deep type compares disappear from checking. |
| 4 | after stage 3 | Flat token arrays and flat AST with side tables for checker results; drop the rewritten `Func` from the memory plan and fold the `TFunc` tree into side tables (R3, R5, R11). | One walk per phase; allocation drops by an order of magnitude. |
| 5 | after stage 4 | Core as a precompiled interface record shared with package records (R8); Cranelift dev backend and prebuilt runtime in the Jet driver (R9); item-level incremental with early cutoff (R10). | Fresh-machine first build and one-line rebuilds reach Go/Zig class. |

Each stage lands with a throughput check against section 2: the same two
inputs, per-phase shares from the sampler in section 1, and lines per second
per phase recorded in the stage's Tower card.

### Stage 0 status (2026-09-30)

Measured with release binaries, same machine, `JET_STORE_DIR=` and
`JET_RECEIPT_BYPASS=1`. The baseline is snapshot67, the binary section 1
profiled; the Stage 0 build includes other work that landed since.

| Command | Before | After |
|---|---|---|
| `jet check main.jet` (warm) | 4.17 s | 1.28 s |
| `jet run main.jet` | 15.4 s | 6.4 s |
| `jet check unit.jet` | 42.1 s | 35.7 s |

- (a) `jet check` no longer builds the semantic index: the check projection
  builds it on first use, and only index consumers (`inspect expand`,
  `impact`, `doc`, `review`, `find`, architecture advice) ask for it. The
  lexical scope of each local and parameter now comes from one sweep over
  the module's structural nodes instead of a scan of every node per
  definition. The TIR cost report stays on `jet check`: it produces the
  L2510 hot-loop cost lints that `jet check` reports.
- (b) and (c) share one change. A dev compile verifies MIR legality once,
  when the lowered program enters the optimizer; the per-pass
  re-verification, the adapter-boundary re-verification, and the
  re-digest of the pass-order seal run only with full verification on
  (compiler test builds, which have debug assertions, and
  `jet build --verify`). The digest is fed to its hash lanes while the
  program is walked, without the byte buffer or a fresh string per
  `Debug`-encoded row; digest values are unchanged. Comptime fragments
  still lower their closure once per evaluation: each fragment embeds the
  values of the constants evaluated before it, so reuse across evaluations
  needs the stage 1 design (lower once, pass values at run time).
- `Compiler/` mirrors: the digest writer feeds its lanes directly
  (`JetFoundation/Source/MIR/Digest/Digest.jet`), and the optimizer digests
  its input only when every function already carries a complete seal
  (`JetOptimizer/Source/Pipeline.jet`). The Jet compiler has no `jet check`
  command, no lexical-scope index, and no per-evaluation optimize step in
  its comptime path; its per-pass legality checks stay on because every Jet
  compiler run today is a bootstrap or test run.
