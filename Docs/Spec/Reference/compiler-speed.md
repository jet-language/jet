# Compiler speed: one build path

Jet has one compiler meaning and one native build path. `jet run` and `jet dev`
compile at O0 through it and run the linked image from memory, without writing
an executable (D-EXEC2=A); the release profile produces an optimized artifact. This page is
for compiler contributors and users reasoning
about build/run trade-offs. The phase budget gate is
[`Tools/Perf/Budget/Check`](../../../Tools/Perf/Budget/Check/run.jet), which
`Compiler/test.jet` runs; [R12 and I9](../philosophy.md) are the semantic
constraints.

## One core, one build path

Every execution consumes the same checked program and IR from the same front end
(R12) and preserves the same supported features and behavior (I9).
Compile-speed work therefore targets development latency through precise
incrementality and backend cost, while optimized builds retain the
optimization work requested by the program's profile.

The compiler is the Jet-authored implementation; the fixed-point chain builds
it with itself. Compile-speed work may replace redundant work only after the
same front-end checks, checked-program meaning, diagnostics, and tier parity
are proven.

## Build-speed constraints

When AOT lowers through a generated backend, backend and toolchain work remains
an irreducible cost. The two-lens contract requires:

- Fast and optimized profiles use explicit optimization settings. Jet links
  native images with its own x86-64 image linker; only a program's foreign C
  closure uses `CC`/`AR` when set, otherwise the C toolchain pack this jet
  ships.
- Native AOT may reuse content-addressed Prelude/runtime and reachable-Core
  objects. Keys include emitted source, compiler identity, target/profile flags,
  environment, and runtime dependency identity; invalid objects fail open to
  the complete inline program.
- Native binary identity carries exact fixed-runtime and reachable-Core
  digests. Disk objects are digest-verified and bounded.
- Incremental checking uses module interfaces and dependent-only invalidation.
  Staged source parsing is bounded and deterministic, with stable module
  discovery order.
- `jet dev` rebuilds only what an edit invalidates before restarting the
  program.

`D-BUILD-DEFAULT1=B` sets `jet run` and `jet dev` to the fast profile while
`jet build` remains optimized. D-EXEC2=A retired `D-AOT-CRANELIFT1`.
The command and backend sources, rather than this paragraph, define which
profile a particular invocation selects.

## Incremental and tier invariants

Jet must not recreate these failure modes:

1. An incremental edit must not become slower than an equivalent clean or
   unchanged run without a diagnostic reason. Unchanged and representative-edit
   cells remain distinct in the perf manifest.
2. A local edit must not rebuild the world. Dirty sets use module-interface
   fingerprints and dependents only; sealed package objects provide the link and
   restore layer. Text sources remain the truth.
3. A type-checking failure must retain a bounded, pinpointed diagnostic rather
   than a wide span that makes the user reshape working code.
4. Cache purge, clean, or reopen must not be required to make diagnostics stable
   and explainable.
5. A development profile must not silently become a ship profile. Cache identity
   includes profile, target, and backend; differential checks protect tier
   behavior.

## Port contract for a Jet-authored compiler

If Jet code takes over a compiler stage, it follows these principles. They are
architecture constraints, not a claim about which stage has already moved.

1. **No redundant verification.** Semantic analysis remains the single
   gatekeeper (R2). The backend consumes the proven checked program; it does not rediscover
   user errors through a hidden host compiler.
2. **Query-based incrementality.** Function- and module-granular memoization is
   the spine. An edit rechecks the touched item and dependents rather than the
   whole world.
3. **Parallel by construction.** Per-module lex/parse/sema work fans out with
   one controlled cross-module resolve point and no global mutable context.
4. **Monomorphization is policy.** Generic instances may be shared, cold code
   outlined, and duplication capped while the IR is built.
5. **Every tier consumes one IR.** O0 (machine code in memory) and O1/O2
   (executable files) are consumers of the same IR; R12 differential gates
   compare the same program and output across tiers.
