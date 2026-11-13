# Memory retention in safe Jet: an audit of every construct that can keep memory alive — 2026-11-12

Scope: every construct through which safe Jet (no `#Unsafe`) can keep an
allocation alive after the program stops using it. For each one this audit
states the structural rule that frees the memory and the case that exercises
it. A construct whose rule is still a language choice is listed under
[Owner ballots](#owner-ballots). Tree: dev `d5b28c297b`.

Leak, as used here: an allocation that no live owner can reach any more but
that is still allocated. Memory a live owner still reaches (a table the
program keeps, a cache it fills) is *retention*. Whether retention that grows
without a bound counts as a leak is ballot B1.

## Constructs and their rules

| Construct | Structural rule | Case |
| --- | --- | --- |
| `Shared` values and reference cycles | `Shared` is counted, so a strong cycle would never be freed. A write that can close a strong `Shared` cycle is a compile error, E0221 (`jet.SharedReferenceCycle`): it is checked through plain field writes, `push`, guards, self-edges and other modules. A cycle the program needs uses a `Shared.Weak<T>` back-edge (`.downgrade()`), which does not keep its target alive. A recursive struct edge (`T` holding `T?` or `[T]`) is an owned box, not a counted handle, and is freed with its struct. | `tests/ui/shared_strong_cycle{,_guard,_push,_self,_cross_module}`; `tests/cases/shared_weak/shared_weak_cycle_matches_aot_and_default_tiers`; `tests/cases/memory_semantics/retaining_constructs_free_what_they_held` (a Weak-closed pair churned 900,000 times stays flat); `tests/cases/memory_semantics/cycle_struct_edges_free_with_their_struct` |
| Closures and captures | A function value is a counted owner of its closure box, and the last owner frees the box with its captures. This holds whether the closure is copied, stored in a struct or map, returned, widened, sent to a task, held by a scope guard, a transaction hook or an event listener. | `tests/cases/memory_semantics/closure_boxes_free_at_their_last_owner` |
| Closure captures that close a `Shared` cycle | A closure stored inside a `Shared` cell that captures a strong handle to the same cell is a cycle that E0221 must see. That check is #3645 c10 (SUB G10Safe-closure-cycle, pending). Until it lands this is the one known safe-code path to a strong cycle. | the G10Safe-closure-cycle SUB's ui snapshot |
| Tasks: joined | A task handle is joined or detached; discarding one is a compile error (`jet.UndischargedTask`, L1101). A join moves the result out, and the task's frame and captures are freed when it ends. | `tests/ui/discarded_task`; `tests/cases/task_state_tiers/joined_task_walks_drop_each_task_once`; `tests/cases/memory_semantics/blocks_shared_by_two_tasks_are_freed_once`; `tests/cases/memory_semantics/retaining_constructs_free_what_they_held` (joined task result) |
| Tasks: detached | A detached task must own everything it can use: a captured or returned view is E1106 and any other unsendable capture is E1103 (D-DETACH1). It is freed when it ends. When `run` returns, the runtime drains the scheduler (`jet_scheduler_drain_after_exit`), so a detached task that is still running runs to its end. | `tests/ui/task_detach_view_capture`; `tests/ui/task_detach_unsound` |
| Tasks parked at exit | A task blocked on a wait that nothing can satisfy any more is reported at exit with E3013 (`Parked tasks remain at process exit`). The runtime then cancels it, so it unwinds and runs its cleanup, and the process exits 70. A task past the registry cap (4,096) is neither reported nor unwound; the operating system reclaims its memory (ballot B2). | `tests/ui/runtime_e3013`; `tests/cases/fiber_liveness/parked_tasks_past_the_registry_cap_exit_with_the_report` |
| Generators (`Stream<T>`, `yield`) | A generator is a producer task. A consumer that breaks its loop, or drops the stream, cancels the producer at its wait point. The producer then runs its deferred cleanup and frees its frame, including locals it never yielded. | `tests/cases/task_control_tiers/stream_drop_uses_task_cancel_cleanup_path`; `tests/cases/memory_semantics/retaining_constructs_free_what_they_held` (generator broken off) |
| Channels | `channel<T>(capacity: N)` bounds queued memory: `send` waits for space and there is no unbounded or dropping channel (spec §Tasks, D-EVENT2). The channel owns its queued values, and they are freed with its last handle, received or not. `AsyncEvent` is the one explicit loss path, and its report shows what it dropped. | `tests/cases/memory_semantics/retaining_constructs_free_what_they_held` (channel dropped with queued values) |
| Module globals | An immutable global is computed while building and stored in the program image, and every read is a copy. A mutable or `#Persist` global's initializer is a tier-stable scalar literal (E0622, `jet.ModuleGlobalNotLiteral`), so it holds a scalar. A `name: Shared{value}` global holds one process-wide cell of literal data. No initializer runs code at start-up (E0643). | `tests/ui/module_value_call`; `tests/ui/module_array_computed_element`; `tests/ui/module_value_calls_code_that_reaches_the_outside_world` |
| Caches (`#Memo`) | A `#Memo` function keeps at most 128 results by default, and the shared Prelude store evicts the oldest past its bound. Only an explicit `#Memo(bound: none)` is unbounded (ballot B1). A `#Memo` field's cached result belongs to its struct and is freed with it, and a write to a stored dependency invalidates it. | `tests/cases/memory_semantics/memo_statistics_before_and_after_calls_use_one_store`; `Examples/Memory/Memoize.jet`; `tests/cases/memory_semantics/retaining_constructs_free_what_they_held` (bounded memo table) |
| Resources (`Close`) | A value whose type implements `Close` is closed exactly once, when its last owner ends: on rebinding, on moving out of a collection, in a branch or loop, and before a panic exits the process. | `tests/cases/resource_close/` (for example `a_close_local_rebound_in_a_loop_and_a_branch_closes_each_value_once`, `automatic_cleanup_runs_before_source_panic_exits_the_process`) |

## What the cases prove, and what they do not

The `retaining_constructs_free_what_they_held` case measures the peak resident
set (VmHWM) before and after eight times more rounds than its warm-up, and
fails when the peak grows by more than 8 MiB. That shows a construct does not
hold memory per round. It does not show that every block is freed at exit;
that needs the runtime leak witness of #6567 c7 (live allocations at exit = 0
on O0, O1 and O2), which does not exist yet. The 2026-11-05 lowering leaks
(#6567 c3) were bugs in the code the compiler emitted, not missing rules.
The ownership verifier of c1 and the leak gate of c2 catch that class of
defect. This audit covers the rules a correct compiler applies.

## Owner ballots

Two rules are language choices, and each goes to the owner as a ballot. The
audit records today's behaviour; the ballot payloads carry the options.

- **B1 — unbounded reachable retention (D-RETAIN-BOUND1).** Three things keep
  memory reachable until exit, so no rule can free it while the program can
  still read it:
  - `#Memo(bound: none)`;
  - a module `Shared{...}` global whose list the program keeps pushing to;
  - a collection owned by a task that never ends.

  The ballot asks whether this growth breaks the no-leak promise. The options
  are:
  - the memory is the program's own data, and the c7 witness frees globals and
    memo stores before it counts;
  - a growing global needs a visible mark;
  - safe code may not grow without a bound.
- **B2 — tasks still alive when `run` returns (D-EXIT-TASKS1).**
  - A detached task that is still computing runs to its end, so one that never
    ends keeps the process alive. Cancellation reaches a task only at a wait,
    so a loop that never waits (`loop { n += 1 }`) cannot be cancelled either.
  - A detached task that is waiting is reported with E3013 and unwound, and
    the process exits 70. A ticker that sleeps between ticks is such a task.
  - Parked tasks past the registry cap (4,096) are neither reported nor
    unwound.

  The options are:
  - exit cancels every remaining task and waits up to one second for it to
    unwind, with no cap;
  - keep today's rule without the cap;
  - no task may outlive `run`;
  - the process ends when `run` returns, without task cleanup.
