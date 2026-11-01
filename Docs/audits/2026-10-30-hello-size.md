# Hello-world binary size: what the runtime keeps alive, 2026-10-30

Card #6328 C1. This audit breaks down the size of `jet build` of hello world:
every runtime function and data object over 1 KB, and the root that keeps it
in the binary. It is a read-only audit; nothing here changes behavior.

The program is `fn run() { print("hello") }`. The compiler is `82d3369757`
plus #6106 C1 and C2 (per-function runtime sections, section gc, and the
vault split). The runtime sources are unchanged from there through dev
`abc19f121d`. The target is x86-64 Linux (musl), static.

## Totals

All five builds are byte-identical at 437,416 B: `jet build`,
`--optimize=none`, `--optimize=basic`, `--optimize=full` and `--release`.
Hello's own code is `main_run` (105 B). It makes one runtime call, so there is
nothing for the optimizer to change. Everything else is runtime.

The optimization levels do work on real code. A loop-and-format probe's
`main_sum` is 572 B at o0 and 484 B at o2. That probe's o2 binary is still
64 B larger than its o0 binary, 458,208 B against 458,144 B.

| section | bytes | share |
|---|---:|---:|
| `.text` | 387,880 | 88.7% |
| `.eh_frame` | 29,148 | 6.7% |
| `.symtab` + `.strtab` + `.shstrtab` | 9,938 | 2.3% |
| `.data` | 3,816 | 0.9% |
| `.debug_*` | 482 | 0.1% |
| `.rodata` | 352 | 0.1% |
| `.init_array` / `.fini_array` | 304 / 16 | 0.1% |
| `.bss` (no file bytes) | 2,044 | |

Program code is 310 B: `main` (36), `main_run` (105) and `jet_ir_stop_poll`
(169). The runtime pack contributes 749 live sections:

- 384,671 B of text;
- 3,747 B of data;
- 1,956 B of bss;
- 313 B of rodata.

Peer hello worlds built on the same machine:

| peer | bytes |
|---|---:|
| Zig `ReleaseSmall`, stripped | 10,720 |
| C, musl static, `-Os`, stripped | 17,696 |
| Rust, `opt-level=z`, stripped | 394,032 |

## Method

Section liveness was recomputed over `jet_runtime.pack.o` with the static
link's own rules (StaticLink.jet `link_collect`).

The roots are:

- `_start`;
- every `.init_array`, `.fini_array` and `SHF_GNU_RETAIN` section;
- the CIE personality routines;
- the program's references.

The program's references were recovered from the binary. They are the call
and RIP-relative operands of the program's functions that land on runtime
symbols. There are seven:

- `jet_rt_main`
- `jet_rt_host_arguments`
- `jet_rt_host_program_start`
- `jet_rt_process_entry_return`
- `jet_rt_session_propagate`
- `jet_rt_stdout_flush`
- `jet_term_write_stdout_line`

An FDE's LSDA edges follow the function's section.

The recomputed live set matches the binary exactly: 749 sections. Their
bytes, plus program code and alignment, equal `.text`.

The pack strips the names of private functions. They were recovered by
matching section bytes against `jet_runtime_jet.o`.

**Held** is the number of bytes that leave the binary if that one section is
never reached. It is computed by removing the section and recomputing
reachability. A cut's **freed alone** figure is the same measure applied to
one edge of the graph.

## Root causes

Cuts overlap, so the freed-alone figures do not sum. The cumulative table
below is the additive view.

1. **Runtime-gated features are linked unconditionally. Observe is kept
   alive by an environment read: 107,301 B.**
   - `jet_rt_main` always calls `jet_rt_observe_runtime_start_from_env`.
     It checks `JET_OBSERVE` at run time.
   - That check is a branch. Liveness cannot prune a branch, so the writer
     thread, the snapshot and JSON publishers and thread spawn are all
     linked.
