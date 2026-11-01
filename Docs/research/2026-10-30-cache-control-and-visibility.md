# Cache control and cache visibility: what Jet has, what a chip and an OS really let a program control, and how to show where data comes from (2026-10-30)

Tower card: #6312. Related cards: #4727 (memory profile method), #3447, #4212. Ballots drafted from this research: `D-PERF-MEMORY1`, `D-CACHE-CONTROL2`.

Code facts cite dev `82d3369757`. Probes used the prebuilt dev compiler `/mnt/jetscratch/discovery/base/latest/jet` (also `82d3369757`) on all three tiers: `--optimize=none` (o0), `--optimize=basic` (o1) and `--optimize=full` (o2).
Probe files live in `/mnt/jetscratch/lanes/MemDesign/probes/`. C comparison programs are in `probes/cprior/`, built with gcc 15.3 `-O2 -march=native`.
Host: AMD Ryzen 9 7950X3D (Zen 4, 16 cores, 32 KiB L1D and 1 MiB L2 per core, two L3 slices totalling 128 MiB), one NUMA node, Linux 7.2.8, `perf_event_paranoid=2`, transparent huge pages `always` with defrag `defer+madvise`, `nr_hugepages=0`, resctrl not mounted. Reasoning that no probe tested is marked `[INFERENCE]`.

## Plain summary

The owner asked two things: how a Jet program can control the L1, L2 and L3 caches, and how to see when code leaves the cache for RAM or disk.

D-CACHE-CONTROL1 A, ratified 2026-10-07, answers the first with three tools: `mem.prefetch`, `mem.stream_store` and `$build.cpu.cache_line`. None of the three exists in code. All probes fail on o0, o1 and o2 with E1004 or E0107.
Measured in C on this host, two of the three effects are real. Streaming stores make a 256 MiB copy 33% faster, and padding two contended counters to separate lines makes them 3.7 times faster. Prefetch is mixed: it gains 14% on a random gather but nothing on a fixed-stride walk, because the hardware prefetcher already covers fixed strides. The ratified gate cell for prefetch is the fixed-stride walk, so it would not pass as written.

