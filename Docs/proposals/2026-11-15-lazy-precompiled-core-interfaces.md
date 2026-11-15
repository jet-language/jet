# Lazy precompiled Core interfaces

Status: design note for #6328 c22 (precompiled Core and Prelude), revision
4. This note ratifies nothing. It narrows
`Docs/proposals/precompiled-libraries-2026-11-05.md`. The record size audit
is `Docs/audits/2026-11-13-core-record-size-ceiling.md`.

Two reviews have run so far:

- Review 1 (`Docs/audits/2026-11-15-lazy-core-interfaces-review.md`)
  reviewed revision 2. Its gaps are G1 to G28.
- Review 2 (`Docs/audits/2026-11-15-lazy-core-interfaces-review-2.md`)
  reviewed revision 3. Its gaps are H1 to H26.

Section 12 maps each gap to the text that answers it.

Four decisions frame the note:

- **2026-11-14:** the front-end Core record was retired. Since then every
  compile loads Core from the sources built into jet.
- **2026-11-15, Main, on how c22 reads:** a compile may load the
  declarations and types of a checked interface that was built when jet was
  built. That counts as loading, not as a re-check, as long as it costs a
  small fraction of hello's user time.
  - Producing those declarations at compile time by parsing Core text counts
    as scanning source, and c22 forbids it. That holds for a header-only
    parse and for a parse of one declaration at a time.
  - Core text is parsed only when jet is built.
- **Review 1 (G25), accepted by Main:** c22 is met only when no compile on
  built-in Core checks a Core body from text. So step B, the checked bodies,
  is required.
- **Main, on review 2:**
  - **Display (H12).** Rendering the span of a reported diagnostic from the
    embedded Core text counts as display. It is not reading Core source. A
    clean compile reads 0 bytes of Core text.
  - **c22 against c37 (H16).** Until the owner ratifies the ballot, a B step
    that makes any 6.2 program slower does not merge. The ballot is
    `Tools/agent/ballots` format, filed with Main.

So the plan has two steps:

- **Step A** ships the declarations: signatures, types, constants, seal
  facts and reach.
- **Step B** ships checked bodies for every Core and Prelude body a compile
  still needs.

c22 is claimed after step B, on the census of section 10.

What landed since revision 3:

- **Step P is code.** The effect vocabulary rows landed as `b221bb6d88`. The
  rows for the other Prelude texts landed as `a568b242df`. Those rows then
  moved from `#run` to generated data, because a `#run` that reaches the
  parser checks the parser's closure in every compile of compiler sources
  (section 3.6).
- **A lazy Core unit's head parse is gone.** `core.interface` holds the
  program each unit's declaration heads parse to, in the syntax codec
  (`JetFoundation/Facts/SyntaxCodec.jet`, generated beside the body codec).
  A load whose unit text hashes the same reads that program and lexes none
  of the unit's text. Its waves count the unit under
  `parse.core_heads_decoded`, not `parse.core_input_bytes`. Held
  declarations and kept bodies still parse from text (4.1).
- **The T1 search removal (step T)** is still to come.

## 1. Measured baseline

### 1.0 How the numbers were taken (H4, H5)

**The jet.** The numbers come from the gate-p155 jet, built from dev
`49836bd978`. That base includes c11c, c11d, c43 and the effect vocabulary
rows (`b221bb6d88`). It does not include the other Prelude rows
(`a568b242df`). So 1.3 states, for each Prelude text, whether p155 still
reads it.

**Where they were taken.** The numbers come from the team machine, outside
any checkout. `JET_TOOLCHAIN_ROOT`, `JET_SEED` and `JET_TRACE_FILE` were
unset, so Core is the built-in Core, unless a row says otherwise.

**Noise.** The machine is shared with other lanes. The load average was 8
to 15 during these runs, so medians move by up to 30% between sets. Each
table names its set.

**Warm and cold.**

- **Warm:** the jet binary and the inputs are in the page cache.
- **Cold:** the jet binary was evicted with `posix_fadvise(DONTNEED)` before
  each run. Only the binary is evicted. The dynamic loader, libc and the
  linker that `native.link` runs stay cached.

**The measuring script.** The script forks a fresh helper for each run. The
helper starts jet and reads the following for that run alone:

| Figure | What it is |
|---|---|
| wall | from start to exit of the jet process |
| user, sys | from `wait4` of the jet process |
| own peak RSS | `ru_maxrss` from `wait4` of the jet process. It equals jet's VmHWM. |
| cgroup peak | cgroup v2 `memory.peak` of the run's scope. It is reset through its own file descriptor before each run. |
| major and minor faults | from `wait4` |

Revision 3 used the running maximum of `RUSAGE_CHILDREN`, which is not a
per-run figure (review 2, M1). Every RSS figure in this revision is per run.

**Own peak RSS depends on page-cache state.** It includes jet's
file-backed image pages (the image is about 250 MB), so it moves with the
page-cache state of the jet file, not only with jet's work.

- The first warm hello sets on a freshly built p155 measured 88 MB.
- After one cold set had evicted and re-read the image, they measured
  63 MB.
- p150 measures 61 MB.
- `[INFERENCE]` The jet file was cached in large folios after the build
  wrote it.

So each arm of an A/B reads its own binary cold once before its warm runs
(6.2).

**The cgroup peak** counts the anonymous memory and new page cache that the
run charges. A cold run's cgroup peak includes the image pages it reads
back in.

### 1.1 hello, untraced (G2, G14, H4)

| Run set | Wall | User | Sys | Own peak RSS (range) | cgroup peak | Major faults | Minor faults |
|---|---|---|---|---|---|---|---|
| `jet run`, warm, median of 11 (set a) | 28.3 ms | 14.9 ms | 13.0 ms | 63.2 MB (61.9 to 64.2) | 25.7 MB | 0 | 4,546 |
| `jet run`, warm, median of 11 (set b) | 27.3 ms | 15.4 ms | 11.1 ms | 63.0 MB (62.7 to 63.7) | 25.4 MB | 0 | 4,546 |
| `jet run`, warm, median of 11 (set c) | 25.2 ms | 13.8 ms | 10.9 ms | 64.1 MB | 27.1 MB | 0 | — |
| `jet run`, cold, median of 5 | 121.3 ms | 17.4 ms | 27.6 ms | 60.2 MB (59.5 to 60.7) | 87.7 MB | 482 | 4,297 |
| `jet check`, warm, median of 11 | 17.8 ms | 9.7 ms | 6.7 ms | 44.3 MB (43.8 to 44.7) | 15.0 MB | 0 | 2,290 |
| `jet --version`, warm, median of 11 | 1.7 ms | 0.0 ms | 1.2 ms | 8.3 MB | 9.7 MB | 0 | 359 |

**Spread of the own-peak median.** Across six hello sets on p155 (the three
above and three named-tree sets in 1.5), the median ranges from 62.9 to
64.5 MB, a spread of 1.6 MB. Revision 3's 0.5 MB band was a property of the
running-maximum statistic, not of jet. Arms run back to back differ by at
most 0.25 MB (63.2 against 63.0 MB; 64.1 against 63.9 MB; 64.5 against
64.5 MB).

**Cold runs.** This cold set has 482 major faults, against 28 in revision
3. `[INFERENCE]` The script now evicts from a helper that does not map jet,
so the image really leaves the page cache.

**`jet --version`** is the floor for process start and runtime mapping,
about 2 ms.

**Threads (H22).** Under `strace -f`, `jet run hello.jet` and
`jet check hello.jet` create no thread and no child process: 0 `clone`, 0
`fork`, 1 `execve`. `jet run fread.jet` creates 12 threads.

### 1.2 hello, traced split (G2, H23)

This is one traced warm `jet run`: wall 34.6 ms, user 16.8 ms, sys 14.8 ms.
It is **traced**, so its numbers are not comparable with 1.1. Each row is
the thread CPU (`cpu_ns`) of a top-level span. hello is single-threaded
(1.1).

| Span | CPU |
|---|---|
| `register` (hello, Core Prelude, compiler-owned Prelude rows) | 5.77 ms |
| `sema.finalize` | 4.03 ms |
| `native.link` | 2.82 ms |
| `load`: parse `Core/Prelude/Prelude.jet` 1.33, `hello.jet` 0.29, `core.jet` 0.10, the rest 0.41 | 2.13 ms |
| `sema.check` | 1.61 ms |
| `sources` | 1.45 ms |
| `ir.build` | 0.68 ms |
| `comptime` | 0.67 ms |
| other top-level spans | 1.26 ms |
| **Named spans** | **20.42 ms (65% of user plus sys)** |
| Not in any span | 11.15 ms (35%) |

`register` fell from 7.62 ms at p150 to 5.77 ms at p155. The effect
vocabulary is no longer parsed: since `b221bb6d88` it comes from rows built
with the compiler.

Today's spans attribute 65%, not the 95% that G2 asks for. The work outside
any span is known but not timed:

- process start and runtime mapping, about 2 ms (1.1);
- the CLI front before the compile opens its trace;
- trace writing and process exit.

A0 (section 10) adds a span for each.

**The Core- and Prelude-attributable part of hello (H23).** These are the
spans at p155 that work only on Core or Prelude:

| Work | Span | CPU |
|---|---|---|
| Parse of `Core/Prelude/Prelude.jet` | `load > Core/Prelude/Prelude.jet` | 1.33 ms |
| Parse of the empty `core` unit | `load > core.jet` | 0.10 ms |
| Body checks, Core Prelude | `sema.finalize > sema.bodies > Core/Prelude/Prelude` | 0.40 ms |
| Body checks, `Prelude/Errors` | `sema.bodies > Prelude/Errors` | 0.14 ms |
| Body checks, the effect module | `sema.bodies > Prelude/Effects.jet` | 0.07 ms |
| Body checks, module `core` | `sema.bodies > core` | 0.07 ms |
| Per-module checks of those four modules | `sema.check > …` (0.033 + 0.035 + 0.033 + 0.032) | 0.13 ms |
| **Measured** | | **2.24 ms** |
| The Prelude share of `register` | not split today | unknown, at most 5.77 ms |

