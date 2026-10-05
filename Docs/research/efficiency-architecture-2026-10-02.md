# Efficiency architecture: one owner, demand-only work (2026-10-02)

This dated synthesis supports **Tower #4308**. It combines the owner's
2026-10-02 directives with the hello audit and six research reports; it does not
ratify a design, establish live implementation status, or own a work queue.
The short phase sequence below is input to #4308, where dependencies,
acceptance criteria and execution belong. Strategy remains in
[`Docs/spec/philosophy.md`](../spec/philosophy.md).

Evidence keys used throughout:

- **H** — [`compile-cost-hello-2026-10-02.md`](../audits/compile-cost-hello-2026-10-02.md),
  especially setup, results and profile/root-cause sections.
- **B** — `~/.cache/jet-dev/sol/efficiency/01-binary-size.md`.
- **P** — `~/.cache/jet-dev/sol/efficiency/02-prelude-linking.md`.
- **S** — `~/.cache/jet-dev/sol/efficiency/03-compile-speed.md`.
- **R** — `~/.cache/jet-dev/sol/efficiency/04-runtime-memory.md`.
- **M** — `~/.cache/jet-dev/sol/efficiency/05-compile-memory.md`.
- **J** — `~/.cache/jet-dev/sol/efficiency/06-jai-target.md`, including its
  qualified public Jai evidence. It became available during synthesis.

These are source reports, not six independent benchmark runs. H supplies the
shared measured baseline. B additionally records three scratch C/Rust/assembly
experiments; J records public claims/community output, not a local Jai run.
The synthesis checked selected current source and measurement contracts
statically. **No build, test, benchmark or profiler was run for this document.**
Numbers described as targets, budgets or expected effects are **[INFERENCE]**,
not earned improvements. Code citations identify the inspected source; concurrent
changes can supersede them without retroactively changing historical evidence.

## 1. Targets as numbers