2. **A shared-state read links the whole scheduler: 72,224 B, inside cause
   1.**
   - The environment read takes `shared_acquire`.
   - Its contention path calls `scheduler_blocking_wait_enter`, which makes
     up for the blocked worker by starting another one. That chain is:
     1. `scheduler_get`
     2. `scheduler_worker_count`
     3. `cpu_available_parallelism`
     4. the cgroup v1 and v2 quota parsers and the mountinfo search
     5. `jet_rt_scheduler_worker`
     6. `task_run`
   - So reading one variable keeps the task scheduler, worker threads and
     cgroup parsing alive in a program that never spawns a task.
3. **The fault handler is always installed: 50,111 B.**
   - `jet_rt_main` calls `fault_start`, which takes the address of
     `jet_rt_fault_signal`.
   - The handler carries an in-process DWARF reader to print crash
     locations. Its largest parts are:

     | function | bytes |
     |---|---:|
     | `fault_line` | 6,115 |
     | `fault_file` | 5,523 |
     | `fault_sections` | 4,715 |
     | `fault_unit` | 4,424 |
     | `fault_source` | 4,137 |
     | `fault_form` | 3,949 |
     | `fault_abbrevs` | 3,829 |
4. **EH frame registration is decided when the pack is built: 22,047 B, plus
   29,148 B of `.eh_frame`.**
   - The pack defines the unwinder. So `link_eh_shims` writes
     `__jet_eh_startup` as `jmp __register_frame_info`, and every program's
     `_start` reaches:
     - libgcc's FDE code, which is one 13,371 B `.text` that is not split
       into function sections;
     - musl `malloc`;
     - `pthread_cond_timedwait`.
   - The Jet runtime never unwinds; stops travel by stop edges. The tables
     and the unwinder serve only foreign code, and hello has none.
5. **Every runtime stop fans out into comptime serving and test
   publication: 18,287 B for the serve protocol alone.**
   - `trap_simple`, `jet_rt_fixed_shift_stop` and the other stop routines
     first call `comptime_serve_stop`. It checks `comptime_serving()` at run
     time, but statically links the whole protocol:
     - `comptime_serve_loop`
     - `comptime_serve_value`
     - `jet_rt_int_from_static`
     - big-int packing
   - The stop routines then go through `rt_task_panic_publish` to
     `test_scope_record_coded_stop` and text formatting.
   - The runtime's own checked arithmetic reaches these stops from
     `jet_rt_alloc_init`.
6. **38 unconditional static constructors.** Every runtime module with
   global state registers an `.init_array` entry, and init arrays are gc
   roots. Hello keeps all 38, including:
   - `alloc`, `task_exit`, `layout` (`layout_literals_init` alone is
     4,893 B) and `test_capture`;
   - `thread`, `codec_text_cells`, `journey`, `session`, `log_state` and
     `fault_allocation`;
   - `net_poller`, `observe`, `crypto_entropy`, `comptime`, `sentry`,
     `test_suite`, `fs_held` and `stm`.

   After the cuts in the next section, the constructors still hold
   108,890 B.
7. **The exit path always reports parked tasks: 12,856 B through
   `main_exit_cleanup`.** It runs `jet_rt_task_exit_handlers` and
   `jet_rt_observe_parked_tasks_report` with its detail and cancel paths.
   This happens even when no task or exit handler exists.
8. **Print goes through test capture: 5,947 B.** It is held by the
   constructor and by `jet_term_write_stdout_line` calling
   `jet_rt_test_capture_print` on every line.

## Cumulative cuts

The pack has 390,687 live bytes. Each row cuts one more edge.