The effect parse is gone (G4, `b221bb6d88`). A0 splits `register` by
module, which completes the table. Section 6.1 names the step that brings
the sum under 1 ms.

### 1.3 The Core and Prelude text a hello compile reads (G3, H3)

| Text | Source | Bytes | p155 (base `49836bd978`) | After step P |
|---|---|---|---|---|
| Core Prelude | `Core/Prelude/Prelude.jet` | 11,256 | parsed (1.33 ms) | parsed until A2 |
| `core.jet` | no file. It is the empty unit that `Loader.jet` adds for the Core module `core`, which carries only its ABI rows | 0 | parsed (0.10 ms) | parsed until A2 |
| Canonical effect source | `EFFECT_CANONICAL_SOURCE` (`Registry/EffectVocabulary.jet`), the concatenation of `Compiler/Prelude/Effects.jet` (3,147 B), `Core/Effects/Effects.jet` (763 B) and `Core/Compiler/Meta.jet` (746 B). So 1,509 B of it are Core text. | 4,656 plus separators | not parsed. The module is built from `EFFECT_CANONICAL_MODULE`, the rows the parser would build, made by `#run` | same |
| Error family | `Compiler/Prelude/Errors.jet` | 5,912 | parsed. hello registers module `Prelude/Errors` (32 items) | registered from `SEMA_PRELUDE_ERROR_PROGRAM`, generated data |
| Derive providers | `Compiler/Prelude/Derives.jet` | 7,158 | word-scanned; providers parsed when a program derives | `SEMA_PRELUDE_DERIVE_WORDS` and `SEMA_PRELUDE_DERIVE_PROVIDERS`, generated data |
| Markers | `Compiler/Prelude/Markers.jet` | 15,789 | scanned for each marker query | `MARKER_SOURCE_RULES` by `#run`; `SEMA_PRELUDE_MARKER_DECLS` and `SEMA_PRELUDE_MARKER_RULES`, generated data |
| Units | `Compiler/Prelude/Units.jet` | 36,368 | scanned when a program names a unit | `SEMA_UNITS_CATALOG`, generated data |

**`parse.core_input_bytes` counts the Core Prelude (H13).** Review 2 read
this counter as 0 for hello. The counter is emitted once per load wave, and
hello has three waves:

| Wave | Bytes |
|---|---|
| `load > hello.jet` | 0 |
| `load > Core/Prelude/Prelude.jet` | 11,256 |
| `load > core.jet` | 0 |

The Core Prelude is loaded as a `.Core` job, not a `.Prelude` job, so it
counts as Core. The review read the last emission. The same holds in the
p150 trace. The sum over waves is 11,256 B for hello today, so a proof that
"the sum is 0" can fail today. The canonical effect source never went
through the Loader's job loop. Since `b221bb6d88` no compile parses it.

**The texts are still embedded.** The compiler-owned texts stay in jet
after `a568b242df`:

- the canonical effect source feeds sema's `module_sources`;
- the error family's text feeds `SEMA_PRELUDE_ERROR_LINE_STARTS` and the
  diagnostics that point into it.

A compile reads these texts only to render a reported diagnostic, which
counts as display (H12).

### 1.4 Larger programs

All rows are warm and untraced. `jet run` exits 64 after the compile for
Summarize and RunColdContext, because they need arguments. The compile is
complete. The RunColdContext source is the dev copy at `49836bd978`.

| Program | Command | Runs | Wall | User | Sys | Own peak RSS (range) | cgroup peak |
|---|---|---|---|---|---|---|---|
| `fread.jet` (reads one file with `core.files`, 6 lines) | `jet run` | median of 11 | 552 ms | 710 ms | 80 ms | 207.4 MB (202.7 to 211.8) | 153.3 MB |
| `abs.jet` (`print("{x.abs()}")`, 4 lines) | `jet run` | median of 11 | 420 ms | 387 ms | 38 ms | 139.3 MB | 151.3 MB |
| jetc-trace Summarize (148 lines) | `jet check` | median of 7 | 467 ms | 721 ms | 67 ms | 202.9 MB (198.7 to 205.9) | 161.6 MB |
| | `jet run` | median of 7 | 651 ms | 952 ms | 107 ms | 230.2 MB (225.7 to 233.9) | 177.3 MB |
| RunColdContext (1,264 lines) | `jet check` | median of 7 | 1,288 ms | 4,001 ms | 283 ms | 457.3 MB (453.0 to 459.7) | 410.6 MB |
| | `jet run` | median of 7 | 1,647 ms | 3,819 ms | 361 ms | 434.2 MB (431.2 to 437.8) | 384.7 MB |

`abs.jet` shows the cost of a Core unit that is a whole file. Its one Core
call loads `Core/Math/Math.jet` whole: 1,193,230 bytes of Core text parsed,
for 29 checked Core bodies.

**How their Core demand splits.** Each row comes from one traced `jet run`
with `JET_TRACE_DETAIL=functions`, which emits the `sema.core_bound.*`
counters.

- A body is **bound** when only direct run-time calls reach its sealed key.
  A seal stand-in then replaces its check.
- **Kept** bodies are still checked from text. Each is counted under its
  reason.
- **Core text parsed** is `parse.core_input_bytes` summed over the waves.

| Program | Core text parsed | Bodies checked | Bound (CPU) | Behind bound | Kept (CPU) | Kept by reason |
|---|---|---|---|---|---|---|
| hello | 11 KB | 0 | 0 | 0 | 0 | — |
| `abs.jet` | 1,193 KB | 29 | 26 | 0 | 3 | — |
| `fread.jet` | 2,026 KB | 169 | 90 (2.4 ms) | 0 | 79 (73.7 ms) | generic 52, unlisted 18, intrinsic 5, function value 4 |
| Summarize | 2,123 KB | 212 | 129 (3.4 ms) | 0 | 83 (79.7 ms) | generic 56, unlisted 18, intrinsic 5, function value 4 |
| RunColdContext | 4,767 KB | 1,538 | 907 (99.3 ms) | 40 (32.3 ms) | 591 (494.6 ms) | generic 260, function value 153, lowered 99, comptime 55, unlisted 19, intrinsic 5 |

The CPU figures are `sema.core_bound.bound_ns` and `kept_ns`, traced and
inclusive. A kept body costs 0.84 to 0.96 ms of check. A bound stand-in
costs 26 to 110 us. These are step B's payoff and the yardstick for its
load cost (3.3).

`jet check` takes no stand-ins, so it checks more bodies: 292 for Summarize
and 2,123 for RunColdContext.

These counters cover only the bodies that demand waves check. Core bodies
checked up front are not in them: impl methods, lowering roots and
derives. A0 counts those too (section 4).

### 1.5 Toolchain trees (G1, H19, H20)

**Calls on the tree.** `jet_cli_toolchain_root(entry)` returns
`JET_TOOLCHAIN_ROOT` if it is set. Otherwise it searches upward from the
entry for a tree. The calls were counted with `strace -f` on
`jet run hello.jet`, taking only calls whose path is under the tree's
`Core/` or `Compiler/Prelude/`:

| Entry | Calls on the tree | Cost |
|---|---|---|
| outside any checkout, no tree named | 0 | — |
| inside a dev checkout, no tree named (Core differs from the built-in Core) | 276 | the lazy tree compile |
| `JET_TOOLCHAIN_ROOT` naming the tree jet was built from (Core matches) | 272, all `statx` and the two reads of `CoreSourceParts.tsv` | within noise of built-in Core |

The hello A/B with the matching tree named was warm, median of 11, in three
interleaved pairs:

| Pair | Built-in Core | Named tree |
|---|---|---|
| 1 | 27.3 ms | 26.7 ms |
| 2 | 25.2 ms | 26.0 ms |
| 3 | 45.7 ms | 43.8 ms |

Pair 3 ran during a load spike. hello never runs the match test, because it
needs no deferred Core body.

**The match test as the jet function (H20).** `jet_cli_core_tree_matches`
runs today when a compile first names a deferred Core body.
`jet run fread.jet` does. Run with the matching tree named:

- Under `strace -f -tt -e trace=open`, the test's 313 file opens (the rows'
  files and the assets) span 25.5 to 36.0 ms of wall time in five runs.
  That includes strace's stop for each open.
- Built-in Core against the named tree, untraced, warm, median of 11, in
  five interleaved pairs:

  | Pair | Wall | User plus sys |
  |---|---|---|
  | 1 | +8.8 ms | +17.7 ms |
  | 2 | +4.7 ms | +8.2 ms |
  | 3 | +30.2 ms | +34.8 ms |
  | 4 | +33.0 ms | +59.2 ms |
  | 5 | −144 ms | — |

  Pair 5 had a load spike on the built-in arm.

The function costs more than 12 ms on this machine. That is review 2's
inference, now measured. Section 5 draws the consequence.

## 2. What the retired record taught

The record precompiled the whole registration graph and decoded it at load.
It lost to parsing for three reasons, logged on #6328 on 2026-11-14:

- **The decoder was slow.** It cost about 10 us per row byte. The whole
  in-tree Core front end costs about 0.58 s.
- **The unit was a whole module.** A record unit kept every declaration,
  and its spellings widened reach. fft-4096 loaded 32 units against 6.
- **It was big and too rich.** It was 61 to 96 MB, against a hand-minimal
  ceiling of 14.2 MB, of which 8.01 MB was checked bodies. Its places
  pointed into a syntax tree that had to be rebuilt first.

