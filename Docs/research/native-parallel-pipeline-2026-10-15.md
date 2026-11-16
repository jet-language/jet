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

## Measured: where a native build spends its time (2026-10-16)

Two perf profiles (`perf record --call-graph fp`, 499 Hz), both of native O0
code with symbols:

- **Whole compiler.** The p26 native stage-1 compiler (`jet.o` relinked with
  symbols against the p26 runtime pack) runs `jet build` on
  `archive_hostile/main.jet` (11.6 KB plus Core). It takes 12.4 s and peaks
  at 4.0 GB RSS, over 5,150 samples.
- **Backend only.** The MemSafety fixed-backend harness (f2 factory build,
  relinked against the f2 runtime pack) lowers the 26 MB `o9m.mird` MIR
  dump in 5.5 s, over 2,765 samples. It spends 32% of that decoding the
  dump.

| Share of the whole-compiler build (inclusive) | Site |
|---|---|
| 22.8% | `lir_lint_module`. It runs twice on the same O0 LIR: once at the end of `lower_selected`, serially, and again inside `x64_compile_module`. `lir_lint_dominators` alone accounts for 18.3%. |
| 17.0% | MIR→LIR lowering (`lower_function`). Of this, `lower_repr`/`lower_size` recomputation is 12.3% (the harness agrees at 9.2%), and `lower_runtime_handle` is 8%, which 43b2963b1 has since fixed. |
| ~6% | x64 selection and register allocation, without lint. |
| 10.1% | `sema_finalize_graph` |
| 9.7% | Codegen TIR→MIR. `jet_codegen_move_last_uses` is the largest self cost; type lowering and qualification (`jet_codegen_lower_type_with_modules`, `jet_codegen_find_source_nominal_identity`) take 8%. |
| 8.6% | Cloning the `CoreModuleExportNames` constant per call (`core_type_modules`). Re-measure after #5001. |
| 7.5% | MIR optimize |
| 3.3% + 3.0% | The SHA-256 target dossier identity and the `mir_program_digest` written per build |
| 5.4% | `jet_loop_range_*`: an `Int` range loop calls the runtime on every iteration. Most of these calls come from the lint's dominator loops. |

The codegen `Builder.jet` scans named in the brief (the `builder.values`
walk in `jet_codegen_exit_value_slot`, the `jet_codegen_phi_ownership`
walk, `pending_exits` and `exit_value_slots`) do not appear above 0.5% in
either profile. Walks over a read parameter's list run in place
(`jet_codegen_lower_read_list_walk`), so they copy nothing; they are
linear, but they are not the bottleneck. The string-built identities
(`jet_codegen_identity` + `stable_id`) appear only inside the 4.1% of
`jet_codegen_emit`.

### Landed on perf-next (2026-10-16)

Measured with the f2-factory backend harness (`lower_selected` keep-going
lowering, the harness's own `lir_lint_module`, and the O0
`x64_relocatable_object_keep_going`, one process per case), on the 20
calibrated probes, the r27 dominance case `cli_schema_pe_sections`, and 160
corpus MIR dumps (181 cases). Every commit leaves every `.issues`, `.lir` and
`.o` byte-identical to the commit before it. Timings are the sum over the
five largest dumps (mF2, o9m, mP4, mO1, cli_schema_pe_sections; 19–26 MB of
MIR each), two alternating repetitions on a loaded machine.

1. **LIR lint dominance.** Cooper–Harvey–Kennedy immediate dominators with
   dominator-tree entry/exit numbers (a dominance query is two comparisons),
   each value's consume sites indexed, block IDs resolved through one
   first-row map. Lint 539/610 ms → 80/77 ms (7.4×); lowering (which lints
   once) 5816/6181 → 4761/4713 ms; object (which lints again) 1226/1603 →
   796/791 ms. A differential harness ran the old lint (renamed) and the new
   lint over every function of every case and over 22,325 mutants (swapped
   and moved instructions, retargeted edges, branches turned into jumps,
   inserted drops, duplicated block IDs, missing targets, rewired jump and
   return values): 17,686 mutants flagged with 34,440 rows (10,042 "not
   dominated", 6,935 unreachable, 5,554 missing block, 3,162 duplicated ID,
   2,461 use after move, 1,013 argument count, 5,273 other), 0 mismatches.
   On `cli_schema_pe_sections`, which this backend still lowers with the
   r27 dominance row, the new lint reports the same row as the old.