6. **Optimization follows the declared budget.** AOT may spend longer on the
   ship step, while the default development path avoids work that does not
   affect the current edit.

### Front-end storage and ordered parallel work

Lexer tokens carry source-local `Name` IDs and literal-table row indices.
Repeated spellings share one first-seen-order name table; interpolation tokens
use that same table. Parser, formatter, manifest readers and REPL relocation
must retain the tables alongside the tokens. IDs from independent lexed
sources are not interchangeable, and AST names still resolve to their source
spellings at the parser boundary.

Emitter manifest symbol maps and identity indexes use the existing U64
identity directly, rather than allocating its decimal spelling for each
lookup. Textual names and composite textual keys remain textual.

Parallel body checking, lowering and emission retain source/program ordering
when collecting results. Module-local authority and Core-usage finalization
runs only after global failure/effect solving; it reads shared immutable
inputs and combines diagnostics and usage rows in module order. Thread
admission uses the compiler's existing work and memory limits. Neither thread
count nor completion order may change diagnostics, emitted code or semantics.

Timing receipts distinguish front-end compilation through emitted source
from backend compilation and execution. Compare feature checkpoints with the
same dev baseline, corpus, CPU affinity and build profile; report their
measured deltas, not projections from older or differently configured runs.


## Always-on checks

Checks and debug info run once in every default build: ownership, effect
tracking and failure inference in sema, the IR consistency check, and DWARF
debug info. None of them leaves the default build to reach a speed milestone,
and none runs twice. If a milestone lands short of its lines-per-second target,
the measured number is what ships (`D-SPEED-CHECKS1`).

`Tools/Perf/Budget/budget.json` marks the rows that carry these checks
`always_on`. `Tools/Perf/Budget/Check` prints their subtotal. It fails a
full-unit trace (the compile reached `lower`) that lacks any always-on span.

## Compile throughput