What the interface must avoid follows from each:

- no self-describing, variable-length field encoding;
- no module-sized units;
- no syntax tree. Bodies ship as checked rows, not as syntax.

Revision 2 also said "no bodies". That cannot hold together with c22
(section 4).

The record's first failure is the main risk for step B. Section 3.3 shows
that a loaded body is not a copy. B1's gate (section 10) fails the step
when loading a body costs more than a tenth of checking it from text.

## 3. The interface

### 3.1 What it holds

When jet is built, the runtime step checks Core to produce:

- `jet_core.o`;
- `core.seal`;
- `core.facts`;
- `core.exports` (`CoreLibrary.jet`).

The same check also writes the interface. It writes from the registered and
checked tables, never from text. It covers:

- Core;
- the Core Prelude;
- the compiler-owned Prelude modules that a compile registers
  (`Prelude/Errors`, `Prelude/Effects`).

Step A rows:

- **Declarations**, one row each: kind, name, owner (the type of a method;
  the type and trait of an impl), module, file, and full byte span.
- **Signatures:**
  - generic parameters and bounds;
  - parameter types and access marks;
  - result type, effect row and failure set;
  - the view provenance of the result.
- **Types:** fields, variants, layouts, derived traits, and impls with
  their method tables. Types are referred to by index into one interned
  type table.
- **Constants:** the value of each constant that user code can read.
- **Seal facts:** the stand-in facts that today come from `core.seal`
  (stop edge, module-state writes, failure facts), and the `jet_core.o`
  symbol a sealed call binds to.
- **Reach:** each declaration's distinct spellings, as a range into one
  word table, and its import demands.
- **Name index:** an open-addressing hash table from a qualified name to its
  declaration rows. It is built at jet build and shipped, so a compile
  builds no index.
- **Impl index:** the impls of each nominal type and of each trait, so a
  reached type brings its impls with it (section 7.3).

Step B adds:

- **Checked bodies:** each Core and Prelude function and method body as
  sema's checked body (`FnBody` and its `BodyTree`,
  `JetFoundation/Facts/Body.jet`), in the row form of 3.3.
  - This is the form JetIR builds from and monomorphizes from (`Build.jet`;
    `IRProgram.functions: [FnBody]`).
  - Rust's crate metadata (MIR) and Go's unified IR export data work the
    same way.
- **Checked defaults:** each default parameter and default field
  expression, as a checked expression tree (`BodyExpr`) with its owner
  module. A call site then uses it without resolving names in Core's scope
  (G7).
- **Generated bodies for Core types,** in the same checked form: derive and
  codec bodies, error conversions, and `close` methods.

The interface holds no Core text. Diagnostics that point into Core render
from the embedded Core text, which stays (3.5).

### 3.2 How it reaches jet (G8, H7)

The interface is a **runtime image part**, `core.interface`. The runtime
step writes it beside `core.seal` and `core.facts`. A compile reads it as
lent bytes through `jet_cli_runtime_part`, the way `JetCLICoreSealRead`
reads the seal today. No second compiler compile is needed.

**Field reads.** Every field is a little-endian 32-bit word at a fixed
offset in its row. Reading a field takes four byte loads, shifts and ORs,
plus a bounds check. The estimate is 1 to 2 ns per field.

**The gate.** A1 measures ns per field with a loop over every row of the
part, timed in thread CPU. **A1's gate is at most 3 ns per field read,
median over all rows.**

- If the gate is not met, A2 does not start.
- The reason for 3 ns: hello loads a few hundred rows of about 20 fields,
  roughly 6,000 reads. At 3 ns that is 18 us, under 2% of the 1 ms budget
  (6.1).

### 3.3 What loading costs (G9, H9)

Rows are read in place, but sema's tables hold sema values. So loading
builds values. Each value kind is built as follows:

| Value | How it is built |
|---|---|
| Scalars, flags, enum tags | read from the row |
| `String` | copied from the part's string table, one copy per field |
| `Type` | built from a type index the first time a compile uses it. A per-compile table, indexed by type index, memoizes it. Nested types are built through the same table. |
| `Span` | the file index maps to the compile's file id for that Core file. The byte offsets are copied. |
| Symbol | a key becomes a symbol id through the per-compile symbol table, as registration does today |
| `DeclID` | resolved through the declaration table. Dev keys that table by `SymbolID` since `094798d943`, so this is one symbol lookup. |
| Lists | one allocation each, filled element by element |

For step A, all of this is inside the `core.interface` spans (6.1).

**Step B builds whole bodies.** A body row is not a copy of the row. Body
loads run inside their own spans, `core.interface.bodies`, which report
thread CPU. The kinds below were counted in `Body.jet` at dev `313533b9f3`.

**`FnBody` has 52 fields:**

- names and identity: `name`, `module_name`, `module_identity`, `key`,
  `source_file`, `source_span`, `line`;
- signature: `params`, `ret`, `generic_params`, `visibility`, `kind`,
  `foreign`, `return_view_provenance`;
- facts: `failure_carrier`, `effects`, `is_pure`, `is_unsafe`,
  `unsafe_gate`, `memo_bound`, `memo_unbounded`, `is_reactive`,
  `reactive_upgrades`, `is_inline`, `is_inline_always`, `is_scalar`,
  `kernel_proof`, `uses_stack_sentry`, `gc_return`, `gc_scope`,
  `max_depth`;
- solver inputs: `origin_facts`, `typed_goals`, `index_proofs`, `pre`;
- taint: `masked`, `fixed_tags`;
- program-wide results: `web_bucket`, `web_reason`, `web_marker`,
  `endpoint`, `web_param_reconstructions`, `effect_scope`,
  `effect_scope_regions`, `heap`, `stack`, `frame_bound`, `stand_in`;
- other: `is_main`, `synthetic`, `body`, `tree`.

**`BodyTree` has 7 fields:** `nodes`, `stmts`, `expr_types` (one
`TExprType` per typed node), `types` (the body's own `TypeTable`: types,
hashes, slots), `node_base`, `stmt_base`, and `symbols` (the body's own
`SymbolKeys`).

**`NodeKind` has 99 variants.** By payload (a variant can be in more than
one row):

| Payload | Variants | Examples | How it is built |
|---|---|---|---|
| none | 4 | | read from the row |
| scalars | 63 | | read from the row |
| other records and enums | 34 | `TPanicLoc`, `TFailureCarrier`, `CoreCallRecord`, `BodyPattern` | built field by field by these same rules |
| lists | 24 | `[Int]`, `[Type]`, `[BodyCallArg]`, `[Bool]` | a side-table range in the part, one allocation each |
| `String` | 12 | `TraitConst`, `UnitConvert`, `MathBuiltin`, `TryWithContext`, `PoolSlot` | copied |
| `Type` | 8 | | through the body's type table |
| symbol or local | 8 | | through the body's `SymbolKeys` |
| `DeclID` | 2 | `Call`, `GlobalShared` | resolved by symbol |
| `BodyLambda` | 2 | `Lambda`, `SharedGuardWait` | a nested record of about 20 fields, built recursively by the same rules |
| `TComptimeValue` | 1 | `CtLit` | a tagged value tree, built node by node. Its strings are copied. |

**Per-body tables.** A body's `TypeTable` ships as a range of interned
type indexes and is built once per loaded body. Its `SymbolKeys` ships as a
range of keys and is remapped once per loaded body. Node fields then index
those tables directly, as they do today.

**A loaded body is owned and mutable.** Whole-program passes restamp
fields of every body (3.3.1). So loading builds a full `FnBody` that sema
owns. That cost is real, and it is measured:

- **Yardstick:** checking a kept Core body from text costs 0.84 to 0.96 ms
  of traced CPU (1.4).
- **B1 prototype:** before B1 lands, a prototype loads the generic bodies
  that the 6.2 programs keep. It reports the median ns per body to load,
  against the median ns per body to check the same body from text.
- **B1's gate:** loading costs at most a tenth of the check, per body, at
  the median. If it costs more, B1 does not land in that form. The record
  failed at exactly this point.

**The codec.** A checked body is written and read by a generated binary
codec, `Compiler/JetFoundation/Source/Facts/BodyCodec.jet`, written by
the tool package `Compiler/Toolchain/BodyCodec` from the struct and enum
shapes `FnBody` and `BodyExpr` reach (`--check` is a land ratchet). Each
struct writes its fields in declaration order, each enum its variant's
position first; integers are LEB128 uvarints, an `Int` zigzagged. The
hand-written half, `BodyCodecRows.jet`, writes each `SymbolID`, `DeclID`
and `ModuleID` as its position in a list kept beside the bytes, so the
caller resolves ids to keys once per body, not once per use. A body-local
symbol is written as it is. A read past the end or of a malformed value
sets `failed` and later reads give zeros and fallback variants, so a bad
part ends the read without a panic. The same tool writes a second codec,
`Compiler/JetSema/Source/Sema/BodyPacketCodec.jet`, for what a function's
check leaves for its module pass besides the body (`SemaParallelBody`: the
check's result, diagnostics, call facts, comptime requests and name
references). A shipped packet stands in for the check the way a packet from
another compiler thread already does. That codec imports the body's puts
from JetFoundation and adds only the JetSema types.
`Compiler/JetSema/Tests/BodyCodec.jet` round-trips every checked body of a
program and a packet per checked function.