| cut | freed alone | freed cumulative | remaining |
|---|---:|---:|---:|
| fault handler | 50,111 | 50,111 | 340,576 |
| observe start (includes shared → scheduler) | 107,301 | 157,412 | 233,275 |
| EH registration shim | 22,047 | 181,635 | 209,052 |
| stop → comptime serve | 18,287 | 199,922 | 190,765 |
| parked-task report | 10,751 | 241,907 | 148,780 |
| test capture (init + print) | 5,947 | 247,854 | 142,833 |
| layout literals init | 5,442 | 253,296 | 137,391 |
| task exit handlers | 1,577 | 254,873 | 135,814 |
| root stop take | 4,877 | 260,759 | 129,928 |

The remaining 130 KB is mostly the 38 constructors (108,890 B). The rest,
about 16 KB, is `jet_rt_main`, host arguments and environment seeding. So the
size is set by the design: unconditional startup, unconditional exit, and
gates that are checked at run time. Removing individual symbols cannot fix
it.

## After C2 (D-SIZE-PAYUSE1 A)

Hello is 134,064 B at every level (default, o0, o1, o2, `--release`), from
437,416 B. The cuts are by liveness and lazy installation, none specific to
hello:

- The observation start is a code word that `jet_rt_main` calls. Its hook
  (ObserveLive.jet `jet_rt_observe_start_init`) is kept only when the program
  links a task, a channel, a live value or an event. The task registry's
  init is no longer called from `jet_rt_main`, so its hook is kept only by
  code that reads the registry.
- The task-stop publication is a code word installed by the first task,
  parallel or realtime body (Scheduler.jet `scheduler_body_enter`); stop
  domain 2 exists only inside one. The test-scope stop recorder is installed
  with the test capture (TestCapture.jet `jet_rt_test_capture_init`).
- The stdout flush's buffer shift no longer checks an address sum, so
  allocator init no longer reaches a numeric stop through it.

What hello still links: no task spawn, worker, observation writer,
environment read or formatter. The 108,763 live pack bytes are the
allocator, the stop reports (`trap_*`, number rendering for them) with the
process root's control (`jet_rt_root_*`, `scheduler_control_*`,
`scheduler_root_publish_owned`), which every program's stop goes through
(D-FAIL-EXIT1), and the fault handler (29,207 B held).

## Data objects

No runtime data object reaches 1 KB. The largest is musl's
`__malloc_context`, 928 B of bss, which is reached only through the EH
unwinder's `malloc` (cause 4). The next largest are two bss blocks of 336 B
and 232 B and a 197 B `.data` table.

## Every runtime function over 1 KB

There are 111 functions over 1 KB.

Column key:

- **size**: the section's own bytes.
- **held**: as defined in Method.
- **root**:
  - `program` is reached from the program's references;
  - `init` is reached from an `.init_array` constructor;
  - `_start` is reached from the process entry.
- **kept by**: the path from the root to the function's parent. `…` elides
  the middle of paths longer than four.

Private names are shown without the pack's `unit_` prefix.

