# Working back from the speed of light (card #6343)

Date: 2026-10-30. Lane: SurfaceDesign-2. Dev: 4e514f96e1.

The owner asked us to work backwards from the machine's physical limits to
what Jet must support and generate, and to be faster than Rust, tail calls
included. This note takes each hardware limit in turn. For each it says what
the hardware allows, what Jet does today, what Rust, Zig, C, Mojo and Odin
guarantee, and what Jet must guarantee.

Most answers are compiler work, not language surface. Those become numbered
criteria in section 5, for codegen cards. One answer needs an owner choice:
whether tail calls are jumps on every build level. Ballot D-TAIL-CALL1
carries it.

## 1. Method

The probe machine is an AMD Ryzen 9 7950X3D (Zen 4, 16 cores, 32 threads,
AVX-512). Its load average ran from 40 to 95 during the runs, so wall time
was useless. Every kernel ran three times under `perf stat -e
cycles:u,instructions:u`, and the tables keep the minimum of each.
Instruction counts are exact; cycle counts still carry some noise from the
shared caches.

- Jet kernels: `$JET_SCRATCH_ROOT/lanes/SurfaceDesign/probes/k6343/*.jet`,
  built at `--optimize=none`, `basic` and `full` (O0, O1, O2) with the dev
  compiler, toolchain root an export of 4e514f96e1.
- Rust peers: `probes/r6343/kernels.rs`, one binary, the kernel picked by
  argument. Built with rustc 1.97.1 at opt-level 0, at opt-level 3, and at
  opt-level 3 with `-C overflow-checks=on`. Inputs pass through `black_box`.
- Harness: `probes/pstat.sh`. Raw table: `probes/pstat6343.tsv`.

Hardware figures come from AMD's Software Optimization Guide for Zen 4,
publication 57647 revision 1.01 (cited below as SOG).

## 2. Measured kernels

Each cell is millions of user cycles / millions of user instructions. The last
two columns divide Jet O2 by Rust O3. The AGENTS.md gate (AGENTS.md:149-154)
is a Jet/Rust ratio at or below 1.05. Every finished kernel printed the same
output in Jet and Rust. `tail2` stops in Jet, so its ratio columns say
"stops". Each figure is the lowest of three runs, taken with `perf stat`
on a loaded 32-core machine.

| kernel | Jet O0 | Jet O1 | Jet O2 | Rust O0 | Rust O3 | Rust O3 + overflow checks | cycles O2/O3 | instructions O2/O3 |
|---|---|---|---|---|---|---|---|---|
| `loop_int` | 5,652 / 31,110 | 6,418 / 28,310 | 2,707 / 16,402 | 14,438 / 30,400 | 1,853 / 5,200 | 1,850 / 7,600 | 1.46 | 3.15 |
| `loop_i64` | 6,682 / 27,110 | 5,906 / 23,910 | 3,558 / 18,310 | 8,014 / 30,400 | 1,415 / 5,200 | 1,472 / 7,600 | 2.51 | 3.52 |
| `sum_index` | 2,009 / 13,472 | 1,759 / 12,132 | 2,182 / 11,542 | 6,892 / 20,040 | 236 / 500 | 442 / 1,350 | 9.25 | 23.08 |
| `sum_iter` | 1,562 / 8,712 | 1,135 / 7,132 | 1,035 / 6,572 | 2,412 / 9,240 | 270 / 500 | 293 / 1,350 | 3.83 | 13.14 |
| `axpy` | 4,524 / 27,008 | 3,630 / 20,572 | 2,758 / 16,992 | 5,284 / 16,356 | 505 / 750 | 542 / 750 | 5.46 | 22.66 |
| `inline_small` | 4,471 / 30,202 | 3,038 / 18,885 | 1,660 / 10,519 | 5,622 / 20,742 | 873 / 2,788 | 620 / 3,259 | 1.90 | 3.77 |
| `struct_ret` | 2,528 / 14,226 | 2,078 / 12,676 | 2,018 / 11,827 | 1,203 / 4,400 | 473 / 1,275 | 473 / 1,275 | 4.27 | 9.28 |
| `option_ret` | 942 / 5,885 | 837 / 4,902 | 537 / 3,319 | 1,490 / 5,450 | 201 / 784 | 212 / 900 | 2.67 | 4.23 |
| `fib` | 362 / 1,809 | 266 / 1,495 | 269 / 1,495 | 198 / 672 | 75 / 302 | 152 / 433 | 3.59 | 4.95 |
| `dispatch` | 5,351 / 16,620 | 3,161 / 14,932 | 2,778 / 9,440 | 4,065 / 14,638 | 925 / 3,175 | 776 / 3,488 | 3.00 | 2.97 |
| `matmul` | 1,101 / 6,056 | 833 / 5,157 | 490 / 2,337 | 2,133 / 5,457 | 20 / 102 | 25 / 102 | 24.50 | 22.91 |
| `byte_count` | 4,174 / 23,900 | 3,098 / 15,041 | 3,006 / 11,622 | 7,617 / 30,410 | 1,084 / 3,364 | 1,114 / 4,872 | 2.77 | 3.45 |
| `closure_map` | 1,876 / 9,818 | 2,837 / 9,124 | 2,076 / 8,776 | 5,191 / 11,019 | 188 / 553 | 184 / 603 | 11.04 | 15.87 |
| `tail2` | 645 / 1,353 | 464 / 1,110 | 504 / 1,110 | 310 / 527 | 6 / 25 | 62 / 185 | stops | stops |