**The capture.** The Core library's check (`JetCli/CoreLibrary.jet`, with
`SemaRegistrationGraph.core_seal` set) captures each function check of a
Core module or of the canonical effect package as its packet, right after
the check and before the module pass reads it
(`Compiler/JetSema/Source/Sema/CoreBodies.jet`). Those checks run on the
compiler threads as any other module's do. Each thread checks on a program
with rows of its own for the codecs and derived methods its checks demand,
and each packet carries its check's demand, which the module pass records
again as it takes the packet; a body the pass checks itself carries the
demand the pass recorded under its capture. The packet is written, then
drained back into the module's rows, so the capture moves rows and copies
none. A method's packet carries no method input: its diagnostics already
hold its preparation's, as a prepared packet's do, and a compile recomputes
the input against its own memory plan. A packet's id lists are written as
keys in that check (symbol and declaration keys, module identities). The
library writes the captures as `{prefix}.bodies`: a header, the context, a
table of record offsets, then one record per function in key order. A Core
demand wave can check a function the body pass checked before, against the
graph as it has grown since; the file keeps the last check, the one the
library keeps. The library reads the file back whole before it writes it
and fails the build on any problem, a repeated or misplaced record among
them. A body the comptime pass checked first and the module pass reused is
not captured, and is checked from text as before.

**The load.** The toolchain ships the bodies file as `core.bodies` beside
the Core seal, a part of every jet the compiler build makes. The file
opens with the facts of the capturing compile that a check reads beyond
the Core text: target triple, profile, OS, keep-going and test modes
(`sema_core_bodies_context`). Two compiles read the file's header
into `SemaRegistrationHostHooks.core_bodies` (`JetCli/Request.jet`): a
native compile already bound to the built-in Core, and a `jet check` of a
program whose Core is the built-in one. Neither may use `--deep-checks`. A
compile whose facts are the file's keeps its Core modules' checks on the
module pass's thread, so no compiler thread checks a body the pass then
loads (`core_bodies_held`). In place of a Core function's check it finds
that function's record by halving the table and loads its packet
(`sema_core_body_loaded`) when every key resolves in its tables. It reads
only the records it looks up, and counts their bytes as
`core.bodies_read_bytes`. The packet then drains as a prepared one does.
This holds in the module pass and in each Core demand wave, so a kept
generic, function-value, lowered or comptime body loads too. Any miss
checks the function from text, as does a stand-in of a run bound to the
Core image, which is checked by its signature alone. A shipped file whose
header does not read refuses the compile as an internal compiler error, and
so does a record or packet that does not read, since the build read them
all. A load records the packet's demand again
(`sema_demand_capture_replay`), as a reused comptime check does. A
loaded body counts as `core.bodies_loaded`, not under `bodies_from_text`.
A run whose tree's Core has not been compared yet checks its Core bodies
from text. That run still reads its tree's Core only when its demand names
a deferred body. A `jet build` loads as a run does: the one check that
tells a shipped artifact's compile apart, the refusal of a `debug` trace
(E3405), the Core library's check makes too, so no captured body holds one.

#### 3.3.1 Fields that whole-program passes write (H10)

These `FnBody` fields are written after a body's own check by passes that
see the whole program. They were found by a search for every assignment to
an `FnBody` field in JetSema and JetDriver at `313533b9f3`.