[`Tools/Perf/CompileThroughput/Throughput.jet`](../../../Tools/Perf/CompileThroughput/Throughput.jet)
measures the compiler corpus (the compiler's packages and Core) the way a user
meets it: live, cold and uncached, with an empty store and cache. Give it any
compiler as `JET=PATH`. Each mode (`check`, `O0`, `release` by default)
compiles `--runs` times (default 3). Each compile reports lines per second,
wall, CPU, CPU ns per line, average parallelism (CPU over wall), peak memory,
and each traced phase's seconds; the mode's median compile by wall is its
result. The `outside` column is the wall time the compiler's trace does not
cover: the package's build steps around the compile. `--budget COLUMN` holds
every phase, and `outside`, to that column of
`Tools/Perf/Budget/budget.json` (#5479).

`budget.json` is the compiler's one per-phase budget table. Its `target`
column, `30M-wall`, gives each phase a wall budget in ns per source line, that
is a lines-per-second rate (1e9 over it); the phases, with the build steps
outside the trace, sum to 33.3 ns per line, 30M lines per second end to end.
Every row and `outside_trace` names its `owners`, the cards whose work brings
it inside budget, its root criteria on #5479 first: each names one measured
root of the phase's time, at most an hour of code. A phase over its budget
names them as its root:
`Tools/Perf/Budget/Check` lists each phase over budget with its owner cards,
and fails one that has none. `--budget` keeps that report in each compile's
`budget.txt`, with the owners of `outside_trace` when the build steps outside
the trace are over.

The table is `budget.json` at its `target` column; the tests of
`Throughput.jet` hold the two equal. Each row gives a phase's wall ns per
corpus line and its rate, 1e9 over it, in millions of lines per second. The
rates compose through their ns per line: the rows and the build steps outside
the trace sum to 33.3 ns per line, so the end-to-end rate is 30 million lines
per second.

| Row | Phase | Spans | ns/line | Lines/s | Owners |
| --- | --- | --- | ---: | ---: | --- |
| P1+P2 | read + lex + intern + parse | `sources`, `load` | 2.8 | 357.1M | #5479 c12, #6328 c22, #5910 c6, #5479 c7 |
| P3 | collect / register / resolve | `register` | 1.1 | 909.1M | #5479 c13, #6328 c22, #5479 c7 |
| P4 | comptime: one check + evaluation | `sema.check`, `comptime`, `comptime.bodies`, `comptime.validate` | 2.8 | 357.1M | #5479 c14, #5479 c15, #6328 c22, #5479 c6 |
| P5a+P5b | sema bodies + ownership (one pass; P5b is not traced apart) | `sema.bodies`, `sema.recheck` | 7.0 | 142.9M | #5479 c16, #5479 c6, #5479 c7 |
| P5c | effect tracking | `sema.effects` | 0.6 | 1666.7M | #5479 c17, #5479 c18, #5479 c7 |
| P5d | failure inference | `sema.failure` | 0.6 | 1666.7M | #5479 c19, #5479 c7 |
| P6 | other whole-program finalize | `sema.finalize` less `sema.bodies`, `sema.recheck`, `sema.effects`, `sema.failure` | 1.7 | 588.2M | #5479 c20, #5479 c7 |
| P7 | typed tree -> IR lowering (driver lower) | `lower` | 0.6 | 1666.7M | #5479 c21, #5479 c8 |
| P8 | IR build | `ir.build` | 3.9 | 256.4M | #5479 c22, #5479 c8, #5479 c7 |
| P9 | IR O1 passes + IR consistency check (ir.lint, once, over the module x64 selection reads) | `ir.opt`, `ir.lint` | 2.8 | 357.1M | #5479 c23, #5479 c8, #5479 c6 |
| P10 | x64 selection, register allocation, encode | `ir.x64` | 1.7 | 588.2M | #5479 c24, #5479 c8 |
| P11 | object write + DWARF debug info (SPEED-500K P13; debug_info is on in stock builds and runs) | `ir.object` | 1.1 | 909.1M | #5479 c25, #5479 c8 |
| P14 | link + foreign objects | `native.link`, `native.foreign`, `native.rust_foreign` | 1.1 | 909.1M | #5479 c26, #5479 c8 |
| P15 | driver, phase boundaries (residual) | the rest of the trace | 2.2 | 454.5M | #5479 c27, #5479 c28, #5479 c6 |
| outside | build steps around the traced compile | none | 3.3 | 303.0M | #5479 c29, #5479 c30, #5479 c6 |
| total | end to end | | 33.3 | 30.0M | |

`--against SEED` is the train speed gate: the compiler `JET` against the seed
it was built with, each compiling the corpus in each mode, `--runs` rounds that
alternate which compiler goes first. Every compile runs under
`perf stat -e instructions:u,cycles:u`. The gate judges per mode the CPU time,
user-mode instructions and the process CPU of each top-level phase of at least
0.5 s, never wall time or lines per second, which load moves. A quantity fails
when its median per-round ratio is over +10% within a 10-point spread, or when
every round is over +10%. A wider spread, fewer than three rounds (one binary
spreads about 10% on a phase from run to run, so one round's ratio is noise) or
a missing count is `INCONCLUSIVE`, never a pass. The log names the candidate
and seed jets it times, with their identities. It prints `speed gate: PASS`,
`FAIL` or `INCONCLUSIVE` and exits 0, 1 or 3; `--out` records every compile in
the throughput ledger.

Every release admit (`jet run Tools/Release/Main.jet -- admit`) runs the gate:
its new compiler against the seed that built it, three rounds at the default
level, where `FAIL` and `INCONCLUSIVE` both fail the admit. The new compiler
then compiles the corpus once at `check`, `O0` and `release`. Every compile of
both is a row of the release's `evidence/throughput.tsv`, so each release
records the cell.

`--batch PREVIOUS --current CURRENT` is the batch build's speed gate. It
reads two batch builds of `jet build --profile dev Compiler`, each a
directory with the build's trace (`trace.txt`), GNU `time -v`'s report
(`time.txt`), `perf stat -x , -e instructions:u`'s count of the build
(`perf.txt`) and the compiler the build made
(`tree/Compiler/JetCLI/.jet/build/jet`). It judges only what load cannot move
much. First, the builds' user-mode instructions: one ratio CURRENT /
PREVIOUS, failing over +10% (the two builds compile different trees). Then an
A/B of the two built compilers, each compiling CURRENT's tree in each
`--modes` mode (default `check`) under `perf stat -e instructions:u,cycles:u`,
`--runs` rounds (default 3) that alternate which goes first, kept in `--work`.
On that one input, instructions fail over +3%; cycles follow the `--against`
rules (+10%, a 10-point spread, three rounds). CPU, traced CPU (every
process's top-level phases, the build step children included, which `time -v`
leaves out), each process's top-level phases' own-thread CPU (shown as
`<process>/<phase>`), wall, parallelism and peak memory are reported, never
judged: load on a shared machine moves them by more than any limit. It prints
`batch speed gate: PASS`, `FAIL` or `INCONCLUSIVE` and exits 0, 1 or 3:
`FAIL` when a judged row fails or an A/B compile fails, `INCONCLUSIVE` when a
build's files, a built compiler or a count is missing or the build failed.
`--out` records every build quantity, and every A/B compile in the throughput
ledger beside it. After the A/B, the report shows each mode's phase budgets
for the current compiler's median compile. Every `budget.json` row appears at
its `target` column, as lines per second against its budget's rate, and each
phase over budget is listed with its root, the owner cards of its row. These
rows are reported, never judged, since load moves wall time. The one exception
is a phase over budget that names no owner card: the gate fails it.

`--lane quick|full` runs the gauntlet's `compile_throughput` cell
(`Tools/Gauntlet/measurement-manifest.json`) with the same tool. The full lane
builds the generated `bench300k` program and the compiler corpus (`selfhost`);
the quick lane builds `bench30k`. Each program is built in two arms, the
default thread count and `--threads 1`, each after a warmup. Generated programs
come from `Tools/Perf/Scaling/Generate/run.jet`, are pinned in
`Tools/Perf/Throughput/bench.json`, and each artifact's stdout must
match the pinned golden byte for byte. Artifacts and diagnostics must be
identical across every sample and arm. The full lane adds the manifest's
memory, wall or lines-per-second, and phase-budget (`budget.json` at its
`target` column) gates. The quick lane
compares wall, peak memory and each phase with
`Tools/Perf/Throughput/quick-baseline.json`, within the regression
budgets of `Tools/Perf/baseline.json`. Every run writes a JSON receipt.

## Performance gate and dated evidence

The repository gate compares each matched cell and metric independently. Every
peer, Rust included, requires Jet/peer below `1.00`; no peer has a tolerance
band.
Missing, wrong, unavailable, uncovered, mismatched, or inconclusive evidence
cannot be averaged away.

The build system has its own required peer: Nix, in the `build_system` axis of
`Tools/Gauntlet/measurement-manifest.json` (harness
`Tools/Gauntlet/Harness/BuildSystem.mjs`). Both build the same medium project.
Jet builds it as a package with a `build: fn` program, and Nix builds it as a
flake. The five paired cells are configuration evaluation (`jet check` of
`@build.jet` against `nix eval`), no-op rebuild, one-file incremental rebuild,
a cold build while a populated cache or substituter holds the result, and a
hermetic build from scratch. Evaluation, no-op and incremental each need
Jet/Nix at or below `0.25`; the other two need it below `1.00`.

A dated receipt is historical evidence, not a live status claim:
[`performance-receipt.md`](../../Audits/performance-receipt.md). The scripts
and their manifest define the measurement protocol; this page records the
meaning-preserving architecture law.