Reading the table:

- No kernel meets the 1.05 gate at O2. The closest in cycles are `loop_int`
  (1.46) and `inline_small` (1.90). In instructions every kernel needs at
  least three times Rust's count.
- The worst gaps are loops over lists: `matmul`, `sum_index`, `axpy`,
  `sum_iter` and `closure_map`, at 13 to 23 times Rust's instructions. Each
  step pays a bounds check, `Int` tests and scalar code, where Rust
  vectorizes and removes the checks.
- Quick runs already beat Rust's debug builds on most kernels. Jet O0 used
  fewer cycles than Rust O0 on 10 of the 13 finished kernels. Rust O0 won on
  `struct_ret`, `fib` and `dispatch`.
- O2 adds nothing over O1 on `fib` and `tail2`: the instruction counts are
  identical.
- Rust with overflow checks on is the fair peer for checked arithmetic. It
  costs 1.46 times plain Rust's instructions on `loop_int` and 2.7 times on
  `sum_index`; Jet costs 3.15 and 23.08 times.
- `tail2` is not a finished run in Jet. It prints the first result, then
  stops with E3012 in `is_even(10000000)` on every level. Rust O3 finishes in
  25 million instructions because LLVM turned both recursions into loops.
  Rust O0 overflows its stack.
- Cycle counts carry noise from the loaded machine; for example Jet O2
  `sum_index` shows more cycles than O1 with fewer instructions.
- The first `dispatch` peer used Rust's truncating `%`, and printed a
  different total. Jet's `%` on `Int` is the floor modulo (`-7 % 3` is 2), so
  the peer now uses `rem_euclid`, and both print 967383. The `dispatch` row
  comes from that rerun (probe `probes/m6343/mod.jet` prints 2 and -4).

## 3. The limits, one by one

### 3.1 Instruction throughput

**Hardware.** Zen 4 dispatches up to 6 macro ops per cycle and keeps up to
320 in flight (SOG sections 2.9.8 and 2.10.3). A loop's speed is set by how many
instructions each step needs, and by the longest chain of dependent ones.

**Jet today.** `loop_int` needs 3.15 times Rust's instructions at O2, and
`inline_small` 3.77 times. Every `Int` operation carries a small-or-big test
(D-INTBIG1=A: "arithmetic carries a small-versus-big check"). The O2 code of
`axpy` holds about seven `add`/`jo` pairs per step. The fixed-width `I64` loop
is slower than the `Int` loop (18,310 against 16,402 million instructions),
because each `I64{i}` conversion and each `I64` operation is checked too.

**Peers.** Rust release builds turn overflow checks off by default, so `+`
wraps silently (Cargo book, profiles: `overflow-checks` defaults to false in
release). C makes signed overflow undefined, so the compiler assumes it never
happens. Zig checks overflow in Debug and ReleaseSafe and assumes it away in
ReleaseFast (ziglang.org/documentation/0.15.2, Build Mode). This note did not
check Odin's or Mojo's overflow rules.

