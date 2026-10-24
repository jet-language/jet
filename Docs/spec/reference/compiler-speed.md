# Compiler speed: one build path

Jet has one compiler meaning and one native build path. Until native in-memory
O0 lands, `jet run` and `jet dev` build through it and execute the result
(D-EXEC2=A); the release profile produces an optimized artifact. This page is
for compiler contributors and users reasoning
about build/run trade-offs. The executable perf schema and gate are in
[`Tools/perf/ci-perf-check.sh`](../../../Tools/perf/ci-perf-check.sh), with
scenario planning in
[`Tools/perf/test-ci-perf-check.sh`](../../../Tools/perf/test-ci-perf-check.sh);
[R12 and I9](../philosophy.md) are the semantic constraints.

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

- Fast and optimized profiles use explicit linker, optimization, and LTO
  settings. Native builds honor explicit `RUSTC_LINKER`/`CC`; otherwise Jet
  selects an available fast linker without overriding the target's system
  linker.
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


## Performance gate and dated evidence

The repository gate compares each matched cell and metric independently. Every
non-Rust peer requires Jet/peer below `1.00`; Rust permits parity only at a
Jet/Rust ratio of `1.05` or lower, which is a noise band rather than a win.
Missing, wrong, unavailable, uncovered, mismatched, or inconclusive evidence
cannot be averaged away.

A dated receipt is historical evidence, not a live status claim:
[`performance-receipt.md`](../../audits/performance-receipt.md). The scripts
and their manifest define the measurement protocol; this page records the
meaning-preserving architecture law.
