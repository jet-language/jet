# #3180 — Finite task, channel, join and fail-fast conformance (evidence, 2026-09-29)

## Binding (criterion 1)

- Binary: `~/.cache/jet-dev/scratch/jet-debug-snapshot17`, pinned with `JET=`, run through `~/.cache/jet-dev/safe-jet.sh`.
- Capsule: `~/.cache/jet-dev/scratch/Closer05/t3180/conformance_capsule.jet`, which uses the
  package from `Examples/features/concurrency/package.jet`. It was run at the tree state of
  2026-09-29 around 04:45.
- Existing examples run on the same binary: `Examples/features/concurrency/{bounded_workers,shield_commit,cancel_cleanup,task_all,task_group,select_channel}.jet`
  (`run` only).
- Earlier overnight excerpts are quarantined, because they ran on snapshots 8 and 9. The
  `bounded_workers` hang recorded there no longer reproduces.

## Capsule output

`run --interpret` (rc=0):

```
backpressure: second send blocked while slot full
backpressure: drained 10
backpressure: second send completed after drain
backpressure: drained 20
worker limit: joined=[1, 2, 3, 4] events=8 max_in_flight=2 final_in_flight=0
join: explicit 42
join: scope-owned 2 4
join: group closed
failfast: error panic:fail_fast (spawn site 8): first
failfast: sibling cancelled at wait point
shield: winner 7
```

`run` (rc=101) prints the same first seven lines, then
`internal compiler error: typed drop 'K<>' field 0 is unavailable` when the `task.group` scope closes.

`build` was not rerun after the last capsule fix. The earlier capsule version failed to
compile on every tier because of a user error in the test itself, since fixed.

Existing examples on `run`:

- `bounded_workers`: rc=0, output matches its golden.
- `cancel_cleanup`, `task_all`, `select_channel`: rc=0.
- `shield_commit`: prints `committed`, `7`, then the same `typed drop K<>` ICE (rc=101).
- `task_group`: prints `inside task.group`, `1275`, `42`, then the same ICE (rc=101).

## What is proven, and on which tiers

| Cell | Observed transition | Tiers |
|---|---|---|
| Capacity-one backpressure (criterion 2) | The second send has not completed after a bounded 100 ms readiness wait. It completes only after the drain. Order: 10, then 20 | `run` and `--interpret` |
| Two-worker limit (criterion 2) | The +1/−1 event log, written while each worker holds a token, gives max_in_flight=2 and final 0 over 8 events | `run` and `--interpret` |
| Explicit join / scope-owned join (criterion 3) | 42; `task.all` inside `task.group` returns 2 and 4, and the group closes | `--interpret`. `run` ICEs when the group closes |
| task.all first failure (criterion 4) | Error identity `panic:… first` (the fast failure wins over the later `second`) | `--interpret` only (never reached on `run`) |
| Sibling cancellation at a wait point (criterion 4) | The sibling's post-sleep `done.send` never arrives within 400 ms | `--interpret` only |
| Shield deferral (criterion 4) | **Not observed.** `shield: committed inside shield` is missing. Only `winner 7` prints, although the `shield_commit` golden prints `committed` | `--interpret` (defect). `run` never reaches it |

Channel wait/readiness (criterion 3) uses the documented readiness table `if { v, rx -> …; after d -> … }` (D-CONC-CHAN2=D). The capsule has no foreign loop syntax. The obligation to join a bound Task is covered by the existing UI fixtures `tests/ui/unjoined_task.jet` and `discarded_task.jet`. I recorded them but did not re-run them.

## Oracle (criterion 6)

Each cell is tied to D-CONC-SPAWN1=D (structured `task.group`/`task.all`), D-CONC-FAIL1=A (first failure wins), D-CONC-JOIN1=A (explicit `^t.join()`), D-CONC-CHAN1=A (bounded channels), D-CONC-CHAN2=D (readiness table) and D-CANCELMODEL1=C (preemptive cancellation at wait points). None was re-balloted.

## Verdict

PARTIAL. Criteria 2 and 6 are met: both hold on `run` and `--interpret` for the backpressure
and worker cells. Criterion 1 is met for this receipt. Criterion 3 is met on `--interpret`,
but `run` ICEs when the group scope closes. Criterion 4 is partial: first-failure identity
and sibling cancellation are shown on `--interpret` only, and shield deferral fails. Criterion
5 was not exercised: READ-F14/READ-F05 were not run, and the evaluator gap is recorded
instead. Criterion 7 is unmet: there is no golden, `run` hits the ICE, and AOT hasn't been
proven.