2. **Lint once at O0.** `lower_selected` keeps its lint findings
   (`LIRLowering.lint`); the O0 object compile takes them instead of linting
   the unchanged module again. Optimized modules are linted again as before.
   Errors, keep-going traps and bytes are unchanged. With the new lint the
   saved run is small: object 798/804 → 750/794 ms over the five dumps (one
   lint run, about 80 ms); with the old lint it was about 540 ms.
3. **Per-type cache.** One module cache (`LowerTypeCache`: `lower_repr`,
   `lower_size`, inline members and needs-drop by `lower_repr_key`), lent to
   the function being lowered, so the value helpers that only see the
   function reach it. Most `lower_repr` time was in helpers that never see
   the module (`lower_size_compute` 4.3%, `lower_inline_type` 2.8% of the
   harness profile; `lower_record_schema_with_handle` was 4.2% inside it).
   Lowering 4808 → 4038 ms over the five dumps (−16%).

## Per-stage profile (2026-10-16)

The largest probe that fits next to the running self-host attempt is the
backend harness unit itself: 73,077 lines of compiler source (the whole
backend with its Foundation support) plus Core. The p26 native compiler
runs out of memory on it at 22 GB (it peaked at 4.0 GB on an 11.6 KB
program), so the whole-pipeline split comes from the f2 factory (the Rust
tier) building it with `JET_TRACE_FILE`, and the backend split from the
native harness above.

| stage | f2 factory, 73 k lines | threads today |
|---|---|---|
| parse (load: unit 2.5 s, Core 4.9 s) | 7.5 s | per file; one assembled file |
| imports + registration | 18.3 s | 1 |
| sema check | 6.1 s | 1 (per module) |
| comptime | 5.1 s | 1 |
| sema finalize (bodies, failure, effects) | 38.1 s | bodies in waves |
| codegen TIR→MIR | 10.3 s | waves |
| MIR assemble + passes | 2.3 + 2.8 s | waves |
| backend: MIR→LIR, LIR lint, x64, object (no span) | ≈ 64 s | lint/x64 in waves |
| total (peak RSS 13.8 GB) | 161.9 s = 451 lines/s | |

Inside the native backend (symbolized harness profile, o9m, before these
commits): MIRD decoding 30% (harness input only), lint 10% in
`lower_selected` plus the harness's and the object's own runs, `lower_repr`
and `lower_size` 11%, the rest of MIR→LIR lowering 7%, x64 selection,
allocation and encoding 3%, object writing 1%. Linking is a separate step
(`x64_link_executable`) and is not in this unit's build.

## Ranked plan to 1M lines/s

451 lines/s on the factory and a few thousand on the native compiler are
three orders of magnitude short. No single stage closes that; every stage
needs both a serial speedup (data layout) and parallelism. In order of
measured weight and readiness:

1. **Sema finalize and registration (35% of the factory build).** Register
   per module in parallel waves and merge in module order; run finalize's
   failure and effect passes per dependency-DAG level. Replace string-keyed
   scope maps with interned integer IDs (one symbol table per build) so
   lookups are array indexes.
2. **Native backend (≈40%).** Parallel MIR→LIR lowering per function (the
   per-worker interning plan under *Stages*); with the type cache above,
   each worker keeps its own cache. The lint and x64 already run as waves.
   Object writing: per-function encoding in parallel, offsets by prefix sum.
3. **Memory traffic.** The factory peaks at 13.8 GB for 73 k lines
   (190 KB per line); the native compiler more. Arena-allocate per stage
   and per function (AST, TIR, MIR, LIR builders), free whole arenas at the
   stage end, and stop copying whole programs between stages (the
   `program.copy()` routes and per-call constant clones measured earlier).
4. **Compact data structures.** MIR/LIR instructions as flat arrays of
   fixed-size rows with integer operands instead of enums of lists; block
   and value IDs as dense indexes (as the lint now uses); types interned to
   integer IDs so `lower_repr`-style queries become table lookups.
5. **Inline `Int` range loops in codegen (≥5% of the native compiler).**
   Lower `loop i in a..<b` to an inline compare and increment.
6. **Lexer/parser SIMD.** Classify ASCII runs 16–32 bytes at a time
   (identifiers, whitespace, string bodies), and parse per top-level item
   range in parallel inside one assembled file.
7. **Per-build identity hashing (≈6% of a small native build).** Compute
   the target dossier SHA-256 and the MIR program digest once.
8. **Codegen `Builder.jet` scans (<1% each).** Index phi ownership and exit
   value slots by `value_positions`; stop copying cleanup frames per action.