**What Jet must guarantee.** Exact `Int` and checked fixed widths stay; they
are ratified safety. The compiler must then remove the checks it can prove
away, so a counted loop costs what Rust's costs. Rust with overflow checks on
is the honest peer for checked arithmetic: `loop_int` costs 7,600 million
instructions there, against Jet's 16,402. Range analysis (P-RANGE in
Docs/research/ir-optimization-inventory-2026-10-21.md:257-259) is the
missing pass.

### 3.2 Memory bandwidth and latency

**Hardware.** The L1 data cache serves three memory operations per cycle, at
most two of them 256-bit loads (SOG section 2.6.2). A main-memory miss costs
hundreds of cycles, so a stream must be sequential and prefetchable.

**Jet today.** `byte_count` scans a 64 MB list of `U8`. `sum_iter` and
`sum_index` scan 80 MB of `Int`. Even so, their instruction ratios (table
above) show the loop body, not memory, is the limit today. Two layout costs
show up as well: optionals and small records returned from functions
(`option_ret`, `struct_ret`).

**Peers.** Rust, Zig and C store plain values inline and return small structs
in registers under the platform C ABI. Rust's `Option<i64>` is two words with
no heap use.

**What Jet must guarantee.** Values in lists, optionals, results and records
are stored inline (D-MEM-UNBOX1=A). Layout control for experts is ratified
(D-JAI-LAYOUT1=A), and so are cache hints (D-CACHE-CONTROL1=A). The compiler
criteria are box elimination for optionals (P-BOX, inventory:270-271) and
register returns for small records (G2, inventory:246).

### 3.3 Caches

**Hardware.** 32 KB of L1 data, 1 MB of L2 per core, and 96 MB of L3 on this
part (SOG sections 2.6.2 to 2.6.4; `lscpu` on the probe machine). Each cache line is 64
bytes.

**Jet today.** A fresh `String` is two allocations: a 32-byte header and the
bytes (Compiler/JetRuntime/Source/Text.jet:54-63, 74-77). That doubles
allocator traffic and splits each text over two lines. `matmul` keeps three
300 by 300 `Float` lists, 2.2 MB in all, so they spill out of L2 into L3. Rust
runs the same layout in 20 million cycles, so Jet's gap there is in the loop
body, not the cache.

**What Jet must guarantee.** Nothing new at the surface. Note #6342
(Docs/research/2026-10-30-fixed-text-and-buffer.md) lists the text layout
criteria.

### 3.4 Branch prediction

**Hardware.** A mispredicted branch costs 11 to 18 cycles, 13 in the common
case (SOG section 2.8). A predictable branch costs almost nothing, so checks
that never fail are cheap if they sit off the hot path.

**Jet today.** `if op == { … }` on an enum compiles to a chain of compares,
one arm per `If` (Compiler/JetIR/Source/BuildExpr.jet:3552-3553). Jet has no
jump table. `dispatch` measures the cost. Bounds and overflow stops in `axpy`
are inline in the loop body, each behind a forward branch.

**Peers.** Rust, C (Clang), Zig and Odin compile through LLVM, which lowers a
dense `match` or `switch` to a jump table when that is cheaper than compares.

**What Jet must guarantee.** Nothing at the surface: `if x == { … }` already
says everything a jump table needs. The criteria are a jump table for dense
enum and integer matches, and stop paths moved out of line.

### 3.5 SIMD width

**Hardware.** Zen 4 runs AVX-512 by issuing each 512-bit operation over two
cycles on 256-bit paths, so peak FLOPS match 256-bit code (SOG section 2.11).
Four `Float`s per operation per pipe is the floor a loop should reach.

**Jet today.** Jet has no vectorizer (P-VEC, inventory:281). `axpy` at O2
uses scalar `movsd`, `mulsd` and `addsd`. It needs 22.66 times Rust's
instructions. Before the loop it copies `dst` when the list might be shared:
a uniqueness test, then `jet_rt_alloc_8` and `jet_rt_mem_move`.

**Peers.** Rust and C vectorize through LLVM, helped by `&mut` being
`noalias` in Rust and by `restrict` in C. Zig has `@Vector`, Mojo makes
`SIMD[type, width]` its base numeric type, and Odin has `#simd`.

