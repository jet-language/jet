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

### Ranked next improvements

1. **LIR lint dominance (estimated 15–18% of a native build, byte-identical).**
   - Replace the dense `[[Bool]]` fixpoint, which costs O(blocks² ×
     iterations) and makes a runtime range call per element, with
     Cooper–Harvey–Kennedy immediate dominators and dominator-tree
     entry/exit numbers. A dominance query then becomes two comparisons.
   - Index each value's consume sites, replacing the O(uses × consumes)
     scan.
   - Resolve block IDs through one first-row map, replacing the per-edge
     `lir_block_index` scans and the O(blocks²) duplicate-ID check.

   The lint keeps its own dominance code, separate from
   `Opt/Facts.jet opt_dominators`, because it referees optimized LIR. The
   change is prepared and parses (`/mnt/jetscratch/discovery/perfnext/lint-dominance.patch`,
   not landed). It still needs two proofs: the harness before/after timing
   (`perfnext/hx.sh`), and a differential test of the old and new lint over
   mutated LIR to show the findings are identical (`perfnext/oldlint.jet` is
   the renamed old lint for that test).
2. **Lint once at O0 (estimated half of the remaining lint time).**
   `x64_compile_module` re-lints the exact module `lower_selected` already
   linted clean. Skip the second lint only when the caller passes that clean
   result, so output stays identical.
3. **Cache `lower_repr`/`lower_size` (estimated 9–12%).** Most callers go
   to `lower_repr(p, ty)` directly instead of through the per-module
   `lower_repr_at` cache, and the size computation walks inline members
   again (`lower_inline_size_in`, `clone.MIRType`).
4. **Parallel MIR→LIR lowering (estimated 17% of the build, divided by the
   thread count).** Follow the plan under *Stages*. It is a large change:
   the per-worker interning of signatures, data, glue, debug and static
   trees, and the merge remap, must be byte-identical.
5. **Inline `Int` range loops in codegen (estimated ≥5% across the whole
   compiler).** Lower `loop i in a..<b` like the read-list walk, with an
   inline compare and increment and no `jet_loop_range_*` calls. This
   changes the MIR the compiler emits, but not program behavior.
6. **Per-build identity hashing (≈6%).** Compute the target dossier SHA-256
   and the MIR program digest once per process, or derive them from
   inputs that are already hashed.
7. **Codegen `Builder.jet` scans (<1% each, measured).**
   - Look up `jet_codegen_phi_ownership` and `jet_codegen_exit_value_slot`
     through `value_positions`.
   - Stop `jet_codegen_emit_cleanup_suffix` from copying every cleanup
     frame per action.
   - Stream `stable_id` over the identity parts instead of formatting the
     string.

   These rank last on current data.