The owner's minimum is binary size at or below C and best among languages,
compile/run speed at or above C, and minimal compiler/program RAM through
move/read/reference rather than copy. The repository's
[performance gate](../../AGENTS.md#performance-gate) is stronger than merely
matching C: each required positive-valued metric/cell must strictly beat every
matched non-Rust peer. Jet/Rust ≤1.05 is noise parity, not a win; the owner's
best-of-languages goal additionally needs an actual Rust win. Never average
across programs, profiles, cache states, metrics or peers.

| Area | Numerical design target [INFERENCE] | Acceptance, not a frozen baseline exemption |
|---|---|---|
| Default and release hello size | First below the exact same-run C executable, historically ≈14 KB stripped; then **<1,024 bytes** for an eligible canonical Linux syscall-only closure | Count as-built and stripped bytes, required runtime/error/security bytes and deployment closure. Both default and release compete; the audit's default-only <133 KB aspiration is superseded. |
| Hello compile | **<100 ms** fresh installed-toolchain end-to-end; **<35 ms** warm actual compile/relink aspiration | Beat same-run C, historically gcc 120 ms cold/50 ms warm, and the strongest matched fast-C arm including TCC when available. A 99 ms warm result still loses to 50 ms. |
| Hello launch | Below fastest matched peer; historical reference **0.28 ms** (Zig ReleaseFast), C **0.36 ms** | Paired spawn-to-exit and first-output distributions; not a promise that Python timing can resolve these thresholds. Warm O0 host execution is a separate cell. |
| Compiler hello RAM | **≤24 MiB = 25,165,824 bytes**, whole process tree | Below exact same-run peer peak, historically gcc 35 decimal MB. Includes touched compiler/catalog/runtime pages and every backend/linker child. |
| Native compiler self-build RAM | **≤768 MiB = 805,306,368 bytes** | Capacity target for the real assembled compiler, not proof of beating a nonexistent “gcc compiling Jet” workload. Require successful stage outputs and probes. |
| Large clean development build | Approximately **300,000 lines in <2 s**, nonincremental, including output/link/debug work | Jai-inspired throughput target, not an optimized-release or matched Jai receipt. At least 150k lines/s end-to-end is the arithmetic lower bound; source-line convention and generated/metaprogram work must be recorded. |
| Runtime ownership | **0 allocations/owner clones/retains** for a proven borrowed read; **0 payload allocations** for a lawful last-use transfer; **0 operand-copy allocations** for big-Int comparison/hash | Structural minima. If the peer also has zero, this is parity, never a fabricated ratio or “strictly fewer than zero.” Speed/RAM/size gates remain independent. |
| Installed-toolchain hello fixed work | **0 full compiler-body restores**, **0 whole Source-program projections**, **0 Core source parse/body-check calls**, **0 runtime builds**, **0 normal runtime-prefix bytes** | Demand-loaded generic/comptime bodies are recorded separately; required checking and integrity verification are never skipped. |

S §7 provides an illustrative **35 ms** warm budget: startup/header 3,
authorization/source 3, parse/register 5, check/stage 8, lower/mandatory analysis
5, native emission 5, link/write 6 ms. It is a planning allocation, not measured
phase timing. M §5's 24 MiB hello reserve is 12 resident code/interfaces + 2
source/literals + 1 remaining AST + 2 catalogs + 3 active bodies + 2 scratch + 1
output + 1 slack MiB. Its successive phase peaks are not additive.
For self-build, the 768 MiB reserve includes 24 resident, 32 source, 96 AST,
64 catalogs, 192 active bodies, 96 analysis, 128 output and 136 overlap/slack
MiB. Admit work against measured ownership, not these unproven partitions.

J §2 makes the large clean-build target more demanding than the older S
source-throughput aspiration. At 150k lines/s, 216k lines permit **1.44 s**;
J's inspected 227,114-line assembled input permits **1.5141 s**. Its proposed
fast-development envelope is:

| Phase | 300k budget, ms [INFERENCE] | 216k budget, ms [INFERENCE] |
|---|---:|---:|
| Startup/authorization/demanded interfaces | 40 | 40 |
| Lex | 120 | 86.4 |
| Parse/build AST | 200 | 144 |
| Registration/catalogs | 100 | 72 |
| Type/ownership/effect/contract checking | 550 | 396 |
| Required comptime/generated-source coordination | 150 | 108 |
| TIR→MIR | 200 | 144 |
| Mandatory legality/fast selected transforms | 150 | 108 |
| Native selection/allocation/encoding/debug records | 280 | 201.6 |
| Link/write | 100 | 100 |
| Deterministic receipt publication | 10 | 10 |
| **Total** | **1,900** | **1,410** |

These are complete-build caps, not phase measurements or a guarantee for
unbounded metaprograms. J models **8 tokens, 4 nodes and 8 MIR units per physical
line**, not an observed node census. At 300k lines this implies 20m tokens/s
lexing, 6m nodes/s parsing/lowering, 2.182m nodes/s checking and 8.571m MIR
units/s native output. Ten full optimization traversals require 160m visits/s
in the 150 ms slot, not 16m. One assembled source's lexer/parser cannot simply
divide those rates by 32 cores. The approximate 1.6 s public Jai claim would
need roughly **25% more useful-work throughput** than this conservative envelope,
with fixed cost controlled separately. Count real tokens/nodes/generated work
and executed comptime operations before converting any target to per-line rates.

J §5 divides the provisional 550 ms checking slot into 220 ms names/types,
160 ms ownership/view/failure facts, 80 ms effects/authority/summaries,
40 ms contracts and 50 ms purity/staging eligibility. Required comptime
execution remains the separate 150 ms slot. These fused-work budgets retain
Jet's extra obligations; they neither measure their cost nor explain a
thousand-fold incomplete self-build gap by richer semantics.

“Best all languages” cannot mean silently selecting heavy idiomatic peers while
ignoring their smaller matched variants. B §3's exercised scratch artifacts are
**243 bytes assembly, 249 bytes freestanding C, 258 bytes Rust no_std**, including
176 bytes of non-overlapping ELF/NX-stack headers. They produce `hello\n` and
handle short writes/EINTR, but lack Jet's full diagnostic/cleanup/security
contract; no launch timing or fault-injection equivalence was measured. They
establish scale, not Jet acceptance. Unavailable required peers remain
unavailable evidence, not wins.

## 2. Where Jet stands: measurements, causes and superseded claims

### Shared measured hello baseline

H used a 32-core, 61 GB x86-64 Linux machine. “Cold” clears toolchain build
caches, not the filesystem cache; ordinary Jet cold already had a runtime
library in its store. Compiler memory is sampled process-group sum-RSS in
**decimal units**. “Run” is best-of-five Python spawn-to-exit wall time, not
hot-loop execution or runtime RAM.

| Captured toolchain/mode | Cold / warm compile | Compiler peak | Built / stripped binary | Run |
|---|---:|---:|---:|---:|
| gcc 15.3 -O2 | 0.12 / 0.05 s | 35 MB | 16 / 14 KB | 0.36 ms |
| clang 21.1 -O2 | 0.32 / 0.06 s | 157 MB | 16 / 14 KB | 0.36 ms |
| rustc 1.97.1 -O | 0.48 / 0.12 s | 166 MB | 531 / 394 KB | 0.64 ms |
| rustc debug | 0.10 / 0.10 s | 164 MB | 527 / 390 KB | 0.50 ms |
| Go 1.26.7 | 2.12 / 0.12 s | 372 MB | 2.4 / 1.6 MB | 0.93 ms |
| Odin dev-2026-05 -o:speed | 1.59 / 1.39 s | 470 MB | 200 / 191 KB | 0.38 ms |
| Zig 0.16 ReleaseSmall | 2.41 / 1.35 s | 282 MB | 133 / 133 KB | 0.33 ms |
| Zig 0.16 ReleaseFast | 10.72 / 8.75 s | 433 MB | 3.7 MB / 530 KB | 0.28 ms |
| Jet Rust-reference default, night12 | 7.03 / 1.33 s | 179 MB | 58.8 / 45.0 MB | 7.87 ms |
| Jet Rust-reference release, night12 | 41.21 / 1.51 s | 2.37 GB | 575 / 575 KB | 0.55 ms |
| Jet-written jetc0, captured bootstrap route | 64.6 s Jet + 19 s rustc / unavailable | Audit labels 11.6 GB; see unit correction below | 17 MB debug / unavailable | unavailable |

These numbers are historical, rounded and not a newly measured checkout.
Jet's captured default/release stripped-size gaps to C are approximately
3,214×/41×; launch gaps are approximately 22×/1.53×. They do not establish
symbol-by-symbol attribution.

### Binary size and Prelude/linking

B §2 and P §2 identify a fixed emitted universe, not indiscriminate parsing of
all Core source. `crates/jet-codegen/src/Codegen/mod.rs:3086–3121` constructs an
empty `used_core`, forces Core assembly and testing-history carriers, then
adds scheduler/streams/collection sources. Other runtime parts already have
typed selection gates. `crates/jet-store/src/runtime.rs` already caches one
content-addressed runtime rlib; warm programs do not always rebuild it, but
still assemble/export/strip/hash broad text.

The historical prefix census (`~/.cache/jet-dev/sol/prep/Runtime-Prefix-Plan.md:7–17`,
quoted by B/P) is **6,524,480 total Rust bytes: 6,523,585 prefix and 895 suffix**.
Unicode literals contribute **2,085,277 source bytes**; the name pool alone
contains 1,041,729 UTF-8 bytes. Rust test-item stripping already exists in
`Codegen/MIRRust.rs:768–784`. Neither prefix-byte removal nor binary asset
conversion proves an equal executable-byte reduction. Default deployment may
also contain a Cranelift runner; separate its bytes instead of crediting all
58.8 MB to Prelude pruning (P §2.4).

Ambient selection is inconsistent rather than absent. The canonical registry
is `crates/jet-codegen/src/Prelude/core/prelude.jet`; `Foundation/Prelude.rs`
parses names once. Rust sema `Sema/Prelude.rs:140–201` scans raw identifiers
and injects module aliases. The Jet loader already queues imported Core owners
selectively, but has a bare-URL scan. Jet sema has hardcoded ambient dispatch,
an `assert_eq` shadowing exception and a narrowed `#NoPrelude` check (P §2.1).
Actual trace spelling is **`debug`**, not a new `dbg` alias.

`Compiler/JetCodegen/Source/Codegen/CoreReachability.jet:83–114` prunes public
top-level Core bodies but roots all methods and non-Core/private parts; it runs
after checking. `Compiler/JetFoundation/Source/MIR/Reachability.jet:1–94` only
selects codec implementations. `MirProgram.core_calls` is an ABI registry, not
artifact liveness. These are useful seams, not complete linked-item collection.

### Compile speed and compile-time RAM

H's first ≈18 seconds grow from ≈0.8 to ≈11.6 “GB” during image/metadata setup;
remaining Jet work lasts to 64.6 s before the 19 s rustc child. M §1 corrects
units from raw Linux samples: **12,181,480 kB = 12.474 decimal GB = 11.617 GiB**.
The image is approximately 178 MB on disk. Wire interning is not resident
interning: `crates/jet-foundation/build/MirImageCodec.rs:1040–1053,1242–1244,1296–1309`
clones interned strings/types into owned graphs. Native expansion, Source
projection, temporary copies, exact-ID nodes and allocator slack all
contribute; the entire amplification cannot be attributed to decoder objects
alone without byte attribution.

Captured hello self samples show `JetInt::clone` **20–37%**, release **3–16%**,
`MIRType::clone` **5–6%**, plus substantial malloc/free. They are not disjoint
percentages to add into a promised speedup. S/M's self-build logs reach
**27,965,316 kB HWM = 28.636 GB = 26.67 GiB** and samples beyond **774 s**;
J §3 pins a later **+1,713 s** sample at the same HWM, still without an observed
completed self-build. Recursive Expr/Stmt/CallArg clone/drop stacks demonstrate
churn, not allocated-byte totals. J's read-only census of that assembled input
records **227,114 physical lines, 11,955,977 bytes and 8,889 anchored fn
declarations**, from 287 source fragments; it is a regex/source census, not a
checked/generic/MIR function count. The old 216k/8,702/11.61 MB shorthand is
dated sizing evidence. The unfinished run implies only an effective throughput
upper bound of ≈133 lines/s, more than 1,131× below 150k lines/s—not a completed
frontend timing. No local Jai memory figure is supplied.

Representation explains why useful work remains expensive:

- Source MIR IDs still use `Int`, with U64 FNV hashes converted to exact values
  (`Compiler/JetFoundation/Source/MIR/MIR.jet:7–80`). Uniform unsigned hashes
  would put ≈75% outside the signed inline range **[INFERENCE]**. Host codec
  `Bootstrap/Host/RuntimeMirCodec.rs:493–505` constructs these exact carriers.
- Source AST recursively owns children/names/types (`Foundation/Source/AST/Expressions.jet:4–55`),
  TIR repeats structural types and typed child trees (`TIR/TIR.jet:476–604`),
  MIR has IDs but still owns recursive types/layouts and many vectors
  (`MIR/MIR.jet:358–386,2111–2157`). Registration retains Program and declaration
  records; explicit `~` copies exist in `Sema/Registration/Items.jet:1749–1768`
  and no-change rewriters. Not every by-value call is proven to deep-copy.
- `Bootstrap/Host/Native.rs` copies all Core source strings into each request
  although Loader parses only a closure (M §2, P §5 R3). Interface-first
  loading removes catalog copies and repeated checking, not just linked bytes.
- Hot metadata readers return owned rows/contexts; repeated summary/CFG/use
  scans and ordered deep-record shifts compound churn (S §3–4, M §2–4).

J §4 adds two independent fixed/per-step costs. The standalone CLI reads every
Core source row (`Compiler/JetCli/Source/Cli/Request.jet:202–208`), then its common
request can emit Rust even for a native/check consumer: only
`runtime_execution` returns before `jet_rust_emit_program`
(`Driver/Pipeline.jet:2629–2680`). Direct native output alone therefore does not
prove a prefix/text-free frontend. Comptime's rich MIR evaluator resolves
function/block per step and scans value facts per stored result
(`JetEval/Source/Evaluator.jet:1602–1639`), independent of ordinary expression
copy sites. Substantial comptime work needs bounded native O0 execution under
D-EXEC1, not a new permanent interpreter; tiny constant evaluation and
required user execution must remain distinguished from optional fold budgets.

### Runtime memory and execution speed

There is **no measured generated-program RAM or hot-loop throughput baseline**
in H. Its peer MB figures are compiler RAM. The Jet compiler itself is a large
real Jet workload demonstrating runtime clone/allocation costs, not a general
percentage estimate for all programs (R §2.2).

Jet already has Read/Write/Move, ownership/drop facts and lawful last-use
transfers (`MIR/MIR.jet:126–138`; `Codegen/LastUse.jet:124–647`). Rust emission
also recognizes inspection-only stable reads (`Emit/Values.jet:553–868`), but
recomputes legality per backend and still materializes many reads with
`.clone()`. Plain print uses `jet_rust_emit_value_read` then owned `jet_show`
for non-String (`Emit/Semantic.jet:211–215`).
`Prelude/Term.rs:184–195` allocates a newline String frame. Deterministic
set/map/heap formatting has additional legitimate sort work and avoidable
per-item owned Strings; removing ordering is not an optimization (R §3).

`crates/jet-foundation/src/Numeric.rs` owns **8-byte tagged exact Int**, inline
**[-2^62, 2^62−1]**, with immutable spilled base-10^9 limbs and atomic ownership.
Inline clone/drop allocate nothing. Spill compare/hash/display currently use
`to_big()` (`:3323–3329,3400–3408`), copying limbs for reads. R §3.2 estimates
≈60 requested bytes/two allocations for a minimally spilled node+limbs on
ordinary x64, before allocator/hazard overhead; this is not a measured ABI.

The experimental native path has a different floor:
`JetBackend/Source/Lower/Lower.jet:230–267` boxes ordinary structs, payload enums,
Option and Result. B/R describe `Image/Runtime.jet`'s mmap-per-allocation,
allocated/copied literals and inline-only Int overflow stop. Those defects
must disappear through canonical-runtime unification, not be tuned into a
second production semantics. `Image/ELF.jet:1–59,83–93` has a genuine 120-byte
ELF shell but links all supplied runtime functions.
`Image/Object.jet:91–153` has one `.text` and `.rodata`, patching user calls early;
section GC cannot discard individual functions. Its function-offset search is
already indexed in the inspected source (`:119–130`); do not add another index.

### Source corrections and conflicts resolved

1. **Image restore claims have two successive corrections.** H describes the
   captured whole-restore path. S/M recognize current envelope-only reads and
   a temporary full Source projection. The synthesis found a further change:
   `Host/Native.rs:2065–2093` now converts a **body-free signature program**, not
   the full Source body graph. `Host/CompilerImage.rs:329–413` implements that
   projection. It still clones all catalog rows/signatures, while the lease
   still calls the full image at `Native.rs:1953` and retains native MIR via
   Arc. Therefore “full Source projection remains” is superseded; “ordinary
   startup is now compact/no full restore” is also false. No seconds/RAM savings
   are credited without new measurements.
2. **Numeric inlining is already present.** H's missing-inline diagnosis applies
   to its captured binary. `Numeric.rs:3314,3361–3377` has inline compare,
   Clone and Drop. Remaining spill-copy kernels and synthetic copy traffic
   are separate problems; adding the attributes again earns no win.
3. **The captured default is not today's profile.** `Source/main.rs:500–557,562–570`
   emits default O2/no cross-crate LTO and release O3/ThinLTO on the Rust bridge.
   D-EXEC1's target default is O1, with LLVM-first O2. These compiler levels and
   rustc flags are different concepts. Historical 58.8 MB is not current output.
4. **Several scaling fixes already landed.** S §3.4 refreshes selective view-return
   summary jobs, exact memory-plan indexing, owner indexing and eligible two-name
   borrowed list walks. They are not absent features or additive savings.
   Clearing flattened TIR already exists; other owners can still retain bodies.
   J §3 further refreshes last-read memoization, dead-value zero-use worklists,
   indexed def-use/dominance, declaration unions and effect reverse-edge
   propagation. `JetOptimizer/Source/Transforms/DeadValues.jet:36–95` confirms
   one final reconstruction, not a dead-chain rebuild per layer. The residual
   target is actual remaining scan/round/copy work, not reimplementing these
   already-present solvers.
5. **Demand-only Core does not permit missed diagnostics.** User declarations
   still receive required checking even if not emitted. A full vtable retains
   every pointer it actually stores; slots cannot be pruned just because a
   syntactic call census used one method. Check demand and link demand differ.
6. **One runtime is not one mandatory DSO, nor arbitrary specialization.** P's
   strict compile-once interpretation governs S's proposed generic-body cache:
   ship semantic kernels; generate layout/drop/formatter adapters only.
   New semantic Core bodies per user instance need an explicit D-EXEC1 decision.
   Likewise one O2 runtime per ABI is preferred over S's bridge “per profile”
   wording; only genuine incompatible layout/capability contracts justify variants.
7. **Exact Int, Columns and ownership are not optional alternatives.** Keep
   arbitrary precision; use private U64/U32 metadata and proven native arithmetic,
   not a default-I64 language change. D-SOA-SITE1=A selects `Columns<T>`, replacing
   `#Layout(columnar)`; R's old marker citations describe inspected implementation,
   not the ratified target. Universal CoW/SVO/GC is not the copy-elimination plan.
8. **Smaller startup is conditional on observable meaning.** Current
   `Prelude/EnvInit.rs:1–27` mandates entry snapshot and env-activated observation;
   `Prelude/Core/CAbi.rs:97–102` roots it. Lazy first-use reads, ignored observer
   settings or `panic=abort` cannot silently remove required semantics.
9. **Jai evidence is no longer wholly absent.** B/P/R/M correctly lack a local
   Jai receipt/inspectable proprietary implementation. J supplies a qualified
   primary public full-build claim and community architecture/phase output,
   not compiler-RAM/binary rankings or a matched benchmark. It does not imply
   an interpreter is intrinsically slow: Jai interprets bytecode at comptime;
   Jet's O0 choice is governed by D-EXEC1, not that inference.

J §1 anchors the 300k-line clean debug target in
[Jonathan Blow's interview, 41:13](https://www.youtube.com/watch?v=vHfdUqLmPNU&t=2473s)
and the [captioned extract](https://www.youtube.com/watch?v=AZWR2PsfDuo): about
2 seconds, then about 1.6 seconds, without incremental builds. Automatic
captions and unspecified hardware prevent a precise local comparison.
[The Way to Jai's compiler chapter](https://github.com/Ivo-Balbaert/The_Way_to_Jai/blob/main/book/04A_More_info_about_the_compiler.md)
reports a C++ job-based compiler, internal bytecode, own x64 development backend
and LLVM release backend; one 11,075-line example totals 425.573 ms with
328.986 ms linking. These are community outputs, not our measurements. Large
full-build speed and small-program/linker fixed cost need separate cells.

## 3. One coherent target architecture

### Compiler data ownership and lifetimes

**One immutable owner per fact; mutable work is function-local.** The session
owns authorized source bytes/line maps, interned names/types and declaration
catalogs. Registration references existing item IDs, not another Program tree.
Checked bodies have one owner, transferred to lowering; workers borrow catalogs
and own scratch, never a by-value program/module/staged-value universe.

Use two identities deliberately:

- **Stable U64 IDs** preserve deterministic external identity, collision/zero
  rules, canonical keys, wire bytes and receipts. If a schema must change,
  migrate Source/Rust codecs and every consumer in one cutover.
- **Dense typed U32 indices** address module/function-local nodes, blocks,
  values, places and side tables. Stable hashes are never array subscripts;
  local indices are never persistent identity. Oversized stores shard or use
  a checked capacity contract, never truncate.

Names are one packed UTF-8 pool plus offset/length rows; symbols hold interned
name/owner/key references. Types are canonical structural rows with child-ID
ranges; use-site spans/obligations and `(type,target)` layouts are separate.
Tokens reference source spans and only interpreted payloads. AST/TIR/MIR use
chunked index arenas with specialized variable-arity/rare-payload side tables;
SoA columns are appropriate for demonstrated hot traversal, not mandatory for
every record. No per-node Rc/Arc or recursive cloning decoder. Wire interning
must survive as resident handles. Share only coarse immutable shards.

Getters yield scalar IDs or bounded read views, not owned TFunc/MIRFunction.
Where safe returned views are unavailable, use callback/push visitation rather
than invent a lifetime surface. Retain IDs and reacquire views across growth;
never retain raw pointers across reallocation/reset. Branch/speculative work
uses marks/deltas, not whole-scope snapshots. CFG/use-def/liveness and root
closure use flat ranges, bitsets and changed-node worklists. Sort scalar
positions, stream canonical digest bytes, and emit into a sink.

Retire tokens after parse, non-template AST after stable check/lower, TIR after
transfer and old analysis generations after mutation. Keep exact authored source,
diagnostics and explicitly required generic/comptime bodies. Full inspection
and bootstrap archiving stream/spool compact records when requested; normal
results are artifacts, summaries and receipts, not all phases plus output.
Bound and release scratch slabs so logical retirement reduces RSS. (S §4;
M §4; precedents below.)

J §5 sharpens the semantic work bound: in terms of actual body operations S,
dependency edges E, contract nodes P, generated/specialized work G and executed
comptime steps X, aim for work proportional to **S+E+P+G+propagated facts+X**,
with lawful sorting and finite-lattice convergence charged separately.
O(EH) dataflow or genuinely large fact output cannot be wished away as O(lines).
Resolve ownership by binding/place identity; update overlapping-loan facts
through deltas, not hash equality that loses overlap semantics. Reuse the
existing reverse-edge effect solver, accumulate lexical region facts once,
and memoize denial/reachability witnesses without losing call-site multiplicity.
Check contract condition/message expressions against bounded local overlays;
derive purity/staging from the same call/effect/provenance graph. Required
summaries must stabilize before callers are published valid. Function/version
change masks preserve unaffected analyses; required final legality still runs.
Pure evaluation and generic memoization within a single build are legitimate
full-build efficiencies, not reuse of previously checked user bodies.

### Prelude, checked-library cache and artifact reachability

**Prelude is ambient item availability, equivalent to an implicit selective
`use` only when ordinary resolution needs it.** Keep one curated registry.
Resolve lexical/module/explicit imports first, then an ambient fallback for the
current edition. Record compiler-owned `ImplicitUse` provenance with exact
Core item and first-use span; do not fabricate a source import. A local
non-callable `print` fails as non-callable, never falls through. `#NoPrelude`
disables registry fallback, not explicit Core access. Existing assertion
shadowing stays explicit until its ballot changes it. Preserve variadic
Printable/debug/comptime behavior through checked adapters; a one-String Core
wrapper is not a sufficient replacement.

Toolchain construction checks Core/Prelude once and publishes independently
addressable **index, checked interfaces, demanded template/comptime bodies,
source maps and runtime manifest**. Driver supplies the verified index; sema
requests immutable headers, not filesystem I/O inside inference. Load body
shards at resumable query boundaries. Opaque nongeneric calls need a header,
ABI and dependency summary, not source/AST. Required user obligations, authority,
lifetimes, debug-build rejection and constant/default evaluation still run.
Installation/publication verifies artifacts; each compile checks the small
identity header and demanded chunk bounds/digests. No trusting corrupt data,
full-archive hash just for an entry ID, or stale/fabricated signature fallback.

A compact sealed **compiler ABI catalog** similarly replaces ordinary compiler
MIR restore. Generate factory signatures, checked type/field/Core-owner/handle
shapes, callback/shared-payload bindings and body references from the canonical
Source accessor. Bind it to source authority, artifact, target, schema and
execution identity. Body-free projection is an intermediate improvement, not
the final catalog: it still copies global rows and requires full native restore.
Comptime loads only its actual reachable body closure.

Keep distinct **check demand**, **runtime service demand** and **artifact link
closure**, connected by canonical IDs. Artifact roots are entry, selected
exports/harnesses, observable init/fini and required lifecycle/boundary services,
not all public Core or user functions. Traverse direct calls, defaults actually
applied, function addresses/closures/callbacks, generic evidence, full current
vtable pointer sets, clone/drop/close/formatter glue, static/data relocations,
exceptional cleanup, reflection/export contracts and service dependencies.
Open plugin/FFI boundaries retain their declared open-world roots. A label or
unused import alone does not imply an executing service, subject to existing
observable initializer rules.

Produce one typed `ArtifactLinkPlan` before backend selection; recollect after
optimizations remove references. Rust/native/Web project it, never reconstruct
liveness from names, emitted Rust or the ABI registry. Dense marks/queues make
closure O(V+E) over reached nodes. Diagnostic root-reason witnesses are optional
inspection metadata, not an unconditional runtime table. (P §4; B §4.1.)

### One runtime, separable storage, exact values

D-EXEC1=A requires one semantic runtime compiled at O2 and shipped per Jet
version/target/ABI. “Shared” is implementation/ABI reuse, not an indivisible
rlib member or mandatory megabyte DSO. Initially keep a legal Rust SCC rlib;
move carrier/trait owners down before splitting crates, respecting orphan
rules. Native packaging uses indexed archive members/SCCs with independently
collectible function/data sections. Never build a runtime per used-Core subset
(2^N variants), use whole-archive/export-everything, or reoptimize the shipped
runtime with mandatory per-program FatLTO.

Canonical runtime kernels implement arithmetic, text, allocation, bounds,
formatting, failure and lifecycle policy. Platform adapters supply syscalls,
marshalling and lawful startup, not another numeric/string/error meaning.
Caller-specific generic glue supplies checked layouts/element operations and
calls kernels, without universal erased boxing. Semantic specialization beyond
that baseline is the explicit ballot below.

Unique compact List remains pointer/len/cap ownership; inline fixed aggregates
and bounded nonescaping temporaries use register/stack/result storage. Native
struct/enum/Option/Result layouts have real field widths/alignment/tags and
proven niches, not one heap box per value. Aggregate returns construct in a
caller destination; growth moves initialized elements once. Shared ownership
is used only when semantics demand it; arenas are scoped opt-in lifetimes,
not the universal heap. Use the canonical system/approved target allocator,
not mmap per object. Separate genuine destructor work from sentry/extent
accounting without weakening provenance. Fixed keeps atomic exhaustion,
reverse drops and **no heap fallback**.

String first gains **borrowed UTF-8 views and consuming buffer finish**, without
waiting for a representation redesign. Then evaluate a unique Empty/Inline/
Static/Owned carrier with a **24-byte first-cutover budget** (no larger than a
common Rust String header); compare a complete 16-byte SSO design empirically.
B's ptr/len view is a borrowed ABI, not that owning carrier. Universal 15-byte
SSO or List SVO is not assumed: header arrays, capacity and movement can cost
more RAM. Static bytes are immortal; mutable owning materialization remains
independent. Inline-byte borrows pin the owner's address/lifetime.

Exact Int retains its 8-byte inline/tagged semantics and base-10^9 limbs
initially. Borrow sign/length/limbs for compare/hash/format; inline operands can
use at most three stack limbs rather than temporary Vecs. Allocate results,
not operand copies; consuming/private builders reuse limb capacity. Refcount
one alone is **not** permission to mutate a published/hazard-visible node.
Keep atomic snapshot hazards and Send/Sync laws. Native range facts let proven
loops remain I64/U64 internally; unproved exact overflow promotes through cold
canonical kernels, never traps/wraps as a substitute. Word-sized tagged Int
still has ownership/drop obligations. (R §4; M §4.)

### Ownership, formatting, startup and assets

Ownership-normalized MIR encodes bounded `BorrowRead`, `BorrowWrite`,
`OwnedCopy` and `Transfer`, with place/projection windows, mutation/escape
barriers, initialization state and drop facts. Extend existing last-use
machinery, do not add a parallel mechanism. Borrow inspection, transfer dead
owners, construct directly in destination and update exclusive projections.
Keep real snapshot/independent copies, explicit view materialization and
observable copy/drop hooks. Never leave holes in live containers, move an
owner while borrowed, or destroy replacement state before fallible evaluation
permits it. Drop exactly once across normal/exceptional paths. Initialization
proof removes unnecessary Option slots; it does not erase partial-move masks.

One canonical writer formatter streams borrowed values. Existing owning String
format results use that formatter with a growable sink. Builtins/derives can
land first; changing user protocol is a ballot, not an unapproved dual API.
Literal print borrows rodata; constant framing can intern `hello\n` generally.
Dynamic line output locks once, preserves flush/newline/order, and handles
short write/EINTR/broken pipe using canonical error policy. Deterministic
unordered rendering retains sort scratch when needed; rendered-text ordering
cannot silently become key ordering. No `fast_print` spelling or ASCII-only
shortcut.

Runtime manifest service init/fini is dependency ordered per artifact. A
literal-only target can reach `_start -> canonical writer -> exit_group` only
when environment/observation/error/finalizer contracts permit that closure.
Preserve before-user environment snapshot meaning; immutable initial envp can
be borrowed only with a proof against mutation/FFI, otherwise copy before a
mutating boundary. Do not defer to `vars_os()` at first read. Keep lexical
cleanup, atexit/report/exit status; compiler-managed exceptional edges can
replace Rust unwinding only with identical behavior.

Unicode 17 assets become versioned immutable binary pools with relative offsets
and separate names/category/case/normalization/segmentation/width capabilities.
Demand-map bytes without startup decompression/hash-map building. Literal
UTF-8 printing roots none of those tables, while actual Unicode APIs retain
full semantics. Converting tables alone reduces Rust parsing/relocations, not
all their data bytes. Fonts use the same asset model only when reached.
`Columns<T>` gets typed field buffers/common len/cap and zero-copy field views;
reserve/commit must keep row lengths consistent on failure. Whole-row owning
reads materialize only the required row, not a full column/AoS list. Preserve
serialization and tier laws; compare the reported #2889 workload with AoS and
the D-ACCEL1 automatic path, not a favorable substitute. (B §4, P §4.5, R §4.)

### Backend/output, parallelism and caching

Lower ownership/layout/ABI once into the D-TIER-FORM1 shared low-level form.
O0 directly selects into memory and preserves live replacement; O1 uses bounded
local optimization and writes an executable; O2 initially uses LLVM with
proven noalias/range/alignment/readonly/initialization metadata. None redecides
meaning. Complete list/map/closure/indirect/Core-call/global/view/drop and
whole-compiler routes before retirement; a native hello is not native coverage.
D-EXEC1 orders current-engine parity, runtime unification, x86-64 then aarch64,
LLVM release/other targets, and only then old-engine retirement.

Mark code/data/imports **before layout**. Objects have per-function/per-data
sections and symbolic relocations even for user-to-user calls; use stable
ID-to-selected-offset maps, COMDAT/unwind associations and minimal ABI exports.
ELF can omit executable section/symbol tables, but needs lawful startup,
permissions and relocation semantics. Prefer ASLR-capable static ET_DYN without
PT_INTERP where the closure permits it, NX stack, W^X, read-only non-executable
assets and separate RW/BSS. Keep segment/file congruence and genuine TLS/FFI
requirements. Use libc only when the provider/foreign contract requires it;
never enter libc from an incomplete handcrafted startup. Cold function
alignment can be compact; hot alignment/ICF/literal merging needs runtime-speed
and address-identity proof. Runtime diagnostic facts remain; optional debug
symbols can use digest-bound sidecars after a product decision.

Parallelism follows interface registration -> summary SCCs -> independent
check/lower/optimize/emit jobs -> deterministic link. Shared frozen catalogs,
small initial slabs and memory admission prevent workers multiplying program
RAM. Tiny hello is serial; batch tiny functions, partition oversized work only
lawfully. Diagnostics/IDs/output order must not depend on completion order.
The assembled compiler is one namespace, not hundreds of independently
parallel semantic modules; function jobs matter. Parallelism comes after
ownership/copy removal, not as an 11 GiB-per-worker shortcut.
J §4 quantifies why threads are secondary: even hypothetical perfect 32-way
scaling of the remaining historical 46.6 s hello Jet work leaves 1.456 s
**plus 19 s rustc**. At 32 workers, Amdahl speedup is only 12.55× with 5%
serial work or 7.80× with 10%. The self-build reserve's 96 MiB analysis scratch
is only 3 MiB/worker at 32, with active bodies budgeted separately. Admission
must use actual scratch/body sizes, not assume ideal utilization or duplicate
global state.

Reuse D-INCR-UNIT1's item/file queries, module-interface fingerprints and sealed
package artifacts in the existing store. Separate complete-input **action keys**
from artifact-content IDs. Checked-function keys include body/signature and
consumed summary/type/trait/constant facts, negative lookup dependencies,
staging/authorized observations and policy; object keys add runtime ABI/backend/
target/CPU/O-level/debug. Unchanged-result fingerprints stop invalidation.
Replay diagnostics, effect/reference ledgers, provenance and source locations;
never bypass required sema/authority checks. Publish atomically, bound caches,
reject mismatches and recompute from authoritative input. A daemon is optional;
fast cold full builds must not rely on it or on a final-artifact hit.

J §6 separates full-build and edit cost: with fixed cost A, useful work B and
true invalidated fraction p, full work is A+B versus ideal incremental A+pB
plus cache/invalidation I/O. A 1% cone can save 100× reusable work, but the
proposed A=0.15 s/B=1.75 s budget gives only **11.34×** wall improvement
(1.90 s versus 167.5 ms) before cache overhead. First make the complete miss
path fast; incremental reuse remains a separate valuable mode under the
ratified policy. Shipped checked libraries and within-build interning are
allowed in both lanes; arbitrary effectful comptime cannot be silently reused.

The useful precedents are specific mechanisms, not language rankings:
[TCC direct emission](https://bellard.org/tcc/tcc-doc.html#Code-generation),
[Zig compact AST](https://github.com/ziglang/zig/blob/0.15.2/lib/std/zig/Ast.zig),
[Carbon typed stores](https://github.com/carbon-language/carbon-lang/blob/trunk/toolchain/base/value_store.h),
[rustc arenas](https://rustc-dev-guide.rust-lang.org/memory.html) and
[red-green queries](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html),
[Go ActionID/OutputID cache](https://go.dev/src/cmd/go/internal/cache/cache.go),
[GNU ld relocation-rooted GC](https://sourceware.org/binutils/docs/ld/Options.html),
[Val exclusivity/destination/sink](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2022/p2676r0.pdf)
and [Rust Vec's compact/no-universal-SVO guarantees](https://doc.rust-lang.org/std/vec/struct.Vec.html#guarantees).
Jai's public job/direct-backend picture supports the same design but does not
establish proprietary arena or scheduler details. S/M/R's upstream citations
are mechanism evidence; moving branches must be pinned in implementation
receipts.

## 4. Ordered phase sequence for Tower #4308

This is a dependency rationale, not a second task ledger. Expected effects are
**[INFERENCE]**, not seconds/bytes earned. Each cutover includes Source/Rust
callers, codecs, active tier adapters, tests and durable contracts; proof and
card state belong in Tower.

1. **Measure actual work and integrate source-visible corrections — Rust bridge.**
   Extend hello phase/artifact/process-tree evidence, distinguish cache hits,
   validate output and record the current envelope/signature/inlining/profile
   changes. Effect: trustworthy target deltas, no duplicate “fix” or invented
   saving. Refresh scaling symbols before changing them.
2. **Remove compiler-sized startup and exact-ID churn — Rust bridge.** Generate
   the sealed canonical ABI catalog, replace full-image/whole-catalog shape
   demand, move private stable IDs to U64/dense locals to U32 and update every
   codec/consumer. Effect: attacks historical ≈18 s/≈12.47 GB fixed setup and
   pointer-backed ID traffic; exact savings require attribution. Intermediate
   front-end budgets from M are ≤256 MiB hello/≤4 GiB self, then ≤64 MiB/≤1 GiB
   after representation work; these do not excuse child RAM or pass final gates.
3. **Normalize reads/moves and repair canonical numeric/output kernels — Rust
   bridge, reused natively.** Borrow print and limbs, stream formatting,
   preserve ordered frame/error semantics, carry read windows/last-use/drop
   facts in MIR and reuse result capacities. Effect: removes demonstrated
   String/newline/operand copies and RC/allocator traffic; improves program
   speed/RAM and compiler self-work together without weakening value semantics.
4. **Publish checked Core and one manifest-driven shipped O2 runtime — Rust
   bridge.** Exact item fallback/provenance, header/body demand, sound artifact
   graph, history/carrier dependency cuts, selective binary assets and prefix-free
   normal linking. Keep a legal SCC rlib initially. Effect: zero ordinary Core
   source checking/runtime rebuild/prefix generation, smaller reached code/data,
   shared cold/warm reuse. Historical 6.52 MB Rust-prefix reduction is a work
   candidate, not equal binary savings or a promise to erase 41 s release cost.
5. **Flatten facts and bound phase lifetimes, then add reusable jobs — Rust
   bridge.** Interned names/types, index AST/TIR/MIR, borrowed metadata APIs,
   summary/worklist deltas, consuming body transfers, streamed digests/output,
   persistent dependency-correct shards and memory-bounded parallel function jobs.
   Effect: removes recursive clone/drop dominance and repeated superlinear
   searches; self-build peak approaches bounded catalogs + live body batch.
   Persistent reuse is additional: the 300k clean-build target forbids using it
   as the explanation for fast full builds.
   Select output adapter after checking: check-only publishes required facts/
   diagnostics, native emits native output, expanded Rust is requested-only.
   Effect: removes the CLI's source-confirmed incidental Rust emission even
   before the final rustc route is retired (J §4.4).
6. **Complete shared-runtime/native ABI and physical layout — D-EXEC1.** One
   lowered ownership/layout form, canonical exact/string/allocator/stop kernels,
   inline aggregates/results, destination ABI, correct captures/indirect/collection/
   drop/global/view routes and x86-64 then aarch64. Retire alternate handwritten
   semantic kernels at parity, retaining platform mechanics only. Effect:
   removes box/mmap/inline-only-number defects and makes native whole-compiler
   builds legitimate; no narrowed hello-only performance claim.
7. **Direct symbol-granular emission/linking and lawful minimal startup —
   D-EXEC1.** Per-symbol sections/relocations, closure before layout, archive
   extraction, service init/fini, static provider/security/sidecar policy,
   O0/O1 direct output and LLVM-first O2. Effect: removes rustc source/check/
   subprocess floor, unused runtime/loader/data roots; enables <100 ms/24 MiB
   hello and sub-C size targets. Native SSO/typed Columns and range/noalias facts
   then remove remaining workload-specific allocations/data traffic; no blanket
   SVO or compile-time-heavy LTO requirement.
8. **Prove composed cutover and remove obsolete routes — D-EXEC1.** Complete real
   stage0→stage1→stage2 outputs/probes, example/error/cleanup/Web parity and O0 live
   replacement; compare one-worker/bounded-parallel cold full builds and matched
   language cells. Only then retire Cranelift/MIREval and obsolete Rust-emission
   normal routes. Effect: no hidden fallback/extra runtime/deployment cost; every
   required metric remains independently gated, including <2 s large clean
   development builds and LLVM release compile/run tradeoffs.

The Rust bridge can remove major front-end work, but H's standalone rustc debug
comparator alone takes ≈100 ms and 164 MB for tiny input (optimized: 166 MB).
The bridge cannot honestly guarantee the final
C-class end-to-end targets. Neither extra threads, cache-only timing nor
`-Oz`/FatLTO is a substitute for the native cutover. D-EXEC1's possible O2 default
reconsideration is benchmark-dependent, not authorization to change it here.

## 5. Owner ballots needed

These are questions for #4308/Tower, not approvals. First check the underlying
ratified rule; if it already decides the exact behavior, apply it rather than
reballot. Private arenas, U64 IDs, borrowing, worklists, demand-only Prelude,
exact Int, Columns, runtime unification and LLVM-first O2 are **not** owner
choices in this research. The beginner path stays ordinary safe source;
expert controls reuse existing mechanisms unless the owner ratifies a new one.

- **B1 — Should every ambient name, including `assert_eq`, use ordinary lexical/
  explicit-import precedence?** A: uniform fallback; B: registry-declared reserved
  assertion exception. **Recommend A**, pending naming-law review; implement B
  faithfully until changed. Witness: local callable/non-callable assertion/print
  shadows, nested scopes and `#NoPrelude`; no failed-call fallback. (P B1.)
- **B2 — Does unused Core import availability trigger observable init/fini?**
  A: operation-demanded Core services only; B: imported-module observable init/fini
  remains a root. **Recommend A for Core**, with explicit service lifecycles;
  retain B if required until migrated/ratified. User-package initialization is
  outside this change. Witness: unused import whose initializer emits a trace,
  including cleanup order. (P B2.)
- **B3 — What entry-environment guarantee is required?** A: current before-user
  snapshot, with only indistinguishable proof-based elision/initial-byte borrowing;
  B: first-use snapshot; C: live host reads. **Recommend A**. Witness: foreign
  mutation before first accessor, including mutation of initial envp storage;
  conservative copy when boundaries are not interceptable. B/C are semantic
  changes, not free lazy-init optimizations. (B §6.)
- **B4 — May environment settings activate observation in an otherwise ordinary
  uninstrumented artifact?** A: always retain current dynamic activation; B:
  canonical build/deployment policy selects instrumentation and defines a clear
  result for unsupported requests; C: remove observation. **Recommend B**, without
  a parallel switch or silently ignored accepted settings. Ballot default/dev/
  release behavior and instrumented/uninstrumented witnesses explicitly. (B §6.)
- **B5a — Which Linux link provider should an eligible ordinary program use?**
  A: canonical static syscall closure where lawful, lawful libc provider for
  actual foreign/host requirements; B: libc always; C: dynamic Jet runtime
  always. **Recommend A**. Compare the same program's artifact+dependencies+
  startup; adding musl/a new public override is a separate dependency/API vote.
  Beginner source stays unchanged; provider overrides reuse existing target/
  build declarations if adequate. (B §6.)
- **B5b — Which executable security/debug packaging default is required?**
  A: ASLR-capable image where feasible, NX stack/W^X, required diagnostic facts
  retained and digest-bound debug sidecar; B: fixed-base byte-golf/no required
  metadata; C: conventional hosted image with embedded debug. **Recommend A**;
  fixed-base embedded targets remain explicitly scoped. Compare deploy/debug
  workflows and same-program sizes, not stripped Jet versus unstripped peers.
  Complete stops, atexit, lexical drops and exit semantics are mandatory:
  compact exceptional cleanup is an implementation choice, not an abort-only
  default ballot. (B §6; P B4.)
- **B6 — What can dynamically named Core/reflection lookup expose?** A: only
  items/types/modules published through the artifact's existing finite export/load
  contract; B: all public Core, charged as a reached optional service.
  **Recommend A**, preserving type-local static reflection. Witness: lookup of
  published/unpublished names and open plugin objects. Never infer a finite
  closure from names observed in one run. (P B3.)
- **B7 — Can caller-specific Core specialization emit new semantic bodies?**
  A: strict shipped O2 kernels plus checked thin layout/drop/formatter glue;
  B: explicit D-EXEC1 exception for demand-instantiated semantic Core bodies,
  cached by complete InstanceKey and charged at first use. **Recommend A**; propose
  B only after same-program evidence shows kernel/glue cannot meet runtime/compile
  targets. This resolves the reports' otherwise ambiguous generic-body cache.
  (P B5; S §4.4.)
- **B8 — What storage/allocation observability does an owning copy guarantee?**
  A: independence only, hidden CoW permitted; B: distinct backing at owning
  materialization boundaries; C: CoW only for an approved immutable/frozen case.
  **Recommend B initially**, respecting D-MEM-COPYSEM1; C requires a winning
  paired cell. Specify AllocError timing, address/provenance and copy/drop hooks.
  Bounded-local SVO may be an internal choice; universal List SVO or a new inline-
  capacity surface is not recommended. Earlier release is allowed only where
  existing observation laws prove it invisible; observable resource close stays
  lexical. (R B1/B2/B5.)
- **B9 — Should the user formatting protocol become writer-based?** A: one
  borrowed writer protocol, existing owning String result as an adapter; B:
  retain current public protocol and optimize builtins/derives only.
  **Recommend A for a ratified clean cutover**; otherwise B honestly leaves
  user-format allocations. Witness: user implementations, Display/Debug/redaction,
  errors, evaluation order and deterministic bytes. No deprecated parallel protocol.
  (R B3.)
- **B10 — May allocator metadata optimization change the exact Fixed fit
  boundary?** A: capacity counts actual payload/alignment/required metadata, so
  lawful compression can improve fit; B: current per-allocation occupancy is
  contractual. **Recommend A**, preserving atomic exhaustion/no heap fallback,
  reverse drops and sentries; verify whether existing acceptance terms require B.
  Update exact-boundary tests rather than hiding a behavior change. (R B4.)
- **B11 — Which public capacity contract accompanies U32 local storage?**
  A: checked <4 GiB per-source/per-shard limits, unlimited aggregate project via
  shards; B: segmented rare oversized-source/arena addressing with compact U32
  locals; C: U64 everywhere. **Recommend B where existing capacity must be
  preserved**, A only if explicitly accepted. This resolves S's preference for
  a diagnosed limit and M's capacity-preserving recommendation. State reserved
  sentinels/exact usable limits; never truncate. (S §6; M §7.)
- **B12a — Should fresh-process checked/object reuse be on by default?** A:
  bounded compiler/target/policy/authority-keyed local cache with opt-out; B:
  opt-in only; C: daemon required for reuse. **Recommend A**, extending existing
  store/query policy where already covered. Ratify any new disk-retention/
  opt-out contract; witnesses include corrupt entries, negative lookup changes,
  identical replayed diagnostics and the separate no-user-reuse full-build lane.
  (S §6.)
- **B12b — Should normal compilation retain full phase IR after its consumers
  finish?** A: no, explicit inspection/archive is streamed; B: always retain;
  C: daemon-global snapshots by default. **Recommend A**. Exact source/diagnostic
  facts and genuinely demanded generic/comptime bodies remain available.
  Witnesses cover normal build, requested inspection and bootstrap archives.
  (M §7.)
- **B12c — What is the default job admission policy?** A: memory/CPU/useful-work
  bounded with tiny programs serial and expert caps; B: serial unless opt-in;
  C: all detected cores without a memory reservation. **Recommend A**. Grain
  size is internal; new CLI options require approval. Witness deterministic
  one-worker/multiworker diagnostics/artifacts and oversized-SCC admission.
  Benchmark RAM caps are not a universal new user memory limit. (S §6; M §7.)
- **B12d — If existing access rules cannot express safe read-view getters,
  should migration add a new borrowed-return surface?** A: general region-tied
  safe returns; B: callback/push access with existing rules; C: Shared ownership
  for every getter. **Recommend B now**; A only with ratified lifetime semantics
  and a paired performance cell. Witness receiver reset/growth/escape, not just
  a successful read. Do not smuggle a new lifetime language into an index-store
  optimization. (M §7.)

Matching semantics/security is the proposed comparator contract; if “best” is
instead intended to include unrestricted weaker-contract 150-byte byte-golf,
or strict victory over zero-valued structural minima, that is a genuine
criterion conflict for the owner to settle, not permission to fabricate wins
or weaken the existing gate.

## 6. Measurement and performance gate

### Existing tools, corrections and receipt identity

Extend [`Tools/perf/hello-compile/`](../../Tools/perf/hello-compile/) rather than
invent another benchmark convention. The inspected `bench.py:19–25,28–77,94–125`
uses stderr Zig debug-print vs stdout peers, `time.time()` rounded to 0.01 s,
50 ms process-group RSS sampling with a hardcoded 4 KiB page, best-of-five
unvalidated runs and newest-ELF guessing. Its docstring mentions GNU time but
`run()` does not invoke it. `measure-jetc.sh:10–14` selects by pgrep and samples
RSS/CPU at 1/10-second intervals; it can attach to the wrong concurrent process
and cannot characterize a 35 ms compiler. Preserve H unchanged; strengthen new
receipts, do not relabel old ones.

Accept a sample only after exact stdout `hello\n`, stderr, exit status and
required error/cleanup semantics match. Discover artifacts/dependencies from
the compiler receipt, not file mtime. Store immutable raw samples and JSON/TSV
bytes/nanoseconds with compiler/source/manifest/runtime/asset/schema/target/
profile/provider/linker/allocator/hardware/cache identities and actual stage
outputs. Toolchain package production is separate from installed user compile:
no prebuilt runtime work hidden outside the timer, and no charged runtime
rebuild when the installed contract promises it is shipped.

Required cache lanes: fresh installed toolchain/cold program store; warm pages
but actual compiler-cache miss; warm checked/object reuse with forced changed
user body/relink; final-artifact hit separately; private body/public interface/
trait or negative-lookup/staging input edits; target/ABI/profile/toolchain
invalidation; developer Core/runtime rebuild. **The Jai-style clean full-build
lane disables user checked/object/final-artifact reuse while retaining the
shipped toolchain library.** Report real stage0/stage1/stage2 and O0/O1/O2; never
replace the slow source-compiler lane with only a native hello.

### What to record

- **Compilation:** monotonic nanoseconds, nonoverlapping startup/authorize/load/
  envelope/restore/ABI projection/Core demand/lex/parse/register/stage/summary/
  check/lower/mandatory analysis/optimize/emit/link/publication phases; backend
  child time while Rust exists. Count bodies checked by reason, SCC rounds,
  nodes/edges/instruction visits, Core requested/decoded/source bytes, generic
  instances, runtime builds/prefix bytes, actual invalidation cone/cache states.
  Parallel traces separate CPU work from critical-path wall time.
- **Compiler RAM:** retain comparable sum-RSS but add exact launched PID/tree,
  kernel high-water/rusage or isolated cgroup peak where available, RSS/PSS/private
  dirty/file pages and allocator live/peak/capacity by phase. Report definitions
  and units explicitly; shared-page double counting, escaped process groups and
  missed sub-50-ms processes cannot prove a tiny peak. Include every rustc/linker
  child and any required daemon. A killed process is a failure, not a low-RAM win.
- **Artifact:** exact as-built/stripped/deployment bytes; PT_LOAD file/memory,
  code/rodata/RW/BSS/TLS/EH/debug/symbol/relocation/header bytes; PT_INTERP and
  DT_NEEDED closure. Use `readelf -W -l -S -d -r`, `size -A`, link map and GC output
  in an attribution run. Sectionless executables need digest-bound writer range
  maps, not “no sections therefore no code.” Root-reason/manifest witnesses must
  prove irrelevant Unicode/XML/DB/TLS/scheduler/history assets are absent.
- **Launch/runtime:** compiled minimal spawn/exec/wait harness, monotonic raw
  samples, interleaved paired order, warmups, median/p95 and uncertainty. Record
  first output separately. Attribution-only traces cover entry/write timing,
  cycles/instructions/faults/syscalls/maps/init/env copies; do not subtract guessed
  Python overhead. Pin/governor/load/page-cache/ASLR/sink conditions are recorded.
  Separate exec latency from hot-loop throughput and warm O0 host calls.
- **Program RAM/copy traffic:** kernel child peak and long-lived `smaps_rollup`
  checkpoints, absolute/base-private/PSS/dirty/stack/heap requested+usable+live+
  high-water/virtual bytes, allocations and copied/retained bytes by ownership
  site. Allocation instruments run separately from timing; preload sees neither
  static/direct-mmap allocators nor every language heap. R §7 flags failed-realloc
  accounting in `Tools/perf/alloccount.c` and policy-tracker bookkeeping; correct
  the witness before using it. “No observed malloc” is not proof of zero allocation.

Instrumentation must not clone/debug-render/hash whole graphs at each marker.
Use scalar counters and coarse buffered publication; instrumented attribution
is not the timing win. Preserve exact units: Linux RSS kB are KiB-sized units;
JSON bytes avoid H's GB/GiB ambiguity.

### Workloads and correctness prerequisites

Hello is necessary but insufficient. Canonical cells include empty entry,
ordinary literal/dynamic/primitive/float/interpolated print; implicit vs explicit
Core; unused imports/comments/strings/shadowing/URL/NoPrelude; reachable callbacks/
constants/defaults/vtables/generic glue/exports/plugins/reflection; Unicode
capability ladders; argv/env/FFI constructors; thread/task cancellation; stop/
assert/report/drop order; tests/fuzz/coverage vs ordinary artifacts. Validate
short-write/EINTR/broken-pipe, output locking/order and observation behavior.
Freestanding/static/dynamic and idiomatic/minimal peer arms are declared
separately with matched error/security contracts; O0 host latency is never
compared with C exec as the same cell.

Runtime sweeps cover read-only nested collections and mutation-after-read,
last-use/phi/field/return/replacement failure and escaping closures; exact Int
±2^62 boundaries, mixed inline/big and 128/1024/4096-bit kernels, atomics/hazards;
String/List size boundaries, append/live copy+mutation and header arrays;
ordered/unordered rendering; arena/Fixed exhaustion/reset/sentries; ratified
Columns #2889/AoS/automatic-copy comparison and #4187 correctness; real CLI,
request/frame and compiler-self workloads. Do not rerun owner-reported defects
merely to reconfirm them; prove the repaired behavior.

Compiler scaling uses the actual assembled self-build, independent-function
and large-single-CFG corpora, repeated vs unique names/types, generic/comptime
SCCs and diagnostic failures. Record source/code/blank/comment/generated lines,
bytes, declaration/function/node counts, biggest live body, workers and
recomputed sets. Doubling independent work should not quadruple visits absent
a real dependency reason. A 300k-line target cannot be extrapolated from hello
or a 1,713 s unfinished sample. Clean and incremental build claims stay separate.

### Encode acceptance in the existing gate

Use `Tools/perf/corpus.tsv`, `ci-perf-check.sh`, source-compiler runner/contract/
policy/gate and existing Core activation/runtime comparison homes. The inspected
peer gate currently declares **latency_ns,memory_bytes**
(`ci-perf-check.sh:34–37,1015–1018`); it is not already a binary/program-runtime
memory gate. Extend its canonical schemas with distinct artifact/deployment/
startup/runtime/RAM metrics and closure correctness facts rather than claiming
prose is enforcement. Replace Core activation's generated-Rust regex liveness
proxy with typed plan + actual artifact evidence (P §7).

Keep two comparisons: **Jet-written versus Rust-reference compiler on the same
Jet workflow**, and **Jet language versus matched peers**.
`source-compiler-policy.tsv:1–9` requires five paired same-run samples, ≤0.10
relative stdev, no outliers and Source/reference <1.00 for every pair and group
on compile/RSS/generated source/artifact/run. That is not the Rust-language
1.05 parity rule. Preserve correctness bytes/counts as equality facts, not
lower-is-better “optimizations.” The current source contract still names
transitional engine rows; migrate them only at the D-EXEC1 cutover, preserving
required workflow/output evidence rather than dropping unsupported cells.

Every required non-Rust positive metric needs ratio <1.00; Rust ≤1.05 is only
parity, and “best” requires stricter win evidence. Add absolute 100 ms/24 MiB/
768 MiB and clean-development throughput targets alongside ratios, not instead
of them. Missing/mismatched/corrupt/unsupported/inconclusive or wrong-output
cells fail closed. No means across profiles, cold/warm states or workloads;
no missing required Jai/TCC/Carbon/Hare/Nim toolchain is counted as a win.

Implementation integration should run the focused ownership/view/String/
numeric/arena/UI proofs; MIR/LIR/ABI/relocation/object/closure fixtures;
implicit-use/cache/integrity/determinism and host/native/Web failure/cleanup
parity; packaged empty-program-store hello; completed bootstrap stage outputs
and canonical probes; then the composed paired performance gate. These are
recommended checks for the main agent after implementation, **not verification
exercised by this research**.
