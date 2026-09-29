# #3257 — UI async-command lifecycle on the existing task contract

Date: 2026-09-29. Card #3257 (CORE-F033). Binary: `jet-debug-snapshot14`.

## Question

Can an MVVM-style async "Save" command, covering disabled activation, duplicate activation, cancellation, domain failure as `Failed` (not panic) and owner teardown, be expressed on Jet's existing task, error and teardown contract? Does it mean the same on every tier? What glue is hand-written?

## Blocker for the card as planned

The plan holds the status in a `core.reactive` `Signal`. `core.reactive` does not parse on snapshot14 or on the current source: `Core/reactive/reactive.jet:46` `pub fn effect` collides with the lexer keyword `effect` (`Compiler/JetLexer/Source/Lexer/Scan.jet:154`, E0003). Main recorded this as owner question D-REACT-EFFECT-NAME. The witness below therefore holds the status in a plain `Status` enum binding. Only the task half of the command is proven; the Signal-bound half is **BLOCKED**.

## Witness

`async_command.jet` is kept at `~/.cache/jet-test-scratch/Closer06/keep/cmd/async_command.jet`:

- a `#Error enum SaveError`;
- `fn save(Int) -> Int SaveError!` (sleeps 10 ms, rejects negatives);
- `fn save_job(Int) -> Status`, which maps the domain `Err` to `Status.Failed(message)` inside the task;
- `slow_save` (sleeps 500 ms);
- `failure_label(TaskFailure)`.

`run()` exercises five sections: disabled click; duplicate click while Running (explicit guard); domain error; `task.cancel()` then join; and a `task.group` whose pending job is cancelled and joined before the group ends.

| tier | command | result |
|---|---|---|
| AOT | `safe-jet.sh build async_command.jet` then `.jet/build/async_command` | rc=0, output below |
| JIT | `safe-jet.sh run async_command.jet` | first 7 lines identical, then `internal compiler error: typed drop 'K<>' field 0 is unavailable` at the end of `task.group` |
| interpreter | `safe-jet.sh run --interpret async_command.jet` | `E0956 'MIR native handle has no checked method adapter' isn't supported by the current evaluator yet` at `long.cancel()` |

AOT output (the correct meaning):

```
disabled click -> Idle
second click while Running ignored=true starts=1
first save -> Done(42)
bad save -> Failed(negative value -1)
cancelled save -> Failed(cancelled)
owner closing
teardown save -> Failed(cancelled)
owner joined
```

## Cells (criteria 1–3)

| lifecycle job | existing contract | hand-written glue |
|---|---|---|
| disabled activation | none; `if can_execute` guard | yes (one `if`) |
| duplicate activation while Running | none; guard on the status value | yes (status check plus start counter) |
| cancellation | `task.cancel()` then `^^t.join()` → `Err(TaskFailure.Cancelled)`. `slow_save`'s print after its sleep never runs (AOT, JIT) | no |
| domain error → Failed, not panic | A `-> T E!` job cannot be spawned bare: `task save(21)` is rejected with E2402 ("can't convert `SaveError` into `Err`") unless the enclosing function carries the error. The job must therefore own its domain result (`save_job` returns `Status`). The compiler forbids a silently dropped domain error across the task boundary | yes (a `Result`→status mapping in the job) |
| task failure (panic, cancel, deadline) | `join()` returns `T !TaskFailure` (D-CONC-FAIL1); matched exhaustively | no |
| owner teardown | `task.group` scope-end join (D-CONC-SPAWN1); explicit `cancel()` before the end gives cancel-and-join | cancel call only |
| observable status | a `Signal` status would be the reactive surface | **BLOCKED** (reactive keyword clash) |

Criterion 3 ("UI commands use the existing task error, cancellation and owner-teardown contract") holds on AOT: every line above comes from `task`, `join`, `cancel`, `TaskFailure` and `task.group` with no new abstraction.

## Criterion 4 — a missing command abstraction?

The general glue is small: an enablement flag, a Running guard and a status value. Every other job is covered by the task contract. A command helper would bundle `can_execute`, the Running guard and a status `Signal` over a group-spawned task. It cannot be drafted honestly while `Signal` does not compile, because the status binding is the helper's whole reason to exist. **Gate: no ballot now.** Revisit after D-REACT-EFFECT-NAME lands. If a ballot follows, it must not copy RelayCommand.

## Defects (repros under `~/.cache/jet-test-scratch/Closer06/keep/cmd/`)

| id | repro | tiers | observed | expected |
|---|---|---|---|---|
| TK-1 | `group_join_min.jet`: `task.group owner { pending :: task value(9); print("joined {^^pending.join() ?? -1}") }` | JIT | `joined 9` then ICE `typed drop 'K<>' field 0 is unavailable`. The existing golden `Examples/features/concurrency/release_join_failure.jet` hits the same ICE after printing its expected 8 lines | `joined 9` / `owner joined`, as the interpreter prints |
| TK-2 | `async_command.jet` line 98 `long.cancel()` | interpreter | E0956 `MIR native handle has no checked method adapter` | cancellation, as on AOT |
| TK-3 | `group_drop_min.jet`: an unjoined `task slow(9)` (50 ms sleep, then print) inside `task.group` | interpreter | `owner closing`, `owner joined`; the child's `slow 9 committed` never prints | scope-end join runs the child (D-CONC-SPAWN1 "Scope-end joins"); the JIT prints `slow 9 committed` before its TK-1 ICE |
| TK-4 | `ice_e0201.jet` / `bisect2.jet`: `fn outcome(job: Task<Int>) -> Int { if ^^job.join() == {…} }` | check (all tiers) | `internal compiler error: diagnostic 'E0201' cannot carry an edit and no-fix reason` (crates/jet-foundation/src/Diagnostics.rs:424) | an E0201 diagnostic (move marker required) |
| RX-1 | any `use core.reactive` | all | E0003 at `Core/reactive/reactive.jet:46` (`effect` keyword) | owner rename (D-REACT-EFFECT-NAME) |

## Verdict

**BLOCKED.**

- Criteria 1–3 are proven on AOT only. JIT and interpreter fail (TK-1, TK-2), and the Signal status is blocked by RX-1.
- Criterion 4 is met: the command abstraction is gated, with no ballot until RX-1 lands, for the reason given above.
- Criterion 5 (golden plus `run` and `--interpret` diff-empty) cannot pass until TK-1, TK-2 and RX-1 are fixed.