**What Jet must guarantee.** Already ratified: auto-vectorization by default
with an opt-out (D-SIMD3=B), one lane type family (D-LANES1=A) and checked
native intrinsics (D-SIMD-NATIVE1=A). Jet's `&` is exclusive
(D-MEMREF-EXCL1=A), and plain parameters cannot be written (D-PARAM-CONST1=A).
So Jet can promise the "no alias" fact that Rust gets from `&mut`, on every
call, with no annotation. The vectorizer must use it.

Float reductions are the one trap. Adding Floats in a different order changes
the result, and I9 (AGENTS.md:135-147) forbids a result that depends on the
build level. A vectorized Float sum must keep the source order, or the
program must ask for lanes. Integer sums can be reordered freely.

### 3.6 Call overhead

**Hardware.** A call and return cost a few cycles plus any registers saved,
restored or spilled around them. Inlining removes all of it and opens the
callee to the caller's optimizations.

**Jet today.** `fib(35)` needs 4.95 times Rust's instructions and 3.96 times
its cycles at O2. Each call and return also costs a fetch bubble (SOG
section 2.8.1.3.3). Jet's inliner skips any callee over 40 IR
instructions at O1 and 60 at O2 (inventory:198), and does not honour
`#Inline(Always)` (P-IA, inventory:272). Every `Int` argument carries a drop
after the call, because an `Int` may own heap digits.

**Tail calls.** A call that is the last thing a function does can reuse the
caller's frame. D-DROP-ASAP1=A promised this: "Emit tail calls as jumps when
no visible drop is pending". D-RESOURCE-LIMIT1=A says "Only O2 may remove
recursive calls". Today no level does it. `walk(10000000, 0)` and
`is_even(10000000)` stop with E3012 on O0, O1 and O2. The O2 code of `walk`
is `call tail2_walk` followed by `call jet_rt_int_drop`. The drop is the
pending work that blocks the jump.

Rust has no stable guarantee. `become` is nightly only and "incomplete"
(doc.rust-lang.org/std/keyword.become.html). Rust O3 turns `walk` into a loop
by choice, but Rust O0 overflows. Clang's `[[clang::musttail]]` and GCC 15's
same attribute guarantee the jump or refuse to compile: a GCC 15.3 build at
`-O0` ran `walk(10000000, 0)` to completion (`probes/c6343/mt.c`). Zig has
`@call(.always_tail, f, args)`. Scheme requires proper tail calls of every
implementation (R7RS section 3.5).

Guaranteed tail calls are a place where Jet can beat stable Rust. Interpreters
and parsers written as small functions that jump to each other keep their
state in registers. The upb protobuf parser reached over 2 GB/s that way
(blog.reverberate.org, 2021-04-21, "Parsing Protobuf at 2+GB/s").

**What Jet must guarantee.** Ballot D-TAIL-CALL1 recommends that every tail
call is a jump on every level, with a lint naming any call that cannot jump.
With ASAP drop, an `Int` argument's digits are freed before the jump, so the
drop no longer blocks it.

### 3.7 Code size

**Hardware.** The op cache holds 6.75 K macro ops and the L1 instruction
cache 32 KB (SOG section 2.9.1). Hot code that fits runs from the op cache at
up to 9 ops per cycle.

**Jet today.** Inline overflow and bounds stops bloat hot loops. The
inventory records O1 code 4.5 percent and O2 code 9.6 percent larger than O0
over 280 programs (inventory:216-221).

**What Jet must guarantee.** Nothing at the surface. The criterion is that
stop paths and cold calls move out of the loop body, so hot loops fit in the
op cache.

## 4. Peer guarantees

| guarantee | Rust | Zig | C | Mojo | Odin | Jet (ratified) |
|---|---|---|---|---|---|---|
| integer overflow | wraps in release, panics in debug | checked except ReleaseFast | signed is undefined | not checked | not checked | exact `Int` (D-INTBIG1); fixed widths trap |
| bounds checks | always, optimizer removes | except ReleaseFast | none | not checked | default on, `#no_bounds_check` per block | always; proofs remove (D-OOBPROOF1) |
| no-alias facts | `&mut` is noalias | `noalias` keyword | `restrict` | not checked | not checked | `&` exclusive (D-MEMREF-EXCL1) |
| guaranteed tail calls | nightly `become` only | `.always_tail` | Clang and GCC 15 `musttail` | not checked | not checked | promised (D-DROP-ASAP1), not built |
| SIMD | auto plus `std::arch` | `@Vector` | auto plus intrinsics | `SIMD` type | `#simd` arrays | auto plus lanes (D-SIMD3, D-LANES1) |
| destruction | end of scope | manual, `defer` | manual | at last use | manual, `defer` | at last use (D-DROP-ASAP1) |
| forced inlining | `#[inline(always)]` | `inline fn` | `always_inline` | `@always_inline` | `#force_inline` | `#Inline(Always)` (D-INLINE-PARAM1), not honoured |

