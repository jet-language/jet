# #3095 — timer/interval cancellation and shutdown (criterion 2 / 4)

Closer07, 2026-09-29. Binary: `jet-debug-snapshot14` (sha256 prefix `00b1e35e25ed941c`). Scratch: `~/.cache/jet-test-scratch/Closer07/sched/` (package allows FS, Time, IO, Log, Mem.Alloc).

## Question
After `tasks.cancel`, does the timer/interval producer stop? At process or App shutdown, are scheduled producers cancelled and joined, or reported rather than silently outliving? Does this hold on JIT (`jet run`), `--interpret` and AOT?

## Evidence
### `cancel_after.jet`
```
c :: tasks.after(60000ms, 9); c2 :: tasks.cancel(c); print(c2.receive() ?? -1); print("done")
```
- JIT, 2 runs:
  ```
  after-cancel receive c=-1
  done
  Stop [E3013]: Parked tasks remain at process exit:
    task@0 (spawn site 0)  state: blocked  wait target: time sleep
  ```
  Exit **70**, wall 5 s. The cancelled one-shot timer's producer is still parked at exit, so a program that correctly cancelled its timer fails at exit.
- `--interpret`: `E0956 core.tasks.after() isn't supported by the current evaluator yet`.
- AOT (`safe-jet.sh build`): `internal compiler error: MIR nominal type "time.Duration" has no declaration row`.

### `cancel_interval.jet`
```
a :: tasks.interval(1ms); print(a.receive()); a2 :: tasks.cancel(a); print(a2.receive() ?? -1); print("done")
```
- JIT: `tick a=1` / `after-cancel receive a=2` / `done`, exit 0. A second tick is still delivered after `cancel`, and no parked-task report appears.
- `--interpret`: `E0956 core.tasks.interval() isn't supported by the current evaluator yet`.
- AOT: the same `time.Duration` ICE as above.

### `cancel_probe.jet` (thread count from `/proc/self/status`, JIT)
The program starts 2×`interval(1ms)` and 1×`after(60000ms)`, receives one tick each, cancels all three, then waits by receiving from `after(100ms)` and `after(500ms)`:
```
baseline Threads: 2
tick a=1 b=1
live Threads: 37
after-cancel receive a=-1 b=-1 c=-1
cancel+100ms Threads: 35
cancel+600ms Threads: 35
Stop [E3013]: Parked tasks remain at process exit: task@0 … wait target: time sleep
```
Exit 70. The thread count does not return toward baseline after cancellation. Some of the 35 threads are likely scheduler workers, so this is not a per-producer count; the E3013 report is the direct evidence that a cancelled producer outlives `cancel`.

`tasks.sleep(1)` itself fails on JIT: `internal compiler error: checked TIR cannot lower to MIR … __jet_core_core_tasks::__jet_host_sleep_ms: missing checked MIR owner type` (`sleep_ice.jet`).

## Verdict
FAIL:
- `tasks.cancel` does not stop the one-shot producer (E3013 at exit, status 70), and an interval still yields a tick after cancel.
- Shutdown is not silent: E3013 reports the parked producer. But it reports it as a fatal error for correct code, rather than cancel-and-join.
- The interpreter does not support `tasks.after`/`interval`, and AOT ICEs on them, so the three-tier requirement of criterion 4 fails before semantics.
- App-level shutdown (serve + SIGTERM) was not exercised.
- `schedule_cancel_stops_producer_and_app_shutdown_joins` does not exist in `tests/dev.rs`.

## Defects
1. JIT: after `tasks.cancel` on a `tasks.after(60000ms, …)` receiver, the process exits 70 with E3013 "Parked tasks remain … wait target: time sleep". Expected: cancel stops the producer, and exit is clean.
2. JIT: `tasks.interval(1ms)` → cancel → `receive()` still returns 2. Expected: None after cancel (or a documented drain of buffered ticks).
3. `--interpret`: E0956 for `core.tasks.after()` / `core.tasks.interval()`.
4. AOT build: ICE `MIR nominal type "time.Duration" has no declaration row` for any program using `tasks.after`/`interval`.
5. JIT: `tasks.sleep(1)` ICE (missing checked MIR owner type for `__jet_host_sleep_ms`).