| size | held | function | root | kept by |
|---:|---:|---|---|---|
| 13371 | 22047 | `_Unwind_Find_FDE` | _start | _start → __jet_eh_startup |
| 7036 | 8220 | `cpu_find_mountpoint` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_quota_v1 |
| 6472 | 26979 | `task_run` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → jet_rt_scheduler_worker |
| 6115 | 6115 | `fault_line` | program | jet_rt_main → jet_rt_fault_signal → … → fault_frame_line |
| 5523 | 5523 | `fault_file` | program | jet_rt_main → jet_rt_fault_signal → fault_overflow |
| 4893 | 4901 | `layout_literals_init` | init | jet_rt_layout_init |
| 4715 | 4928 | `fault_sections` | program | jet_rt_main → jet_rt_fault_signal |
| 4675 | 5511 | `cpu_quota_v2` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_quota |
| 4452 | 32660 | `jet_rt_scheduler_worker` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → scheduler_start_worker |
| 4424 | 8253 | `fault_unit` | program | jet_rt_main → jet_rt_fault_signal → … → fault_function |
| 4342 | 20720 | `observe_publish` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → jet_rt_observe_writer |
| 4242 | 16186 | `observe_snapshot` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → observe_publish |
| 4137 | 4162 | `fault_source` | program | jet_rt_main → jet_rt_fault_signal → fault_overflow |
| 3949 | 3949 | `fault_form` | program | jet_rt_main → jet_rt_fault_signal → … → fault_file |
| 3837 | 3837 | `observe_live_start` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env |
| 3829 | 3829 | `fault_abbrevs` | program | jet_rt_main → jet_rt_fault_signal → … → fault_unit |
| 3813 | 13651 | `cpu_quota_v1` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_quota |
| 3623 | 5574 | `observe_process_facts` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → observe_live_start |
| 3562 | 23429 | `cpu_quota` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_available_parallelism |
| 3539 | 3539 | `observe_parked_details` | program | jet_rt_main → main_exit_cleanup → jet_rt_observe_parked_tasks_report |
| 3218 | 3218 | `rt_text_push_scalar` | init | jet_rt_alloc_init → alloc_out_of_memory → … → rt_text_push_word |
| 3124 | 3124 | `int_scale` | init | jet_rt_alloc_init → alloc_out_of_memory → … → jet_rt_int_from_static |
| 2944 | 3145 | `observe_events_json` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → observe_snapshot |
| 2895 | 2895 | `comptime_stop_frame_write` | program | jet_rt_main → jet_rt_root_take_stop |
| 2862 | 9644 | `fault_overflow` | program | jet_rt_main → jet_rt_fault_signal |
| 2840 | 3632 | `observe_tasks_json` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → observe_snapshot |
| 2734 | 215492 | `jet_rt_main` | program | (root) |
| 2627 | 12159 | `fault_report` | program | jet_rt_main → jet_rt_fault_signal |
| 2534 | 2565 | musl `.text` (malloc) | _start | _start → __jet_eh_startup → … → __libc_malloc_impl |
| 2478 | 2494 | `jet_rt_host_arguments` | program | (root) |
| 2371 | 2371 | `jet_rt_test_capture_print` | program | jet_term_write_stdout_line |
| 2368 | 5350 | `jet_rt_sentry_frame_leave` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → sentry_thread_detach |
| 2302 | 2302 | `jet_rt_thread_attach` | init | jet_rt_thread_init |
| 2300 | 2300 | `jet_rt_u64_to_string` | init | jet_rt_alloc_init → alloc_out_of_memory → jet_rt_fixed_shift_stop |
| 2285 | 2285 | `scheduler_wait_while_paused` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → task_run |
| 2283 | 17230 | `comptime_serve_loop` | init | jet_rt_alloc_init → alloc_out_of_memory → … → comptime_serve_stop |
| 2257 | 2257 | `trap_defect_unreachable_head` | init | jet_rt_alloc_init → trap_gs_failure |
| 2244 | 6301 | `alloc_check_fail` | init | jet_rt_alloc_init → alloc_out_of_memory → … → alloc_check_give |
| 2165 | 2165 | `cpu_path_append` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_quota |
| 2164 | 2164 | `cpu_read` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_quota |
| 2140 | 2901 | `logical_environment_append` | program | jet_rt_host_arguments → logical_environment_seed |
| 2127 | 72224 | `scheduler_blocking_wait_enter` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → shared_acquire |
| 2114 | 8229 | `fault_frame_line` | program | jet_rt_main → jet_rt_fault_signal → fault_report |
| 2045 | 2099 | `cpu_online_count` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_available_parallelism |
| 2009 | 2513 | `alloc_check_give` | init | jet_rt_alloc_init → alloc_out_of_memory → … → alloc_free_validated |
| 1965 | 1965 | `sentry_expire` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → jet_rt_sentry_frame_leave |
| 1955 | 3203 | `pthread_create` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → thread_spawn |
| 1950 | 1950 | `jet_scheduler_shield_leave` | init | jet_rt_task_exit_init → jet_rt_task_exit_destructor |
| 1950 | 4747 | `world_task_finished` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → task_run |
| 1942 | 10751 | `jet_rt_observe_parked_tasks_report` | program | jet_rt_main → main_exit_cleanup |
| 1941 | 10459 | `jet_rt_alloc` | init | jet_rt_alloc_init → alloc_out_of_memory → … → comptime_serve_stop |
| 1922 | 10175 | `fault_function` | program | jet_rt_main → jet_rt_fault_signal → fault_overflow |
| 1897 | 107301 | `jet_rt_observe_runtime_start_from_env` | program | jet_rt_main |
| 1878 | 50111 | `jet_rt_fault_signal` | program | jet_rt_main |
| 1795 | 1795 | `scheduler_msg_deadline_tail` | init | jet_rt_task_exit_init → jet_rt_task_exit_destructor → jet_scheduler_shield_leave |
| 1789 | 2571 | `scheduler_control_cancel` | program | jet_rt_main → main_exit_cleanup → … → observe_cancel_live_tasks |
| 1751 | 4362 | `alloc_try_validated` | init | jet_rt_alloc_init → alloc_out_of_memory → … → alloc_try_reserved |
| 1751 | 1751 | `int_normalize` | init | jet_rt_alloc_init → alloc_out_of_memory → … → int_pack |
| 1751 | 1751 | `observe_task_sift` | program | jet_rt_main → main_exit_cleanup → … → observe_parked_details |
| 1725 | 1725 | `scheduler_task_completion_end` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → task_run |
| 1723 | 1723 | `rt_text_reserve_extra` | init | jet_rt_alloc_init → alloc_out_of_memory → … → rt_text_append |
| 1687 | 1687 | `alloc_check_symbol` | init | jet_rt_alloc_init → alloc_out_of_memory → … → alloc_check_chain |
| 1684 | 75419 | `shared_acquire` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → jet_rt_logical_env_get |
| 1651 | 2178 | `jet_rt_free` | init | jet_rt_alloc_init → alloc_out_of_memory → … → jet_rt_string_drop |
| 1631 | 1631 | `sync_park` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → jet_rt_observe_writer |
| 1600 | 1600 | `observe_channels_json` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → observe_snapshot |
| 1599 | 1599 | `world_event` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → world_task_finished |
| 1594 | 1594 | `thread_tls_destroy` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → jet_rt_thread_detach |
| 1577 | 1577 | `jet_rt_task_exit_handlers` | program | jet_rt_main → main_exit_cleanup |
| 1545 | 1545 | `jet_rt_thread_bootstrap` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → jet_rt_thread_entry |
| 1527 | 1527 | `scheduler_text_concat` | init | jet_rt_task_exit_init → jet_rt_task_exit_destructor → jet_scheduler_shield_leave |
| 1517 | 1517 | `int_limb` | init | jet_rt_alloc_init → alloc_out_of_memory → … → int_fits |
| 1488 | 1488 | `thread_unregister_tcb` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → jet_rt_thread_detach |
| 1464 | 1464 | `trap_footer_e3001` | init | jet_rt_alloc_init → alloc_out_of_memory → … → trap_footer_bytes |
| 1449 | 1449 | `jet_rt_world_drop` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → task_drop |
| 1445 | 1445 | `cpu_parse_file` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → cpu_quota_v1 |
| 1444 | 4015 | `observe_cancel_live_tasks` | program | jet_rt_main → main_exit_cleanup → jet_rt_observe_parked_tasks_report |
| 1431 | 4877 | `jet_rt_root_take_stop` | program | jet_rt_main |
| 1417 | 1417 | `scheduler_control_publish` | init | jet_rt_alloc_init → alloc_out_of_memory → … → scheduler_native_publish |
| 1416 | 11353 | `jet_rt_int_from_static` | init | jet_rt_alloc_init → alloc_out_of_memory → … → comptime_serve_value |
| 1406 | 1849 | `fault_snippet` | program | jet_rt_main → jet_rt_fault_signal → fault_overflow |
| 1386 | 1386 | `pthread_cond_timedwait` | _start | _start → __jet_eh_startup → … → pthread_cond_wait |
| 1311 | 1311 | `observe_read_file` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → observe_process_facts |
| 1311 | 35062 | `scheduler_start_worker` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → scheduler_blocking_wait_enter |
| 1301 | 1301 | `jet_rt_logical_env_get_os` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env |
| 1298 | 1298 | `test_scope_record_raw_stop` | init | jet_rt_alloc_init → alloc_out_of_memory → … → test_scope_record_coded_stop |
| 1285 | 33127 | `cpu_available_parallelism` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → scheduler_worker_count |
| 1272 | 1272 | `jet_rt_i64_to_string` | init | jet_rt_alloc_init → alloc_out_of_memory → jet_rt_fixed_shift_stop |
| 1270 | 1270 | `alloc_out_of_memory` | init | jet_rt_alloc_init |
| 1259 | 5044 | `jet_rt_fixed_shift_stop` | init | jet_rt_alloc_init → alloc_out_of_memory |
| 1256 | 1256 | `scheduler_abort_double_panic` | init | jet_rt_alloc_init → alloc_out_of_memory → … → scheduler_panic_check |
| 1203 | 1203 | `thread_words_grow` | init | jet_rt_codec_text_cells_init → jet_rt_tls_key_new |
| 1198 | 1198 | `world_timer_remove` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → world_task_finished |
| 1190 | 2238 | `test_scope_state_drop` | init | jet_rt_test_capture_init → jet_rt_test_capture_abort |
| 1167 | 1167 | `rt_text_append` | init | jet_rt_alloc_init → alloc_out_of_memory → jet_rt_fixed_shift_stop |
| 1158 | 1158 | `int_node_init` | init | jet_rt_alloc_init → alloc_out_of_memory → … → int_new |
| 1157 | 1157 | `jet_rt_tls_key_new` | init | jet_rt_codec_text_cells_init |
| 1147 | 2404 | `jet_rt_root_begin` | program | jet_rt_main |
| 1144 | 1144 | `scheduler_thread_detach` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → jet_rt_thread_detach |
| 1139 | 4625 | `__libc_malloc_impl` | _start | _start → __jet_eh_startup → … → malloc |
| 1117 | 1117 | `scheduler_blocking_wait_leave` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → shared_acquire |
| 1115 | 1528 | `scheduler_completion_order` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → task_run |
| 1109 | 1109 | `comptime_serve_filter` | init | jet_rt_alloc_init → alloc_out_of_memory → … → comptime_serve_loop |
| 1101 | 1101 | `task_group_release` | program | jet_rt_main → jet_rt_observe_runtime_start_from_env → … → task_run |
| 1094 | 1094 | `trap_number` | init | jet_rt_alloc_init → alloc_out_of_memory → … → trap_context |
| 1077 | 3315 | `jet_rt_test_capture_abort` | init | jet_rt_test_capture_init |
| 1074 | 1074 | `rt_text_lossy` | init | jet_rt_alloc_init → alloc_out_of_memory → … → jet_rt_string_from_static |
| 1072 | 2865 | `jet_std_time_now` | init | jet_rt_task_exit_init → jet_rt_task_exit_destructor → jet_scheduler_shield_leave |
| 1050 | 1474 | `host_stdout_write` | program | jet_term_write_stdout_line |
| 1049 | 18287 | `comptime_serve_stop` | init | jet_rt_alloc_init → alloc_out_of_memory → jet_rt_fixed_shift_stop |
| 1048 | 1048 | `test_scope_expect_clear` | init | jet_rt_test_capture_init → jet_rt_test_capture_abort → test_scope_state_drop |