| Field | Written by | Rule |
|---|---|---|
| `web_bucket`, `web_reason` | `WebPartition.jet` 120 to 121 | cleared in the shipped row; the partition pass recomputes them, as today |
| `effects` | `Effects/Checks.jet` 5821 | shipped as Core's own solved row; the whole-program effect solve recomputes it, as today |
| `effect_scope`, `heap`, `stack`, `frame_bound` | `Effects/Checks.jet` 5822 to 5825 | cleared; recomputed by the same solve, as today. They depend on the program's `#FX` frames and stack denials. |
| `failure_carrier` | `CheckProgram.jet` 4117 (close methods) and 11062 (failure reconcile) | shipped; the failure solve restamps it, as today |
| `effects`, `effect_scope`, `masked`, `web_bucket`, `web_reason`, `fixed_tags` of a comptime-spliced body | `Comptime/LocalSplice.jet` 389 to 394 | only for bodies spliced in this compile, never a shipped Core body |
| `masked`, `fixed_tags` | `CheckProgram.jet` 11984 (`sema_mark_masked_branches`, from the module's own taint state) | shipped. Both come from the body's own module. |
| `gc_scope`, `gc_return` | `FunctionMemory.jet` 285 to 287 (the policy ladder) | shipped if a Core function's ladder has no program rung. B1 checks this on Core; a rung found there makes the fields recomputed. |
| `stand_in` | `CheckProgram.jet` 2102 | cleared; set at load for a bound body, as today |

**The body guard (7.2) compares only the shipped fields.** The cleared and
recomputed fields are compared after the whole-program passes, against the
text path's values on the same program.

**These passes are not new work.** Today they already walk every Core body
the compile checked from text. Traced p155 thread CPU:

| Program | `sema.effects` | `sema.failure` |
|---|---|---|
| hello | 0.17 ms | — |
| fread | 3.99 ms | 1.18 ms |
| Summarize | 4.45 ms | 1.86 ms |
| RunColdContext | 17.30 ms | 3.38 ms |

The web partition has no span of its own today. A0 adds `sema.web`.

Step B swaps a check from text for a load. It walks the same set of
bodies, so these passes cost the same before and after. The 6.2 A/B of each
B step times them.

### 3.4 Size (G11, G12, H11)

The estimate below is computed from the record audit's counts, with 4-byte
words:

| Part | Count | Words per row | Estimate |
|---|---|---|---|
| Declarations (functions and methods) | 19,643 | 22 | 1.73 MB |
| Parameters | 37,164 | 6 | 0.89 MB |
| Nominals / fields / variants / impls / method tables | 1,664 / 5,670 / 1,436 / 2,618 / 3,654 | 12 / 6 / 6 / 8 / 2 | 0.36 MB |
| Type table and type arguments | 6,296 | 8 | 0.20 MB |
| Exports / imports | 16,233 / 1,405 | 3 | 0.21 MB |
| String table (identifiers and literal texts, unpacked) | 35,250 + 42,985 | — | 1.63 MB |
| Name index (2^16 slots of 2 words, load factor 0.55) | 36,000 names | 2 | 0.52 MB |
| Reach spellings (distinct per top-level item, from an awk census of `Core/**/*.jet`) | 290,478 | 1 | 1.16 MB |
| Seal facts and bound symbol names | 14,574 seal lines, 9,635 symbols | 4 + name | 1.10 MB |
| Constant values (the audit's figure) | 3,529 | — | 2.94 MB |
| **Step A** | | | **about 11 MB** |

**Step B is a floor, not an estimate.** Review 2 listed what revision 3
left out. The known parts:

| Part | Count | Estimate |
|---|---|---|
| Body nodes, 7 words each | 695,467 | 19.5 MB |
| `expr_types`, 2 words per node | 695,467 | 5.6 MB |
| Statements, 5 words each | 160,087 | 3.2 MB |
| Checked defaults | about 1,020 | 0.3 MB |
| **Known so far** | | **about 29 MB** |

These parts are not counted yet:

- the 52 `FnBody` fields;
- the per-body type tables and `SymbolKeys`;
- the side tables for list, string, record and lambda payloads.

The node and statement counts come from the record audit, not from today's
Core.

**The census before B1 (H11).** A0's census (section 10) also walks every
Core and Prelude body that the runtime step checks today. It counts, from
the checked bodies:

- bodies, nodes, statements and `expr_types` rows;
- per-body type-table and symbol-key entries;
- list elements, string bytes, lambda records and comptime values by
  payload kind;
- set header fields.

It reports bytes at the 3.3 word widths.

**Byte limits:**

- **A1:** the step A part is at most **16 MB**. A1 fails over it.
- **B:** the body rows are at most the census figure plus 25%. **B must
  also fit the jet limit below.** If the census figure plus A's measured
  size exceeds 40 MB, the B rows are narrowed before B1 lands. The order
  is:
  1. 16-bit node words where the field allows it;
  2. per-body type and symbol tables shared across bodies of one module;
  3. only then, a variable-length encoding of the side tables.
- **jet:** with Core text still embedded (3.5), jet grows by at most
  **48 MB** across A and B, about 18% of its 264 MB at p150. Every step
  records the growth.

The 32 MB B limit of revision 3 is withdrawn. It was set against a floor
that omitted whole columns.

### 3.5 What stays embedded

The embedded Core text (`JET_CLI_CORE_SOURCES`, about 20 MB) stays. Three
things use it:

- diagnostics that quote a Core line;
- the tree match test (section 5);
- the jet build that writes the interface.

On built-in Core, a compile reads it only to render a reported diagnostic
that points into Core. Main ruled that this is display, not reading Core
source (H12). A clean compile reads 0 bytes of it. The counter
`prelude.text_bytes_scanned` (A0) and a matching `core.text_bytes_read`
count every other read.

### 3.6 Compiler-owned Prelude texts: step P, landed (G4, G5, H13, H14)

The texts in 1.3 are owned by the compiler, not by Core. They are fixed
when jet-bare is compiled, so they become compiled data then. Both changes
are on dev:

- **`b221bb6d88`, the effect vocabulary.**
  - `EFFECT_CANONICAL_MODULE` holds the text, its line starts, and the
    `EffectDecl` items the parser would build, spans included, made by
    `#run`.
  - `jet_driver_canonical_effect_module` builds the module from those rows.
  - The request field `canonical_effect_source` became `canonical_effects`
    in every request builder.
- **`a568b242df`, the other texts.** Each was made by `#run`:
  - the marker rules;
  - the marker declarations;
  - the derive words and providers;
  - the error family's program and line starts;
  - the unit catalog.

  The units outline scanner is deleted.
- **Generated rows.** A `#run` that calls the parser makes the first
  compile of compiler sources check the parser's whole closure (about 1,537
  functions): 1.48 s of comptime in a `jet check` of
  `Compiler/JetCli/Source/Cli/InspectBuild.jet` on p156. So the six rows
  that need the parser or a reader are now Jet literals in
  `Compiler/JetSema/Source/Sema/PreludeRows.jet`, written by the tool
  package `Compiler/Toolchain/PreludeRows`. The tool is not a build step,
  since one would compile JetSema in every compiler build. The two cheap
  ones stay `#run`: `MARKER_SOURCE_RULES` (a scanner) and the error line
  starts.

**What replaces the effect module's source in sema (H14).** The text stays
in `module_sources`, so diagnostics that point into the effect vocabulary
keep the same bytes and spans. A drift test checks this
(`Compiler/JetDriver/Tests/CanonicalEffectModule.jet`): the parser's
`EffectDecl`s of the module text must equal the build-time rows, spans
included. Compiler queries and scripts take the same `canonical_effects`
rows.

**Derive providers (H13).** `SEMA_PRELUDE_DERIVE_PROVIDERS` holds the
providers' declarations as generated data. So no compile parses
`Derives.jet`, whether or not the program derives. Review 2 asked that the
corpus claim either cover the providers in P or move to B3. P now covers
them.

**What P leaves.** The provider `expand` bodies, the error family's bodies
and the effect module's bodies are still checked by sema in each compile,
from the build-time syntax rows. That is not text scanning, but it is
re-checking. Those bodies are categories in section 4 and ship checked in
B0.

**Drift guards.** `Compiler/JetParser/Tests/PreludeBuildRows.jet` holds four
drift tests over the stored rows. `jet Compiler/Toolchain/PreludeRows/Main.jet
--check`, one of the ratchets every commit keeps, fails on a stale row.
`Toolchain/PreludeSources.jet --check` keeps the generated Prelude sources
current.

### 3.7 Request builders (G6, H15)

Every request builder that carries Core sources or the effect rows today
(dev `313533b9f3`):

| Builder | Carries |
|---|---|
| main request, `jet_cli_driver_request` (`Request.jet` 1366, fields at 1381 to 1382) | `canonical_effects`, `core_sources` |
| CLI sources for a compile (`Request.jet` 1991) | `jet_cli_core_sources` |
| comptime child, both copies (`ComptimeCompiler.jet` 138 to 139 and 272 to 273) | `canonical_effects`, `core_sources` |
| compiler queries (`CompilerQueries.jet`, through `jet_driver_registration_module_inputs` at 274) | the loaded Core modules |
| scripts (`ScriptHelpers.jet` 257) | the loaded Core modules |
| config evaluation (`ConfigEval.jet` 41) | `jet_cli_core_sources` |
| `jet inspect` compiler and facts (`InspectCompiler.jet` 85; `InspectFacts.jet` 172, 367, 400) | `jet_cli_core_sources` |

The effect rows are already carried as rows (`b221bb6d88`).

On built-in Core, A2 changes every builder in the table to carry the lent
interface part instead of `core_sources`. Each then loads declarations,
seal facts and bodies through the same reader as the main compile. No Core
text is copied into any request.

**A2's proof** has three parts:

- a test per builder that its request holds no Core text on built-in Core;
- a compile of a comptime case, a compiler query, a script and a config
  file, each with `core.text_bytes_read` = 0;
- `JetDriverCoreSource` disappears from built-in requests.

## 4. What needs a Core or Prelude body, and how each gets one (G15, G25, H2, H10)

After step A, a compile has every Core declaration without text. Each row
below still needs a body. The count comes from one of these sources, as the
row says:

- `core.facts`. At p150 it holds 16,169 keys: sealed 8,978, lowered 5,783,
  generic 743, function value 653, intrinsic 11, open dispatch 1.
- a trace;
- a source census.

| Category | Why a body is needed | Count | Source of the count | B step |
|---|---|---|---|---|
| Core Prelude bodies | `Core/Prelude/Prelude.jet` (17 functions) is checked in every compile | hello: `sema.bodies > Core/Prelude/Prelude` 0.40 ms CPU | trace (p155) | B0 |
| Compiler-owned Prelude bodies | `Prelude/Errors` bodies and comptime items (`Err.__jet___memo_error_metadata`), the effect module's bodies, derive provider `expand` bodies | hello: `Prelude/Errors` 0.14 ms, `Prelude/Effects.jet` 0.07 ms | trace (p155) | B0 |
| Module `core` | the ABI unit's bodies | hello: 0.07 ms | trace (p155) | B0 |
| Generic functions | instantiated for the caller's type arguments | 743 keys | `core.facts` | B1 |
| Lowered packages (35 in `JET_CLI_CORE_LOWERED`) | not compiled into `jet_core.o` | 5,783 keys | `core.facts` | B2 |
| Function values | a value needs the body, not a call binding | 653 keys | `core.facts` | B2 |
| Intrinsics | kept in demand today (5 per program in 1.4) | 11 keys | `core.facts` | B2 |
| Open dispatch | dispatch through an open set | 1 key | `core.facts` | B2 |
| Sealed keys with no stand-in line (view provenance, unclean failure union) | the seal cannot state their facts | 406 keys | `core.facts` against `core.seal` | B2 |
| Unlisted keys (keys `core.facts` lacks) | generated or nested items the facts do not list | 18 (fread, Summarize), 19 (RunColdContext) | trace `kept.unlisted` | B2. A1 adds them to the facts |
| Trait-impl methods | checked up front today | 3,654 methods in 2,618 impls (455 impl blocks in Core source) | record audit, source census | B3 |
| Generated derive and codec bodies | generated per type at registration | Summarize: 448 codec candidates, 236 derives deferred | trace | B3. Core types get checked bodies generated at jet build. User types get bodies from providers whose bodies ship checked (B0). |
| `close` methods | lowering roots by owner | 52 | source census | B3 |
| Other lowering roots (`display`, `debug`, `next`, `show`, `compare`, `read_line`, the `__Decoding` reads, name-kept helpers, every function of a module with foreign rows) | lowering selects them with no call in a checked body (`jet_driver_core_lowering_roots`) | 87 owner-selected methods besides `close`, plus the name-kept and foreign-module functions (A0 counts these) | source census, A0 counter | B3 |
| Shared-global initializers | a shared cell's initializer runs at first use | 3 shared sites, 1 memo | `core.exports` | B3 |
| Error conversions (`impl A -> B`) | converted at `?` sites | 9 | source census | B3 |
| Template items (declaration loops, generic modules) | expanded per use | 0 in Core today | source census | the B step of its kind, if one appears |
| Default parameters and fields | checked in the owner's scope at each call site | at least 124 functions, about 895 fields | source census | B4 |
| Comptime calls into Core | the compile-time run needs the body | RunColdContext: 55 | trace `kept.comptime` | B5 |
| `jet build` (`-O1`, `-O2`) | builds every reached Core body | RunColdContext: 2,123 bodies under `jet check` | trace | B5 |
| Per-program facts on loaded bodies | not a body from text. The whole-program passes of 3.3.1 restamp every loaded body. | every loaded Core body: 169 (fread), 212 (Summarize), 1,538 (RunColdContext) | trace `sema.core_checked` | not removed. Their cost is today's cost (3.3.1) and is timed in each B step's A/B. |

**The counter.** A0 adds `core.bodies_from_text.<category>`, with these
rows as its categories, the Prelude rows included. It also covers bodies
checked up front, which today's `sema.core_bound.kept.*` counters do not.
In this note, "Core bodies from text" means that counter's total. For the
Prelude rows it counts bodies checked from the build-time syntax rows of
3.6 as well, because those are re-checks.

**Step A alone leaves almost every program short of c22.** 1.4 shows that
`fread.jet`, a 6-line `core.files` program, still checks 79 Core bodies from
text. So step B is required:

- **B lands in a form that meets c22.** Section 6's speed gate ranks the
  forms of B:
  - per-body load against per-module load;
  - word widths;
  - whether up-front bodies also load on demand.
- **No B step that makes a 6.2 program slower merges until the owner has
  ruled.** The ballot sets out the c22-against-c37 conflict with that
  step's numbers (H16; Main's ruling). The ballot is drafted with Main. Its
  options, numbers and edge cases are there, not here.
- **Instance facts (H17).** A generic body is checked once, generically, at
  jet build. Effect and failure facts for an instance come from the
  whole-program solve over the checked body and the type arguments, as
  they do today for a text-checked generic body. B1's proof compares
  them with today's results (section 10).

### 4.1 Bodies that stay on text after A2

A2's proof is that `parse.core_input_bytes`, summed over the load's
waves, is 0 (section 10). Until its B step lands, each row of the table
above still checks its bodies from text, and every row of B5 does so until
B5. After A2, such a body is found in one way, and none of it parses a
wave.

- **The declaration comes from the interface.** Its name, signature,
  owner and full byte span are an interface row (3.1). Registration,
  reach and seal facts read that row, never text.
- **The body comes from its own declaration's text, parsed alone.** For
  each reached declaration whose body sema checks from text, the load copies
  the unit's embedded text and blanks every byte outside that declaration's
  span. Inside a type or impl block, it also blanks every other member's
  body. Then it parses the copy. The parsed item supplies the body, and the
  interface row stays the declaration. The bytes keep their offsets, so
  spans, line tables and diagnostics are the unit's.
- **The path is today's path, minus the head parse.** The two halves are
  already code. A held top-level declaration is parsed alone, after reach,
  by `jet_driver_reach_parse_held` (`CoreReach.jet`). A lazy module's kept
  bodies parse back with every other body still blank, through
  `jet_driver_core_restore_bodies` (`CoreInterface.jet`). A2 removes only
  the head parse that comes before them. It also limits the parse-back to
  the bodies sema checks from text. A body that the seal stands in for
  binds to `jet_core.o` and is never parsed.