Sources: Cargo book profiles (doc.rust-lang.org/cargo/reference/profiles.html);
Zig 0.15.2 documentation, Build Mode and `@call`; Clang AttributeReference,
`musttail`; Mojo manual, "Value destruction" (mojolang.org/docs/manual/lifecycle/death);
Odin overview and directives (odin-lang.org/docs/overview, /docs/directives).

## 5. Compiler criteria (no owner choice)

Each criterion is a codegen card's acceptance list. Each names the kernel
that proves it; a criterion is met when that kernel's Jet O2 / Rust O3
instruction ratio is at or below 1.05.

1. **Range facts on `Int`** (P-RANGE). A loop counter with known bounds, and
   arithmetic on it that provably stays in range, compile to plain machine
   arithmetic with no small-or-big test and no drop. Kernels: `loop_int`,
   `inline_small`, `sum_index`.
2. **Checked `I64` costs one test.** A fixed-width operation compiles to the
   machine operation plus one overflow branch, and `I64{i}` from an in-range
   `Int` counter costs nothing. Kernel: `loop_i64`, against Rust with
   overflow checks on.
3. **Bounds-check removal** (P-BCE). An index proven below the list length
   needs no check, and the length is loaded once per loop when the list
   cannot change. Kernels: `sum_index`, `axpy`, `matmul`.
4. **Hoisting** (P-LICM). Loop-invariant loads, such as a list's data pointer
   and length, move before the loop. Kernels: `axpy`, `matmul`.
5. **Vectorizer** (P-VEC, D-SIMD3=B). Inner loops over `Int` and `Float`
   lists use AVX2 or AVX-512 lanes. Exclusive `&` and read-only parameters
   are the no-alias proof. Float sums keep source order unless lanes are
   written. Kernels: `axpy`, `sum_iter`, `byte_count`, `matmul`.
6. **No copy on write when unique.** A `&` list parameter is unique by
   construction, so the uniqueness test and copy at function entry go.
   Kernel: `axpy`.
7. **Small records and optionals in registers** (P-BOX, G2, G3). A record of
   up to four words, and an optional of one word, return in registers with no
   heap box. Kernels: `struct_ret`, `option_ret`.
8. **Inliner** (P-IA, D-OPT-INLINE1, D-INLINE-PARAM1). `#Inline(Always)` is
   honoured at every level that inlines. Leaf calls in hot loops are inlined
   past the 40/60 budget when the caller's loop is hot. Kernels:
   `inline_small`, `fib`.
9. **Jump tables.** A match on an enum or a dense integer range with five or
   more arms becomes one indexed jump. Kernel: `dispatch`.
10. **Tail calls** (D-DROP-ASAP1, ballot D-TAIL-CALL1). Under D-TAIL-CALL1 A
    every tail call is a jump on every tier, compile-time evaluation
    included. Kernels: `tail1`, `tail2`, `tail3`.
11. **Cold stop paths.** Bounds, overflow and division stops sit after the
    function's hot code, reached by a forward branch that falls through on
    success. Kernels: all loops.
12. **Closures.** `map` and `filter` with a literal lambda inline the lambda
    and fuse into one pass with one result allocation. Kernel: `closure_map`.
13. **Debug info for jumps.** Release builds record tail calls in DWARF
    (`DW_TAG_call_site` with `DW_AT_call_tail_call`, DWARF 5 section 3.4).
    Quick runs keep the last 16 tail calls per thread for stop reports.

## 6. Paired perf cells against Rust

Each row is one cell for Tools/perf/native-cells: a Jet program, a Rust peer
and a metric. The gate is AGENTS.md:149-154: Jet/Rust at or below 1.05 in
cycles. The sources exist already in `probes/k6343` and `probes/r6343`.

