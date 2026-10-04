# #3108: event-driven idle scripting loop (SCRIPT-F18)

Closer10, 2026-09-29. Binary: `~/.cache/jet-dev/safe-jet.sh` →
`jet-debug-snapshot14`. Host: Linux x86_64 (CachyOS 7.0.11), 32 cores.
Verdict: **PARTIAL**: criterion 2 met; criteria 1, 3 (latency not measured) and 4 unmet.

## Question

On the current binary, does an ordinary timer/task loop idle without busy
work? Does cancellation release handles and stop queued work? What do CPU and
wakeups cost?

## Method

- Program: `~/.cache/jet-dev/scratch/Closer10/idle/idle_loop.jet`, a
  verbatim copy of the overnight prover's probe. It has 10 × `time.sleep(1000ms)`
  ticks in a task, then `task.race {forever(), short()}`. The loser prints
  `queued work i` every 100 ms. The fd count comes from
  `files.list_dir("/proc/self/fd")` before the race and 500 ms after it.
- AOT build: `safe-jet.sh build --allow=FS.Read,Time.Wait idle_loop.jet` →
  `.jet/build/idle_loop`.
- rusage: `python3 ~/.cache/jet-dev/scratch/Closer10/stream/measure.py 3 ./.jet/build/idle_loop`,
  which calls `os.wait4` and reads utime, stime, nvcsw and maxrss.
- syscalls: `strace -f -c -o idle.strace …` in a separate run.

## Evidence (AOT, snapshot14)

Program output (every run):

```
tick 1 … tick 10
ticks: 10
queued work 0
queued work 1
queued work 2
race winner: 7
fds leaked after cancel: 0
```

| Run | wall s | user s | sys s | CPU % of one core | voluntary ctx switches | maxrss KiB |
|---|---|---|---|---|---|---|
| 1 | 10.853 | 0.187 | 0.294 | 4.4 | 33,891 | 13,204 |
| 2 | 10.853 | 0.204 | 0.268 | 4.3 | 33,803 | 13,204 |
| 3 | 10.853 | 0.148 | 0.335 | 4.5 | 33,956 | 13,204 |

`strace -f -c` over one run:

- futex: 33,789 calls, 33,649 of them errors (ETIMEDOUT).
- sched_yield: 19,016 calls.
- clone3: 34.
- clock_nanosleep: 1.

The program does 10 ticks in about 10.8 s, so the runtime makes about 3,100
timed futex wakeups per second while idle. That matches the snapshot8
overnight numbers (33,906 switches, 33,740 futex). Nothing has changed.

Source still in place: `crates/jet-codegen/src/Prelude/Scheduler.rs` has these
timed-poll idle waits:

- `:3772` `wait_timeout(guard, Duration::from_millis(2))` in the worker loop;
- `:882` and `:897` `from_micros(50)`;
- `:1565` capped at 50 ms;
- `:2445` 5 ms;
- `:3752` 10 ms.

Plus `thread::yield_now` / `jet_scheduler_yield_now` spins at `:4041` and
`:4470`.

## Criteria

1. **Idle without an application busy loop: met at the application level,
   failed at the runtime level.** User code has no polling loop. The runtime
   itself polls: about 3,400 voluntary context switches per tick against a
   bar of about 1 wakeup per tick, and 4.4 % of one core while idle, against
   the bar of near zero.
2. **Cancellation: met on this probe.**
   - The race loser printed `queued work 0..2` (three 100 ms steps inside the
     350 ms race) and nothing afterwards, including during the 500 ms
     post-race sleep. Queued work stops.
   - `fds leaked after cancel: 0`.
3. **Measurements recorded:** CPU, wakeups and syscalls, above.
   - Tick latency p50/p99 was not measured.
   - Hosts: Linux measured; macOS and Windows are unavailable on this
     machine.
   - No scheduling API is proposed. The fix is a scheduler defect: idle
     workers should block on an event or timer rather than doing short timed
     condvar polls.
4. **Fail.** `effects` has no `idle_event_loop_*` test. The prebuilt
   `target-integ/debug/deps/effects-5f44c447fa356898 --list` shows 0 matches,
   and `Examples/features/concurrency/idle_event_loop.jet` does not exist.

## Defect

The scheduler idles by timed polling. Repro:

- `.jet/build/idle_loop`, 10 s idle, gives about 33.9k voluntary context
  switches and about 33.8k futex calls (99.6 % ETIMEDOUT).
- The expected figure is on the order of 10-20 wakeups.
