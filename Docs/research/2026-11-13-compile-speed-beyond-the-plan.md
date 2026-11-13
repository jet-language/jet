# Compile speed beyond the plan (2026-11-13)

The speed program on #5479 already plans precompiled Core (#6328 c22), the
data-oriented compiler core (#6793), the parallel pipeline (#6794), a fused
per-function pipeline with a direct image writer (#5479 c8) and a SIMD lexer
(#5910 c6). This report lists the techniques that could push cold compile
speed further than that plan, toward the theoretical limit. It is analysis,
not measurement: nothing here has been prototyped in Jet, and every number
below is either cited or marked as an estimate.

Owner disposition (2026-11-13): ideas 1, 3, 4 and 6 are planned as cards;
the others go to ballots.

## Constraints every idea must meet

- Cold compile counts. There is no compile cache, no incremental
  compilation, no reuse of a previous run's work, and no resident compiler
  server that keeps checked state between runs (D-EXEC2=A and the
  performance gate in AGENTS.md).
- There is no interpreter or JIT tier. `jet run`, `jet dev` and
  compile-time code compile natively at O0 (D-EXEC2=A, I9).
- Work done once when the toolchain is built is toolchain, not cache. The
  owner ruled this for precompiled Core, and the same reading applies to
  any table or code template built with the compiler and embedded in it.
- Every tier keeps one meaning (I9), and every fix is general, never tied
  to one benchmark.

## The physical limit

The exit target is 30 million lines per second (#5479 c3). At about 30
bytes per line, that is about 1 GB/s of source, far below memory
bandwidth, so reading the source is not the bound. On the reference
machine's 32 hardware threads it allows about 1 µs of CPU per line per
thread, roughly 4,000 instructions for a line of 6 to 8 tokens
(estimate). Single-pass C compilers such as TCC run at very roughly a
million lines per second on one core. So 30M lines/s on 32 threads means
TCC-class work per token for a much richer language, and the levers that
remain are less work per token, cheaper machine-code generation, and a
language that is cheap to check by construction.

Today (2026-11-13, #5479 log): the c1 throughput cell runs at 8,510
lines/s, with about 5 of 32 threads busy (#5479 c6), so roughly 590 µs of
CPU per line.

## Ideas

### 1. Copy-and-patch machine code at O0 (planned: #6795)

Each IR operation has a pre-built machine-code template with holes. The O0
backend copies the template and patches constants, frame offsets and jump
and call targets. There is no instruction selection and no register
allocation at O0. Xu and Kjolstad report code generation one to two
orders of magnitude faster than LLVM -O0, with better code than a classic
baseline compiler; CPython's JIT (PEP 744) uses the technique. The
templates are built with the toolchain, so the constraints hold. This is
likely the largest single backend win for `jet run`, `jet dev` and
compile-time code. O1 and O2 keep the optimizing backend.

### 2. No syntax tree for ordinary code (ballot)

The parser emits checked IR directly while it reads, as Wirth's Oberon
compilers and TCC do. A tree is built only where a later pass needs one:
generics, compile-time code and macros. This removes the tree's
allocation and one full memory pass. It goes further than #5479 c8, which
still builds a tree before fusing phases. It needs a ballot because it
constrains the language: every construct outside those three must be
checkable in one forward pass.

### 3. Parallel lexing and parsing inside one file (planned: #6796)

A SIMD pre-scan finds top-level declaration boundaries, and chunks are
lexed and parsed on separate threads. The compiler is essentially one
large module, so module-level parallelism (#6794) cannot split it. Lexing
must be able to restart at a top-level boundary. If that needs a language
rule, such as no raw newline inside a string literal, the rule goes on a
ballot first.

### 4. Shared generic code at O0 (planned: #6797)

At O0, a generic function is compiled once and receives its type
information (size, alignment, trait method tables) as a hidden argument,
as Swift does for unspecialized generics and Go does with GC-shape
stenciling and dictionaries (Go 1.18). O1 and O2 keep per-type
specialization. The meaning is the same on every tier; only the
implementation differs. Per-type copies are the main compile-time cost of
generics in Rust and C++.

### 5. A language designed for linear-time checking (ballot)

Every function body checks independently of other bodies. Inference never
crosses a function boundary. Overloads and trait lookups resolve through
perfect-hash tables built per compile. No rule needs backtracking or
search. This is a language-design discipline: an audit of the current
rules would list every rule that can be super-linear, and each finding
becomes its own ballot.

### 6. Cheaper compile-time execution (planned: #6798)

Compile-time code runs natively in a child of the compiler. Today each
batch pays process start-up, an O0 compile and value serialization. The
plan is one child per compile kept warm for that compile only, compile-time
functions compiled through idea 1, and values passed through shared memory
instead of serialized. On 2026-11-13 the compile-time re-finalize alone was
1.19 s of a 2.86 s `RunColdContext` check.

### 7. Operating-system costs (ballot)

Huge pages and pre-faulted memory regions, since page faults are a real
cost at GB/s. Never freeing memory and letting process exit release it.
Reading every source file in one batch with io_uring or mmap. Writing the
executable image straight into memory, with the runtime and Core already
laid out at fixed addresses, so each run only patches relocations. Each is
small alone; together they remove most fixed per-compile cost. The ballot
covers the policy choices: a Linux-first fast path versus one portable
path, and whether "never free" is acceptable given I1 (safe Jet cannot
leak).

### 8. Profile-guided compilation of the compiler itself (ballot)

Build the `jet` binary with profile-guided optimization and post-link
layout (BOLT: Panchenko et al., CGO 2019). That is typically worth tens of
percent on instruction-cache-bound programs like compilers. It is #5479
plan step 6, which the owner placed after the language stabilizes; the
ballot asks whether to start it earlier, and which profile corpus defines
"typical" so that no benchmark is favored.

### 9. A fully data-parallel compiler (ballot)

Aaron Hsu's Co-dfns compiler (PhD thesis, Indiana University, 2019) and
the Pareas GPU compiler write every pass as array operations over flat
node tables, including tree walks, name resolution and type propagation.
That turns the data-oriented core into something that runs on wide SIMD or
a GPU. It is a full compiler rewrite, unproven for a language as rich as
Jet, and the only known path to throughput well beyond what 32 CPU cores
allow. The ballot asks whether to fund a research prototype after #6793.

### 10. Checking as SIMD over token columns (ballot)

Some checks, such as name lookup inside a block, scope depth and simple
literal typing, can run as vector operations over the token and node
columns instead of a per-node walk. This extends #6793's flat columns. The
ballot asks which checks qualify and whether diagnostics keep their exact
spans and order, which they must.

## Ruled out

Compiling a function on its first call (a JIT), incremental compilation, a
resident compiler server with checked state, and any cross-run cache are
excluded by D-EXEC2=A and the performance gate. These are where other
toolchains get instant rebuilds, so Jet's answer has to be cold speed.

## Sources

- Haoran Xu and Fredrik Kjolstad, "Copy-and-Patch Compilation: A fast
  compilation algorithm for high-level languages and bytecode", OOPSLA 2021.
- PEP 744, "JIT Compilation" (CPython 3.13).
- Fabrice Bellard, Tiny C Compiler (TCC), bellard.org/tcc.
- Niklaus Wirth, "Compiler Construction" (single-pass Oberon compiler).
- Go 1.18 generics implementation: GC-shape stenciling with dictionaries
  (go.googlesource.com/proposal, generics-implementation-dictionaries-go1.18.md).
- Swift unspecialized generics through witness tables (Swift ABI
  documentation, TypeMetadata and generic calling convention).
- Maksim Panchenko et al., "BOLT: A Practical Binary Optimizer for Data
  Centers and Beyond", CGO 2019.
- Aaron W. Hsu, "A data parallel compiler hosted on the GPU", PhD thesis,
  Indiana University, 2019 (Co-dfns).
- Robin Voetter et al., "Compilation on the GPU? A Feasibility Study",
  Computing Frontiers 2022 (Pareas).
