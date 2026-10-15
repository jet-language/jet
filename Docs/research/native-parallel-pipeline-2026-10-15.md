# Native JetC on all cores: the pipeline's parallel structure (P0 #4999)

Owner goal: the native compiler uses every core on its way to 1M LOC/s. This
note fixes the parallel structure of every stage and records what is
measured and done. No incremental or caching work.

## Measured serial share

r25 factory (`source-70d1cf244…/regression/Main-DevGreen-sharded-stage0-r25/retained/jetc0`)
compiling the runtime unit (7 MB, 4010 functions), traced and profiled with
perf: 57 s wall at 152% CPU.

| stage | wall | threads today |
|---|---|---|
| parse (one assembled file) | 4.5 s | 1 (per-file waves exist; one file) |
| register | 9.5 s | 1 |
| sema.check | 1.6 s | 1 |
| sema.finalize | 7.0 s | bodies 3.8 s in waves, failure/effects 1 |
| codegen TIR→MIR | 1.5 s | waves |
| MIR optimize | 2.4 s | waves |
| native backend (no trace span) | ~27 s | 1 |

Inside the backend (share of perf samples): MIR→LIR lowering 12%, LIR O1
passes 8%, LIR lint 5% (run twice), x64 selection and register allocation
5%, object/DWARF writing 6%. The backend is the largest serial share (47%
of wall), so it goes first. Stage 1 (the compiler compiling itself, 1088 s at
115% CPU) is the same code over 23,629 functions.

## The rule every stage follows

1. The stage's inputs move into a `Shared` value that the workers only read
   (`guard_read`). Each worker owns its part: its functions move into a slot,
   or it has a part state. Nothing is copied to give a thread work.
2. Work is cut into consecutive parts weighed by size (Threads.jet
   `compiler_threads_admit` and `compiler_threads_partition`). There is one
   task per part in a `task.group`, and the last part runs on the calling
   thread. Parts join in part order. When one thread is admitted, the
   identical code runs inline.
3. Results concatenate in part order. IDs that name rows (function,
   signature, data and glue IDs) are either fixed before the wave or
   renumbered in the same first-use order on every thread count. Output
   bytes, diagnostics and keep-going census rows are therefore identical for
   any thread count.
4. A worker's panic is raised again on the caller with its text unchanged
   (`compiler_threads_failure_text`). Keep-going parts catch a stopped item
   per attempt and trap it.
5. Only Jet `task.group`, `task` and `Shared` are used, so the meaning is the
   same on the Rust tier (the factory) and the native tier (I9).

## Stages

- **Parse:** per file (exists). An assembled unit is one file, so it is
  parsed per top-level item range. One linear pass finds the item starts at
  column 0; each part parses its ranges with absolute byte offsets; the ASTs
  concatenate in source order.
- **Registration** (9.5 s serial): a wave over modules collects each module's
  declarations; they merge in module order. Cross-module resolution stays
  one pass over the merged table.
- **Sema:** bodies already run in waves that share the post-summary module
  scope read-only (0fb1bac33). The declaration check and the failure and
  effect passes run per module, one dependency-DAG level at a time (the
  modules of one level in parallel).
- **Codegen TIR→MIR per function:** exists (`jet_codegen_lower_ranges`).
- **MIR optimize, lint and legality:** exist (`mir_pass_wave` and its
  siblings).
- **Backend, done here** (7082f3235):
  - the per-function LIR O1/O2 passes;
  - LIR lint of the function rows;
  - x64 selection, register allocation and machine code per function.

  These run as backend waves (`LIR.jet` `lir_wave_*`). `jet build`, `jet run`
  and `jet test`, the runtime-library feed and the factory's native and
  library feeds all pass the request's `CompilerThreads`.
- **MIR→LIR lowering, next.** Each function lowers against a frozen copy of
  the registered module tables. New signatures, text data, glue helpers and
  debug rows are interned per worker by key. After the wave the parts merge
  in part order, assigning global IDs in first-use order, and each part's
  LIR is remapped:
  - `Call` signature and callee;
  - `DataAddress` and `FunctionAddress`;
  - data relocations;
  - debug file and type indices;
  - glue `id`s.

  Two issues remain to solve before this is byte-identical:
  - Frozen static trees (Statics.jet) create anonymous child rows and
    attribute a failed freeze to the first reading function.
  - Debug types reserve a placeholder row and fill it in later.

  Both need their owner key recorded, so that a duplicate tree a later part
  built is dropped whole, or the constant trees need to be frozen in a
  serial pre-pass.
- **Object writing:** each function's code, relocations, CFI and DWARF line
  program encode in parallel. Placement offsets are a prefix sum in function
  order, and the sections concatenate in order.

## Measured: backend waves (Tests/ThreadsProbe.jet, native tier)

The probe was compiled by the r25 factory and linked against the r25 runtime
with this branch's arena allocator (97eda4603). It runs one process per
thread count, on 3061 LIR functions (30 groups x 50 workers):

| threads | LIR O2 passes | O2 object (lint + x64 + writer) | O0 object |
|---|---|---|---|
| 1 | 5559 ms | 3418 ms | 3736 ms |
| 2 | 3051 ms | 2023 ms | 2094 ms |
| 4 | 1868 ms | 1130 ms | 1288 ms |
| 8 | 1394 ms | 788 ms | 901 ms |
| 12 | 1888 ms | 803 ms | 834 ms |
| 16 | 1391 ms | 738 ms | 773 ms |

Every count printed the same digests: LIR text 6770362:485693944, O2 object
2080520:391830200, O0 object 2195296:671040537.

With the r25 runtime's single global allocator spin lock, the same waves get
*slower* with threads: the optimize pass goes 3.8 s → 14.4 s at 8 threads,
and 60.8% of samples sit in one spin loop. The waves therefore need the
arena allocator, which is on perf-next. Gains flatten past 8 threads, and
the remaining limit is the allocator (stack-band arenas). Per-thread free
lists are the next runtime item.