Caches themselves cannot be steered by a normal program. A chip offers hints (prefetch, non-temporal stores, cache-line flushes), not placement. Cache partitioning (Intel CAT, AMD's equivalent, Arm MPAM) is a root-only operating-system setting. Huge pages and NUMA placement are memory controls, not cache controls. Huge pages proved unreliable here: asking for them stalled a 512 MiB fill for 6 to 7 seconds and granted none.

The second question has a working answer on Linux today, without root. Hardware counters sampled per instruction and mapped through DWARF line tables already point at the Jet source line that waits on memory. On this host they name `walk.jet:12`, the strided loop, for 58% of the L2-to-L1 refills, while the sequential loop gets 0.09%. Page-fault counters separate RAM faults from disk faults. What Jet lacks is the command: `jet perf` is registered but not ported, and the old Rust command never read hardware counters.

## 1. The ratified cache hints do not exist yet

`D-CACHE-CONTROL1` A specifies `mem.prefetch(place, .Read | .Write, level: .L1 | .L2 | .L3 = .L1)`, `mem.stream_store(&dst, value)` and `$build.cpu.cache_line`, with a default of 64 on generic targets. Its lowering goes through D-CORE-BOUNDARY1 instruction entries. The decision requires a paired two-program cell for each surface. The cells are a strided grid walk with and without prefetch, a large copy with and without streaming stores, and contended counters with and without padding.

Probes, identical on o0, o1 and o2:

| Probe | Result |
|---|---|
| `c1_prefetch.jet` | E1004 `core.mem` has no item `prefetch` (line 8) |
| `c2_stream_store.jet` | E1004 `core.mem` has no item `stream_store` (line 10) |
| `c3_cache_line.jet` | E0107 nothing named `$build` (line 2) |
| `c4_align_line.jet` | E1105 `align` requires one integer byte value |
| `c5b_align64.jet` | prints `3`, `64`, `64`: `#Layout(c, align(64))` works |
| `c6_stride.jet` | runs, plain strided sums agree on every tier |

Code facts:

- `$build` facts are a closed list of eight paths in `Compiler/JetSema/Source/Sema/Expressions/FactReads.jet:627-639`: package name and version, `os`, `profile` and four stamp facts. There is no `cpu` fact. The snapshot struct is `SemaBuildFacts` in `Registration/Items.jet:613-627`.
- No D-CORE-BOUNDARY1 instruction entry exists. A search for `#Instruction` finds no code.
- The native backend is Jet's own x86-64 selector and encoder (`Compiler/JetIR/Source/X64Select.jet`, `Compiler/JetBackend/Source/X64/Encoder.jet`). Neither knows a prefetch, a non-temporal store or a fence. Inline assembly accepts `pause` and `syscall` by name (`X64/InlineAsm.jet:162`).
- A full o2 build of a 2-million-element loop (`probes/perf/A/bin/soa`, 4.6 MB of text) contains no `prefetch` and no `movnt` instruction.
- No CPU can be selected. The compiler has no `--cpu` or target-CPU flag, and its output is baseline x86-64 with scalar SSE2 (`movsd`, `addsd` in the o2 loop). `$build.cpu.cache_line` would therefore always read its generic default today.
- `#Layout(c, align(64))` is the one cache-related control that works. `concurrency/bounded_spsc_atomic` uses it and prints `layout=c:64:4160`.

## 2. What each control is worth on this host

The C program `probes/cprior/prior.c` runs each pair five times and keeps the best. Arrays are 256 MiB, twice the size of L3.

| Pair | Plain | With the hint | Change |
|---|---|---|---|
| Fixed-stride walk, 4 KiB stride, prefetch 8 strides ahead | 212.7 ms | 211.5 ms | none |
| Random gather `xs[idx[i]]`, prefetch 16 items ahead | 219.8 ms | 188.3 ms | 14% faster |
| 256 MiB copy, plain store vs `movnti` + `sfence` | 18.5 ms | 12.4 ms | 33% faster |
| Two threads, adjacent counters vs 64-byte padded | 70.3 ms | 19.0 ms | 3.7x faster |

Prefetch helps only where the hardware cannot guess the next address, such as a gather, a hash probe or a pointer chase. Zen 4, like current Intel cores, detects constant strides by itself [INFERENCE for Intel; measured here for Zen 4]. A paired cell built on a fixed-stride walk would show no gain, and AGENTS.md's strict comparator would then refuse it.

Streaming stores help when a large output is written once and not read soon. They hurt when the output is read right after, because it must come back from RAM [INFERENCE; not measured]. A streaming store also needs a store fence before another thread may read the data, so the lowering has to place one.

Padding is the largest win, and Jet already has it: `#Layout(c, align(64))`. The missing part is the value 64 as a fact, so code need not hard-code it.

## 3. What a program can really control

**Prefetch levels.** x86 has `prefetcht0`, `prefetcht1`, `prefetcht2` and `prefetchnta`, and `prefetchw` for write intent (this CPU reports `3dnowprefetch`, which includes `prefetchw`). Arm has `PRFM PLDL1KEEP` through `PLDL3STRM`. Both are hints: the chip may ignore them, and they never fault. The level a hint names is the closest level the line should reach; the chip decides the rest. GCC and Clang expose this as `__builtin_prefetch(addr, rw, locality)`. Zig exposes `@prefetch(ptr, .{ .rw, .locality, .cache })` (ziglang.org/documentation/master/#prefetch). LLVM has one intrinsic, `llvm.prefetch(ptr, rw, locality, cache_type)`.

**Non-temporal stores.** x86 has `movnti` for one register and `movntdq` and `vmovntdq` for vector registers. They write around the cache with write-combining and need `sfence` before publication. Arm has `STNP`, a pair store with a non-temporal hint.

**Flushes.** `clflushopt` and `clwb` (both present on this CPU) push lines out to memory. They matter for persistent memory and device buffers, not for speed, so no ballot proposes them.

**Cache partitioning.** Intel Cache Allocation Technology, AMD's L3 allocation enforcement (flags `cat_l3`, `cdp_l3` and `mba` on this CPU) and Arm MPAM give a group of processes a share of the L3 or of memory bandwidth. Linux exposes them only through the `resctrl` filesystem, which root must mount and configure (kernel.org/doc/html/latest/filesystems/resctrl.html). resctrl is not mounted on this host. A program cannot claim cache ways for itself, so this belongs to deployment tools, not to a language API.

**Huge pages.** A 2 MiB page needs one TLB entry instead of 512. Linux offers transparent huge pages, `madvise(MADV_HUGEPAGE)` per region, and `MAP_HUGETLB` from a pool root reserves (`nr_hugepages=0` here, so that pool is empty). Jet's runtime heap opts out on purpose. `Compiler/JetRuntime/Source/Alloc.jet:346-356` advises `MADV_NOHUGEPAGE` on every region. The comment there says an advised huge region compacts memory synchronously on a fault and stalls every allocating thread. `probes/cprior/thp.c` confirms this on this host. Filling 512 MiB with `MADV_HUGEPAGE` took 6,242 ms and 7,044 ms, against 159 ms and 176 ms with `MADV_NOHUGEPAGE`. The kernel still granted no huge pages (130,622 against 131,133 minor faults, one per 4 KiB page). dTLB misses on a following random gather were the same either way (16.6 million).

**NUMA placement.** `mbind` and `set_mempolicy` place memory on a node, and `sched_setaffinity` places threads. This host has one node, so nothing could be measured here. On multi-socket servers remote memory is slower than local memory [INFERENCE; no multi-node host was available].

## 4. Seeing where data comes from

**What the kernel offers without root.** `perf_event_open` with `perf_event_paranoid=2`, the upstream default, lets a user count and sample events in their own processes in user mode. Level 3 and above, which some distributions patch in, refuse it [INFERENCE: Debian and Android kernel patches]. Per-process software events include `page-faults`, `minor-faults` and `major-faults`. A major fault waited for storage, and a minor fault was served from RAM or the page cache.

**Generic events are not portable.** On this Zen 4 host, `perf stat` reports `LLC-load-misses` as `<not supported>`. L1D loads, L1D load misses and dTLB load misses do count. For the `soa` probe they read 505 million loads, 108 million L1D misses and 1.76 million dTLB misses. The last-level cache on AMD is outside the core's own counters, so "LLC miss" must come from a model-specific event.

**Model-specific events give the source level.** Zen 4 counts demand fills by where the line came from: `ls_dmnd_fills_from_sys.local_l2`, `.local_ccx` (the own L3), `.near_cache` and `.far_cache` (another core complex's cache), and `.dram_io_near` and `.dram_io_far` (RAM). Each can be sampled. Intel's equivalents are the `mem_load_retired.l1_hit` through `.l3_miss` events [INFERENCE; no Intel host was available].

**Precise per-instruction sources need privilege on AMD.** `perf mem record` uses AMD IBS, and on this host it fails: "ibs_op//u ... in per-thread mode, enable system wide with -a". System-wide capture needs `CAP_PERFMON` or a lower paranoid level. Intel PEBS samples carry a data-source field in per-thread mode [INFERENCE from the perf-mem documentation; not run here].

**Per-line attribution works today.** `probes/perf/S/walk.jet` sums a 16-million-element list once in order and once with a 4096-element stride. It was built at o2 with `--debug-info=true` and sampled with `perf record -e ls_dmnd_fills_from_sys.dram_io_near:u -e ls_dmnd_fills_from_sys.local_l2:u -e minor-faults:u -c 2000`. `perf report --sort sym,srcline` then gives:

| Event | Top Jet rows |
|---|---|
| L2-to-L1 demand fills | `walk_strided walk.jet:12` 58.0%, `:13` 3.4%, `:11` 2.3%; `walk_sequential walk.jet:3` 0.09% |
| RAM demand fills | `walk_strided walk.jet:12` 2.4%; most samples land in kernel page-fault code |
| Minor faults | `walk_run walk.jet:21` 44.8% (the list fill), `jet_rt_mem_move` 55.2% (list growth) |

Line 12 is `total += xs[i]` inside the strided loop. The sequential loop barely appears, because the hardware prefetcher fills its lines before the load asks. The same holds with inlining on: the rows still name `walk.jet:12`. Runtime functions such as `jet_rt_mem_move` have no line table and show as `??:0`. Without `--debug-info=true` the o2 binary keeps a 0xc1-byte `.debug_line`, too little for user lines.

**Disk shows as major faults.** `probes/cprior/majfault.c` writes a 128 MiB file, drops it from the page cache with `posix_fadvise(DONTNEED)`, maps it and touches every page. `perf stat` counts 16 major faults and 856 minor faults. Readahead turns one disk wait into many pages, so a major-fault count is a count of waits, not of bytes.

## 5. What Jet has for the report

- `jet perf` is registered with run, test, attach, view, compare and export subcommands (`Compiler/JetCli/Source/Cli/CLICommands.jet:640`). Every call returns `jet_cli_not_ported` (`Compiler/JetCli/Source/Cli/Front.jet:1601`).
- The deleted Rust command (`Source/CmdPerf.rs` at `c9129d174d^`, 4,075 lines) recorded wall time, CPU time, allocation and span lanes into the trace. It never called `perf_event_open`. Its "unavailable" states carried a reason (`native_unavailable_reason_for`, line 1402), which is the pattern card #4727 asks for.
- Ratified law already fixes the frame: D-PERFSESSION1 D (`jet perf run` with `--lens=cpu,alloc`, `.jtrace` v1), D-PROFILE-METHOD1 A (`--profile-method=sample|count`, unavailable evidence with a reason, no silent fallback), D-PROFILE-DETAIL1 A (rows per source range, function totals derived), D-CODEMAP-VIEW1 A (the Canvas code map, with run-time hotness out of scope), D-COSTLAW1 A (`jet explain --cost`).
- Card #4727 proposes a third method, `--profile-method=memory`. A memory view is a different thing to observe, not a different way to collect evidence, so it fits D-PERFSESSION1's lens list better than D-PROFILE-METHOD1's method list. Ballot `D-PERF-MEMORY1` puts that choice and the code-map shading to the owner.

## 6. Prior art

- Linux `perf mem` and `perf c2c` report load sources and false sharing per line (man7.org/linux/man-pages/man1/perf-mem.1.html, perf-c2c.1.html).
- Intel VTune's Memory Access analysis shows per-line bandwidth and the level a load hit.
- AMD uProf reads IBS for per-instruction data sources.
- Valgrind's Cachegrind simulates the caches. It runs on any host, about 20 to 100 times slower, and its numbers come from a model, not the chip (valgrind.org/docs/manual/cg-manual.html).
- Zig's `@prefetch`, GCC and Clang's `__builtin_prefetch` and `__builtin_nontemporal_store`, and C++17's `std::hardware_destructive_interference_size` are the hint surfaces D-CACHE-CONTROL1 already cites. Rust offers the same instructions only as target intrinsics, `core::arch::x86_64::_mm_prefetch` and `_mm_stream_si64` (doc.rust-lang.org/core/arch/x86_64/).