- **It is counted where it can be seen.** Each such body counts once under
  `core.bodies_from_text.<category>` when sema checks it. The bytes it parses
  count under `core.text_bytes_read` (3.5), not under
  `parse.core_input_bytes`. A2's 0 is therefore not a claim that no Core
  text is read. It says that no declaration comes from text, and that the
  only Core text read is the text of these counted bodies. Each B step
  brings its categories to 0. After B5 a clean compile reads 0 bytes, which
  is c22's claim (section 10).
- **`jet build` at `-O1` and `-O2` takes this path for every reached body
  until B5.** The census row `core-text-build` builds and runs a Core-using
  program at both levels and requires `core.bodies_from_text` above 0. So a
  B step that breaks the path fails that row at once.

The case
`tests/cases/core/a_core_body_kept_on_text_parses_back_alone_and_runs.case`
pins the path. In it, a DateTime's display, a lowering root kept on text
until B3, is checked from text and runs. `core.time` loads lazily, its held
`from_timestamp` parses alone, and its kept bodies parse back. When A2
lands, its proof adds that the same run's waves report 0 for
`parse.core_input_bytes`.

## 5. Toolchain trees (G1, G20, G21, H18, H19, H20, H21)

**Ruled (Main, on #6328): T1.** A compile uses a toolchain tree only when
one is named. Otherwise it uses built-in Core, even when the entry is
inside a checkout. A tree is named by one of:

- `JET_TOOLCHAIN_ROOT`;
- a Core test run;
- a compiler package build.

c22's strace clause has no exception for checkouts. So case (b) of G1 must
show 0 calls on the tree. Today jet finds the tree from the entry
(`jet_cli_toolchain_root(entry)`), which gives the 276 calls of 1.5. T1
removes that search for compiles.

The cost: a contributor who edits Core must name the tree. The agent
tooling and the build already do. `Land.jet`, `Suite.jet`, `VerifyFull.jet`,
`LaneCheck.jet`, `CompilerRegistries.jet`, `CoreModuleCheck.jet`,
`Chain.jet` and the orchestrator scripts all set `JET_TOOLCHAIN_ROOT`.

### 5.1 The callers of `jet_cli_toolchain_root` under T1 (H18)

These are all 14 callers at dev `313533b9f3`:

| Caller | What it serves | Rule under T1 |
|---|---|---|
| `Budget.jet` 419, `jet_cli_budget_stdlib_id` | the stdlib id of a budget run | named tree only. Otherwise the built-in Core's id. |
| `BuildCache.jet` 176, `jet_cli_build_cache_key` | the build cache key | named tree only. Otherwise the jet's runtime key. |
| `CoreSources.jet` 115, `jet_cli_core_binding` | whether a run binds to the built-in Core | named tree only. With no tree named the binding is built-in, with no test. |
| `Request.jet` 1003, `jet_cli_batteries_dir` | the batteries directory | named tree only. Otherwise the built-in batteries. |
| `Request.jet` 1227, `jet_cli_core_sources` | the Core sources of a compile | named tree, or the recognized Core test or compiler package root below. Otherwise built-in. |
| `Request.jet` 1310, `jet_cli_toolchain_tree` | the toolchain tree of a compile | as `jet_cli_core_sources` |
| `ScriptCache.jet` 105, `jet_cli_script_toolchain` | the Core root a script cache keys on | named tree only |
| `Request.jet` 438, `jet_cli_is_core_library` | `jet check Core` | recognition rule below |
| `Request.jet` 2033, 2052, 2073: `jet_cli_core_test_path`, `jet_cli_core_test_source`, `jet_cli_core_test_files` | Core test runs | recognition rule below |
| `Host.jet` 3264, `jet_cli_fmt_is_library_source` | `jet fmt` deciding whether a file is library source | not a compile. It keeps the search. |
| `Inspect.jet` 656, `jet_cli_inspect_coverage` | `jet inspect coverage`, from the current directory | not a compile. It keeps the search, so it works inside a checkout. |
| `EditorGrammars.jet` 82, `jet_cli_devtools_grammars` | `jet devtools grammars`, from the current directory | not a compile. It keeps the search, so it works inside a checkout. |

**Recognizing a Core test run or a compiler package build, without a
search.** The entry's own path decides:

- A Core test run or `jet check Core`: the absolute entry path has a
  component named `Core`.
- A compiler package build: the absolute entry path has a component named
  `Compiler`.

Only then is the directory that holds that component checked for `Core/`
and `Compiler/Prelude/`, with two `statx` calls. If it holds both, it is
the tree.

This test is on the path string. An ordinary entry, whose path has neither
component, makes no file system call. That is the hello case of the strace
proof. An entry such as `~/notes/Core/x.jet`, outside any checkout, costs
two `statx` calls on a directory that is not a tree, and then takes
built-in Core.

**The tools that keep the search** (`jet fmt`, `jet inspect coverage`,
`jet devtools grammars`) need the checkout's other files. They are not
compiles, so the strace clause does not cover them. Inside a checkout
they keep working without `JET_TOOLCHAIN_ROOT`, as today.

### 5.2 The three cases

With T1:

- **No tree named, inside a checkout or not:** built-in Core, through the
  interface after A2. No toolchain path is touched (G1, both cases).
- **A named tree** (`JET_TOOLCHAIN_ROOT`, a Core test run, a compiler
  package build), **whether its Core matches or not:** keeps today's lazy
  text path. The match test runs only where it runs today, when the
  compile first names a deferred Core body, and only to decide the run's
  seal binding. The cost of a named-tree compile is today's cost by
  construction.
- **Built-in Core with no tree** is the only case that loads the
  interface.

### 5.3 Why a named matching tree no longer loads the interface (H19, H20)

Revision 3 had a named matching tree load the built-in interface. The
match test then had to decide before any Core declaration loaded, so it
would run on every compile that names a tree. 1.5 times it as the jet
function: more than 12 ms (+4.7 to +33 ms wall on fread; 25 to 36 ms under
strace). Every golden, ui case, lane check and agent tool names the tree,
so all of them would pay that cost. That is a c37 regression created by
the design.

Cheaper decisions, judged against c22's "no runtime cache" rule:

| Decision | Cost | Sound | c22 | Verdict |
|---|---|---|---|---|
| Compare every file byte for byte, before load (revision 3) | more than 12 ms per compile (1.5) | yes | yes | rejected: c37 |
| Compare sizes, or stat data, against rows embedded in jet | about 0.3 ms (272 `statx` calls, already made today) | **no.** An edit that keeps a file's size binds the run to the wrong Core image: silent wrong code. | yes | rejected: unsound |
| Hash files, cached by stat data, as Cargo, ccache and Go do | small after the first compile | yes, if mtimes are honest | **no**, it is a runtime cache | rejected: c22 forbids it |
| Defer the decision until a deferred body is needed, as today; a named tree takes the text path for declarations | today's cost | yes | not applicable: a named tree is not built-in Core | **chosen** |

So 6.2's never-slower set includes the named-tree arms (H19). Their
expected change is none. The gate (6.2) checks that.

### 5.4 Two paths, and which lanes exercise which (H21)

Edited-tree and named-tree compiles never exercise the interface path. Most
suite runs name the tree, so they run the text path.

**The gate that catches a divergence** is the drift guards (7.2) and the
reach differential (7.3). They run in the build's tests on the jet the
build makes, whose built-in Core is the lane's Core. A lane that edits Core
lands through a gate that builds jet with the edited Core embedded and
writes its interface. The guards then compare the interface path with the
text path on exactly the changed Core.

A0 also adds a built-in arm to the full verification: the golden and ui
corpus once more with no tree named, outside the checkout. That arm runs
the interface path end to end.

**The 2026-11-05 rule is not adopted.** That rule stops an edited tree with
an error that names the rebuild step. Each Core edit would then need a jet
runtime build before any program could try it, which costs minutes and
several GB per edit.

**T1 lands on its own.** It does not depend on the interface, so it lands
before A2 (step T in section 10).

## 6. Budgets and gates

### 6.1 hello (G13, G24, H22, H23, H24, H25)

On built-in Core, against the jet before:

- **(a) CPU budget.** The CPU of all Core- and Prelude-attributable work is
  at most **1 ms** of thread CPU (user plus sys). This counts:
  - interface mapping;
  - row loading and materialization (3.3);
  - the compiler-owned Prelude modules;
  - seal facts and reach;
  - the per-module sema passes over Core and Prelude modules.

  Today's measured part is 2.24 ms, plus the Prelude share of `register`
  (1.2). The steps reduce it as follows:

  | Step | Removes | Measured share removed |
  |---|---|---|
  | A2 | the Core Prelude parse and the `core` unit parse; registration of the Core Prelude from text | 1.43 ms, plus the Core Prelude's share of `register` |
  | B0 | the body checks of the Core Prelude, `Prelude/Errors`, the effect module and `core` | 0.68 ms |

  What remains are the per-module `sema.check` passes over four modules
  (0.13 ms) and the interface load itself.

  **The budget is gated at B0, not A2.**

  - A2's gate is weaker. The Core- and Prelude-attributable CPU must fall
    by at least the 1.43 ms parse share, and no other hello span may grow
    beyond noise.
  - If A0's split of `register` shows that the Prelude share left after A2
    exceeds 0.6 ms, 1 ms cannot be met at B0. Main then rules a different
    budget before B0 starts.
- **(b)** No other hello span grows beyond noise.
- **(c)** Process user plus sys does not rise beyond the 6.2 band. It is
  measured as untraced warm runs, median of 11. Five cold runs are reported
  for major faults and sys (G14).
- **(d) RSS:** the 6.2 rule (H25), with no separate hello rule. hello's
  own peak at p155 is 63.0 to 64.1 MB in the sets of 1.1.
- **(e) Pages touched:** the counter `interface.pages_touched` counts
  distinct 4 KB pages of `core.interface` read during the compile. It is
  kept in a bitmap while tracing. **The limit for hello is 64 pages
  (256 KB).** `interface.bytes_touched` is reported beside it.
- **(f) Faults (H8):** A2 reports major and minor faults and the resident
  pages of `core.interface` (`mincore` at exit, while tracing) for hello,
  cold and warm, against the jet before.

**How the load is timed (H22).** Thread CPU, never a wall clock.

- Interface loads are grouped into spans:
  - `core.interface.load` for the setup;
  - one `core.interface.rows` span per reach wave (hello has three
    waves);
  - `core.interface.bodies` for body loads (B).
- Spans already report per-thread `cpu_ns`, from
  `CLOCK_THREAD_CPUTIME_ID`. On Linux that clock is a system call, not a
  vDSO read, but a few spans per compile cost microseconds.
- No load is timed per row.
- hello is single-threaded (1.1: no `clone` under `strace -f`). So its
  span CPU is its process CPU. A0's proof repeats the `strace -f` check on
  the A0 jet.
- For programs that start compiler threads (fread starts 12), the budget
  sums `cpu_ns` over every thread's interface spans.

### 6.2 Other programs (G24, G26, H4, H19, H25)

- **core.files program** (`fread.jet`, 1.4): own peak RSS stays within the
  RSS rule below of today's 207.4 MB (p155) after A2. Each B step must not
  raise it beyond the band.
- **Never slower (c37), defined for every step of A and B:**
  - **programs:** hello, `abs.jet`, e2404, lint, e3401, RunColdContext and
    jetc-trace Summarize, plus `fread.jet`;
  - **commands:** `jet check` and `jet run`;
  - **arms (H19):** each program runs in three arms:
    - built-in Core, outside any checkout;
    - `JET_TOOLCHAIN_ROOT` naming the matching tree the jet was built
      from;
    - `JET_TOOLCHAIN_ROOT` naming an edited tree (one comment changed in
      `Core/Files/Files.jet`).
  - **runs:** before each arm's warm set, its jet binary is read cold once.
    Then 11 untraced warm runs per arm, with the jet before and the jet
    after alternating run by run. Five cold runs per arm.
  - **time band:** a program is worse when its median wall, or its median
    user plus sys, rises by more than 3% and by more than 2 ms. The band is
    wide because the machine is shared (section 1).
    - The 2 ms band is 7% of hello. A hello overrun within it shows only in
      6.1's span gate, which is why 6.1 gates hello's Core and Prelude CPU
      by span.
    - A2 does not land if the named matching tree arm makes hello slower
      than today's named-tree hello beyond this band.
  - **RSS rule, one rule for every program, hello included (H25):** the
    statistic is the per-run own peak RSS from `wait4` (VmHWM), median of
    the alternating runs. A program is worse when that median rises by
    more than **1 MB or 1%, whichever is larger**.
    - The band is above the 0.25 MB difference between interleaved arms,
      which is what 1.1 measures. It is below the 1.6 MB drift between
      separate sets, which is why the arms alternate.
    - The cgroup peak is reported beside it.
  - **binary size:** within 3.4's limits. Generic instances shipped as
    checked bodies count toward B's limit. No instance ships as machine
    code in this note.
- **The measuring script** of 1.0 is checked in by A0 under
  `Tools/perf/core-interface/`, with the program set and the arms.

### 6.3 Who owns hello's "user time on the user code" (G17)

hello's untraced user plus sys is about 25 to 28 ms (1.1). After B0, Core
and Prelude work is at most 1 ms of it. The rest is fixed compiler work:
`register` 5.8 ms, `sema.finalize` 4.0 ms, `native.link` 2.8 ms, and so on.
No change to Core loading can move that.

**Ruled (Main, on #6328): the clause splits in two.**

- **This note, after B0:** Core- and Prelude-attributable CPU is at most
  1 ms (6.1a).
- **A new #6759 criterion, in c22's own wording:** more than half of
  hello's cold user CPU is attributed to user code. It is measured as
  untraced medians of at least 5 runs. No softer target stands in for it.

c22's hello clause is met when both are met. This note claims only the
first.

## 7. Identity and drift

### 7.1 Provenance (G18)

`provenance.json` (`Compiler/Toolchain/RuntimePack.jet`) records
`coreFactsSha256`, `coreSealSha256` and the other Core part digests beside
`runtimeKey`. A1 adds `coreInterfaceSha256` with them.

No check reads these keys today. A1 adds one to the runtime step. It
recomputes each Core part's digest from the image it embeds and fails on a
mismatch. The release gate runs that check, so the interface is part of the
toolchain identity, as c22 requires.

### 7.2 Drift guards (G19, H10)

- **Round trip (A1):** for every row, writing it and reading it back gives
  the same row.
- **Against the text path (A1, not circular):** for every Core and Prelude
  declaration, the registration rows loaded from the interface equal the
  rows the text path registers from source.
  - This runs in the build's tests on the built-in Core.
  - The text path stays for named trees (section 5), so the guard stays.
- **Bodies (B):** for every body B ships, the loaded body equals the body
  sema checks from text.
  - The comparison covers the shipped fields of 3.3.1.
  - Cleared and recomputed fields are compared after the whole-program
    passes, on the 6.2 programs.

### 7.3 Reach (G27)

Before A2 lands, a differential guard runs text reach (today's `CoreReach`)
and interface reach on every corpus program. The reached sets must be
identical.

Impls are not reached by spelling. A reached nominal type loads every impl
in its impl index row (3.1). If a reached type has an impl row that was not
loaded, that is an internal error, not a fallback.

**The hazard this closes.** `ir_core_method_lowered_by_owner` names it:
when a Core type's own `impl Debug` is not rooted, the value renders in the
derived struct form instead. That is a silent change of meaning.

**The proof case.** A case prints `{value:Debug}` of a Core type that has
its own `impl Debug`. It must print the impl's form. A run with that impl
row dropped from the interface must fail with the internal error.

**Counts (H6).** A0 counts the Core rows hello and fread register today, by
kind: declarations, impl rows, method tables and strings. The trace already
has part of this:

| Program | `load.core_items` | Reached | Held | Demands | Impls (reached) |
|---|---|---|---|---|---|
| hello | 13 | 0 | 0 | 1 | 0 |
| fread | 2,547 | 680 | 865 | 42 | 9 (5) |

A2's proof gives the interface rows loaded by kind, and their CPU, next to
the 6.1 budget.

## 8. One meaning across tiers (G22, G23, H26)

- **Tiers:** every golden and ui row gives the same output and diagnostics
  under each of these, with the interface path on:
  - `jet run` (O0, seal stand-ins, image code);
  - `jet build -O1`;
  - `jet build -O2`.

  That is a proof of A2 and of each B step. Before B, `jet build` checks
  reached bodies from text. The proof shows that the declarations it builds
  against come from the interface.
- **Web and Wasm (H26):** every corpus program that opts into the web
  partition (A0 lists them) produces, with the interface path on:
  - the same partition (`web_bucket` and `web_reason`, as
    `jet inspect choices` prints them);
  - the same output, against the text path.

  The partition fields are cleared in shipped bodies and recomputed
  (3.3.1), so this is the proof that the recomputation is complete.
- **Comptime:** compile-time code resolves Core through the same interface
  and the same seal facts as the main compile (3.7). A case calls the same
  Core function at comptime and at run time. It prints identical results
  at O0, O1 and O2.
- **Diagnostics (G10):** spans are full byte spans into the embedded Core
  text. Every diagnostic that points into Core must be byte-identical to
  today's on the ui corpus. That is a proof of A2.

## 9. Alternatives considered (G28, H20)

- **Separately compiled generics** (Swift witness tables and value
  witnesses, OCaml's uniform representation, Go's GC-shape stenciling with
  dictionaries). Each would let generic Core code live in `jet_core.o`, so
  no generic body would ship.
  - **Costs:** a second lowering for generic code (dictionary passing, or
    boxing to a uniform representation). Generic Core code at user call
    sites gets slower: Swift relies on specialization to recover speed, and
    Go's dictionaries cost indirect calls. OCaml's uniform representation
    would change value layout, which is a language change.
  - **Not proposed.** Jet monomorphizes (JetIR `Build.jet`), and each of
    these changes language-level performance. Any of them would go to an
    owner ballot.
- **Checked generic bodies (Rust MIR, Go unified IR):** adopted, as B1.
  Generic is the largest kept reason in 1.4.
- **Machine code for common generic instances** (2026-11-05 note, 4.3):
  - **For:** fewer bodies to load.
  - **Against:** it does not remove the need for checked bodies (other
    instances, `jet build` at O1 and O2), and it adds binary size.
  - **Verdict:** deferred. It may come back as a speed measure once B
    lands, inside B's size limit.
- **Parsing Core text at compile time, by declaration or header-first:**
  cheaper than today, but c22 forbids it, because it scans source.
- **The retired record with lazy decode:** its decoding cost more than
  parsing, and its places needed a syntax tree (section 2).
- **A mapped object graph (Lean `.olean`, Java class data sharing):** fast
  to map, but sema's tables carry process-specific ids. Making them
  position-independent changes every pass. Fixed rows remapped on first use
  give the same laziness.
- **A cache per program or per user:** ruled out by c22 ("no runtime
  cache") and by the no-incremental ruling on #2517.
- **An edited tree that writes its own interface:** a full Core check per
  compile (review 1, C8), so dropped (section 5).
- **Deciding a tree match by stat data or sizes (Cargo, ccache), or by a
  stat-keyed hash cache (Go):** unsound, or a runtime cache (5.3).
- **Per-row timing with a monotonic clock:** wall time, not CPU. Replaced by
  CPU-timed spans per wave (6.1).

## 10. Landing order and proofs (H1, H13, H22)

Each step is its own submission, with 6.2's A/B against the jet before it.
Every proof below can fail on today's jet. The last column gives today's
value.

| Step | Change | Proof | Today |
|---|---|---|---|
| P | **Landed** (`b221bb6d88`, `a568b242df`, then the generated rows): compiled rows for the compiler-owned Prelude texts (3.6) | each drift test passes: `CanonicalEffectModule.jet`, `PreludeBuildRows.jet`, the `PreludeRows` ratchet | — |
| A0 | Spans: the CLI front, part mapping, exit, `register` by module, `sema.web`. Counters: `core.bodies_from_text.<category>`, `prelude.text_bytes_scanned`, `core.text_bytes_read`, `interface.pages_touched`, `interface.bytes_touched`. The census below. The measuring script and the built-in verification arm. No behavior change. | the hello traced table attributes at least 95% of user plus sys; the census report; hello is single-threaded under `strace -f`; `prelude.text_bytes_scanned` = 0 for hello and the corpus | 65% attributed |
| T | T1 (5.1): a tree is used only when named | `strace -f` of `jet run hello.jet`, entry inside a checkout, no tree named: 0 calls on the tree's `Core/` and `Compiler/Prelude/`. `jet inspect coverage` and `jet devtools grammars` still work inside a checkout. | 276 calls |
| A1 | The writer runs in the runtime step and writes `core.interface`. `coreInterfaceSha256` and its digest check (7.1). Compiles do not read the part yet. | size at most 16 MB; two builds give identical bytes; the round-trip and text-path guards (7.2); the reach differential and the impl case (7.3); ns per field read at most 3 (3.2) | no part |
| A2 | On built-in Core, reach, registration, constants and seal facts read the interface. Core text parsing, hold stubs and the strip of unreached items go away on built-in Core. Every request builder carries the part (3.7). | see the list below this table | 11,256 B for hello; 276 calls inside a checkout |
| A3 | `jet check` and `jet test` take seal facts for sealed bodies on built-in Core, as `jet run` does | never slower; diagnostics identical on the case corpus | — |
| B0 | Checked bodies of the Core Prelude, the compiler-owned Prelude modules and module `core`, including derive provider `expand` bodies | `core.bodies_from_text.prelude` = 0 for hello and the corpus; 6.1(a) at most 1 ms | 0.68 ms of body checks in hello |
| B1 | Checked generic bodies | `core.bodies_from_text.generic` = 0; the load gate of 3.3 (at most a tenth of the check per body); the body guard (7.2); instance facts after the whole-program solve equal today's on every generic Core function the corpus instantiates, with each case that needs more than substitution named (H17); 6.2 | 52 to 260 kept per program |
| B2 | Lowered packages, function values, intrinsics, open dispatch, unsealed and unlisted keys | those categories are 0 | 27 to 331 kept per program |
| B3 | Impl methods, generated derive and codec bodies, `close` and other lowering roots, shared initializers, error conversions | those categories are 0 | not counted yet (A0) |
| B4 | Checked defaults | a case that calls a defaulted Core function and builds a defaulted Core struct has 0 Core bodies from text (G7) | not counted yet (A0) |
| B5 | `jet build` (`-O1`, `-O2`) and comptime calls take bodies from the interface | `core.bodies_from_text` = 0 under `jet build` for the 6.2 programs; the tier proof (section 8) | 55 comptime bodies in RunColdContext |

**A2's proofs:**

- `parse.core_input_bytes`, summed over every wave, is 0 for hello and the
  corpus. It is 11,256 B for hello today (1.3).
- The hello budget gate of A2 (6.1a): the 1.43 ms parse share is gone, and
  no other span grows.
- Bodies that stay on text (4.1):
  - the kept-body case runs with 0 `parse.core_input_bytes` in its waves;
  - the census row `core-text-build` passes at `-O1` and `-O2`.
- 6.1(e) and (f).
- The c22 strace of hello, outside a checkout and inside one.
- The request-builder proofs (3.7).
- The tier, web, comptime and diagnostic proofs (section 8).
- 6.2, including the named-tree arms.

Any B step that makes a 6.2 program slower waits for the owner's ruling
(section 4).

**The census (G16, H1).** A0 runs `jet run` over the example and gauntlet
corpus and over the 6.2 programs. For each program it reports:

- `core.bodies_from_text` by category, under step A as planned;
- `prelude.text_bytes_scanned`;
- `parse.core_input_bytes`, summed over the waves. It counts
  `Core/Prelude/Prelude.jet`. No compile parses the canonical effect source
  since `b221bb6d88`. Its text is read only to render a diagnostic, which
  `core.text_bytes_read` excludes as display (3.5).
- calls on any toolchain tree, from `strace -f -e trace=%file` of the run.

Before A2 lands, the census report gives:

- how many programs would have every count at 0 under step A;
- which category blocks each of the rest.

**c22 is claimed when the census, rerun after B5, reports 0 in all four
counts for the whole c22 program set:** hello, the 6.2 programs, and the
example and gauntlet corpus, under `jet run` on built-in Core.

What the claim does not cover, stated (R0):

- `jet check` over the corpus is outside it. A3 gives `jet check` seal
  facts.
- `jet build` over the corpus is outside it. B5 proves `jet build` on the
  6.2 programs only.

These are the bounds of the claim. They are not hidden gaps.

## 11. Risks

- **The load is not fast enough.** The record failed here.
  - Step A is gated by the 3 ns field gate (A1), the hello budget and the
    pages gate.
  - Step B is gated by the load-against-check gate of 3.3 (B1).
  - A B step that is slower overall waits for the owner (section 4).
- **Body size.** The B figure is a floor until A0's census. The census, the
  narrowing order of 3.4 and the 48 MB jet limit bound it.
- **Instance facts differ from today's per-program result.** B1's proof
  (H17), the body guard (7.2) and the tier proof (section 8) catch a
  difference. Any case that needs more than substitution becomes an
  instantiation rule in B1, named there.
- **Seal coverage.** A wrong seal fact gives a wrong effect or failure row
  with no body check. The seal comes from the same checker that a body
  check would use. A3 proves diagnostic parity on the whole case corpus
  before `jet check` relies on it.
- **Two paths for named trees.** The text path stays for every named tree.
  The guards of 7.2 and 7.3 run in the land gate's build, on the lane's
  Core, and A0's built-in verification arm runs the corpus on the
  interface path (5.4).

## 12. Review gaps answered

### 12.1 Review 2 (H1 to H26)

| Gap | Answered in |
|---|---|
| H1 claim covers every clause | section 10 (census: four counts; bounds stated) |
| H2 Prelude body categories | section 4 (three Prelude rows), B0 |
| H3 canonical effect source named | 1.3 |
| H4 per-run RSS | 1.0, 1.1, 1.4, 6.2 |
| H5 re-measured on its own base | section 1 (p155, base `49836bd978`) |
| H6 interface rows by kind | 7.3 counts, A2 proof |
| H7 ns-per-field gate | 3.2 (at most 3 ns, A1) |
| H8 faults and resident pages | 6.1(f) |
| H9 field kinds, load cost per body | 3.3, B1's gate |
| H10 fields written by whole-program passes | 3.3.1, 7.2 |
| H11 step B census and limit | 3.4 |
| H12 display ruling | framing (Main), 3.5 |
| H13 proofs that can fail | 1.3, section 10 (Today column), 3.6 |
| H14 effect module source | 3.6 |
| H15 every request builder | 3.7 |
| H16 owner ruling before a slower B step | framing, section 4, section 10 |
| H17 instance facts proof | section 4, B1 |
| H18 callers and recognition | 5.1 |
| H19 named-tree arms | 5.3, 6.2 |
| H20 match test timed as the function | 1.5, 5.3 |
| H21 edited-tree lanes | 5.4 |
| H22 CPU timing, threads | 1.1, 6.1 |
| H23 full hello attribution and the step that meets 1 ms | 1.2, 6.1(a) |
| H24 pages touched | 6.1(e) |
| H25 one RSS rule | 6.1(d), 6.2 |
| H26 web and Wasm tier | section 8 |

### 12.2 Review 1 (G1 to G28)

| Gap | Answered in |
|---|---|
| G1 strace inside a checkout | 1.5, section 5 (T1, ruled), step T |
| G2 hello split reconciles | 1.1, 1.2, A0 |
| G3 unidentified input, `core.jet` | 1.3 |
| G4 effect vocabulary | 3.6, step P (landed) |
| G5 compiler-owned Prelude | 3.6, step P (landed), B0 |
| G6 nested compiles | 3.7 |
| G7 default expressions | 3.1, section 4, B4 |
| G8 embedding route | 3.2 |
| G9 decode cost | 3.3, 6.1 |
| G10 lookup index and spans | 3.1, section 8 |
| G11 size estimate | 3.4 |
| G12 binary size | 3.4 |
| G13 hello budget measured soundly | 6.1 |
| G14 cold means cold | 1.0, 1.1, 6.1 |
| G15 complete leftover list | section 4 |
| G16 qualifying census | section 10 |
| G17 c22 hello clause owner | 6.3 (split, ruled) |
| G18 provenance | 7.1 |
| G19 drift guard is not circular | 7.2 |
| G20 tree mode cost | section 5 |
| G21 matching tree | 5.3 (it keeps the text path) |
| G22 cross-tier parity | section 8 |
| G23 comptime parity | section 8 |
| G24 RSS target | 6.1, 6.2 |
| G25 step B required | framing, section 4 |
| G26 step B metric | 6.2 |
| G27 reach differential | 7.3 |
| G28 alternatives | section 9 |