| cell | Jet program | Rust peer | criteria | today (O2/O3 instructions) |
|---|---|---|---|---|
| `numerics/int_loop` | `k6343/loop_int.jet` | `kernels.rs loop_int` | 1, 11 | 3.15 |
| `numerics/i64_loop` | `k6343/loop_i64.jet` | `kernels.rs loop_i64` | 2, 11 (peer: Rust O3 with overflow checks) | 3.52 |
| `collections/sum_index` | `k6343/sum_index.jet` | `kernels.rs sum_index` | 1, 3, 4, 5 | 23.08 |
| `collections/sum_iter` | `k6343/sum_iter.jet` | `kernels.rs sum_iter` | 5 | 13.14 |
| `numerics/axpy` | `k6343/axpy.jet` | `kernels.rs axpy` | 3, 4, 5, 6 | 22.66 |
| `numerics/inline_small` | `k6343/inline_small.jet` | `kernels.rs inline_small` | 1, 8 | 3.77 |
| `numerics/struct_ret` | `k6343/struct_ret.jet` | `kernels.rs struct_ret` | 7 | 9.28 |
| `numerics/option_ret` | `k6343/option_ret.jet` | `kernels.rs option_ret` | 7 | 4.23 |
| `numerics/fib` (exists) | `k6343/fib.jet` | `kernels.rs fib` | 8, 1 | 4.95 |
| `numerics/dispatch` | `k6343/dispatch.jet` | `kernels.rs dispatch` | 9 | 2.97 |
| `numerics/matmul` | `k6343/matmul.jet` | `kernels.rs matmul` | 3, 4, 5 | 22.91 |
| `text/byte_scan` (exists) | `k6343/byte_count.jet` | `kernels.rs byte_count` | 5 | 3.45 |
| `closures/map_filter_collect` (exists) | `k6343/closure_map.jet` | `kernels.rs closure_map` | 12 | 15.87 |
| `numerics/tail_walk` | `k6343/tail2.jet` | `kernels.rs tail2` | 10 | Jet stops with E3012 |

Two cells pair a surface with its plain spelling, as AGENTS.md:171-174
requires for a surface added for performance:

- `numerics/tail_walk`: `walk(n - 1, acc + n)` as a tail call against the
  same sum written with `loop`. D-TAIL-CALL1 A passes when the tail form costs
  no more than the loop form, on every level.
- `numerics/tail_interp`: a 16-opcode bytecode interpreter written as
  handlers that tail-call the next handler, against the same interpreter as a
  `loop` around a match. The peer is the Rust `loop` plus `match` version,
  because stable Rust cannot promise the tail form.

## 7. Bugs found

1. Tail calls are not jumps on any level, against D-DROP-ASAP1=A. Repro:
   `probes/k6343/tail1.jet` stops with E3012 on O0, O1 and O2. `tail2.jet`
   stops in `is_even(10000000)`, and `tail3.jet` stops at depth 1,000,000.
2. `#Inline(Always)` is not honoured (already tracked as P-IA,
   inventory:272).
3. The fixed-width `I64` loop needs more instructions than the `Int` loop
   (`probes/k6343/loop_i64.jet` against `loop_int.jet`).
4. A `&` list parameter is tested for uniqueness and may be copied on entry,
   though `&` is exclusive. Repro: disassemble `axpy_axpy` in a full build of
   `probes/k6343/axpy.jet`.

## 8. Decisions consulted

D-DROP-ASAP1=A, D-RESOURCE-LIMIT1=A, D-TAIL-RETURN1=A, D-TASK-SLICE1=A,
D-DEADLINE-ONE1=A, D-STACKBOUND1=A, D-INTBIG1=A, D-OOBPROOF1=A,
D-UB-BOUNDARY1 (open, owned by MemDesign), D-SIMD3=B, D-SIMD-NATIVE1=A,
D-SIMD-COMPILER1=A, D-LANES1=A, D-OPT-INLINE1=A, D-CODEGEN-HINTS1=A,
D-INLINE-PARAM1=A, D-MEMREF-EXCL1=A, D-PARAM-CONST1=A, D-MEM-UNBOX1=A,
D-CABI-PLATFORM1=A, D-CACHE-CONTROL1=A, D-JAI-LAYOUT1=A, D-ARRAY-CAPACITY1=A,
D-ARRAY-INDEX1=A, D-OPTFLAG1=A, D-ADAPTIVE-TIER1=B, D-EXEC3=A,
D-SCRIPT-SLOTADD1 (open: compound update of a list slot).
