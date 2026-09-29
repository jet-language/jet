# #3366 — Timing and readiness batching against existing stream windows (evidence, 2026-09-29)

## Question

Can processing-time jobs (timeout, throttle, chunks_timeout) and readiness jobs
(ready_chunks) be expressed with existing task deadlines, channel readiness tables and
`try_receive`? The event-time window must stay on the #2853 operator. Which operations,
if any, genuinely need a new public form?

## Method

- Witness: `~/.cache/jet-test-scratch/Closer05/t3366/timing_batches.jet`. It has one
  function per row, and each printed line names its clock. The package grants
  `[IO, Mem.Alloc, Mem.Rc, Panic, Time]`.
- `no_event.jet` is the same file without the event-time row. `row_*.jet` run each row
  alone (`bisect.sh`).
- Tiers: `safe-jet.sh run`, `run --interpret`, and `build` (see "Tier results").
- Source read: `Core/tasks/tasks.jet:107-113,150-171`, `Core/data/stream.jet:22-82`,
  `Examples/features/foundations/distributed/run.jet:130-151`, and the readiness table in
  `Examples/features/concurrency/select_channel.jet`.

## Mapping (criteria 1–3)

| Job | Clock | Existing mechanism | Reset rule | Timeout kind | Partial batch | Cancellation | Retained bound |
|---|---|---|---|---|---|---|---|
| timeout | processing-time | readiness table `if { v, rx -> …; after d -> … }` | Per wait. Each readiness wait arms a fresh `after`, so the deadline resets per item | Recoverable. The channel stays open. The witness receives value 3 after the timeout | n/a | Closing the sender makes `receive` return the closed outcome | The channel capacity (`channel<Int>(capacity: n)`) |
| ready_chunks | readiness | `rx.try_receive()` drained up to `max` | n/a (never waits) | n/a | The last chunk may be short. An empty channel yields `[]` without waiting | n/a | `max` per chunk plus the channel capacity |
| chunks_timeout | processing-time | readiness table with a size check plus `after d` | Per wait. [INFERENCE] A per-batch deadline needs an absolute deadline, and no `after_until` form exists (see gates) | Recoverable. A deadline flushes the partial batch and accumulation continues | Emitted on deadline (`[[1,2,3],[4]]`, reasons `[size, deadline]`) | `tx.close()` then drain: the partial tail is flushed (`[5]`) | size `N` plus the channel capacity |
| throttle | processing-time | `core.tasks.interval(period)`: each emission first consumes one tick | Fixed period (interval) | n/a | n/a | Closing the interval receiver (`tasks.cancel`) ends it | One pending item per tick plus the channel capacity |
| event-time window | event-time | `.with_event_time(…).key_by(…).window(10s, watermark: 5s, late: .Drop)` (#2853, unchanged) | Watermark-driven, not wall time | n/a | The window closes at the watermark | The stream consumer `break` cancels the producer (generators.jet) | Keyed window state is bounded by the watermark. Late events follow `late:` |

Processing time (`after`, `interval`), readiness (`try_receive`, `is_ready`) and event
time (the operator's timestamp extractor) are separate mechanisms, and none reads another's
clock. That meets criterion 1. The watermark and late policy reuse #2853 without change,
which meets criterion 3.

## Tier results

See the JSON report for the pinned-binary rerun. Observed on the current tree:

- `run` of `no_event.jet` (rows 1 to 4): all rows print the values in the table above (rc=0).
- `run --interpret`: the timeout, ready_chunks and chunks_timeout rows match `run`.
  The throttle row fails with `E0956 core.tasks.interval() isn't supported by the current evaluator yet`.
- Event-time row: `run` of the full file is an ICE,
  `Cranelift cannot execute MIR function … run: MIR enum type MirTypeId(2019131428900252029) is missing`.
  The ICE happens for every row when the event-time code is present in the file. `--interpret`
  fails with `E0956 MIR enum type ID has no type row`. A first version that called
  `events().with_event_time(…)` directly was an ICE at check time:
  `TExprKind::Try (non-carrier operand) has no canonical MIR operation`.

## Owner gates (criterion 4)

The batching jobs need no new async library. Two small public forms would remove
boilerplate. Each would need its own ballot, and neither is proposed as a default:

1. **An absolute-deadline receive arm.** A readiness arm keyed to a fixed instant
   (for example `until instant -> …`) would give chunks_timeout a per-batch deadline
   instead of a per-wait one. Today a per-batch deadline needs manual `Instant` arithmetic
   and a recomputed `after` on each wait.
2. **A `Stream` batching adapter.** For example `.chunks(n, within: d)` on `DataStream`/`Stream`, for callers
   who want chunks_timeout without writing the readiness loop.

## Verdict

PARTIAL. Criteria 1 to 4 are met by this document and the exercised rows. Criterion 5
(the golden `streams/timing_batches` with empty three-tier diffs) is unmet: the event-time
row ICEs on `run` and is unsupported on `--interpret`, and `interval` is unsupported on
`--interpret`. The witness was not landed as an Example, because a golden can't hold a
tier-divergent result.
